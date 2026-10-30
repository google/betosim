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

//! BDD integration tests for BLE beacon create, patch, and remove over the
//! `/v1/devices` HTTP REST API (`b/566532200`).

use std::{
    collections::HashMap,
    io::{Read, Write},
    net::TcpListener,
    sync::{Arc, Mutex, OnceLock, atomic::AtomicU32},
    thread::JoinHandle,
};

use ap_actor::{ApActor, ApClient, SharedKeyStore};
use device_actor::{DeviceActor, DeviceClient};
use http_body_util::BodyExt;
use hyper::{Method, StatusCode};
use netsim_model::{
    AdvertiseData, AdvertiseMode, AdvertiseSettings, AdvertiseTxPower, BeaconParams, BleBeacon,
    BluetoothMode, Chip, ChipClient, ChipCreate, ChipCreateVariant, ChipError, ChipId, ChipKind,
    ChipUpdate, ChipVariant, ClientError, Device, DeviceChipCreate, DeviceConfig, DeviceCreate,
    DeviceUpdate, FrontendStats, Interval, NetsimRadioStats, Pose, Service, TxPower,
};
use netsim_rest_api::{ListDeviceResponse, NetsimRestClient, RestError, RestServer};

use crate::server_test::MockLinkClient;

/// Validates a colon-separated 6-octet MAC address (`xx:xx:xx:xx:xx:xx`).
fn is_valid_mac_address(addr: &str) -> bool {
    let parts: Vec<&str> = addr.split(':').collect();
    if parts.len() != 6 {
        return false;
    }
    parts.iter().all(|part| part.len() == 2 && u8::from_str_radix(part, 16).is_ok())
}

/// Real in-memory `ChipClient` for `ChipKind::BLUETOOTH` that models BLE beacon
/// creation, MAC address validation, duplicate chip detection, default name
/// synchronization to `DeviceActor` (matching `BluetoothActor::sync_device_name`),
/// updates, and deletion.
#[derive(Clone, Debug)]
struct BluetoothBeaconTestChip {
    chips: Arc<Mutex<HashMap<ChipId, Chip>>>,
    device_client: Arc<OnceLock<DeviceClient>>,
}

impl BluetoothBeaconTestChip {
    fn new(device_client: Arc<OnceLock<DeviceClient>>) -> Self {
        Self { chips: Arc::new(Mutex::new(HashMap::new())), device_client }
    }
}

#[async_trait::async_trait]
impl ChipClient for BluetoothBeaconTestChip {
    async fn create(&self, id: ChipId, params: ChipCreate) -> Result<(), ClientError> {
        let mut chip = params.chip;
        chip.id = id.0;

        let is_default_name = chip.name.is_empty();
        if is_default_name {
            chip.name = format!("Beacon-{}", id.0);
        }

        {
            let mut chips = self.chips.lock().expect("lock chips");
            if chips.values().any(|existing| existing.name == chip.name) {
                return Err(ClientError::Chip(ChipError::ChipExists(id.0)));
            }

            if let Some(ChipVariant::Bluetooth(ref mut bt)) = chip.variant {
                let raw_addr = if !bt.address.is_empty() {
                    bt.address.clone()
                } else if let BluetoothMode::Beacon(ref beacon_params) = bt.mode {
                    beacon_params.ble_beacon.address.clone()
                } else {
                    String::new()
                };

                let resolved_addr = if raw_addr.is_empty() {
                    format!("00:00:00:00:00:{:02x}", id.0 & 0xff)
                } else {
                    if !is_valid_mac_address(&raw_addr) {
                        return Err(ClientError::Chip(ChipError::InvalidAddress(Box::from(
                            format!("Invalid MAC address: {raw_addr}"),
                        ))));
                    }
                    raw_addr
                };

                let addr_in_use = chips.values().any(|existing| {
                    matches!(
                        &existing.variant,
                        Some(ChipVariant::Bluetooth(existing_bt))
                            if existing_bt.address == resolved_addr
                    )
                });
                if addr_in_use {
                    return Err(ClientError::Chip(ChipError::ChipExists(id.0)));
                }

                bt.address = resolved_addr.clone();
                bt.low_energy.state = Some(true);
                bt.classic.state = Some(false);
                if let BluetoothMode::Beacon(ref mut beacon_params) = bt.mode {
                    beacon_params.ble_beacon.address = resolved_addr;
                }
            }

            chips.insert(id, chip.clone());
        }

        if is_default_name {
            if let Some(dc) = self.device_client.get().cloned() {
                let device_id = chip.device_id;
                let default_name = chip.name;
                tokio::spawn(async move {
                    let update = DeviceUpdate {
                        id: device_id.0,
                        name: Some(default_name),
                        ..Default::default()
                    };
                    let _ = dc.update(device_id, update).await;
                });
            }
        }

        Ok(())
    }

    async fn read(&self, id: ChipId) -> Result<Chip, ClientError> {
        let chips = self.chips.lock().expect("lock chips");
        chips.get(&id).cloned().ok_or(ClientError::Chip(ChipError::ChipNotFound(id)))
    }

