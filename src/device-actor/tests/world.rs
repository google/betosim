// Copyright 2026 The Android Open Source Project
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

#![allow(
    clippy::too_many_arguments,
    clippy::needless_update,
    clippy::field_reassign_with_default,
    clippy::expect_fun_call
)]

use std::{
    collections::HashMap,
    sync::{Arc, atomic::AtomicU32},
    time,
};

use bytes::Bytes;
use device_actor::{DeviceActor, DeviceClient, DeviceError};
use device_api::{DeviceChipCreate, DeviceConfig, DeviceCreate, DeviceId, DeviceUpdate};
use futures::{SinkExt, StreamExt};
use link_api::MockLinkClient;
use netsim_model::{
    BluetoothUpdate, ChipClient, ChipId, ChipKind, ChipUpdate, ChipVariantUpdate, ClientError,
    MockChipClient, Pose, RadioUpdate,
};

pub const DEFAULT_MANUFACTURER: &str = "Netsim";
pub const UPDATED_MANUFACTURER: &str = "updated-manufacturer";
pub const UPDATED_PRODUCT_NAME: &str = "updated-product-name";
pub const MATCHED_MANUFACTURER: &str = "Matched-Vendor";

#[derive(Clone)]
pub struct LinkTestState {
    pub links: Arc<std::sync::Mutex<Vec<netsim_model::Link>>>,
    pub reset_called: Arc<std::sync::atomic::AtomicBool>,
}

impl LinkTestState {
    pub fn new() -> Self {
        Self {
            links: Arc::new(std::sync::Mutex::new(Vec::new())),
            reset_called: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }
}

/// The BDD World for Device Actor tests.
pub struct World {
    pub client: DeviceClient,
    _actor_task: tokio::task::JoinHandle<()>,
    pub radio_stats: Arc<std::sync::Mutex<Vec<netsim_model::NetsimRadioStats>>>,
    pub wifi_stats: Arc<std::sync::Mutex<HashMap<u32, netsim_proto::stats::WifiStats>>>,
    pub last_radio_stats: Option<Vec<netsim_model::NetsimRadioStats>>,
    pub stats_file_to_cleanup: Option<std::path::PathBuf>,
    pub current_device_id: Option<DeviceId>,
    pub current_chip_id: Option<netsim_model::ChipId>,
    pub transport_tx: Option<tokio::sync::mpsc::UnboundedSender<Bytes>>,
    pub device_config: Option<DeviceConfig>,
    pub stats_path: Option<std::path::PathBuf>,
    pub link_state: LinkTestState,
    pub mock_chips: Arc<std::sync::Mutex<HashMap<netsim_model::ChipId, netsim_model::Chip>>>,
    updated_chips: Arc<std::sync::Mutex<Vec<netsim_model::ChipId>>>,
    pub last_error: std::sync::Mutex<Option<ClientError>>,
}

impl Drop for World {
    fn drop(&mut self) {
        self._actor_task.abort();
        if let Some(path) = &self.stats_file_to_cleanup
            && path.exists()
        {
            let _ = std::fs::remove_file(path);
        }
    }
}

impl World {
    /// Detaches the stats file cleanup responsibility from the World.
    /// Useful when the test wants to verify the file content after World drop.
    pub fn detach_stats_cleanup(&mut self) {
        self.stats_file_to_cleanup = None;
    }

    pub async fn new() -> Self {
        let (link_client, link_state) = Self::create_default_link_client_and_state();
        let radio_stats = Arc::new(std::sync::Mutex::new(Vec::new()));
        let wifi_stats = Arc::new(std::sync::Mutex::new(HashMap::new()));
        let mock_chips = Arc::new(std::sync::Mutex::new(HashMap::new()));
        let updated_chips = Arc::new(std::sync::Mutex::new(Vec::new()));
        let chip_clients = Self::create_default_chip_clients_with_stats(
            radio_stats.clone(),
            wifi_stats.clone(),
            mock_chips.clone(),
            updated_chips.clone(),
        );
        let (stats_path, _) = Self::temp_stats_path();
        Self::with_clients_internal(
            chip_clients,
            link_client,
            None,
            None,
            Some(stats_path.clone()),
            None,
            radio_stats,
            wifi_stats,
            link_state,
            Some(stats_path),
            mock_chips,
            updated_chips,
        )
        .await
    }

    pub async fn new_with_stats(
        path: std::path::PathBuf,
        interval: Option<std::time::Duration>,
    ) -> Self {
        let (link_client, link_state) = Self::create_default_link_client_and_state();
        let radio_stats = Arc::new(std::sync::Mutex::new(Vec::new()));
        let wifi_stats = Arc::new(std::sync::Mutex::new(HashMap::new()));
        let mock_chips = Arc::new(std::sync::Mutex::new(HashMap::new()));
        let updated_chips = Arc::new(std::sync::Mutex::new(Vec::new()));
        let chip_clients = Self::create_default_chip_clients_with_stats(
            radio_stats.clone(),
            wifi_stats.clone(),
            mock_chips.clone(),
            updated_chips.clone(),
        );
        Self::with_clients_internal(
            chip_clients,
            link_client,
            None,
            None,
            Some(path.clone()),
            interval,
            radio_stats,
            wifi_stats,
            link_state,
            Some(path),
            mock_chips,
            updated_chips,
        )
        .await
    }

    /// Creates a new World with injected custom mock clients.
    pub async fn with_clients(
        chip_clients: HashMap<ChipKind, Box<dyn ChipClient>>,
        link_client: MockLinkClient,
    ) -> Self {
        let link_state = LinkTestState::new();
        let radio_stats = Arc::new(std::sync::Mutex::new(Vec::new()));
        let wifi_stats = Arc::new(std::sync::Mutex::new(HashMap::new()));
        let mock_chips = Arc::new(std::sync::Mutex::new(HashMap::new()));
        let updated_chips = Arc::new(std::sync::Mutex::new(Vec::new()));
        let (stats_path, _) = Self::temp_stats_path();
        Self::with_clients_internal(
            chip_clients,
            link_client,
            None,
            None,
            Some(stats_path.clone()),
            None,
            radio_stats,
            wifi_stats,
            link_state,
            Some(stats_path),
            mock_chips,
            updated_chips,
        )
        .await
    }

    /// Creates a new World with injected custom mock clients and idle timeout.
    pub async fn with_clients_and_timeout(
        chip_clients: HashMap<ChipKind, Box<dyn ChipClient>>,
        link_client: MockLinkClient,
        startup_timeout: Option<std::time::Duration>,
        idle_timeout: Option<std::time::Duration>,
        stats_path: Option<std::path::PathBuf>,
    ) -> Self {
        let radio_stats = Arc::new(std::sync::Mutex::new(Vec::new()));
        let wifi_stats = Arc::new(std::sync::Mutex::new(HashMap::new()));
        let mock_chips = Arc::new(std::sync::Mutex::new(HashMap::new()));
        let updated_chips = Arc::new(std::sync::Mutex::new(Vec::new()));
        let link_state = LinkTestState::new();

        // If stats_path is None, create a temp one to avoid pollution
        let (final_path, cleanup_path) = if let Some(p) = stats_path {
            (Some(p), None)
        } else {
            let (p, _) = Self::temp_stats_path();
            (Some(p.clone()), Some(p))
        };

        Self::with_clients_internal(
            chip_clients,
            link_client,
            startup_timeout,
            idle_timeout,
            final_path,
            None,
            radio_stats,
            wifi_stats,
            link_state,
            cleanup_path,
            mock_chips,
            updated_chips,
        )
        .await
    }

    /// Internal helper to create World
    async fn with_clients_internal(
        chip_clients: HashMap<ChipKind, Box<dyn ChipClient>>,
        link_client: MockLinkClient,
        startup_timeout: Option<std::time::Duration>,
        idle_timeout: Option<std::time::Duration>,
        stats_path: Option<std::path::PathBuf>,
        stats_interval: Option<std::time::Duration>,
        radio_stats: Arc<std::sync::Mutex<Vec<netsim_model::NetsimRadioStats>>>,
        wifi_stats: Arc<std::sync::Mutex<HashMap<u32, netsim_proto::stats::WifiStats>>>,
        link_state: LinkTestState,
        stats_file_to_cleanup: Option<std::path::PathBuf>,
        mock_chips: Arc<std::sync::Mutex<HashMap<netsim_model::ChipId, netsim_model::Chip>>>,
        updated_chips: Arc<std::sync::Mutex<Vec<netsim_model::ChipId>>>,
    ) -> Self {
        let (runner, client) = device_actor::new();
        let mut actor = DeviceActor::new(
            chip_clients,
            Arc::new(AtomicU32::new(0)),
            None,
            Box::new(link_client),
            startup_timeout,
            idle_timeout,
            "0.0.0-test".to_string(),
            stats_path.clone(),
            stats_interval,
            Arc::new(netsim_model::FrontendStats::default()),
        );
        actor.set_self_client(client.clone());
        let actor_task = tokio::spawn(runner.run(actor));
        World {
            client,
            _actor_task: actor_task,
            radio_stats,
            wifi_stats,
            last_radio_stats: None,
            stats_file_to_cleanup,
            current_device_id: None,
            current_chip_id: None,
            transport_tx: None,
            device_config: None,
            stats_path,
            link_state,
            mock_chips,
            updated_chips,
            last_error: std::sync::Mutex::new(None),
        }
    }

