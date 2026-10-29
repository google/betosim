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

use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicU32, Ordering},
    },
};

use actor_framework::{ActorService, Context, DynContext};
use capture_api::CaptureSender;
use device_api::{
    DeviceAction, DeviceActionResult, DeviceAddChip, DeviceCreate, DeviceId, DeviceUpdate,
};
use link_api::LinkClient;
use netsim_model::{
    Chip, ChipClient, ChipCreate, ChipId, ChipKind, ChipUpdate, PacketSink, PacketStream,
};
use netsim_proto::protobuf::Message;
use serde::{Deserialize, Serialize};
use tracing::{debug, error, info, warn};

use crate::{
    device_actor::DeviceActor,
    error::DeviceError,
    utils::{StreamStats, create_capture_and_wrap_streams, to_proto_stats},
};

const CHIP_READ_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(50);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct InternalDevice {
    device: device_api::Device,
    create_params: Option<DeviceCreate>,
    guid: Option<String>,
    #[serde(skip)]
    chip_stats: HashMap<ChipId, Arc<StreamStats>>,
    #[serde(skip)]
    last_known_stats: HashMap<ChipId, Vec<netsim_model::NetsimRadioStats>>,
}

impl InternalDevice {
    fn from_create_params(id: DeviceId, params: DeviceCreate) -> Result<Self, DeviceError> {
        Ok(InternalDevice {
            device: device_api::Device {
                id: id.0,
                name: params.device_config.name.clone(),
                visible: params.device_config.visible,
                pose: params.device_config.pose,
                builtin: params.device_config.builtin,
                chips: vec![],
                device_info: params.device_config.device_info.clone(),
            },
            create_params: Some(params),
            guid: None,
            chip_stats: HashMap::new(),
            last_known_stats: HashMap::new(),
        })
    }
}

impl DeviceActor {
    fn update_idle_state(&mut self, ctx: &mut DynContext<Self>) {
        let has_active_devices = self.devices.values().any(|d| !d.device.builtin);

        if has_active_devices {
            self.has_seen_device = true;
            if let Some(key) = self.startup_timer.take() {
                ctx.cancel_timer(key);
            }
            if let Some(key) = self.idle_timer.take() {
                ctx.cancel_timer(key);
            }
        } else if self.has_seen_device
            && self.idle_timer.is_none()
            && let Some(timeout) = self.idle_timeout
        {
            info!("DeviceActor: Scheduling idle shutdown in {:?}", timeout);
            let key = ctx.run_later(timeout, Box::new(Self::on_idle_timeout));
            self.idle_timer = Some(key);
        }
    }

    /// Collects radio stats from local atomic counters (synchronous).
    /// This is preferred for persistence as it doesn't require IPC to chip
    /// actors.
    pub(crate) async fn save_stats_async(&mut self) {
        let active_model_stats = self.collect_radio_stats_async().await;
        // Convert to Proto for persistence
        let active_proto_stats = active_model_stats.into_iter().map(to_proto_stats).collect();

        // Collect WiFi specific stats (Global)
        let wifi_ipc_stats = self
            .collect_chip_stats::<netsim_proto::stats::WifiIpcStats>(netsim_model::ChipKind::WIFI)
            .await;
        let mut wifi_stats = None;
        let mut wifi_api = None;
        if let Some(mut ipc) = wifi_ipc_stats {
            wifi_stats = ipc.wifi_stats.take();
            wifi_api = ipc.wifi_api_stats.take();
        }
        // Collect UWB specific stats (Global)
        let uwb_api = self
            .collect_chip_stats::<netsim_proto::stats::UwbApiStats>(netsim_model::ChipKind::UWB)
            .await;
        // Collect NFC specific stats (Global)
        let nfc_ipc_stats = self
            .collect_chip_stats::<netsim_proto::stats::NfcIpcStats>(netsim_model::ChipKind::NFC)
            .await;
        let mut nfc_stats = None;
        let mut nfc_api = None;
        if let Some(mut ipc) = nfc_ipc_stats {
            nfc_stats = ipc.nfc_stats.take();
            nfc_api = ipc.nfc_api_stats.take();
        }
        let nfc_service_stats = self.collect_nfc_service_stats_async().await;

        let combined_stats = self.stats.get_combined_stats(crate::stats::CombinedStatsInputs {
            active_stats: active_proto_stats,
            wifi_stats,
            wifi_api,
            uwb_api,
            nfc_api,
            nfc_stats,
            nfc_service_stats,
        });
        let path = self.stats.stats_path.clone();

        let previous_task = self.stats_write_task.take();

        // Spawn a background async task to prevent the actor from blocking on disk I/O.
        // It awaits the previous write task to ensure sequential writes without race
        // conditions.
        let task = tokio::spawn(async move {
            if let Some(t) = previous_task
                && let Err(error) = t.await
            {
                warn!(%error, "DeviceActor: Previous stats write failed");
            }

            // Spawn blocking write for disk I/O
            let res = tokio::task::spawn_blocking(move || {
                if let Err(error) = crate::stats::write_combined_stats(combined_stats, path) {
                    error!(%error, "Failed to write stats");
                }
            })
            .await;

            if let Err(error) = res {
                error!(%error, "DeviceActor: Stats write task panicked");
            }
        });

        self.stats_write_task = Some(task);
    }