    async fn update(&self, id: ChipId, patch: ChipUpdate) -> Result<Chip, ClientError> {
        let mut chips = self.chips.lock().expect("lock chips");
        if !chips.contains_key(&id) {
            return Err(ClientError::Chip(ChipError::ChipNotFound(id)));
        }

        if let Some(netsim_model::ChipVariantUpdate::Bluetooth(ref bt_patch)) = patch.variant {
            if let Some(ref beacon_patch) = bt_patch.ble_beacon {
                if !beacon_patch.address.is_empty() {
                    if !is_valid_mac_address(&beacon_patch.address) {
                        return Err(ClientError::Chip(ChipError::InvalidAddress(Box::from(
                            format!("Invalid MAC address: {}", beacon_patch.address),
                        ))));
                    }
                    let addr_in_use = chips.iter().any(|(existing_id, existing)| {
                        *existing_id != id
                            && matches!(
                                &existing.variant,
                                Some(ChipVariant::Bluetooth(existing_bt))
                                    if existing_bt.address == beacon_patch.address
                            )
                    });
                    if addr_in_use {
                        return Err(ClientError::Chip(ChipError::ChipExists(id.0)));
                    }
                }
            }
        }

        let chip = chips.get_mut(&id).expect("chip verified present");
        patch.apply(chip);
        Ok(chip.clone())
    }

    async fn delete(&self, id: ChipId) -> Result<(), ClientError> {
        let mut chips = self.chips.lock().expect("lock chips");
        chips.remove(&id).map(|_| ()).ok_or(ClientError::Chip(ChipError::ChipNotFound(id)))
    }

    async fn read_statistics(&self) -> Result<Box<[NetsimRadioStats]>, ClientError> {
        Ok(Box::new([]))
    }

    async fn read_count_for_testing(&self) -> Result<usize, ClientError> {
        let chips = self.chips.lock().expect("lock chips");
        Ok(chips.len())
    }

    async fn shutdown(&self) -> Result<(), ClientError> {
        Ok(())
    }

    async fn reset(&self, id: ChipId) -> Result<Chip, ClientError> {
        self.read(id).await
    }

    fn clone_box(&self) -> Box<dyn ChipClient> {
        Box::new(self.clone())
    }
}

/// BDD World harness for testing `/v1/devices` beacon REST operations against
/// `RestServer` backed by a real `DeviceActor`.
struct BeaconRestWorld {
    server: RestServer,
    last_status: Option<StatusCode>,
    last_body: Vec<u8>,
    _device_task: tokio::task::JoinHandle<()>,
    _ap_task: tokio::task::JoinHandle<()>,
}

impl Drop for BeaconRestWorld {
    fn drop(&mut self) {
        self._device_task.abort();
        self._ap_task.abort();
    }
}

impl BeaconRestWorld {
    fn new() -> Self {
        let shared_device_client = Arc::new(OnceLock::new());
        let bt_chip_client = BluetoothBeaconTestChip::new(shared_device_client.clone());
        let mut chip_clients: HashMap<ChipKind, Box<dyn ChipClient>> = HashMap::new();
        chip_clients.insert(ChipKind::BLUETOOTH, Box::new(bt_chip_client));

        let (device_runner, device_client) = device_actor::new();
        let _ = shared_device_client.set(device_client.clone());
        let mut device_actor = DeviceActor::new(
            chip_clients,
            Arc::new(AtomicU32::new(1)),
            None,
            Box::new(MockLinkClient),
            None,
            None,
            "0.3.92".to_string(),
            None,
            None,
            Arc::new(FrontendStats::default()),
        );
        device_actor.set_self_client(device_client.clone());
        let device_task = tokio::spawn(device_runner.run(device_actor));

        let shared_keys = Arc::new(SharedKeyStore::new());
        let ap_actor = ApActor::new(shared_keys);
        let (ap_runner, ap_client_base) = actor_framework::ResourceActor::new(32);
        let ap_client = ApClient::new(ap_client_base);
        let ap_task = tokio::spawn(ap_runner.run(ap_actor));

        let link_client = Arc::new(MockLinkClient);
        let (_, capture_client) = capture_actor::new();

        let server =
            RestServer::new(device_client, link_client, ap_client, capture_client, "0.3.92".into());

        Self {
            server,
            last_status: None,
            last_body: Vec::new(),
            _device_task: device_task,
            _ap_task: ap_task,
        }
    }

    fn sample_beacon_create_request(
        device_name: &str,
        chip_name: &str,
        address: &str,
    ) -> DeviceCreate {
        let ble_beacon = BleBeacon {
            address: address.to_string(),
            settings: Some(AdvertiseSettings {
                interval: Some(Interval::AdvertiseMode(AdvertiseMode::LowLatency)),
                tx_power: Some(TxPower::TxPowerLevel(AdvertiseTxPower::High)),
                scannable: true,
                timeout: 5000,
            }),
            adv_data: Some(AdvertiseData {
                include_device_name: true,
                include_tx_power_level: true,
                manufacturer_data: vec![0x01, 0x02],
                services: vec![Service { uuid: "180D".to_string(), data: vec![0xAB] }],
            }),
            scan_response: Some(AdvertiseData {
                include_device_name: false,
                include_tx_power_level: false,
                manufacturer_data: vec![0x03, 0x04],
                services: vec![],
            }),
        };

        DeviceCreate {
            device_config: DeviceConfig::new(device_name, true, Pose::default(), false),
            chip: DeviceChipCreate::new(
                chip_name,
                "Google",
                "Netsim BLE Beacon",
                ChipCreateVariant::Beacon(ble_beacon),
            ),
        }
    }