    pub fn create_default_chip_clients() -> HashMap<ChipKind, Box<dyn ChipClient>> {
        let radio_stats = Arc::new(std::sync::Mutex::new(Vec::new()));
        let wifi_stats = Arc::new(std::sync::Mutex::new(HashMap::new()));
        let mock_chips = Arc::new(std::sync::Mutex::new(HashMap::new()));
        let updated_chips = Arc::new(std::sync::Mutex::new(Vec::new()));
        Self::create_default_chip_clients_with_stats(
            radio_stats,
            wifi_stats,
            mock_chips,
            updated_chips,
        )
    }

    pub fn create_default_chip_clients_with_stats(
        radio_stats: Arc<std::sync::Mutex<Vec<netsim_model::NetsimRadioStats>>>,
        wifi_stats: Arc<std::sync::Mutex<HashMap<u32, netsim_proto::stats::WifiStats>>>,
        mock_chips: Arc<std::sync::Mutex<HashMap<netsim_model::ChipId, netsim_model::Chip>>>,
        updated_chips: Arc<std::sync::Mutex<Vec<netsim_model::ChipId>>>,
    ) -> HashMap<ChipKind, Box<dyn ChipClient>> {
        let mut clients: HashMap<ChipKind, Box<dyn ChipClient>> = HashMap::new();
        // Add default mocks for common chip kinds
        for kind in [ChipKind::BLUETOOTH, ChipKind::WIFI, ChipKind::UWB] {
            clients.insert(
                kind,
                Box::new(Self::create_default_mock_chip(
                    radio_stats.clone(),
                    wifi_stats.clone(),
                    mock_chips.clone(),
                    updated_chips.clone(),
                    kind,
                )),
            );
        }
        clients
    }

    pub(crate) fn create_default_mock_chip(
        radio_stats: Arc<std::sync::Mutex<Vec<netsim_model::NetsimRadioStats>>>,
        wifi_stats: Arc<std::sync::Mutex<HashMap<u32, netsim_proto::stats::WifiStats>>>,
        mock_chips: Arc<std::sync::Mutex<HashMap<netsim_model::ChipId, netsim_model::Chip>>>,
        updated_chips: Arc<std::sync::Mutex<Vec<netsim_model::ChipId>>>,
        _kind: ChipKind,
    ) -> MockChipClient {
        let mut mock = MockChipClient::new();
        let initial_chips = Arc::new(std::sync::Mutex::new(HashMap::new()));
        Self::setup_mock_chip_client(
            &mut mock,
            mock_chips,
            initial_chips,
            radio_stats,
            wifi_stats,
            updated_chips,
        );
        mock
    }

    fn setup_mock_chip_client(
        mock: &mut MockChipClient,
        chips: Arc<std::sync::Mutex<HashMap<netsim_model::ChipId, netsim_model::Chip>>>,
        initial_chips: Arc<std::sync::Mutex<HashMap<netsim_model::ChipId, netsim_model::Chip>>>,
        radio_stats: Arc<std::sync::Mutex<Vec<netsim_model::NetsimRadioStats>>>,
        wifi_stats: Arc<std::sync::Mutex<HashMap<u32, netsim_proto::stats::WifiStats>>>,
        updated_chips: Arc<std::sync::Mutex<Vec<netsim_model::ChipId>>>,
    ) {
        let chips_clone = chips.clone();
        let rs_clone = radio_stats.clone();
        let ws_clone = wifi_stats.clone();
        let initial_chips_clone = initial_chips.clone();
        let updated_chips_clone = updated_chips.clone();
        mock.expect_clone_box().returning(move || {
            Self::create_shared_mock(
                chips_clone.clone(),
                initial_chips_clone.clone(),
                rs_clone.clone(),
                ws_clone.clone(),
                updated_chips_clone.clone(),
            )
        });

        let chips_create = chips.clone();
        let initial_chips_create = initial_chips.clone();
        mock.expect_create()
            .with(mockall::predicate::always(), mockall::predicate::always())
            .returning(move |id, params| {
                let mut chips = chips_create.lock().unwrap();
                let mut chip = params.chip;
                chip.id = id.0;
                chip.enabled = true;
                chips.insert(id, chip.clone());
                let mut initial_chips = initial_chips_create.lock().unwrap();
                initial_chips.insert(id, chip);
                if let Some(mut stream) = params.packet_stream {
                    tokio::spawn(async move { while stream.next().await.is_some() {} });
                }
                Ok(())
            });

        let chips_read = chips.clone();
        mock.expect_read().returning(move |id| {
            let chips = chips_read.lock().unwrap();
            chips
                .get(&id)
                .cloned()
                .ok_or(netsim_model::ClientError::Chip(netsim_model::ChipError::ChipNotFound(id)))
        });

        let chips_update = chips.clone();
        let updated_chips_rec = updated_chips.clone();
        mock.expect_update().returning(move |id, patch| {
            updated_chips_rec.lock().unwrap().push(id);
            let mut chips = chips_update.lock().unwrap();
            if let Some(chip) = chips.get_mut(&id) {
                // Apply patches (simplified)
                if let Some(pos) = &patch.pose.position {
                    chip.pose.position = *pos;
                }
                if let Some(orient) = &patch.pose.orientation {
                    chip.pose.orientation = *orient;
                }
                if let Some(mfr) = &patch.manufacturer {
                    chip.manufacturer = mfr.clone();
                }
                if let Some(prod) = &patch.product_name {
                    chip.product_name = prod.clone();
                }
                if let Some(enabled) = patch.enabled {
                    chip.enabled = enabled;
                }
                if let (Some(update_variant), Some(chip_variant)) =
                    (&patch.variant, &mut chip.variant)
                {
                    update_variant.apply(chip_variant);
                }
                Ok(chip.clone())
            } else {
                Err(netsim_model::ClientError::Chip(netsim_model::ChipError::ChipNotFound(id)))
            }
        });

        let chips_delete = chips.clone();
        mock.expect_delete().returning(move |id| {
            let mut chips = chips_delete.lock().unwrap();
            chips.remove(&id);
            Ok(())
        });
        let chips_reset = chips.clone();
        let initial_chips_reset = initial_chips.clone();
        mock.expect_reset().returning(move |id| {
            let mut chips = chips_reset.lock().unwrap();
            let initial_chips = initial_chips_reset.lock().unwrap();
            let initial_chip = initial_chips.get(&id).ok_or(netsim_model::ClientError::Chip(
                netsim_model::ChipError::ChipNotFound(id),
            ))?;
            if let Some(chip) = chips.get_mut(&id) {
                chip.pose = initial_chip.pose;
                chip.enabled = true;
                if let Some(netsim_model::ChipVariant::Bluetooth(bt)) = &mut chip.variant {
                    bt.low_energy.state = Some(true);
                    bt.classic.state = Some(true);
                }
                Ok(chip.clone())
            } else {
                Err(netsim_model::ClientError::Chip(netsim_model::ChipError::ChipNotFound(id)))
            }
        });

        let rs = radio_stats.clone();
        mock.expect_read_statistics().returning(move || Ok(Box::from(rs.lock().unwrap().clone())));

        let ws = wifi_stats.clone();
        let chips_wifi = chips.clone();
        mock.expect_get_global_stats().returning(move || {
            let chips = chips_wifi.lock().unwrap();
            let ws_lock = ws.lock().unwrap();
            // Return stats for the first chip found in the map
            for id in chips.keys() {
                if let Some(stat) = ws_lock.get(&id.0) {
                    use netsim_proto::protobuf::Message;
                    let mut ipc_stats = netsim_proto::stats::WifiIpcStats::new();
                    ipc_stats.wifi_stats = netsim_proto::protobuf::MessageField::some(stat.clone());
                    return Ok(Some(ipc_stats.write_to_bytes().unwrap_or_default()));
                }
            }
            Ok(None)
        });
    }

    /// Helper to create a mock that shares existing state (for clone_box of an
    /// ACTIVE client)
    fn create_shared_mock(
        chips: Arc<std::sync::Mutex<HashMap<netsim_model::ChipId, netsim_model::Chip>>>,
        initial_chips: Arc<std::sync::Mutex<HashMap<netsim_model::ChipId, netsim_model::Chip>>>,
        radio_stats: Arc<std::sync::Mutex<Vec<netsim_model::NetsimRadioStats>>>,
        wifi_stats: Arc<std::sync::Mutex<HashMap<u32, netsim_proto::stats::WifiStats>>>,
        updated_chips: Arc<std::sync::Mutex<Vec<netsim_model::ChipId>>>,
    ) -> Box<MockChipClient> {
        let mut mock = MockChipClient::new();
        Self::setup_mock_chip_client(
            &mut mock,
            chips,
            initial_chips,
            radio_stats,
            wifi_stats,
            updated_chips,
        );
        Box::new(mock)
    }

    pub fn create_default_link_client() -> MockLinkClient {
        let (mock, _) = Self::create_default_link_client_and_state();
        mock
    }

    pub fn create_default_link_client_and_state() -> (MockLinkClient, LinkTestState) {
        let state = LinkTestState::new();
        (Self::create_link_client_with_state(state.clone()), state)
    }