    async fn collect_radio_stats_async(&mut self) -> Vec<netsim_model::NetsimRadioStats> {
        let mut stats_futures = Vec::new();
        let mut state_futures = Vec::new();

        for client in self.chip_clients.values() {
            // Stats requests
            stats_futures.push(async move {
                tokio::time::timeout(CHIP_READ_TIMEOUT, client.read_statistics())
                    .await
                    .ok()
                    .and_then(|res| res.ok())
            });
        }

        // Identify BT and CELLULAR chips for state fetching
        let mut state_chips = Vec::new();
        for device in self.devices.values() {
            for chip in &device.device.chips {
                if matches!(
                    chip.kind,
                    netsim_model::ChipKind::BLUETOOTH | netsim_model::ChipKind::CELLULAR
                ) {
                    state_chips.push((chip.kind, chip.id));
                }
            }
        }

        for (kind, id) in state_chips {
            if let Some(client) = self.chip_clients.get(&kind) {
                let id = ChipId(id);
                state_futures.push(async move {
                    let res = tokio::time::timeout(CHIP_READ_TIMEOUT, client.read(id))
                        .await
                        .ok()
                        .and_then(|res| res.ok());
                    (id, res)
                });
            }
        }

        let stats_results = futures::future::join_all(stats_futures).await;
        let state_results = futures::future::join_all(state_futures).await;

        let mut chip_stats_map: HashMap<ChipId, Vec<netsim_model::NetsimRadioStats>> =
            HashMap::new();
        for stats in stats_results.into_iter().flatten() {
            for stat in stats {
                chip_stats_map.entry(ChipId(stat.id)).or_default().push(stat);
            }
        }

        let mut chip_state_map: HashMap<ChipId, Chip> = HashMap::new();
        for (id, res) in state_results {
            if let Some(chip) = res {
                chip_state_map.insert(id, chip);
            }
        }

        let mut stats_list = Vec::new();
        for device_entry in self.devices.values_mut() {
            for chip in &mut device_entry.device.chips {
                if chip.kind == netsim_model::ChipKind::CELLULAR
                    && let Some(fresh_chip) = chip_state_map.get(&ChipId(chip.id))
                {
                    chip.enabled = fresh_chip.enabled;
                    chip.variant = fresh_chip.variant.clone();
                }

                let stream_stats = device_entry.chip_stats.get(&ChipId(chip.id));
                let duration_secs =
                    stream_stats.map(|s| s.start_time.elapsed().as_secs()).unwrap_or(0);

                if let Some(client_stats) =
                    chip_stats_map.get(&ChipId(chip.id)).filter(|v| !v.is_empty())
                {
                    // Update in-memory model with fresh stats
                    for stat in client_stats {
                        match (&mut chip.variant, stat.kind) {
                            (
                                Some(netsim_model::ChipVariant::Wifi(wifi)),
                                netsim_model::RadioKind::Wifi,
                            ) => {
                                wifi.radio.tx_count = stat.tx_count;
                                wifi.radio.rx_count = stat.rx_count;
                            }
                            (
                                Some(netsim_model::ChipVariant::Uwb(uwb)),
                                netsim_model::RadioKind::Uwb,
                            ) => {
                                uwb.radio.tx_count = stat.tx_count;
                                uwb.radio.rx_count = stat.rx_count;
                            }
                            (
                                Some(netsim_model::ChipVariant::Bluetooth(bt)),
                                netsim_model::RadioKind::BluetoothLowEnergy,
                            ) => {
                                bt.low_energy.tx_count = stat.tx_count;
                                bt.low_energy.rx_count = stat.rx_count;
                            }
                            (
                                Some(netsim_model::ChipVariant::Bluetooth(bt)),
                                netsim_model::RadioKind::BluetoothClassic,
                            ) => {
                                bt.classic.tx_count = stat.tx_count;
                                bt.classic.rx_count = stat.rx_count;
                            }
                            (
                                Some(netsim_model::ChipVariant::Nfc(nfc)),
                                netsim_model::RadioKind::Nfc,
                            ) => {
                                nfc.radio.tx_count = stat.tx_count;
                                nfc.radio.rx_count = stat.rx_count;
                            }
                            _ => {}
                        }
                    }

                    let mut chip_stats: Vec<netsim_model::NetsimRadioStats> = client_stats
                        .iter()
                        .map(|stat| {
                            let mut radio_stats = stat.clone();
                            radio_stats.id = device_entry.device.id;
                            radio_stats.duration_secs = duration_secs;
                            radio_stats
                        })
                        .collect();

                    device_entry.last_known_stats.insert(ChipId(chip.id), chip_stats.clone());
                    if let Some(stream_stats) = stream_stats {
                        crate::utils::distribute_stream_stats(&mut chip_stats, stream_stats);
                    }
                    stats_list.extend(chip_stats);
                    continue;
                }

                if let Some(cached_stats) = device_entry.last_known_stats.get(&ChipId(chip.id)) {
                    let mut chip_stats = cached_stats.clone();
                    for stat in &mut chip_stats {
                        stat.duration_secs = duration_secs;
                    }
                    if let Some(stream_stats) = stream_stats {
                        crate::utils::distribute_stream_stats(&mut chip_stats, stream_stats);
                    }
                    stats_list.extend(chip_stats);
                    continue;
                }

                if let Some(ss) = stream_stats {
                    // Resolve Kind Synchronously using pre-fetched state
                    let radio_kind = if chip.kind == netsim_model::ChipKind::BLUETOOTH {
                        let chip_state = chip_state_map.get(&ChipId(chip.id)).unwrap_or(chip);
                        Self::resolve_bluetooth_kind(chip, Some(chip_state))
                    } else {
                        Some(netsim_model::chip_kind_to_radio_kind(chip.kind))
                    };

                    if let Some(kind) = radio_kind {
                        stats_list.push(crate::utils::stream_to_model_stats(
                            device_entry.device.id,
                            kind,
                            duration_secs,
                            ss,
                        ));
                    }
                }
            }
        }
        stats_list
    }