    async fn given_created_beacon(
        &mut self,
        device_name: &str,
        chip_name: &str,
        address: &str,
    ) -> Device {
        let req = Self::sample_beacon_create_request(device_name, chip_name, address);
        self.when_create_beacon(&req).await;
        self.then_status_is(StatusCode::CREATED);
        self.then_response_device_matches_beacon(device_name, chip_name, AdvertiseTxPower::High)
    }

    async fn when_post_device(&mut self, body: Vec<u8>) {
        let resp = self.server.handle_request(&Method::POST, "/v1/devices", &body).await;
        self.last_status = Some(resp.status());
        self.last_body =
            resp.into_body().collect().await.expect("collect response body").to_bytes().to_vec();
    }

    async fn when_create_beacon(&mut self, req: &DeviceCreate) {
        let body = serde_json::to_vec(req).expect("serialize DeviceCreate");
        self.when_post_device(body).await;
    }

    async fn when_patch_device(&mut self, id_segment: &str, body: Vec<u8>) {
        let path = format!("/v1/devices/{id_segment}");
        let resp = self.server.handle_request(&Method::PATCH, &path, &body).await;
        self.last_status = Some(resp.status());
        self.last_body =
            resp.into_body().collect().await.expect("collect response body").to_bytes().to_vec();
    }

    async fn when_delete_device(&mut self, id_segment: &str) {
        let path = format!("/v1/devices/{id_segment}");
        let resp = self.server.handle_request(&Method::DELETE, &path, &[]).await;
        self.last_status = Some(resp.status());
        self.last_body =
            resp.into_body().collect().await.expect("collect response body").to_bytes().to_vec();
    }

    async fn when_get_devices(&mut self) {
        let resp = self.server.handle_request(&Method::GET, "/v1/devices", &[]).await;
        self.last_status = Some(resp.status());
        self.last_body =
            resp.into_body().collect().await.expect("collect response body").to_bytes().to_vec();
    }

    fn then_status_is(&self, expected: StatusCode) {
        let actual = self.last_status.expect("response status must be recorded");
        let body_text = String::from_utf8_lossy(&self.last_body);
        assert_eq!(actual, expected, "unexpected HTTP status (body: {body_text})");
    }

    fn then_body_is_empty(&self) {
        assert!(
            self.last_body.is_empty(),
            "expected empty response body, got: {:?}",
            String::from_utf8_lossy(&self.last_body)
        );
    }

    fn then_response_device_matches_beacon(
        &self,
        expected_name: &str,
        expected_chip_name: &str,
        expected_tx_power: AdvertiseTxPower,
    ) -> Device {
        let device: Device =
            serde_json::from_slice(&self.last_body).expect("deserialize response as Device");
        assert!(device.id > 0, "expected positive device id");
        if !expected_name.is_empty() {
            assert_eq!(device.name, expected_name);
        } else {
            assert!(
                device.name.is_empty() || device.name.starts_with("Beacon-"),
                "expected empty or default Beacon-{{id}} device name before sync, got: {}",
                device.name
            );
        }
        assert_eq!(device.chips.len(), 1, "expected exactly 1 chip on beacon device");
        let chip = &device.chips[0];
        assert_eq!(chip.kind, ChipKind::BLUETOOTH);
        if !expected_chip_name.is_empty() {
            assert_eq!(chip.name, expected_chip_name);
        } else {
            assert!(
                chip.name.starts_with("Beacon-"),
                "expected default Beacon-{{id}} chip name, got: {}",
                chip.name
            );
        }

        match &chip.variant {
            Some(ChipVariant::Bluetooth(bt)) => {
                assert!(!bt.address.is_empty(), "expected non-empty Bluetooth MAC address");
                match &bt.mode {
                    BluetoothMode::Beacon(beacon_params) => {
                        let settings = beacon_params
                            .ble_beacon
                            .settings
                            .as_ref()
                            .expect("expected beacon AdvertiseSettings");
                        assert_eq!(
                            settings.tx_power,
                            Some(TxPower::TxPowerLevel(expected_tx_power))
                        );
                    }
                    other => panic!("expected BluetoothMode::Beacon, got: {other:?}"),
                }
            }
            other => panic!("expected ChipVariant::Bluetooth, got: {other:?}"),
        }

        device
    }

    fn then_device_list_contains(&self, id: u32, expected: bool) {
        let list: ListDeviceResponse = serde_json::from_slice(&self.last_body)
            .expect("deserialize response as ListDeviceResponse");
        let found = list.devices.iter().any(|d| d.id == id);
        assert_eq!(
            found, expected,
            "expected device id {id} presence={expected} in list: {:?}",
            list.devices
        );
    }
}