    pub fn create_link_client_with_state(state: LinkTestState) -> MockLinkClient {
        let mut mock = MockLinkClient::new();
        let links_create = state.links.clone();
        mock.expect_create().returning(move |params| {
            let mut links = links_create.lock().unwrap();
            let id = netsim_model::LinkId(links.len() as u32);
            links.push(netsim_model::Link {
                id,
                sender: params.sender,
                receiver: params.receiver,
                kind: netsim_model::ChipKind::BLUETOOTH, // Default for testing
                rssi: params.rssi,
            });
            Ok(id)
        });

        let links_list = state.links.clone();
        mock.expect_list().returning(move || Ok(links_list.lock().unwrap().clone()));

        let reset_called_clone = state.reset_called.clone();
        let links_reset = state.links.clone();
        mock.expect_reset().returning(move || {
            reset_called_clone.store(true, std::sync::atomic::Ordering::Relaxed);
            links_reset.lock().unwrap().clear();
            Ok(())
        });

        mock.expect_action().returning(|_, _| Ok(()));
        mock.expect_update().returning(|_, _| Ok(()));
        mock.expect_delete().returning(|_| Ok(()));
        mock.expect_notify_chip_added().returning(|_, _| Ok(()));
        mock.expect_notify_chip_removed().returning(|_| Ok(()));
        mock
    }

    /// BDD Step: When I create a new device.
    pub async fn when_create_device(&self, name: &str) -> DeviceId {
        let params = DeviceCreate {
            device_config: DeviceConfig::new(name.to_string(), true, Pose::default(), false),
            chip: DeviceChipCreate {
                name: "beacon".to_string(),
                manufacturer: "Netsim".to_string(),
                product_name: "NetsimBeacon".to_string(),
                chip: device_api::ChipCreateVariant::Beacon(Default::default()),
            },
        };
        self.client.create_device(params).await.unwrap()
    }

    /// BDD Step: When I create a new device with specific position and
    /// orientation.
    pub async fn when_create_device_at_position_and_orientation(
        &self,
        name: &str,
        position: device_api::Position,
        orientation: device_api::Orientation,
    ) -> DeviceId {
        let params = DeviceCreate {
            device_config: DeviceConfig::new(
                name.to_string(),
                true,
                Pose { position, orientation },
                false,
            ),
            chip: DeviceChipCreate {
                name: "beacon".to_string(),
                manufacturer: "Netsim".to_string(),
                product_name: "NetsimBeacon".to_string(),
                chip: device_api::ChipCreateVariant::Beacon(Default::default()),
            },
        };
        self.client.create_device(params).await.unwrap()
    }

    pub async fn when_add_chip(&self, device_guid: &str, chip_name: &str) -> DeviceId {
        let params = Self::create_device_add_chip_params(
            device_guid.to_string(),
            chip_name.to_string(),
            "00:00:00:00:00:00".to_string(),
        );
        self.client.add_chip(params).await.unwrap()
    }

    pub async fn when_add_chip_with_position(
        &self,
        device_guid: &str,
        chip_name: &str,
        position: device_api::Position,
        orientation: device_api::Orientation,
    ) -> DeviceId {
        let mut params = Self::create_device_add_chip_params(
            device_guid.to_string(),
            chip_name.to_string(),
            "00:00:00:00:00:00".to_string(),
        );
        params.device_config.pose.position = position;
        params.device_config.pose.orientation = orientation;
        self.client.add_chip(params).await.unwrap()
    }

    pub async fn when_add_wifi_chip(&self, device_guid: &str, chip_name: &str) -> DeviceId {
        let mut params = Self::create_device_add_chip_params(
            device_guid.to_string(),
            chip_name.to_string(),
            "".to_string(),
        );
        params.chip.kind = netsim_model::ChipKind::WIFI;
        params.chip.variant =
            Some(netsim_model::ChipVariant::Wifi(netsim_model::Wifi { radio: Default::default() }));
        self.client.add_chip(params).await.unwrap()
    }

    pub async fn when_add_uwb_chip(&self, device_guid: &str, chip_name: &str) -> DeviceId {
        let mut params = Self::create_device_add_chip_params(
            device_guid.to_string(),
            chip_name.to_string(),
            "".to_string(),
        );
        params.chip.kind = netsim_model::ChipKind::UWB;
        params.chip.variant =
            Some(netsim_model::ChipVariant::Uwb(netsim_model::Uwb { ..Default::default() }));
        self.client.add_chip(params).await.unwrap()
    }

    /// BDD Step: When I update the device.
    pub async fn when_update_device(&self, device_id: DeviceId, update: DeviceUpdate) {
        self.client.update(device_id, update).await.unwrap();
    }

    /// BDD Step: When I update the device with a specific chip update.
    pub async fn when_update_device_chip(&self, device_id: DeviceId, chip_update: ChipUpdate) {
        let mut update = DeviceUpdate::default();
        update.id = device_id.0;
        update.chips = Some(vec![chip_update]);
        self.when_update_device(device_id, update).await;
    }

    /// BDD Step: Given a device with chips sharing the given name and specified kinds.
    pub async fn given_device_with_chips_sharing_name(
        &self,
        guid: &str,
        name: &str,
        kinds: &[ChipKind],
    ) -> (DeviceId, Vec<ChipId>) {
        let mut chip_ids = Vec::new();
        let mut device_id = None;
        for (i, &kind) in kinds.iter().enumerate() {
            let addr = if kind == ChipKind::BLUETOOTH {
                format!("00:00:00:00:00:{:02x}", (i + 1) as u8)
            } else {
                String::new()
            };
            let mut params =
                Self::create_device_add_chip_params(guid.to_string(), name.to_string(), addr);
            params.chip.kind = kind;
            params.chip.variant = match kind {
                ChipKind::BLUETOOTH => params.chip.variant,
                ChipKind::WIFI => Some(netsim_model::ChipVariant::Wifi(netsim_model::Wifi {
                    radio: Default::default(),
                })),
                ChipKind::UWB => {
                    Some(netsim_model::ChipVariant::Uwb(netsim_model::Uwb { ..Default::default() }))
                }
                _ => None,
            };
            let (dev_id, chip_id) = self.client.add_chip_with_id(params).await.unwrap();
            device_id = Some(dev_id);
            chip_ids.push(chip_id);
        }
        (device_id.expect("kinds must not be empty"), chip_ids)
    }

    /// BDD Step: When I try to update the device with a specific chip update.
    pub async fn when_try_update_device_chip(&self, device_id: DeviceId, chip_update: ChipUpdate) {
        let mut update = DeviceUpdate::default();
        update.id = device_id.0;
        update.chips = Some(vec![chip_update]);
        let res = self.client.update(device_id, update).await;
        *self.last_error.lock().unwrap() = res.err();
    }

    /// BDD Step: Then the update was rejected as invalid input containing the expected substring.
    pub fn then_update_rejected_as_invalid_input(&self, expected_substring: &str) {
        let err_guard = self.last_error.lock().unwrap();
        let err = err_guard.as_ref().expect("Expected error, but last_error was None");
        assert!(
            matches!(
                DeviceError::downcast_from(err),
                Some(DeviceError::InvalidInput(msg)) if msg.contains(expected_substring)
            ),
            "Expected InvalidInput containing '{expected_substring}', got: {err:?}"
        );
    }

    /// BDD Step: Then the specified chips received updates.
    pub fn then_chips_received_updates(&self, expected_chip_ids: &[ChipId]) {
        let actual = self.updated_chips.lock().unwrap();
        assert_eq!(
            actual.as_slice(),
            expected_chip_ids,
            "updated chip IDs mismatch: expected {expected_chip_ids:?}, got {actual:?}"
        );
    }

    /// BDD Step: When I notify that a chip was removed.
    pub async fn when_notify_chip_removed(
        &self,
        device_id: DeviceId,
        chip_id: netsim_model::ChipId,
    ) {
        self.client.notify_chip_removed(device_id, chip_id).await.unwrap();
    }

    /// BDD Step: When I shut down the actor politely to trigger shutdown hooks
    pub async fn when_shutdown_actor(&self) {
        let _ = self.client.shutdown().await;
        self.then_actor_should_shutdown().await;
    }

    /// BDD Step: When I delete the device.
    pub async fn when_delete_device(&self, id: DeviceId) {
        self.client.delete(id).await.unwrap();
    }

    /// BDD Step: When I delete a chip by ID.
    pub async fn when_delete_chip(&self, chip_id: netsim_model::ChipId) {
        self.client.delete_chip(chip_id).await.unwrap();
    }

    /// BDD Step: When I try to delete a chip by ID.
    pub async fn when_try_delete_chip(&self, chip_id: netsim_model::ChipId) {
        let res = self.client.delete_chip(chip_id).await;
        *self.last_error.lock().unwrap() = res.err();
    }