    async fn collect_chip_stats<T: Message>(&self, kind: netsim_model::ChipKind) -> Option<T> {
        let client = self.chip_clients.get(&kind)?;
        let stats_bytes = tokio::time::timeout(CHIP_READ_TIMEOUT, client.get_global_stats())
            .await
            .inspect_err(
                |error| debug!(%error, "DeviceActor: Timeout getting {:?} global stats", kind),
            )
            .ok()?
            .inspect_err(
                |error| warn!(%error, "DeviceActor: Failed to get global stats for {:?}", kind),
            )
            .ok()??;

        T::parse_from_bytes(&stats_bytes)
            .inspect_err(|error| error!(%error, "DeviceActor: Failed to parse {:?}Stats", kind))
            .ok()
    }

    async fn collect_nfc_service_stats_async(
        &self,
    ) -> Option<netsim_proto::stats::NfcServiceStats> {
        let client = self.chip_clients.get(&netsim_model::ChipKind::NFC)?;
        match tokio::time::timeout(CHIP_READ_TIMEOUT, client.get_service_stats()).await {
            Ok(Ok(Some(stats_bytes))) => {
                match netsim_proto::stats::NfcServiceStats::parse_from_bytes(&stats_bytes) {
                    Ok(stats) => Some(stats),
                    Err(error) => {
                        error!(%error, "DeviceActor: Failed to parse NfcServiceStats");
                        None
                    }
                }
            }
            Ok(Ok(None)) => None,
            Ok(Err(error)) => {
                warn!(%error, "DeviceActor: Failed to get NFC service stats");
                None
            }
            Err(_) => {
                debug!("DeviceActor: Timeout getting NFC service stats");
                None
            }
        }
    }

    /// Resolves the specific RadioKind for a chip, handling ambiguous cases
    /// like Bluetooth. Fetches fresh chip state if necessary to distinguish
    /// between LE and Classic.
    async fn resolve_radio_kind(&self, chip: &Chip) -> Option<netsim_model::RadioKind> {
        if chip.kind != netsim_model::ChipKind::BLUETOOTH {
            return Some(netsim_model::chip_kind_to_radio_kind(chip.kind));
        }

        // Try to get fresh chip state
        let fresh_chip = if let Some(client) = self.chip_clients.get(&chip.kind) {
            tokio::time::timeout(CHIP_READ_TIMEOUT, client.read(ChipId(chip.id)))
                .await
                .ok()
                .and_then(|res| res.ok())
        } else {
            None
        };

        let chip = fresh_chip.as_ref().unwrap_or(chip);
        Self::resolve_bluetooth_kind(chip, Some(chip))
    }