/// Accepts one connection on an ephemeral loopback port, reads the HTTP request,
/// and replies with `response`.
fn serve_once(response: String) -> (NetsimRestClient, JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
    let port = listener.local_addr().expect("local_addr").port();
    let handle = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept");
        let mut buf = Vec::new();
        let mut chunk = [0u8; 1024];
        loop {
            let n = stream.read(&mut chunk).expect("read request");
            buf.extend_from_slice(&chunk[..n]);
            let text = String::from_utf8_lossy(&buf);
            if let Some((head, body)) = text.split_once("\r\n\r\n") {
                let len = head
                    .lines()
                    .find_map(|l| l.strip_prefix("Content-Length: "))
                    .map_or(0, |v| v.parse::<usize>().expect("Content-Length"));
                if body.len() >= len || n == 0 {
                    break;
                }
            } else if n == 0 {
                break;
            }
        }
        stream.write_all(response.as_bytes()).expect("write response");
        String::from_utf8_lossy(&buf).into_owned()
    });
    (NetsimRestClient::new("127.0.0.1", port), handle)
}

// Scenario: Create a BLE beacon device over REST with explicit fields and with empty defaults
//   Given a running RestServer backed by DeviceActor
//   When I send POST /v1/devices with explicit BLE beacon settings, advertise data, and scan response
//   Then the server responds with 201 Created and a Device JSON containing the Bluetooth Beacon chip
//   When I send POST /v1/devices with empty device name, empty chip name, and empty MAC address
//   Then the server responds with 201 Created and assigns default Beacon-{id} names and a valid 00:00:00:00:xx:xx MAC address
#[tokio::test]
async fn test_beacon_rest_create_happy_path_and_defaults() {
    // Given a running RestServer backed by DeviceActor
    let mut world = BeaconRestWorld::new();

    // When I send POST /v1/devices with explicit BLE beacon settings, advertise data, and scan response
    let create_req = BeaconRestWorld::sample_beacon_create_request(
        "beacon-dev-1",
        "beacon-chip-1",
        "02:11:22:33:44:55",
    );
    world.when_create_beacon(&create_req).await;

    // Then the server responds with 201 Created and a Device JSON containing the Bluetooth Beacon chip
    world.then_status_is(StatusCode::CREATED);
    let created = world.then_response_device_matches_beacon(
        "beacon-dev-1",
        "beacon-chip-1",
        AdvertiseTxPower::High,
    );
    assert_eq!(created.id, 1);

    // When I send POST /v1/devices with empty device name, empty chip name, and empty MAC address
    let defaulted_req = BeaconRestWorld::sample_beacon_create_request("", "", "");
    world.when_create_beacon(&defaulted_req).await;

    // Then the server responds with 201 Created and assigns default Beacon-{id} names and a valid 00:00:00:00:xx:xx MAC address
    world.then_status_is(StatusCode::CREATED);
    let defaulted = world.then_response_device_matches_beacon("", "", AdvertiseTxPower::High);
    assert_ne!(defaulted.id, created.id);
    let defaulted_chip_id = defaulted.chips[0].id;
    let expected_default_name = format!("Beacon-{defaulted_chip_id}");
    tokio::task::yield_now().await;
    world.when_get_devices().await;
    world.then_status_is(StatusCode::OK);
    let list: ListDeviceResponse =
        serde_json::from_slice(&world.last_body).expect("deserialize ListDeviceResponse");
    let synced_device = list
        .devices
        .iter()
        .find(|d| d.id == defaulted.id)
        .expect("defaulted beacon present in GET /v1/devices");
    assert_eq!(synced_device.name, expected_default_name);
    assert_eq!(synced_device.chips[0].name, expected_default_name);
    match &synced_device.chips[0].variant {
        Some(ChipVariant::Bluetooth(bt)) => {
            assert!(
                bt.address.starts_with("00:00:00:00:"),
                "expected default 00:00:00:00:xx:xx BLE address, got: {}",
                bt.address
            );
        }
        other => panic!("expected ChipVariant::Bluetooth, got: {other:?}"),
    }
}

// Scenario: Reject invalid POST /v1/devices requests with 400 Bad Request or 409 Conflict
//   Given a running RestServer with an existing BLE beacon device
//   When I send POST /v1/devices with malformed JSON
//   Then the server responds with 400 Bad Request
//   When I send POST /v1/devices with an invalid MAC address
//   Then the server responds with 400 Bad Request
//   When I send POST /v1/devices with a duplicate device name or duplicate BLE address
//   Then the server responds with 409 Conflict
#[tokio::test]
async fn test_beacon_rest_create_error_cases() {
    // Given a running RestServer with an existing BLE beacon device
    let mut world = BeaconRestWorld::new();
    let _existing =
        world.given_created_beacon("beacon-dup", "beacon-dup-chip", "02:11:22:33:44:01").await;

    // When I send POST /v1/devices with malformed JSON
    world.when_post_device(b"{invalid-json".to_vec()).await;
    // Then the server responds with 400 Bad Request
    world.then_status_is(StatusCode::BAD_REQUEST);

    // When I send POST /v1/devices with an invalid MAC address
    let bad_mac_req = BeaconRestWorld::sample_beacon_create_request(
        "beacon-bad-mac",
        "chip-bad-mac",
        "not-a-valid-mac",
    );
    world.when_create_beacon(&bad_mac_req).await;
    // Then the server responds with 400 Bad Request
    world.then_status_is(StatusCode::BAD_REQUEST);

    // When I send POST /v1/devices with a duplicate device name (distinct chip name and BLE address)
    let dup_device_name_req = BeaconRestWorld::sample_beacon_create_request(
        "beacon-dup",
        "distinct-chip-name",
        "02:11:22:33:44:02",
    );
    world.when_create_beacon(&dup_device_name_req).await;
    // Then the server responds with 409 Conflict
    world.then_status_is(StatusCode::CONFLICT);

    // When I send POST /v1/devices with a duplicate BLE address (distinct device name and chip name)
    let dup_ble_address_req = BeaconRestWorld::sample_beacon_create_request(
        "distinct-dev-name",
        "distinct-chip-name-2",
        "02:11:22:33:44:01",
    );
    world.when_create_beacon(&dup_ble_address_req).await;
    // Then the server responds with 409 Conflict
    world.then_status_is(StatusCode::CONFLICT);
}