    /// BDD Step: When I add two chips with the same device GUID concurrently
    pub async fn when_concurrently_add_chips(
        &self,
        device_guid: &str,
        chip_name_1: &str,
        chip_name_2: &str,
    ) -> (DeviceId, DeviceId) {
        let client1 = self.client.clone();
        let client2 = self.client.clone();
        let guid1 = device_guid.to_string();
        let guid2 = device_guid.to_string();
        let name1 = chip_name_1.to_string();
        let name2 = chip_name_2.to_string();

        let t1 = tokio::spawn(async move {
            let params =
                Self::create_device_add_chip_params(guid1, name1, "00:00:00:00:00:01".to_string());
            client1.add_chip(params).await.unwrap()
        });

        let t2 = tokio::spawn(async move {
            let params =
                Self::create_device_add_chip_params(guid2, name2, "00:00:00:00:00:02".to_string());
            client2.add_chip(params).await.unwrap()
        });

        let (res1, res2) = tokio::join!(t1, t2);
        (res1.unwrap(), res2.unwrap())
    }

    /// BDD Step: When I add a chip with an injected packet stream.
    /// Returns the sending end of the channel to simulate transport traffic.
    pub async fn when_add_chip_with_stream(
        &self,
        device_guid: &str,
        chip_name: &str,
    ) -> (DeviceId, tokio::sync::mpsc::UnboundedSender<Bytes>) {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let stream = tokio_stream::wrappers::UnboundedReceiverStream::new(rx);
        let boxed_stream: netsim_model::PacketStream = Box::new(stream);
        let sink = futures::sink::drain()
            .sink_map_err(|_| std::io::Error::from(std::io::ErrorKind::Other));
        let boxed_sink: netsim_model::PacketSink = Box::pin(sink);

        let mut params = Self::create_device_add_chip_params(
            device_guid.to_string(),
            chip_name.to_string(),
            "00:00:00:00:00:00".to_string(),
        );
        params.packet_stream = Some(boxed_stream);
        params.packet_sink = Some(boxed_sink);

        let device_id = self.client.add_chip(params).await.unwrap();
        (device_id, tx)
    }

    /// BDD Step: When I add a WiFi chip with an injected packet stream.
    pub async fn when_add_wifi_chip_with_stream(
        &self,
        device_guid: &str,
        chip_name: &str,
    ) -> (DeviceId, tokio::sync::mpsc::UnboundedSender<Bytes>) {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let stream = tokio_stream::wrappers::UnboundedReceiverStream::new(rx);
        let boxed_stream: netsim_model::PacketStream = Box::new(stream);
        let sink = futures::sink::drain()
            .sink_map_err(|_| std::io::Error::from(std::io::ErrorKind::Other));
        let boxed_sink: netsim_model::PacketSink = Box::pin(sink);

        let mut params = Self::create_device_add_chip_params(
            device_guid.to_string(),
            chip_name.to_string(),
            "".to_string(),
        );
        params.chip.kind = netsim_model::ChipKind::WIFI;
        params.chip.variant =
            Some(netsim_model::ChipVariant::Wifi(netsim_model::Wifi { radio: Default::default() }));
        params.packet_stream = Some(boxed_stream);
        params.packet_sink = Some(boxed_sink);

        let device_id = self.client.add_chip(params).await.unwrap();
        (device_id, tx)
    }

    /// Checks if the actor task has finished (e.g. due to shutdown).
    pub fn is_actor_finished(&self) -> bool {
        self._actor_task.is_finished()
    }

    pub async fn then_actor_should_shutdown(&self) {
        let mut found = false;
        for _ in 0..Self::SHUTDOWN_RETRIES {
            if self.is_actor_finished() {
                found = true;
                break;
            }
            tokio::time::sleep(Self::SHUTDOWN_RETRY_INTERVAL).await;
        }
        assert!(found, "Server should have shut down, but did not");
    }

    /// Helper to create DeviceAddChip params with defaults.
    pub fn create_device_add_chip_params(
        device_guid: String,
        chip_name: String,
        chip_address: String,
    ) -> device_api::DeviceAddChip {
        let mut device_config =
            DeviceConfig::new("test-dev".to_string(), true, Pose::default(), false);
        device_config.device_info = Some(netsim_model::DeviceInfo {
            name: "test_device".to_string(),
            ..Default::default()
        });

        device_api::DeviceAddChip {
            device_guid,
            packet_stream: None,
            packet_sink: None,
            device_config,
            chip: netsim_model::Chip {
                name: chip_name,
                manufacturer: "Netsim".to_string(),
                product_name: "NetsimBeacon".to_string(),
                kind: netsim_model::ChipKind::BLUETOOTH,
                variant: Some(netsim_model::ChipVariant::Bluetooth(Box::new(
                    netsim_model::Bluetooth {
                        address: chip_address,
                        mode: netsim_model::BluetoothMode::Device(Default::default()),
                        bt_properties: Default::default(),
                        ..Default::default()
                    },
                ))),
                ..Default::default()
            },
        }
    }

    /// Creates a Bluetooth ChipUpdate with the specified Low Energy and Classic
    /// radio states.
    pub fn create_bluetooth_chip_update(
        le_state: Option<bool>,
        classic_state: Option<bool>,
    ) -> ChipUpdate {
        ChipUpdate {
            variant: Some(ChipVariantUpdate::Bluetooth(BluetoothUpdate {
                low_energy: RadioUpdate { state: le_state },
                classic: RadioUpdate { state: classic_state },
                ..Default::default()
            })),
            ..Default::default()
        }
    }

    const STATS_RETRY_INTERVAL: std::time::Duration = std::time::Duration::from_millis(100);
    const STATS_RW_RETRIES: usize = 50;
    const STATS_ABSENT_RETRIES: usize = 10;
    const SHUTDOWN_RETRY_INTERVAL: std::time::Duration = std::time::Duration::from_millis(100);
    const SHUTDOWN_RETRIES: usize = 50;

    /// Helper to get a unique temporary path for stats.
    pub fn temp_stats_path() -> (std::path::PathBuf, String) {
        let mut path = std::env::temp_dir();
        let unique_id = format!(
            "{}_{:?}_{:?}",
            std::process::id(),
            std::thread::current().id(),
            time::SystemTime::now().duration_since(time::UNIX_EPOCH).unwrap().as_nanos()
        );
        let filename = format!("netsim_session_stats_{}.json", unique_id);
        path.push(&filename);
        if path.exists() {
            let _ = std::fs::remove_file(&path);
        }
        (path, filename)
    }

    /// Helper to read stats file with retry
    pub async fn get_stats_from_file(path: &std::path::PathBuf) -> serde_json::Value {
        let mut last_content = String::new();
        // Wait up to 5 seconds
        for _ in 0..Self::STATS_RW_RETRIES {
            if path.exists()
                && let Ok(c) = std::fs::read_to_string(path)
                && !c.is_empty()
            {
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(&c) {
                    return json;
                }
                last_content = c;
            }
            tokio::time::sleep(Self::STATS_RETRY_INTERVAL).await;
        }
        panic!("Stats file content mismatch or timeout. Last content: {}", last_content);
    }

    /// Verifies the content of the stats file.
    pub async fn verify_stats_file_content(
        path: &std::path::PathBuf,
        expected_version: &str,
        expected_device_count: u32,
        expected_peak_devices: u32,
    ) {
        let json = Self::get_stats_from_file(path).await;
        assert_eq!(json["version"], expected_version, "version mismatch");

        let val = json["device_count"].as_u64().expect("device_count missing");
        assert_eq!(val, expected_device_count as u64, "device_count mismatch");

        let val =
            json["peak_concurrent_devices"].as_u64().expect("peak_concurrent_devices missing");
        assert_eq!(val, expected_peak_devices as u64, "peak_concurrent_devices mismatch");

        let duration = json["duration_secs"].as_u64();

        if duration.is_none() {
            eprintln!("WARNING: duration_secs missing in stats file");
        }
    }

    // ... (rest of methods)

    /// Verifies that radio stats are NOT present for the current device.
    /// This is used to verify that stats are dropped when ambiguous (e.g., dual
    /// mode cleanup fallback).
    pub async fn verify_radio_stats_absent(&self) {
        let device_id = self.current_device_id.expect("No current device set in World");
        let path = self.stats_path.as_ref().expect("Stats path not set in World");

        let mut found_any = false;
        // Wait for potential async flush
        for _ in 0..Self::STATS_ABSENT_RETRIES {
            if path.exists() {
                let content = std::fs::read_to_string(path).unwrap_or_default();
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(&content)
                    && let Some(radio_stats) = json["radio_stats"].as_array()
                {
                    for s in radio_stats {
                        let id = s["device_id"].as_u64().unwrap_or(0);
                        let tx_count = s["tx_count"].as_u64().unwrap_or(0);
                        if id == device_id.0 as u64 && tx_count > 0 {
                            found_any = true;
                        }
                    }
                }
            }
            if found_any {
                break;
            }
            tokio::time::sleep(Self::STATS_RETRY_INTERVAL).await;
        }

        assert!(
            !found_any,
            "Radio stats should be absent for device {}, but were found.",
            device_id
        );
    }