    fn resolve_bluetooth_kind(
        chip: &Chip,
        state: Option<&Chip>,
    ) -> Option<netsim_model::RadioKind> {
        let chip_state = state.unwrap_or(chip);
        match (chip_state.is_le_enabled(), chip_state.is_classic_enabled()) {
            (true, false) => Some(netsim_model::RadioKind::BluetoothLowEnergy),
            (false, true) => Some(netsim_model::RadioKind::BluetoothClassic),
            _ => None, /* Ambiguous (Dual Mode) or Invalid -> Drop
                        * TODO: Requires HCI packet inspection to accurately distinguish traffic */
        }
    }

    async fn perform_device_deletion(
        &mut self,
        id: DeviceId,
        ctx: &mut DynContext<Self>,
    ) -> Result<(), DeviceError> {
        let stats_to_archive = {
            let Some(internal_device) = self.devices.get(&id) else {
                return Err(DeviceError::DeviceNotFound(id.to_string()));
            };

            let chips = internal_device.device.chips.clone();
            let mut stats_to_archive = Vec::new();
            for chip in &chips {
                let chip_id = ChipId(chip.id);
                let stream_stats = internal_device.chip_stats.get(&chip_id).cloned();
                let cached_stats = internal_device.last_known_stats.get(&chip_id).cloned();
                stats_to_archive.push((chip.clone(), stream_stats, cached_stats));
            }
            stats_to_archive
        };

        for (chip, stream_stats, cached_stats) in stats_to_archive {
            let chip_id = ChipId(chip.id);
            self.archive_chip_stats(id, &chip, stream_stats, cached_stats.as_ref()).await;
            if let Some(chip_client) = self.chip_clients.get(&chip.kind)
                && let Err(error) = chip_client.delete(chip_id).await
            {
                warn!(%error, "Failed to delete chip {chip_id} of device {id}; continuing");
            }
            if let Err(error) = self.link_client.notify_chip_removed(chip_id).await {
                warn!(
                    %error,
                    "Failed to notify link actor about chip {chip_id} removal; continuing"
                );
            }
        }

        if let Some(internal_device) = self.devices.remove(&id)
            && let Some(guid) = &internal_device.guid
        {
            self.guid_to_id.remove(guid);
        }
        self.stats.update_device_count(self.devices.len(), false);
        self.update_idle_state(ctx);
        self.save_stats_async().await;
        Ok(())
    }

    async fn perform_chip_removal(
        &mut self,
        device_id: DeviceId,
        chip_id: ChipId,
        ctx: &mut DynContext<Self>,
    ) -> Result<(), DeviceError> {
        // Find and remove the chip + stats for archiving
        let (chip_to_archive, should_delete) = {
            let entity = self
                .devices
                .get_mut(&device_id)
                .ok_or_else(|| DeviceError::DeviceNotFound(device_id.to_string()))?;
            let archived =
                if let Some(pos) = entity.device.chips.iter().position(|c| c.id == chip_id.0) {
                    let chip = entity.device.chips.remove(pos);
                    let stream_stats = entity.chip_stats.remove(&chip_id);
                    let cached_stats = entity.last_known_stats.remove(&chip_id);
                    Some((chip, stream_stats, cached_stats))
                } else {
                    None
                };
            (archived, entity.device.chips.is_empty())
        };

        if let Some((chip, stream_stats, cached_stats)) = chip_to_archive {
            info!(
                "DeviceActor: Removed chip {chip_id} (kind {}) from device {device_id}",
                chip.kind
            );
            self.archive_chip_stats(device_id, &chip, stream_stats, cached_stats.as_ref()).await;
            if let Some(chip_client) = self.chip_clients.get(&chip.kind) {
                let _ = chip_client.delete(chip_id).await;
            }
            if let Err(error) = self.link_client.notify_chip_removed(chip_id).await {
                warn!(%error, "DeviceActor: Failed to notify LinkActor of chip remove");
            }
        } else if !should_delete {
            return Ok(());
        }

        if should_delete {
            info!("DeviceActor: Device {} is empty, auto-deleting", device_id);
            self.handle_delete(device_id, ctx).await?;
        } else {
            // Save stats if we didn't delete the device (partial update)
            self.save_stats_async().await;
            info!("DeviceActor: Device {} is NOT empty after chip removal", device_id);
        }

        Ok(())
    }