// Scenario: Patch a BLE beacon device over REST matching chip by id, by name, and by BLUETOOTH kind
//   Given a created BLE beacon device
//   When I send PATCH /v1/devices/{id} matching the chip by explicit chip id
//   Then the server responds with 200 OK and updated BleBeacon settings
//   When I send PATCH /v1/devices/{id} matching the chip by chip name
//   Then the server responds with 200 OK and updated BleBeacon settings
//   When I send PATCH /v1/devices/{id} matching the chip by BLUETOOTH variant kind without id or name
//   Then the server responds with 200 OK and updated BleBeacon settings
#[tokio::test]
async fn test_beacon_rest_patch_happy_path_and_chip_matching() {
    // Given a created BLE beacon device
    let mut world = BeaconRestWorld::new();
    let created = world
        .given_created_beacon("beacon-patch-dev", "beacon-patch-chip", "02:aa:bb:cc:dd:ee")
        .await;
    let device_id_str = created.id.to_string();
    let chip_id = created.chips[0].id;

    // When I send PATCH /v1/devices/{id} matching the chip by explicit chip id
    let updated_beacon_by_id = BleBeacon {
        address: "02:aa:bb:cc:dd:ee".to_string(),
        settings: Some(AdvertiseSettings {
            interval: Some(Interval::AdvertiseMode(AdvertiseMode::Balanced)),
            tx_power: Some(TxPower::TxPowerLevel(AdvertiseTxPower::Medium)),
            scannable: false,
            timeout: 8000,
        }),
        adv_data: Some(AdvertiseData {
            include_device_name: false,
            include_tx_power_level: true,
            manufacturer_data: vec![0xAA, 0xBB],
            services: vec![],
        }),
        scan_response: Some(AdvertiseData::default()),
    };
    let patch_by_id_body = serde_json::to_vec(&serde_json::json!({
        "id": created.id,
        "pose": {},
        "chips": [{
            "id": chip_id,
            "pose": {},
            "variant": {
                "Bluetooth": {
                    "classic": {},
                    "low_energy": {},
                    "ble_beacon": updated_beacon_by_id
                }
            }
        }]
    }))
    .expect("serialize patch by id");
    world.when_patch_device(&device_id_str, patch_by_id_body).await;

    // Then the server responds with 200 OK and updated BleBeacon settings
    world.then_status_is(StatusCode::OK);
    let patched_by_id = world.then_response_device_matches_beacon(
        "beacon-patch-dev",
        "beacon-patch-chip",
        AdvertiseTxPower::Medium,
    );
    match &patched_by_id.chips[0].variant {
        Some(ChipVariant::Bluetooth(bt)) => match &bt.mode {
            BluetoothMode::Beacon(params) => {
                assert!(!params.ble_beacon.settings.as_ref().expect("settings").scannable);
            }
            other => panic!("expected BluetoothMode::Beacon, got: {other:?}"),
        },
        other => panic!("expected ChipVariant::Bluetooth, got: {other:?}"),
    }

    // When I send PATCH /v1/devices/{id} matching the chip by chip name
    let updated_beacon_by_name = BleBeacon {
        address: "02:aa:bb:cc:dd:ee".to_string(),
        settings: Some(AdvertiseSettings {
            interval: Some(Interval::AdvertiseMode(AdvertiseMode::LowPower)),
            tx_power: Some(TxPower::TxPowerLevel(AdvertiseTxPower::Low)),
            scannable: true,
            timeout: 12000,
        }),
        adv_data: Some(AdvertiseData {
            include_device_name: true,
            include_tx_power_level: false,
            manufacturer_data: vec![0xCC],
            services: vec![],
        }),
        scan_response: Some(AdvertiseData::default()),
    };
    let patch_by_name_body = serde_json::to_vec(&serde_json::json!({
        "id": created.id,
        "pose": {},
        "chips": [{
            "name": "beacon-patch-chip",
            "pose": {},
            "variant": {
                "Bluetooth": {
                    "classic": {},
                    "low_energy": {},
                    "ble_beacon": updated_beacon_by_name
                }
            }
        }]
    }))
    .expect("serialize patch by name");
    world.when_patch_device(&device_id_str, patch_by_name_body).await;

    // Then the server responds with 200 OK and updated BleBeacon settings
    world.then_status_is(StatusCode::OK);
    world.then_response_device_matches_beacon(
        "beacon-patch-dev",
        "beacon-patch-chip",
        AdvertiseTxPower::Low,
    );

    // When I send PATCH /v1/devices/{id} matching the chip by BLUETOOTH variant kind without id or name
    let updated_beacon_by_kind = BleBeacon {
        address: "02:aa:bb:cc:dd:ee".to_string(),
        settings: Some(AdvertiseSettings {
            interval: Some(Interval::AdvertiseMode(AdvertiseMode::LowLatency)),
            tx_power: Some(TxPower::TxPowerLevel(AdvertiseTxPower::UltraLow)),
            scannable: true,
            timeout: 3000,
        }),
        adv_data: Some(AdvertiseData::default()),
        scan_response: Some(AdvertiseData::default()),
    };
    let patch_by_kind_body = serde_json::to_vec(&serde_json::json!({
        "id": created.id,
        "pose": {},
        "chips": [{
            "pose": {},
            "variant": {
                "Bluetooth": {
                    "classic": {},
                    "low_energy": {},
                    "ble_beacon": updated_beacon_by_kind
                }
            }
        }]
    }))
    .expect("serialize patch by kind");
    world.when_patch_device(&device_id_str, patch_by_kind_body).await;

    // Then the server responds with 200 OK and updated BleBeacon settings
    world.then_status_is(StatusCode::OK);
    world.then_response_device_matches_beacon(
        "beacon-patch-dev",
        "beacon-patch-chip",
        AdvertiseTxPower::UltraLow,
    );
}