    pub async fn verify_radio_stats_persisted(
        &self,
        expected_tx_bytes: Option<u64>,
        expected_rx_bytes: Option<u64>,
        expected_tx_count: Option<u64>,
        expected_rx_count: Option<u64>,
        expected_kind: Option<&str>,
    ) {
        let device_id = self.current_device_id.expect("No current device set in World");
        let path = self.stats_path.as_ref().expect("Stats path not set in World");

        let mut found = false;
        let mut last_content = String::new();

        // Retry loop to handle async write latency
        for _ in 0..Self::STATS_RW_RETRIES {
            tokio::time::sleep(Self::STATS_RETRY_INTERVAL).await;
            if !path.exists() {
                continue;
            }

            let content = std::fs::read_to_string(path).unwrap_or_default();
            if content.is_empty() {
                continue;
            }
            last_content = content.clone();

            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&content)
                && let Some(radio_stats) = json["radio_stats"].as_array()
            {
                for s in radio_stats {
                    let id = s["device_id"].as_u64().unwrap_or(0);
                    if id != device_id.0 as u64 {
                        continue;
                    }

                    // Check match criteria
                    let tx_bytes = s["tx_bytes"].as_u64();
                    let rx_bytes = s["rx_bytes"].as_u64();
                    let tx_count = s["tx_count"].as_u64();
                    let rx_count = s["rx_count"].as_u64();

                    let mut matches = true;

                    if let Some(expected) = expected_kind {
                        let k_str = s["kind"].as_str();
                        let k_num = s["kind"].as_u64();

                        // Normalize actual kind to string if possible, or keep as is
                        let actual_kind_match = match (k_str, k_num) {
                            (Some(s), _) => s.eq_ignore_ascii_case(expected),
                            (_, Some(n)) => {
                                // strictly match known numbers
                                let mapped = match n {
                                    1 => "BLUETOOTH_LOW_ENERGY",
                                    2 => "BLUETOOTH_CLASSIC",
                                    4 => "WIFI",
                                    5 => "UWB",
                                    _ => "UNSPECIFIED",
                                };
                                mapped.eq_ignore_ascii_case(expected)
                            }
                            _ => false,
                        };
                        if !actual_kind_match {
                            matches = false;
                        }
                    }

                    if let Some(expected) = expected_tx_bytes
                        && tx_bytes != Some(expected)
                    {
                        matches = false;
                    }
                    if let Some(expected) = expected_rx_bytes
                        && rx_bytes != Some(expected)
                    {
                        matches = false;
                    }
                    if let Some(expected) = expected_tx_count
                        && tx_count != Some(expected)
                    {
                        matches = false;
                    }
                    if let Some(expected) = expected_rx_count
                        && rx_count != Some(expected)
                    {
                        matches = false;
                    }

                    if matches {
                        found = true;
                        break;
                    }
                }
            }
            if found {
                break;
            }
        }

        assert!(
            found,
            "Radio stats matching criteria not found for device {}. Last content: {}",
            device_id, last_content
        );
    }

    /// BDD Step: Given a device with a transport stream
    pub async fn given_device_with_transport_stream(&mut self, device_guid: &str, chip_name: &str) {
        let (device_id, tx) = self.when_add_chip_with_stream(device_guid, chip_name).await;
        self.current_device_id = Some(device_id);
        self.transport_tx = Some(tx);

        // Fetch Chip ID
        let device =
            self.client.get(device_id).await.expect("RPC failed").expect("Device not found");
        let chip_id =
            device.chips.first().expect("Device created with transport stream has no chips").id;
        self.current_chip_id = Some(netsim_model::ChipId(chip_id));
    }

    /// BDD Step: Given a WiFi device with a transport stream
    pub async fn given_wifi_device_with_transport_stream(
        &mut self,
        device_guid: &str,
        chip_name: &str,
    ) {
        let (device_id, tx) = self.when_add_wifi_chip_with_stream(device_guid, chip_name).await;
        self.current_device_id = Some(device_id);
        self.transport_tx = Some(tx);

        // Fetch Chip ID
        let device =
            self.client.get(device_id).await.expect("RPC failed").expect("Device not found");
        let chip_id =
            device.chips.first().expect("Device created with transport stream has no chips").id;
        self.current_chip_id = Some(netsim_model::ChipId(chip_id));
    }

    /// BDD Step: Given mock radio stats are primed for the current device
    pub async fn given_radio_stats_primed(&self, kind: netsim_model::RadioKind, tx: u64, rx: u64) {
        let device_id = self.current_device_id.expect("No current device set in World");
        let device =
            self.client.get(device_id).await.expect("RPC failed").expect("Device not found");
        let chip_id = device.chips[0].id;

        self.given_radio_stats(chip_id, kind, tx, rx);
    }

    /// BDD Step: Given mock radio stats are primed
    pub fn given_mock_radio_stats(&self, tx: u64, rx: u64) {
        let device_id = self.current_device_id.expect("No current device set in World").0;
        // Default to Bluetooth Low Energy for generic test if not specified,
        // to avoid UNSPECIFIED in generic tests
        self.given_radio_stats(device_id, netsim_model::RadioKind::BluetoothLowEnergy, tx, rx);
    }

    /// BDD Step: When I send packets to the transport
    pub async fn when_send_packets_to_transport(&self, packet_count: usize, packet_size: usize) {
        let tx = self.transport_tx.as_ref().expect("No transport_tx in World");
        let packet = vec![0u8; packet_size];
        for _ in 0..packet_count {
            tx.send(bytes::Bytes::from(packet.clone()))
                .expect("Failed to send packet to transport");
        }
        // Small delay to allow async propagation (Stream -> Stats -> Chip)
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }

    /// BDD Step: Then radio stats should match expected values
    pub fn then_radio_stats_should_match(&self, expected_tx: u64, expected_rx: u64) {
        let stats = self
            .last_radio_stats
            .as_ref()
            .expect("No radio stats fetched. Call when_fetch_radio_stats() first.");

        let device_id = self.current_device_id.expect("No current device set in World").0;

        // Find stats for our chip
        let s = stats
            .iter()
            .find(|s| s.id == device_id)
            .unwrap_or_else(|| panic!("No stats found for device {}", device_id));

        assert_eq!(s.tx_bytes, expected_tx, "Tx bytes mismatch for device {}", device_id);
        assert_eq!(s.rx_bytes, expected_rx, "Rx bytes mismatch for device {}", device_id);
    }

    /// Helper to setup mock for verifying radio stats
    pub fn given_radio_stats(
        &self,
        device_id: u32,
        kind: netsim_model::RadioKind,
        tx: u64,
        rx: u64,
    ) {
        let mut stats_vec = self.radio_stats.lock().unwrap();
        let mut stats = netsim_model::NetsimRadioStats::default();
        stats.id = device_id;
        stats.kind = kind;
        stats.tx_bytes = tx;
        stats.rx_bytes = rx;
        // Assume nonzero equals 1
        stats.tx_count = if tx > 0 { 1 } else { 0 };
        stats.rx_count = if rx > 0 { 1 } else { 0 };

        // Replace or Append
        if let Some(existing) = stats_vec.iter_mut().find(|s| s.id == device_id) {
            *existing = stats;
        } else {
            stats_vec.push(stats);
        }
    }

    /// Actual implementation of when_get_radio_stats assuming client support
    pub async fn when_fetch_radio_stats(&mut self) {
        match self.client.get_radio_stats().await {
            Ok(stats) => self.last_radio_stats = Some(stats),
            Err(e) => panic!("Failed to get radio stats: {}", e),
        }
    }

    /// BDD Step: Given I have a device configuration
    pub fn given_device_config(
        &mut self,
        name: &str,
        kind: &str,
        version: &str,
        sdk_version: &str,
        build_id: &str,
        variant: &str,
        arch: &str,
    ) {
        let mut config = DeviceConfig::new(name.to_string(), true, Pose::default(), false);
        config.device_info = Some(netsim_model::DeviceInfo {
            name: name.to_string(),
            kind: kind.to_string(),
            version: version.to_string(),
            sdk_version: sdk_version.to_string(),
            build_id: build_id.to_string(),
            variant: variant.to_string(),
            arch: arch.to_string(),
            ..Default::default()
        });
        self.device_config = Some(config);
    }

    /// BDD Step: When I create the device from the pending configuration
    pub async fn when_create_device_from_config(&mut self) {
        let config = self.device_config.take().expect("No pending config");
        let params = DeviceCreate {
            device_config: config,
            chip: DeviceChipCreate {
                name: "beacon".to_string(),
                manufacturer: "Netsim".to_string(),
                product_name: "NetsimBeacon".to_string(),
                chip: device_api::ChipCreateVariant::Beacon(Default::default()),
            },
        };
        let id = self.client.create_device(params).await.unwrap();
        self.current_device_id = Some(id);
    }

    /// BDD Step: Then the stats file should match the device details
    pub async fn then_stats_should_contain_device_details(
        &self,
        kind: &str,
        version: &str,
        sdk_version: &str,
        build_id: &str,
        variant: &str,
        arch: &str,
    ) {
        let path = self.stats_path.as_ref().expect("No stats_path in World");
        let json = Self::get_stats_from_file(path).await;

        let device_stats = json["device_stats"]
            .as_array()
            .expect(&format!("device_stats missing in JSON: {}", json));

        let device_id = self.current_device_id.expect("No current_device_id").0;

        let ds = device_stats.iter().find(|s| {
            let id_val = s["device_id"].as_u64();
            id_val == Some(device_id as u64)
        });

        assert!(ds.is_some(), "Stats for device {} not found in: {:?}", device_id, device_stats);
        let ds = ds.unwrap();

        assert_eq!(ds["kind"], kind);
        assert_eq!(ds["version"], version);
        assert_eq!(ds["sdk_version"].as_str().expect("sdk_version missing"), sdk_version);
        assert_eq!(ds["build_id"].as_str().expect("build_id missing"), build_id);
        assert_eq!(ds["variant"], variant);
        assert_eq!(ds["arch"], arch);
    }