    async fn archive_chip_stats(
        &mut self,
        device_id: DeviceId,
        chip: &Chip,
        stream_stats: Option<Arc<StreamStats>>,
        cached_stats: Option<&Vec<netsim_model::NetsimRadioStats>>,
    ) {
        debug!("DeviceActor: Archiving stats for device {} chip {}", device_id, chip.id);

        let mut archive_stats = |mut chip_stats: Vec<netsim_model::NetsimRadioStats>| {
            if let Some(stream_stats) = &stream_stats {
                crate::utils::distribute_stream_stats(&mut chip_stats, stream_stats);
            }
            for radio_stats in chip_stats {
                self.stats.archive(to_proto_stats(radio_stats));
            }
        };

        if let Some(client) = self.chip_clients.get(&chip.kind)
            && let Ok(Ok(stats)) =
                tokio::time::timeout(CHIP_READ_TIMEOUT, client.read_statistics()).await
        {
            let chip_stats: Vec<_> = stats
                .into_vec()
                .into_iter()
                .filter(|s| s.id == chip.id)
                .map(|mut s| {
                    s.id = device_id.0;
                    s.duration_secs =
                        stream_stats.as_ref().map_or(0, |x| x.start_time.elapsed().as_secs());
                    s
                })
                .collect();

            if !chip_stats.is_empty() {
                archive_stats(chip_stats);
                return;
            }
        }

        if let Some(cached) = cached_stats {
            debug!("DeviceActor: Using cached LKG stats for chip {}", chip.id);
            archive_stats(cached.clone());
            return;
        }

        if let Some(stream_stats) = stream_stats {
            if let Some(kind) = self.resolve_radio_kind(chip).await {
                self.stats.archive(crate::utils::stream_to_proto_stats(
                    device_id.0,
                    kind,
                    stream_stats.start_time.elapsed().as_secs(),
                    &stream_stats,
                ));
            } else {
                warn!(
                    "DeviceActor: Dropping ambiguous stats for chip {} kind {:?}",
                    chip.id, chip.kind
                );
            }
        }
    }

    /// Adds a chip to a device.
    #[allow(clippy::too_many_arguments)]
    async fn perform_add_chip(
        next_chip_id: &Arc<AtomicU32>,
        chip_clients: &HashMap<ChipKind, Box<dyn ChipClient>>,
        link_client: &dyn LinkClient,
        capture_client: &Option<Arc<dyn CaptureSender>>,
        entity: &mut InternalDevice,
        mut chip: netsim_model::Chip,
        packet_stream: Option<PacketStream>,
        packet_sink: Option<PacketSink>,
    ) -> Result<ChipId, DeviceError> {
        let chip_name =
            if chip.name.is_empty() { entity.device.name.clone() } else { chip.name.clone() };
        let manufacturer = if chip.manufacturer.is_empty() {
            entity
                .device
                .device_info
                .as_ref()
                .map(|info| info.kind.clone())
                .unwrap_or("Unknown".to_string())
        } else {
            chip.manufacturer.clone()
        };
        let product_name = if chip.product_name.is_empty() {
            entity.device.name.clone()
        } else {
            chip.product_name.clone()
        };
        let chip_id = ChipId(next_chip_id.fetch_add(1, Ordering::SeqCst));
        let chip_kind = chip.kind;

        if !chip.name.is_empty() && chip.name != entity.device.name {
            info!(
                "DeviceActor: AddChip {chip_id} (kind {chip_kind}, name \"{}\") to device {} ({})",
                chip.name, entity.device.id, entity.device.name
            );
        } else {
            info!(
                "DeviceActor: AddChip {chip_id} (kind {chip_kind}) to device {} ({})",
                entity.device.id, entity.device.name
            );
        }

        let (packet_stream, packet_sink, stream_stats) = create_capture_and_wrap_streams(
            capture_client.clone(),
            chip_id,
            chip_kind,
            entity.device.name.clone(),
            packet_stream,
            packet_sink,
        )
        .await;

        if let Some(stats) = stream_stats {
            entity.chip_stats.insert(chip_id, stats);
        }

        let chip_client = chip_clients.get(&chip_kind).ok_or_else(|| {
            DeviceError::ChipKindNotSupported(format!("No chip client for {chip_kind}"))
        })?;

        chip.id = chip_id.0;
        chip.name = chip_name;
        chip.manufacturer = manufacturer;
        chip.product_name = product_name;
        chip.device_id = DeviceId(entity.device.id);
        chip.pose = entity.device.pose;

        let chip_create_params = ChipCreate { packet_stream, packet_sink, chip: chip.clone() };

        chip_client.create(chip_id, chip_create_params).await?;
        entity.device.chips.push(chip);

        if let Err(error) = link_client.notify_chip_added(chip_id, chip_kind).await {
            warn!(%error, "DeviceActor: Failed to notify LinkActor of chip add");
        }

        Ok(chip_id)
    }