// Scenario: Reject invalid PATCH /v1/devices/{id} requests with 400, 404, or 409
//   Given a running RestServer with two created BLE beacon devices
//   When I send PATCH /v1/devices/not-a-u32 or /v1/devices/999999999999
//   Then the server responds with 400 Bad Request
//   When I send PATCH /v1/devices/{id} with malformed JSON or an invalid BLE address
//   Then the server responds with 400 Bad Request
//   When I send PATCH /v1/devices/99999 for a non-existent device id
//   Then the server responds with 404 Not Found
//   When I send PATCH /v1/devices/{id} targeting a non-existent chip name or chip id
//   Then the server responds with 404 Not Found
//   When I send PATCH /v1/devices/{id} updating ble_beacon.address to an address already in use
//   Then the server responds with 409 Conflict
#[tokio::test]
async fn test_beacon_rest_patch_error_cases() {
    // Given a running RestServer with two created BLE beacon devices
    let mut world = BeaconRestWorld::new();
    let created =
        world.given_created_beacon("beacon-err-dev", "beacon-err-chip", "02:aa:bb:cc:dd:01").await;
    let second = world
        .given_created_beacon("beacon-err-dev-2", "beacon-err-chip-2", "02:aa:bb:cc:dd:02")
        .await;
    let device_id_str = created.id.to_string();
    let second_id_str = second.id.to_string();

    let valid_patch_body = serde_json::to_vec(&serde_json::json!({
        "id": created.id,
        "pose": {},
        "chips": [{
            "name": "beacon-err-chip",
            "pose": {},
            "variant": {
                "Bluetooth": {
                    "classic": {},
                    "low_energy": {},
                    "ble_beacon": BleBeacon::default()
                }
            }
        }]
    }))
    .expect("serialize valid patch");

    // When I send PATCH /v1/devices/not-a-u32
    world.when_patch_device("not-a-u32", valid_patch_body.clone()).await;
    // Then the server responds with 400 Bad Request
    world.then_status_is(StatusCode::BAD_REQUEST);

    // When I send PATCH /v1/devices/999999999999 (u32 overflow)
    world.when_patch_device("999999999999", valid_patch_body.clone()).await;
    // Then the server responds with 400 Bad Request
    world.then_status_is(StatusCode::BAD_REQUEST);

    // When I send PATCH /v1/devices/{id} with malformed JSON
    world.when_patch_device(&device_id_str, b"{bad-json".to_vec()).await;
    // Then the server responds with 400 Bad Request
    world.then_status_is(StatusCode::BAD_REQUEST);

    // When I send PATCH /v1/devices/{id} with an empty chips list
    let empty_chips_body =
        serde_json::to_vec(&serde_json::json!({ "id": created.id, "pose": {}, "chips": [] }))
            .expect("serialize empty chips patch");
    world.when_patch_device(&device_id_str, empty_chips_body).await;
    // Then the server responds with 400 Bad Request
    world.then_status_is(StatusCode::BAD_REQUEST);

    // When I send PATCH /v1/devices/{id} with an invalid BLE address
    let invalid_addr_patch_body = serde_json::to_vec(&serde_json::json!({
        "id": created.id,
        "pose": {},
        "chips": [{
            "name": "beacon-err-chip",
            "pose": {},
            "variant": {
                "Bluetooth": {
                    "classic": {},
                    "low_energy": {},
                    "ble_beacon": {
                        "address": "not-a-valid-mac"
                    }
                }
            }
        }]
    }))
    .expect("serialize invalid address patch");
    world.when_patch_device(&device_id_str, invalid_addr_patch_body).await;
    // Then the server responds with 400 Bad Request
    world.then_status_is(StatusCode::BAD_REQUEST);

    // When I send PATCH /v1/devices/99999 for a non-existent device id
    world.when_patch_device("99999", valid_patch_body).await;
    // Then the server responds with 404 Not Found
    world.then_status_is(StatusCode::NOT_FOUND);

    // When I send PATCH /v1/devices/{id} targeting a non-existent chip name
    let missing_chip_name_body = serde_json::to_vec(&serde_json::json!({
        "id": created.id,
        "pose": {},
        "chips": [{
            "name": "missing-chip",
            "pose": {},
            "variant": {
                "Bluetooth": {
                    "classic": {},
                    "low_energy": {},
                    "ble_beacon": BleBeacon::default()
                }
            }
        }]
    }))
    .expect("serialize missing chip name patch");
    world.when_patch_device(&device_id_str, missing_chip_name_body).await;
    // Then the server responds with 404 Not Found
    world.then_status_is(StatusCode::NOT_FOUND);

    // When I send PATCH /v1/devices/{id} targeting a non-existent explicit chip id
    let missing_chip_id_body = serde_json::to_vec(&serde_json::json!({
        "id": created.id,
        "pose": {},
        "chips": [{
            "id": 99999,
            "pose": {},
            "variant": {
                "Bluetooth": {
                    "classic": {},
                    "low_energy": {},
                    "ble_beacon": BleBeacon::default()
                }
            }
        }]
    }))
    .expect("serialize missing chip id patch");
    world.when_patch_device(&device_id_str, missing_chip_id_body).await;
    // Then the server responds with 404 Not Found
    world.then_status_is(StatusCode::NOT_FOUND);

    // When I send PATCH /v1/devices/{id} updating ble_beacon.address to an address already in use
    let conflict_addr_patch_body = serde_json::to_vec(&serde_json::json!({
        "id": second.id,
        "pose": {},
        "chips": [{
            "name": "beacon-err-chip-2",
            "pose": {},
            "variant": {
                "Bluetooth": {
                    "classic": {},
                    "low_energy": {},
                    "ble_beacon": {
                        "address": "02:aa:bb:cc:dd:01"
                    }
                }
            }
        }]
    }))
    .expect("serialize conflicting address patch");
    world.when_patch_device(&second_id_str, conflict_addr_patch_body).await;
    // Then the server responds with 409 Conflict
    world.then_status_is(StatusCode::CONFLICT);
}