    pub async fn given_wifi_chip(&self, chip_name: &str) -> DeviceId {
        self.when_add_wifi_chip("guid-wifi-detail", chip_name).await
    }

    /// BDD Step: Given mock Wifi stats are primed for a specific chip
    pub fn given_mock_wifi_stats_for_chip(
        &self,
        chip_id: u32,
        stats: netsim_proto::stats::WifiStats,
    ) {
        let mut wifi_stats_map = self.wifi_stats.lock().unwrap();
        wifi_stats_map.insert(chip_id, stats);
    }
    pub async fn then_wifi_stats_should_match<F>(&self, verifier: F)
    where
        F: FnOnce(&serde_json::Value),
    {
        let path = self.stats_file_to_cleanup.as_ref().expect("Stats file path not set");
        let mut found = false;
        let mut last_json = serde_json::Value::Null;

        for _ in 0..Self::STATS_RW_RETRIES {
            let json = Self::get_stats_from_file(path).await;
            if !json["wifi_stats"].is_null() {
                last_json = json;
                found = true;
                break;
            }
            tokio::time::sleep(Self::STATS_RETRY_INTERVAL).await;
        }

        assert!(found, "wifi_stats missing in JSON after retries");
        verifier(&last_json["wifi_stats"]);
    }

    /// BDD Step: Given all devices are modified (visible=false, position
    /// nonzero)
    pub async fn given_all_devices_are_modified(&self) {
        let devices = self
            .client
            .list()
            .await
            .expect("Failed to list devices during given_all_devices_are_modified");
        for device in devices {
            let id = DeviceId(device.id);
            let mut update = DeviceUpdate::default();
            update.id = id.0;
            update.visible = Some(false);
            update.pose.position = Some(device_api::Position { x: 1.0, y: 1.0, z: 1.0 });
            self.client
                .update(id, update)
                .await
                .expect("Failed to update device during given_all_devices_are_modified");
        }
    }

    /// BDD Step: Given a specific device is modified
    pub async fn when_device_is_modified(&self, id: DeviceId) {
        let mut update = DeviceUpdate::default();
        update.id = id.0;
        update.visible = Some(false);
        update.pose.position = Some(device_api::Position { x: 1.0, y: 1.0, z: 1.0 });
        self.client
            .update(id, update)
            .await
            .expect("Failed to update device during when_device_is_modified");
    }

    /// BDD Step: When I call reset (global)
    pub async fn when_reset_is_called(&self) {
        self.client.reset(None).await.expect("Global reset RPC failed");
    }

    /// BDD Step: When I call reset for a specific device
    pub async fn when_reset_device_is_called(&self, id: DeviceId) {
        self.client.reset(Some(id)).await.expect(&format!("Reset RPC failed for device {}", id.0));
    }

    /// BDD Step: Then the link client reset should have been called
    pub fn then_link_reset_was_called(&self) {
        assert!(
            self.link_state.reset_called.load(std::sync::atomic::Ordering::Relaxed),
            "Link reset was NOT called"
        );
    }

    /// BDD Step: Then the link client reset should NOT have been called
    pub fn then_link_reset_was_not_called(&self) {
        assert!(
            !self.link_state.reset_called.load(std::sync::atomic::Ordering::Relaxed),
            "Link reset was called unexpectedly"
        );
    }

    /// BDD Step: Then all device properties should be back to default
    pub async fn then_all_devices_are_reset(&self) {
        let devices = self
            .client
            .list()
            .await
            .expect("Failed to list devices during then_all_devices_are_reset");
        for device in devices {
            assert!(device.visible, "Device {} should be visible after reset", device.id);
            assert_eq!(
                device.pose.position,
                device_api::Position::default(),
                "Device {} position mismatch",
                device.id
            );
        }
    }

    /// BDD Step: Then a specific device should exist in the list
    pub async fn then_device_exists(&self, id: DeviceId) {
        let devices = self.client.list().await.unwrap();
        assert!(devices.iter().any(|d| d.id == id.0), "Device {} should exist in the list", id.0);
    }

    /// BDD Step: Then a specific device should NOT exist in the list
    pub async fn then_device_does_not_exist(&self, id: DeviceId) {
        let devices = self.client.list().await.unwrap();
        assert!(
            !devices.iter().any(|d| d.id == id.0),
            "Device {} should NOT exist in the list",
            id.0
        );
    }

    /// BDD Step: Then fetching a specific device by ID returns None.
    pub async fn then_device_get_is_none(&self, id: DeviceId) {
        assert!(
            self.client.get(id).await.unwrap().is_none(),
            "Device {} should not be retrievable via get()",
            id.0
        );
    }

    /// BDD Step: Then the mock chip actor presence matches expected.
    pub fn then_mock_chip_present(&self, chip_id: netsim_model::ChipId, expected: bool) {
        let present = self.mock_chips.lock().unwrap().contains_key(&chip_id);
        assert_eq!(
            present, expected,
            "Mock chip {chip_id} presence mismatch: expected {expected}, got {present}"
        );
    }

    /// BDD Step: Then the device contains a chip matching ID, name, and kind.
    pub async fn then_device_has_chip(
        &self,
        device_id: DeviceId,
        chip_id: netsim_model::ChipId,
        expected_name: &str,
        expected_kind: netsim_model::ChipKind,
    ) {
        let device = self
            .client
            .get(device_id)
            .await
            .unwrap()
            .unwrap_or_else(|| panic!("Device {} not found", device_id.0));
        let chip = device
            .chips
            .iter()
            .find(|c| c.id == chip_id.0)
            .unwrap_or_else(|| panic!("Chip {chip_id} not found on device {}", device_id.0));
        assert_eq!(chip.id, chip_id.0);
        assert_eq!(chip.name, expected_name);
        assert_eq!(chip.kind, expected_kind);
    }

    /// BDD Step: Then the last error is NotFound containing the expected substring.
    pub fn then_error_is_not_found(&self, expected_substring: &str) {
        let err_guard = self.last_error.lock().unwrap();
        let err = err_guard.as_ref().expect("Expected error, but last_error was None");
        assert!(
            matches!(
                DeviceError::downcast_from(err),
                Some(DeviceError::NotFound(msg)) if msg.contains(expected_substring)
            ),
            "Expected NotFound containing '{expected_substring}', got: {err:?}"
        );
    }

    /// BDD Step: Then a specific device should be reset
    pub async fn then_device_properties_are_reset(&self, id: DeviceId) {
        let device = self
            .client
            .get(id)
            .await
            .expect("RPC get() failed during then_device_properties_are_reset")
            .expect("Device not found during then_device_properties_are_reset");
        assert!(device.visible, "Device {} should be visible after reset", id.0);
        assert_eq!(
            device.pose.position,
            device_api::Position::default(),
            "Device {} position mismatch",
            id.0
        );
    }

    /// BDD Step: Then a specific device's Bluetooth states should match
    pub async fn then_bluetooth_states_are(&self, id: DeviceId, le: bool, classic: bool) {
        let device = self
            .client
            .get(id)
            .await
            .expect("RPC get() failed during then_bluetooth_states_are")
            .expect("Device not found during then_bluetooth_states_are");
        let chip =
            device.chips.first().expect("Device has no chips during then_bluetooth_states_are");
        if let Some(netsim_model::ChipVariant::Bluetooth(bt)) = &chip.variant {
            assert_eq!(bt.low_energy.state, Some(le), "LE state mismatch for device {}", id.0);
            assert_eq!(
                bt.classic.state,
                Some(classic),
                "Classic state mismatch for device {}",
                id.0
            );
        } else {
            panic!("Not a Bluetooth chip");
        }
    }

    /// BDD Step: Then a specific device and its chips should match properties
    pub async fn then_device_and_chips_position_and_orientation_match(
        &self,
        id: DeviceId,
        position: device_api::Position,
        orientation: device_api::Orientation,
    ) {
        let device = self
            .client
            .get(id)
            .await
            .expect("RPC get() failed during then_device_and_chips_position_and_orientation_match")
            .expect("Device not found during then_device_and_chips_position_and_orientation_match");

        assert_eq!(
            device.pose.position, position,
            "Device {} position mismatch: expected {:?}, got {:?}",
            id.0, position, device.pose.position
        );
        assert_eq!(
            device.pose.orientation, orientation,
            "Device {} orientation mismatch: expected {:?}, got {:?}",
            id.0, orientation, device.pose.orientation
        );

        for chip in &device.chips {
            assert_eq!(
                chip.pose.position, position,
                "Chip {} (device {}) position mismatch: expected {:?}, got {:?}",
                chip.id, id.0, position, chip.pose.position
            );
            assert_eq!(
                chip.pose.orientation, orientation,
                "Chip {} (device {}) orientation mismatch: expected {:?}, got {:?}",
                chip.id, id.0, orientation, chip.pose.orientation
            );
        }
    }