    async fn perform_create_device(
        &mut self,
        id: Option<DeviceId>,
        params: DeviceCreate,
        packet_stream: Option<PacketStream>,
        packet_sink: Option<PacketSink>,
        ctx: &mut DynContext<Self>,
    ) -> Result<DeviceId, DeviceError> {
        info!("DeviceActor: Create device {}", params.device_config.name);

        let id = id.unwrap_or_else(|| {
            let id = DeviceId(self.next_device_id);
            self.next_device_id += 1;
            id
        });

        let mut entity = InternalDevice::from_create_params(id, params.clone())?;
        let chip: netsim_model::Chip = params.chip.into();
        Self::perform_add_chip(
            &self.next_chip_id,
            &self.chip_clients,
            self.link_client.as_ref(),
            &self.capture_client,
            &mut entity,
            chip,
            packet_stream,
            packet_sink,
        )
        .await?;

        self.devices.insert(id, entity);
        self.stats.update_device_count(self.devices.len(), true);

        if let Some(device_info) = &params.device_config.device_info {
            self.stats.add_device_stats(crate::utils::to_proto_device_stats(id.0, device_info));
        }

        self.update_idle_state(ctx);
        Ok(id)
    }

    /// Adds a chip to a device by GUID, creating the device if necessary.
    async fn perform_add_chip_by_guid(
        &mut self,
        params: DeviceAddChip,
        ctx: &mut DynContext<Self>,
    ) -> Result<DeviceActionResult, DeviceError> {
        debug!("DeviceActor: AddChipByGuid {} for device {}", params.chip.kind, params.device_guid);

        if let Some(id) = self.guid_to_id.get(&params.device_guid) {
            // Add Chip to Existing Device
            let id = *id;
            let entity = self
                .devices
                .get_mut(&id)
                .ok_or_else(|| DeviceError::DeviceNotFound(id.to_string()))?;

            let chip_id = Self::perform_add_chip(
                &self.next_chip_id,
                &self.chip_clients,
                self.link_client.as_ref(),
                &self.capture_client,
                entity,
                params.chip,
                params.packet_stream,
                params.packet_sink,
            )
            .await?;

            // Trigger immediate save
            self.save_stats_async().await;

            Ok(DeviceActionResult::AddChipByGuidSuccess { device_id: id, chip_id })
        } else {
            // Create New Device
            let chip_create_params = params.chip.clone().into();
            let create_params =
                DeviceCreate { device_config: params.device_config, chip: chip_create_params };

            let id = self
                .perform_create_device(
                    None,
                    create_params,
                    params.packet_stream,
                    params.packet_sink,
                    ctx,
                )
                .await?;

            // Update GUID mapping
            if let Some(entity) = self.devices.get_mut(&id) {
                entity.guid = Some(params.device_guid.clone());
            }
            self.guid_to_id.insert(params.device_guid, id);

            // Get the chip id (it's the first one, as we just created the device)
            let chip_id = self
                .devices
                .get(&id)
                .ok_or_else(|| DeviceError::DeviceNotFound("Device created without chips".into()))?
                .device
                .chips
                .first()
                .ok_or_else(|| DeviceError::DeviceNotFound("Device created without chips".into()))?
                .id;

            // Trigger immediate save
            self.save_stats_async().await;

            Ok(DeviceActionResult::AddChipByGuidSuccess { device_id: id, chip_id: ChipId(chip_id) })
        }
    }

    /// Callback for startup timeout
    pub(crate) fn on_startup_timeout(&mut self, ctx: &mut dyn Context<Self>) {
        let has_active_devices = self.devices.values().any(|d| !d.device.builtin);
        if !self.has_seen_device && !has_active_devices {
            info!(
                "DeviceActor: Startup timeout reached (no devices connected), initiating shutdown"
            );
            self.trigger_shutdown(ctx);
        }
        self.startup_timer = None;
    }

    /// Callback for idle timeout
    pub(crate) fn on_idle_timeout(&mut self, ctx: &mut dyn Context<Self>) {
        let has_active_devices = self.devices.values().any(|d| !d.device.builtin);
        if !has_active_devices {
            info!("DeviceActor: Idle timeout reached, initiating shutdown");
            self.trigger_shutdown(ctx);
        }
        self.idle_timer = None;
    }