// Scenario: Delete a BLE beacon device over REST and verify post-delete invariants
//   Given a created BLE beacon device
//   When I send DELETE /v1/devices/{id}
//   Then the server responds with 204 No Content and an empty body
//   And a subsequent GET /v1/devices omits the deleted device
//   And a subsequent PATCH /v1/devices/{id} responds with 404 Not Found
//   And a subsequent DELETE /v1/devices/{id} responds with 404 Not Found
#[tokio::test]
async fn test_beacon_rest_delete_and_post_delete_invariants() {
    // Given a created BLE beacon device
    let mut world = BeaconRestWorld::new();
    let created =
        world.given_created_beacon("beacon-del-dev", "beacon-del-chip", "02:de:ad:be:ef:01").await;
    let id_str = created.id.to_string();

    // When I send DELETE /v1/devices/{id}
    world.when_delete_device(&id_str).await;

    // Then the server responds with 204 No Content and an empty body
    world.then_status_is(StatusCode::NO_CONTENT);
    world.then_body_is_empty();

    // And a subsequent GET /v1/devices omits the deleted device
    world.when_get_devices().await;
    world.then_status_is(StatusCode::OK);
    world.then_device_list_contains(created.id, false);

    // And a subsequent PATCH /v1/devices/{id} responds with 404 Not Found
    let patch_body = serde_json::to_vec(&serde_json::json!({
        "id": created.id,
        "pose": {},
        "chips": [{
            "name": "beacon-del-chip",
            "pose": {},
            "variant": {
                "Bluetooth": {
                    "classic": {},
                    "low_energy": {},
                    "ble_beacon": BleBeacon::default()
                }
            }
        }]
    }))
    .expect("serialize patch");
    world.when_patch_device(&id_str, patch_body).await;
    world.then_status_is(StatusCode::NOT_FOUND);

    // And a subsequent DELETE /v1/devices/{id} responds with 404 Not Found
    world.when_delete_device(&id_str).await;
    world.then_status_is(StatusCode::NOT_FOUND);
}

// Scenario: Reject invalid DELETE /v1/devices/{id} requests with 400 Bad Request or 404 Not Found
//   Given a running RestServer
//   When I send DELETE /v1/devices/not-a-u32
//   Then the server responds with 400 Bad Request
//   When I send DELETE /v1/devices/99999 for a non-existent device id
//   Then the server responds with 404 Not Found
#[tokio::test]
async fn test_beacon_rest_delete_error_cases() {
    // Given a running RestServer
    let mut world = BeaconRestWorld::new();

    // When I send DELETE /v1/devices/not-a-u32
    world.when_delete_device("not-a-u32").await;
    // Then the server responds with 400 Bad Request
    world.then_status_is(StatusCode::BAD_REQUEST);

    // When I send DELETE /v1/devices/99999 for a non-existent device id
    world.when_delete_device("99999").await;
    // Then the server responds with 404 Not Found
    world.then_status_is(StatusCode::NOT_FOUND);
}