    /// BDD Step: Then all chip actors for a device should match position and
    /// orientation
    pub async fn then_all_chip_actors_position_and_orientation_match(
        &self,
        device_id: DeviceId,
        position: device_api::Position,
        orientation: device_api::Orientation,
    ) {
        let device =
            self.client.get(device_id).await.expect("RPC get() failed").expect("Device not found");

        let mock_chips = self.mock_chips.lock().unwrap();
        for device_chip in &device.chips {
            let chip_id = netsim_model::ChipId(device_chip.id);
            let chip = mock_chips
                .get(&chip_id)
                .expect(&format!("Chip {} not found in mock_chips", chip_id.0));

            assert_eq!(
                chip.pose.position, position,
                "Chip Actor {} position mismatch: expected {:?}, got {:?}",
                chip_id.0, position, chip.pose.position
            );
            assert_eq!(
                chip.pose.orientation, orientation,
                "Chip Actor {} orientation mismatch: expected {:?}, got {:?}",
                chip_id.0, orientation, chip.pose.orientation
            );
        }
    }

    /// BDD Step: Given a device with a named chip of the specified variant.
    pub async fn given_device_with_chip(
        &self,
        device_name: &str,
        chip_name: &str,
        chip_variant: device_api::ChipCreateVariant,
    ) -> DeviceId {
        let params = DeviceCreate {
            device_config: DeviceConfig::new(device_name.to_string(), true, Pose::default(), false),
            chip: DeviceChipCreate {
                name: chip_name.to_string(),
                manufacturer: DEFAULT_MANUFACTURER.to_string(),
                product_name: "Netsim".to_string(),
                chip: chip_variant,
            },
        };
        self.client.create_device(params).await.unwrap()
    }

    /// BDD Constructor: Given a running Device Actor with default passthrough Bluetooth and Wi-Fi mocks.
    pub async fn given_with_passthrough_bluetooth_and_wifi_mocks() -> Self {
        Self::new().await
    }

    /// BDD Constructor: Given a running Device Actor with chip client configured to accept metadata updates.
    pub async fn given_with_bluetooth_metadata_mock() -> Self {
        let mut mock_chip_client = MockChipClient::new();
        mock_chip_client.expect_create().returning(|_, _| Ok(()));
        mock_chip_client
            .expect_update()
            .withf(|_, patch| {
                patch.name.is_none()
                    && patch.manufacturer.as_deref() == Some(UPDATED_MANUFACTURER)
                    && patch.product_name.as_deref() == Some(UPDATED_PRODUCT_NAME)
            })
            .returning(|id, patch| {
                Ok(netsim_model::Chip {
                    id: id.0,
                    kind: ChipKind::BLUETOOTH,
                    name: "beacon".to_string(),
                    manufacturer: patch.manufacturer.unwrap_or_default(),
                    product_name: patch.product_name.unwrap_or_default(),
                    ..Default::default()
                })
            });

        let mut chip_clients = HashMap::new();
        chip_clients.insert(ChipKind::BLUETOOTH, Box::new(mock_chip_client) as Box<dyn ChipClient>);
        let mock_link_client = Self::create_default_link_client();
        Self::with_clients(chip_clients, mock_link_client).await
    }

    /// BDD Constructor: Given a running Device Actor with a chip client configured to fail on update.
    pub async fn given_with_failing_bluetooth_update_mock() -> Self {
        let mut mock_chip_client = MockChipClient::new();
        mock_chip_client.expect_create().returning(|_, _| Ok(()));
        mock_chip_client
            .expect_update()
            .returning(|_, _| Err(netsim_model::ClientError::Send("transport error".to_string())));

        let mut chip_clients = HashMap::new();
        chip_clients.insert(ChipKind::BLUETOOTH, Box::new(mock_chip_client) as Box<dyn ChipClient>);
        let mock_link_client = Self::create_default_link_client();
        Self::with_clients(chip_clients, mock_link_client).await
    }

    /// BDD Step: Given a created device.
    pub async fn given_device_created(&self, device_name: &str) -> DeviceId {
        self.when_create_device(device_name).await
    }

    /// BDD Step: Given a chip added to an existing device.
    pub async fn given_chip_added_to_device(
        &self,
        device_id: DeviceId,
        chip_name: &str,
        chip_variant: device_api::ChipCreateVariant,
    ) -> netsim_model::ChipId {
        self.when_add_chip_variant(device_id, chip_name, chip_variant).await.unwrap()
    }

    /// BDD Step: Given ID of the first chip on device.
    pub async fn given_first_chip_id(&self, device_id: DeviceId) -> netsim_model::ChipId {
        let device = self.client.get(device_id).await.unwrap().unwrap();
        netsim_model::ChipId(device.chips[0].id)
    }

    /// BDD Step: Given recorded last modified timestamp.
    pub async fn given_recorded_last_modified(&self) -> std::time::SystemTime {
        self.client.get_last_modified().await.unwrap()
    }

    /// BDD Step: When I add a chip with the specified variant to an existing device.
    pub async fn when_add_chip_variant(
        &self,
        device_id: DeviceId,
        chip_name: &str,
        chip_variant: device_api::ChipCreateVariant,
    ) -> Result<netsim_model::ChipId, netsim_model::ClientError> {
        let extra_chip = DeviceChipCreate {
            name: chip_name.to_string(),
            manufacturer: "Netsim".to_string(),
            product_name: "Netsim".to_string(),
            chip: chip_variant,
        };
        self.client.add_chip_to_device(device_id, extra_chip).await
    }

    /// BDD Step: When I try to add a chip with the specified variant to an existing device.
    pub async fn when_try_add_chip_variant(
        &self,
        device_id: DeviceId,
        chip_name: &str,
        chip_variant: device_api::ChipCreateVariant,
    ) {
        let res = self.when_add_chip_variant(device_id, chip_name, chip_variant).await;
        *self.last_error.lock().unwrap() = res.err();
    }

    /// BDD Step: When I attempt to create a device with a named chip.
    pub async fn when_try_create_device_with_chip(
        &self,
        device_name: &str,
        chip_name: &str,
        chip_variant: device_api::ChipCreateVariant,
    ) {
        let params = DeviceCreate {
            device_config: DeviceConfig::new(device_name.to_string(), true, Pose::default(), false),
            chip: DeviceChipCreate {
                name: chip_name.to_string(),
                manufacturer: "Netsim".to_string(),
                product_name: "Netsim".to_string(),
                chip: chip_variant,
            },
        };
        let res = self.client.create_device(params).await;
        *self.last_error.lock().unwrap() = res.err();
    }

    /// BDD Step: When I patch chip metadata on device.
    pub async fn when_patch_chip_metadata(
        &self,
        device_id: DeviceId,
        chip_name: &str,
        manufacturer: &str,
        product_name: &str,
    ) {
        let mut update = DeviceUpdate::default();
        update.id = device_id.0;
        update.chips = Some(vec![ChipUpdate {
            name: Some(chip_name.to_string()),
            manufacturer: Some(manufacturer.to_string()),
            product_name: Some(product_name.to_string()),
            ..Default::default()
        }]);
        self.when_update_device(device_id, update).await;
    }

    /// BDD Step: When I patch chip manufacturer on device.
    pub async fn when_patch_chip_manufacturer(
        &self,
        device_id: DeviceId,
        chip_name: &str,
        manufacturer: &str,
    ) {
        let mut update = DeviceUpdate::default();
        update.id = device_id.0;
        update.chips = Some(vec![ChipUpdate {
            name: Some(chip_name.to_string()),
            manufacturer: Some(manufacturer.to_string()),
            ..Default::default()
        }]);
        self.when_update_device(device_id, update).await;
    }

    /// BDD Step: When I try to patch a chip by name.
    pub async fn when_try_patch_chip_by_name(&self, device_id: DeviceId, chip_name: &str) {
        let mut update = DeviceUpdate::default();
        update.id = device_id.0;
        update.chips =
            Some(vec![ChipUpdate { name: Some(chip_name.to_string()), ..Default::default() }]);
        let res = self.client.update(device_id, update).await;
        *self.last_error.lock().unwrap() = res.err();
    }

    /// BDD Step: When I patch chip with variant filter and manufacturer.
    pub async fn when_patch_chip_variant_manufacturer(
        &self,
        device_id: DeviceId,
        chip_name: &str,
        variant: ChipVariantUpdate,
        manufacturer: &str,
    ) {
        let mut update = DeviceUpdate::default();
        update.id = device_id.0;
        update.chips = Some(vec![ChipUpdate {
            name: Some(chip_name.to_string()),
            variant: Some(variant),
            manufacturer: Some(manufacturer.to_string()),
            ..Default::default()
        }]);
        self.when_update_device(device_id, update).await;
    }

    /// BDD Step: When I try to patch Bluetooth chip properties.
    pub async fn when_try_patch_bluetooth_chip(
        &self,
        device_id: DeviceId,
        low_energy: Option<bool>,
        classic: Option<bool>,
    ) {
        let mut update = DeviceUpdate::default();
        update.id = device_id.0;
        update.chips = Some(vec![Self::create_bluetooth_chip_update(low_energy, classic)]);
        let res = self.client.update(device_id, update).await;
        *self.last_error.lock().unwrap() = res.err();
    }