    /// Helper utility to initiate graceful shutdown, ensuring stats are
    /// flushed.
    fn trigger_shutdown(&mut self, ctx: &mut dyn Context<Self>) {
        let stats_task = self.stats_write_task.take();
        if let Some(client) = self.self_client.clone() {
            tokio::spawn(async move {
                if let Some(t) = stats_task
                    && let Err(error) = t.await
                {
                    warn!(%error, "DeviceActor: Stats write failed during shutdown flush");
                }
                if let Err(error) = client.shutdown().await {
                    error!(%error, "DeviceActor: Failed to shutdown");
                }
            });
        } else {
            warn!("DeviceActor: No self_client, forcing immediate shutdown");
            ctx.shutdown();
        }
    }

    async fn perform_reset(
        &mut self,
        target_id: Option<DeviceId>,
        ctx: &mut DynContext<Self>,
    ) -> Result<(), DeviceError> {
        let (devices_to_delete, devices_to_reset) = if let Some(target_id) = target_id {
            if !self.devices.contains_key(&target_id) {
                return Err(DeviceError::DeviceNotFound(target_id.to_string()));
            };
            (vec![], vec![target_id])
        } else {
            let mut to_delete = vec![];
            let mut to_reset = vec![];

            for (device_id, device) in &self.devices {
                if device.guid.is_some() || device.device.builtin {
                    to_reset.push(*device_id);
                } else {
                    to_delete.push(*device_id);
                }
            }
            (to_delete, to_reset)
        };

        let mut device_delete_errors = Vec::new();
        for device_id in devices_to_delete {
            info!("DeviceActor: Deleting internal device {}", device_id);
            if let Err(error) = self.perform_device_deletion(device_id, ctx).await {
                warn!(%error, "DeviceActor: Failed to reset chip device {}", device_id);
                device_delete_errors.push((device_id, error));
            }
        }

        let mut chip_reset_errors = Vec::new();
        for device_id in devices_to_reset {
            let device = self.devices.get_mut(&device_id).expect("list has not changed");
            info!("DeviceActor: Resetting device {}", device.device.name);

            let (original_pos, original_orient) = device
                .create_params
                .as_ref()
                .map(|p| (p.device_config.pose.position, p.device_config.pose.orientation))
                .unwrap_or_default();

            device.device.visible = true;
            device.device.pose.position = original_pos;
            device.device.pose.orientation = original_orient;

            for chip in device.device.chips.iter_mut() {
                chip.pose.position = original_pos;
                chip.pose.orientation = original_orient;

                if let Some(chip_client) = self.chip_clients.get(&chip.kind) {
                    match chip_client.reset(ChipId(chip.id)).await {
                        Ok(updated_chip) => {
                            *chip = updated_chip;
                        }
                        Err(error) => {
                            warn!(
                                %error,
                                "DeviceActor: Failed to reset chip {} kind {:?}",
                                chip.id, chip.kind
                            );
                            chip_reset_errors.push((chip.id, error));
                        }
                    }
                }
            }
        }

        let link_client_error = if target_id.is_none() {
            info!("DeviceActor: Resetting all links");
            if let Err(error) = self.link_client.reset().await {
                warn!(%error, "DeviceActor: Failed to reset links");
                Some(error)
            } else {
                None
            }
        } else {
            None
        };

        self.save_stats_async().await;

        if !chip_reset_errors.is_empty() {
            Err(DeviceError::ResetErrors {
                device_delete_errors,
                chip_reset_errors,
                link_client_error,
            })
        } else {
            Ok(())
        }
    }
}

impl ActorService for DeviceActor {
    type Id = DeviceId;
    type Create = DeviceCreate;
    type Update = DeviceUpdate;
    type Action = DeviceAction;
    type ActionResult = DeviceActionResult;
    type Error = DeviceError;
    type Entity = device_api::Device;
    type TypedStream = ();

    async fn handle_create(
        &mut self,
        id: Option<Self::Id>,
        params: Self::Create,
        ctx: &mut DynContext<Self>,
    ) -> Result<Self::Id, Self::Error> {
        self.perform_create_device(id, params, None, None, ctx).await
    }

    async fn handle_get(
        &self,
        id: Self::Id,
        _ctx: &mut DynContext<Self>,
    ) -> Result<Option<Self::Entity>, Self::Error> {
        Ok(self.devices.get(&id).map(|e| e.device.clone()))
    }