// Scenario: Verify NetsimRestClient wire format for beacon create/patch/delete
//   Given canned HTTP/1.1 responses for 201 Created, 200 OK, 204 No Content, and 400/404/409 errors
//   When NetsimRestClient sends POST /v1/devices, PATCH /v1/devices/{id}, and DELETE /v1/devices/{id}
//   Then the HTTP request lines, headers, JSON bodies, and decoded responses match the REST contract
#[tokio::test]
async fn test_beacon_rest_client_wire_format() {
    let sample_device = Device {
        id: 7,
        name: "beacon-client-1".to_string(),
        visible: true,
        pose: Pose::default(),
        builtin: false,
        chips: vec![Chip {
            id: 11,
            kind: ChipKind::BLUETOOTH,
            name: "beacon-chip-1".to_string(),
            manufacturer: "Google".to_string(),
            product_name: "Netsim BLE Beacon".to_string(),
            pose: Pose::default(),
            device_id: 7.into(),
            variant: Some(ChipVariant::Bluetooth(Box::new(netsim_model::Bluetooth {
                address: "02:11:22:33:44:55".to_string(),
                mode: BluetoothMode::Beacon(Box::new(BeaconParams {
                    ble_beacon: BleBeacon {
                        address: "02:11:22:33:44:55".to_string(),
                        settings: Some(AdvertiseSettings {
                            interval: Some(Interval::AdvertiseMode(AdvertiseMode::LowLatency)),
                            tx_power: Some(TxPower::TxPowerLevel(AdvertiseTxPower::High)),
                            scannable: false,
                            timeout: 5000,
                        }),
                        adv_data: Some(AdvertiseData::default()),
                        scan_response: Some(AdvertiseData::default()),
                    },
                })),
                ..Default::default()
            }))),
            links: vec![],
            enabled: true,
        }],
        device_info: None,
    };
    let device_json = serde_json::to_string(&sample_device).expect("serialize sample Device");

    // When NetsimRestClient sends POST /v1/devices -> 201 Created
    let create_req = BeaconRestWorld::sample_beacon_create_request(
        "beacon-client-1",
        "beacon-chip-1",
        "02:11:22:33:44:55",
    );
    let (client, server) = serve_once(format!("HTTP/1.1 201 Created\r\n\r\n{device_json}"));
    let created: Device = client
        .post::<Device>("/v1/devices")
        .json(&create_req)
        .send()
        .await
        .expect("POST /v1/devices");
    assert_eq!(created.id, 7);
    assert_eq!(created.name, "beacon-client-1");
    let raw_post = server.join().expect("join server");
    assert!(raw_post.starts_with("POST /v1/devices HTTP/1.1\r\n"), "{raw_post}");
    assert!(raw_post.contains(r#""Beacon""#), "{raw_post}");

    // When NetsimRestClient sends PATCH /v1/devices/7 -> 200 OK
    let patch_payload = serde_json::json!({
        "id": 7,
        "pose": {},
        "chips": [{
            "id": 11,
            "name": "beacon-chip-1",
            "pose": {},
            "variant": {
                "Bluetooth": {
                    "classic": {},
                    "low_energy": {},
                    "ble_beacon": {
                        "address": "02:11:22:33:44:55",
                        "settings": {
                            "interval": { "AdvertiseMode": "LowPower" },
                            "tx_power": { "TxPowerLevel": "Low" },
                            "scannable": true,
                            "timeout": 1000
                        }
                    }
                }
            }
        }]
    });
    let (client, server) = serve_once(format!("HTTP/1.1 200 OK\r\n\r\n{device_json}"));
    let patched: Device = client
        .patch::<Device>("/v1/devices/7")
        .json(&patch_payload)
        .send()
        .await
        .expect("PATCH /v1/devices/7");
    assert_eq!(patched.id, 7);
    let raw_patch = server.join().expect("join server");
    assert!(raw_patch.starts_with("PATCH /v1/devices/7 HTTP/1.1\r\n"), "{raw_patch}");
    assert!(raw_patch.contains(r#""ble_beacon""#), "{raw_patch}");

    // When NetsimRestClient sends DELETE /v1/devices/7 -> 204 No Content
    let (client, server) = serve_once("HTTP/1.1 204 No Content\r\n\r\n".to_string());
    client.delete("/v1/devices/7").send().await.expect("DELETE /v1/devices/7");
    let raw_delete = server.join().expect("join server");
    assert!(raw_delete.starts_with("DELETE /v1/devices/7 HTTP/1.1\r\n"), "{raw_delete}");

    // Then error statuses 400, 404, 409 map to RestError::Status with matching StatusCode
    for (raw_resp, expected_status) in [
        (
            "HTTP/1.1 400 Bad Request\r\n\r\n{\"error\":\"Invalid MAC address\"}",
            StatusCode::BAD_REQUEST,
        ),
        (
            "HTTP/1.1 404 Not Found\r\n\r\n{\"error\":\"Device not found: 99\"}",
            StatusCode::NOT_FOUND,
        ),
        (
            "HTTP/1.1 409 Conflict\r\n\r\n{\"error\":\"Device already exists\"}",
            StatusCode::CONFLICT,
        ),
    ] {
        let (client, _server) = serve_once(raw_resp.to_string());
        let err = client
            .post::<Device>("/v1/devices")
            .json(&create_req)
            .send()
            .await
            .expect_err("expected error response");
        assert!(matches!(err, RestError::Status { status, .. } if status == expected_status));
        assert_eq!(err.status(), Some(expected_status));
    }
}