    /// BDD Step: When I try to patch a chip variant by ID.
    pub async fn when_try_patch_chip_variant_by_id(
        &self,
        device_id: DeviceId,
        chip_id: netsim_model::ChipId,
        variant: ChipVariantUpdate,
    ) {
        let mut update = DeviceUpdate::default();
        update.id = device_id.0;
        update.chips = Some(vec![ChipUpdate {
            id: Some(chip_id),
            variant: Some(variant),
            ..Default::default()
        }]);
        let res = self.client.update(device_id, update).await;
        *self.last_error.lock().unwrap() = res.err();
    }

    /// BDD Step: When I patch device pose and chip.
    pub async fn when_patch_device_pose_and_chip(
        &self,
        device_id: DeviceId,
        x: f32,
        y: f32,
        z: f32,
        yaw: f32,
        chip_name: &str,
        manufacturer: &str,
    ) {
        let mut update = DeviceUpdate::default();
        update.id = device_id.0;
        update.pose = device_api::PoseUpdate {
            position: Some(device_api::Position { x, y, z }),
            orientation: Some(device_api::Orientation { yaw, pitch: 0.0, roll: 0.0 }),
        };
        update.chips = Some(vec![ChipUpdate {
            name: Some(chip_name.to_string()),
            manufacturer: Some(manufacturer.to_string()),
            ..Default::default()
        }]);
        self.when_update_device(device_id, update).await;
    }

    /// BDD Step: Then a device and all its chips have the specified position and yaw.
    pub async fn then_device_and_chips_pose_is(
        &self,
        device_id: DeviceId,
        x: f32,
        y: f32,
        z: f32,
        yaw: f32,
    ) {
        self.then_device_and_chips_position_and_orientation_match(
            device_id,
            device_api::Position { x, y, z },
            device_api::Orientation { yaw, pitch: 0.0, roll: 0.0 },
        )
        .await;
    }

    /// BDD Step: When patching device pose while echoing a stale chip offset pose.
    pub async fn when_patch_device_pose_with_stale_chip_offset(
        &self,
        device_id: DeviceId,
        chip_name: &str,
        x: f32,
        y: f32,
        z: f32,
        yaw: f32,
    ) {
        let mut update = DeviceUpdate::default();
        update.id = device_id.0;
        update.pose = device_api::PoseUpdate {
            position: Some(device_api::Position { x, y, z }),
            orientation: Some(device_api::Orientation { yaw, pitch: 0.0, roll: 0.0 }),
        };
        update.chips = Some(vec![ChipUpdate {
            name: Some(chip_name.to_string()),
            manufacturer: Some(MATCHED_MANUFACTURER.to_string()),
            pose: netsim_model::PoseUpdate {
                position: Some(netsim_model::Position { x: 1.0, y: 2.0, z: 3.0 }),
                orientation: Some(netsim_model::Orientation { yaw: 0.5, pitch: 0.0, roll: 0.0 }),
            },
            ..Default::default()
        }]);
        self.when_update_device(device_id, update).await;
    }

    /// BDD Step: Then the last error is ChipNotFound.
    pub fn then_error_is_chip_not_found(&self, expected_chip: &str, expected_device: &str) {
        let err_guard = self.last_error.lock().unwrap();
        let err = err_guard.as_ref().expect("Expected error, but last_error was None");
        assert!(
            matches!(
                DeviceError::downcast_from(err),
                Some(DeviceError::ChipNotFound(name, dev)) if name == expected_chip && dev == expected_device
            ),
            "Expected ChipNotFound({expected_chip}, {expected_device}), got: {err:?}"
        );
    }

    /// BDD Step: Then the last error is AmbiguousMatch.
    pub fn then_error_is_ambiguous_match(&self, expected_name: &str) {
        let err_guard = self.last_error.lock().unwrap();
        let err = err_guard.as_ref().expect("Expected error, but last_error was None");
        assert!(
            matches!(
                DeviceError::downcast_from(err),
                Some(DeviceError::AmbiguousMatch(name)) if name == expected_name
            ),
            "Expected AmbiguousMatch({expected_name}), got: {err:?}"
        );
    }

    /// BDD Step: Then the last error is AmbiguousEmptyMatch.
    pub fn then_error_is_ambiguous_empty_match(&self, expected_kind: ChipKind) {
        let err_guard = self.last_error.lock().unwrap();
        let err = err_guard.as_ref().expect("Expected error, but last_error was None");
        assert!(
            matches!(
                DeviceError::downcast_from(err),
                Some(DeviceError::AmbiguousEmptyMatch { kind }) if *kind == expected_kind
            ),
            "Expected AmbiguousEmptyMatch({expected_kind:?}), got: {err:?}"
        );
    }

    /// BDD Step: Then the last error is MismatchedChipKind.
    pub fn then_error_is_mismatched_chip_kind(&self, expected: ChipKind, actual: ChipKind) {
        let err_guard = self.last_error.lock().unwrap();
        let err = err_guard.as_ref().expect("Expected error, but last_error was None");
        assert!(
            matches!(
                DeviceError::downcast_from(err),
                Some(DeviceError::MismatchedChipKind { expected: e, actual: a }) if *e == expected && *a == actual
            ),
            "Expected MismatchedChipKind({expected:?}, {actual:?}), got: {err:?}"
        );
    }

    /// BDD Step: Then the last error is DuplicateChip.
    pub fn then_error_is_duplicate_chip(&self, expected_name: &str) {
        let err_guard = self.last_error.lock().unwrap();
        let err = err_guard.as_ref().expect("Expected error, but last_error was None");
        assert!(
            matches!(
                DeviceError::downcast_from(err),
                Some(DeviceError::DuplicateChip { name, .. }) if name == expected_name
            ),
            "Expected DuplicateChip({expected_name}), got: {err:?}"
        );
    }

    /// BDD Step: Then the last error is AlreadyExists.
    pub fn then_error_is_already_exists(&self, expected_name: &str) {
        let err_guard = self.last_error.lock().unwrap();
        let err = err_guard.as_ref().expect("Expected error, but last_error was None");
        assert!(
            matches!(
                DeviceError::downcast_from(err),
                Some(DeviceError::AlreadyExists(name)) if name == expected_name
            ),
            "Expected AlreadyExists({expected_name}), got: {err:?}"
        );
    }

    /// BDD Step: Then the last error is ActorCommunicationError.
    pub fn then_error_is_actor_communication(&self) {
        let err_guard = self.last_error.lock().unwrap();
        let err = err_guard.as_ref().expect("Expected error, but last_error was None");
        assert!(
            matches!(
                DeviceError::downcast_from(err),
                Some(DeviceError::ActorCommunicationError(_))
            ),
            "Expected ActorCommunicationError, got: {err:?}"
        );
    }

    /// BDD Step: Then the device's last modified timestamp matches expected.
    pub async fn then_last_modified_is(&self, expected: std::time::SystemTime) {
        let ts = self.client.get_last_modified().await.unwrap();
        assert_eq!(ts, expected);
    }

    /// BDD Step: Then a chip's manufacturer matches expected.
    pub async fn then_chip_manufacturer_is(
        &self,
        device_id: DeviceId,
        chip_name: &str,
        expected: &str,
    ) {
        let device = self.client.get(device_id).await.unwrap().unwrap();
        let chip = device.chips.iter().find(|c| c.name == chip_name).unwrap();
        assert_eq!(chip.manufacturer, expected);
    }

    /// BDD Step: Then the chip count on device matches expected.
    pub async fn then_chip_count_is(&self, device_id: DeviceId, expected_len: usize) {
        let device = self.client.get(device_id).await.unwrap().unwrap();
        assert_eq!(device.chips.len(), expected_len);
    }

    /// BDD Step: Then the chip metadata matches expected.
    pub async fn then_chip_metadata_is(
        &self,
        device_id: DeviceId,
        chip_name: &str,
        expected_mfr: &str,
        expected_product: &str,
    ) {
        let device = self.client.get(device_id).await.unwrap().unwrap();
        let chip = device.chips.iter().find(|c| c.name == chip_name).unwrap();
        assert_eq!(chip.manufacturer, expected_mfr);
        assert_eq!(chip.product_name, expected_product);
    }

    /// BDD Step: Then the chip of specified kind has the expected manufacturer.
    pub async fn then_chip_of_kind_manufacturer_is(
        &self,
        device_id: DeviceId,
        kind: ChipKind,
        expected_mfr: &str,
    ) {
        let device = self.client.get(device_id).await.unwrap().unwrap();
        let chip = device.chips.iter().find(|c| c.kind == kind).unwrap();
        assert_eq!(chip.manufacturer, expected_mfr);
    }

    /// BDD Step: Then the first chip of specified kind has the expected name.
    pub async fn then_first_chip_of_kind_name_is(
        &self,
        device_id: DeviceId,
        kind: ChipKind,
        expected_name: &str,
    ) {
        let device = self.client.get(device_id).await.unwrap().unwrap();
        let chip = device.chips.iter().find(|c| c.kind == kind).unwrap();
        assert_eq!(chip.name, expected_name);
    }

    /// BDD Step: Then the chip with specified ID has the expected name.
    pub async fn then_chip_name_by_id_is(
        &self,
        device_id: DeviceId,
        chip_id: netsim_model::ChipId,
        expected_name: &str,
    ) {
        let device = self.client.get(device_id).await.unwrap().unwrap();
        let chip = device.chips.iter().find(|c| c.id == chip_id.0).unwrap();
        assert_eq!(chip.name, expected_name);
    }
}