    async fn handle_update(
        &mut self,
        id: Self::Id,
        update: Self::Update,
        _ctx: &mut DynContext<Self>,
    ) -> Result<Self::Entity, Self::Error> {
        // Update does not affect device count, so no timeout logic change needed.
        let Some(entity) = self.devices.get_mut(&id) else {
            return Err(DeviceError::DeviceNotFound(id.to_string()));
        };
        if update.pose.position.is_some()
            || update.pose.orientation.is_some()
            || update.name.is_some()
            || update.visible.is_some()
        {
            info!("DeviceActor: Updating device {id} with {:?}", update);
        }
        update.apply(&mut entity.device);

        for chip in entity.device.chips.iter_mut() {
            let Some(chip_client) = self.chip_clients.get(&chip.kind) else {
                continue;
            };

            let mut chip_update = ChipUpdate { pose: update.pose.clone(), ..Default::default() };

            // Start with ID-based matching
            let mut specific_update = None;
            if let Some(chips) = &update.chips {
                // Priority 1: Exact ID match
                specific_update = chips.iter().find(|u| u.id == Some(ChipId(chip.id)));

                // Priority 2: Variant match (if no ID match found)
                if specific_update.is_none() {
                    specific_update = chips.iter().find(|u| {
                        u.id.is_none() && u.variant.as_ref().is_some_and(|v| v.kind() == chip.kind)
                    });
                }
            }

            if let Some(u) = specific_update {
                if u.variant.is_some() {
                    chip_update.variant = u.variant.clone();
                }
                if u.enabled.is_some() {
                    chip_update.enabled = u.enabled;
                }
            }

            if chip_update.pose.position.is_some()
                || chip_update.pose.orientation.is_some()
                || chip_update.variant.is_some()
                || chip_update.enabled.is_some()
            {
                info!(
                    "DeviceActor: Updating chip {} (kind {}) with {:?}",
                    chip.id, chip.kind, chip_update
                );
                *chip = chip_client.update(ChipId(chip.id), chip_update).await?;
            }
        }
        Ok(entity.device.clone())
    }

    async fn handle_delete(
        &mut self,
        id: Self::Id,
        ctx: &mut DynContext<Self>,
    ) -> Result<(), Self::Error> {
        self.perform_device_deletion(id, ctx).await
    }

    async fn handle_action(
        &mut self,
        id: Option<Self::Id>,
        action: Self::Action,
        ctx: &mut DynContext<Self>,
    ) -> Result<Self::ActionResult, Self::Error> {
        // Handle actions that don't require looking up the entity first
        match action {
            DeviceAction::GetRadioStats => {
                let stats = self.collect_radio_stats_async().await;
                Ok(DeviceActionResult::Statistics(stats))
            }
            DeviceAction::SaveStats => {
                self.save_stats_async().await;
                Ok(DeviceActionResult::Success)
            }
            DeviceAction::Reset => {
                self.perform_reset(id, ctx).await?;
                Ok(DeviceActionResult::Success)
            }
            DeviceAction::AddChipByGuid { params } => {
                self.perform_add_chip_by_guid(*params, ctx).await
            }
            DeviceAction::NotifyChipRemoved(device_id, chip_id) => {
                // Verify ID match if provided
                if let Some(id) = id
                    && id != device_id
                {
                    return Err(DeviceError::NotFound(format!(
                        "Device ID mismatch: {} vs {}",
                        id, device_id
                    )));
                }

                self.perform_chip_removal(device_id, chip_id, ctx).await?;
                Ok(DeviceActionResult::Success)
            }

            DeviceAction::DeleteDevice(id) => {
                let Some(internal_device) = self.devices.get(&id) else {
                    return Err(DeviceError::NotFound(format!("Device not found: {}", id)));
                };
                if internal_device.guid.is_some() {
                    return Err(DeviceError::NotFound("Cannot delete external device".to_string()));
                }
                self.perform_device_deletion(id, ctx).await?;
                Ok(DeviceActionResult::Success)
            }
            DeviceAction::AddChip { chip_config, packet_stream, packet_sink } => {
                let Some(id) = id else {
                    return Err(DeviceError::NotFound("AddChip requires a device ID".into()));
                };
                let Some(entity) = self.devices.get_mut(&id) else {
                    return Err(DeviceError::DeviceNotFound(id.to_string()));
                };

                // Convert API DeviceChipCreate to Model Chip
                let chip: netsim_model::Chip = (*chip_config).into();
                let chip_id_res = Self::perform_add_chip(
                    &self.next_chip_id,
                    &self.chip_clients,
                    self.link_client.as_ref(),
                    &self.capture_client,
                    entity,
                    chip,
                    packet_stream,
                    packet_sink,
                )
                .await;
                match chip_id_res {
                    Ok(chip_id) => {
                        self.save_stats_async().await;
                        Ok(DeviceActionResult::ChipId(chip_id))
                    }
                    Err(e) => Err(e),
                }
            }
        }
    }

    async fn handle_list(
        &mut self,
        _ctx: &mut DynContext<Self>,
    ) -> Result<Vec<Self::Entity>, Self::Error> {
        Ok(self.devices.values().map(|e| e.device.clone()).collect())
    }
}
