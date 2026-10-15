// Copyright 2025 The Android Open Source Project
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

use actor_framework::{ActorService, DynContext};
use netsim_model::{BluetoothMode, Chip, ChipCreate, ChipError, ChipId, ChipUpdate, ChipVariant};
use tracing::{info, warn};

use crate::{
    BluetoothEvent,
    actions::{BluetoothAction, BluetoothActionResult},
    bluetooth_actor::BluetoothActor,
    error::BluetoothError,
    hci_callbacks::HciCallbacks,
    utils::ToChipError,
};

impl ActorService for BluetoothActor {
    type Id = ChipId;
    type Create = ChipCreate;
    type Update = ChipUpdate;
    type Action = BluetoothAction;
    type ActionResult = BluetoothActionResult;
    type Error = BluetoothError;
    type Entity = Chip;
    type TypedStream = BluetoothEvent;

    async fn handle_create(
        &mut self,
        id: Option<Self::Id>,
        params: Self::Create,
        _ctx: &mut DynContext<Self>,
    ) -> Result<Self::Id, Self::Error> {
        let chip_id = id.ok_or_else(|| BluetoothError::invalid_arg("missing chip id"))?;

        let variant = params
            .chip
            .variant
            .as_ref()
            .ok_or_else(|| BluetoothError::invalid_arg("Missing chip variant"))?;
        let bluetooth = match variant {
            ChipVariant::Bluetooth(b) => b,
            _ => {
                return Err(BluetoothError::invalid_arg("Expected Bluetooth variant"));
            }
        };

        // Validate Scanner constraints: No PacketStream, Must have PacketSink.
        if let BluetoothMode::Scanner(_) = &bluetooth.mode {
            if params.packet_stream.is_some() {
                return Err(BluetoothError::invalid_arg(
                    "Scanner chip cannot have a packet stream",
                ));
            }
            if params.packet_sink.is_none() {
                return Err(BluetoothError::invalid_arg("Scanner chip must have a packet sink"));
            }
        }

        // Validate Sniffer constraints: No PacketStream, Must have PacketSink.
        if let BluetoothMode::Sniffer(_) = &bluetooth.mode {
            if params.packet_stream.is_some() {
                return Err(BluetoothError::invalid_arg(
                    "Sniffer chip cannot have a packet stream",
                ));
            }
            if params.packet_sink.is_none() {
                return Err(BluetoothError::invalid_arg("Sniffer chip must have a packet sink"));
            }
        }

        let name =
            if params.chip.name.is_empty() && matches!(bluetooth.mode, BluetoothMode::Beacon(_)) {
                let name = crate::beacon_utils::generate_default_name(chip_id.0);
                self.sync_device_name(params.chip.device_id, name.clone());
                name
            } else {
                params.chip.name.clone()
            };

        let raw_address = if bluetooth.address.is_empty() {
            crate::beacon_utils::generate_legacy_address(chip_id.into())
        } else {
            bluetooth.address.clone()
        };

        let address: rootcanal::Address =
            raw_address.parse().map_err(BluetoothError::AddressParse)?;

        let mut mode = bluetooth.mode.clone();
        if let BluetoothMode::Beacon(ref mut beacon_params) = mode
            && beacon_params.ble_beacon.address.is_empty()
        {
            beacon_params.ble_beacon.address = raw_address;
        }

        let mut chip = params.chip;
        chip.id = chip_id.0;
        chip.name = name;

        // Ensure radio states are enabled by default for initial state
        if let Some(netsim_model::ChipVariant::Bluetooth(ref mut bluetooth)) = chip.variant {
            if bluetooth.low_energy.state.is_none() {
                bluetooth.low_energy.state = Some(true);
            }
            if bluetooth.classic.state.is_none() {
                // Beacons are BLE-only and must not respond to classic BR/EDR inquiries
                let is_beacon = matches!(bluetooth.mode, BluetoothMode::Beacon(_));
                bluetooth.classic.state = Some(!is_beacon);
            }
        }

        // 1. Register Stream
        if let Some(stream) = params.packet_stream {
            _ctx.add_stream(chip_id, Box::pin(stream));
        }

        // 2. Setup Sink and Callbacks
        let callback = if let Some(sink) = params.packet_sink {
            // Bounded to 100 packets (~100ms of lag absorption at 2Mbps) to:
            // - Tolerate transient guest lag.
            // - Protect netsimd from OOM crashes if the client becomes unresponsive.
            let (tx, rx) = tokio::sync::mpsc::channel(100);

            // Spawn the sink task which forwards packets from the channel to the sink.
            let sink_id = chip_id;
            _ctx.spawn(sink_id, Box::pin(crate::hci_callbacks::sink_loop(sink, rx, sink_id)));

            if let BluetoothMode::Sniffer(_) = &mode {
                HciCallbacks { id: chip_id, hci_tx: None, ll_tx: Some(tx) }
            } else {
                HciCallbacks { id: chip_id, hci_tx: Some(tx), ll_tx: None }
            }
        } else {
            HciCallbacks { id: chip_id, hci_tx: None, ll_tx: None }
        };

        // 3. Create Rootcanal Controller
        // Construct the Protobuf configuration for Rootcanal
        let mut quirks = netsim_proto::configuration::ControllerQuirks::new();
        // This quirk forces Rootcanal to reject post-init commands from uninitialized
        // hosts, causing a HAL restart that properly unmasks the LE Meta
        // events. We only apply this to actual Emulator Devices, not internal
        // netsim beacons or scanners.
        if let BluetoothMode::Device(_) = &mode {
            quirks.hardware_error_before_reset = Some(true);
        }

        let mut config_controller = netsim_proto::configuration::Controller::new();
        config_controller.quirks = netsim_proto::protobuf::MessageField::some(quirks);

        let config_bytes = netsim_proto::protobuf::Message::write_to_bytes(&config_controller)
            .map_err(|e| {
                BluetoothError::invalid_arg(format!("Failed to serialize bt config: {e}"))
            })?;

        self.rootcanal
            .new_controller(chip_id.0, address, Box::new(callback), Some(&config_bytes))
            .to_chip_error()?;

        // 4. Create Chip Info in Context
        match &mode {
            BluetoothMode::Beacon(params) => {
                crate::beacon::create(&self.rootcanal, chip_id, params, &chip.name)?
            }
            BluetoothMode::Device(params) => {
                crate::device::create(&self.rootcanal, chip_id, params)?
            }
            BluetoothMode::Scanner(params) => {
                crate::scanner::create(&self.rootcanal, chip_id, params)?
            }
            BluetoothMode::Sniffer(params) => {
                crate::sniffer::create(&self.rootcanal, chip_id, params)?
            }
        }
        self.chips.write().insert(chip_id, chip.clone());
        self.initial_chips.insert(chip_id, chip);
        Ok(chip_id)
    }

    async fn handle_get(
        &self,
        id: Self::Id,
        _ctx: &mut DynContext<Self>,
    ) -> Result<Option<Self::Entity>, Self::Error> {
        let chips = self.chips.read();
        Ok(chips.get(&id).cloned())
    }

    // TODO: Implement radio state enforcement (stopping HCI/transmission when
    // disabled).
    async fn handle_update(
        &mut self,
        id: Self::Id,
        update: Self::Update,
        _ctx: &mut DynContext<Self>,
    ) -> Result<Self::Entity, Self::Error> {
        let config_bytes = if let Some(netsim_model::ChipVariantUpdate::Bluetooth(
            netsim_model::BluetoothUpdate { preset: Some(preset_name), .. },
        )) = &update.variant
        {
            let preset = parse_controller_preset(preset_name)?;
            let mut config = netsim_proto::configuration::Controller::new();
            config.set_preset(preset);
            Some(netsim_proto::protobuf::Message::write_to_bytes(&config).map_err(|e| {
                BluetoothError::invalid_arg(format!("Failed to serialize config: {e}"))
            })?)
        } else {
            None
        };

        if let Some(bytes) = config_bytes {
            self.rootcanal
                .set_properties(id.0, &bytes)
                .map_err(|e| BluetoothError::Rootcanal(Box::new(e)))?;
        }

        let chip = {
            let mut chips = self.chips.write();
            let mut chip =
                chips.get(&id).cloned().ok_or(BluetoothError::Chip(ChipError::ChipNotFound(id)))?;

            update.apply(&mut chip);
            chips.insert(id, chip.clone());
            chip
        };

        Ok(chip)
    }

    async fn handle_delete(
        &mut self,
        id: Self::Id,
        _ctx: &mut DynContext<Self>,
    ) -> Result<(), Self::Error> {
        let chip_to_delete = {
            let mut chips = self.chips.write();
            chips.remove(&id)
        };
        if let Some(chip) = chip_to_delete {
            let chip_id = ChipId(chip.id);
            let device_id = chip.device_id;

            info!("Deleting chip {chip_id}");
            self.rootcanal.remove_controller(chip_id.0).to_chip_error()?;

            // Notify DeviceService
            let dc = self.device_client.clone();
            if device_id.0 != 0 {
                tokio::spawn(async move {
                    let _ = dc.notify_chip_removed(device_id, chip_id).await;
                });
            }
            Ok(())
        } else {
            Err(BluetoothError::Chip(ChipError::ChipNotFound(id)))
        }
    }

    async fn handle_action(
        &mut self,
        _id: Option<Self::Id>,
        _action: Self::Action,
        _ctx: &mut DynContext<Self>,
    ) -> Result<Self::ActionResult, Self::Error> {
        match _action {
            BluetoothAction::Reset { id } => {
                info!("Resetting Bluetooth chip {id}");
                let _ = self.rootcanal.clear_stats(id.0);

                let chip = {
                    let mut chips = self.chips.write();
                    let initial_chip = self
                        .initial_chips
                        .get(&id)
                        .ok_or(BluetoothError::Chip(ChipError::ChipNotFound(id)))?;
                    let chip = chips
                        .get_mut(&id)
                        .ok_or(BluetoothError::Chip(ChipError::ChipNotFound(id)))?;
                    *chip = initial_chip.clone();
                    chip.clone()
                };
                Ok(BluetoothActionResult::Chip(Box::new(chip)))
            }
            BluetoothAction::GetStatistics => {
                let mut stats_list = Vec::new();
                let chips_info: Vec<(ChipId, String)> = {
                    let chips = self.chips.read();
                    chips.values().map(|chip| (ChipId(chip.id), chip.name.clone())).collect()
                };
                for (id, name) in chips_info {
                    if let Ok(stats) = self.rootcanal.get_stats(id.0) {
                        // BLE Stats
                        let mut radio_stats = netsim_model::NetsimRadioStats::default();
                        radio_stats.id = id.0;
                        radio_stats.name = name.clone();
                        radio_stats.kind = netsim_model::RadioKind::BluetoothLowEnergy;
                        radio_stats.tx_count = stats.ll_packets_out_ble;
                        radio_stats.rx_count = stats.ll_packets_in_ble;
                        radio_stats.p2p_tx_count = stats.ble_p2p_tx_count;
                        radio_stats.p2p_rx_count = stats.ble_p2p_rx_count;
                        stats_list.push(radio_stats);

                        // Classic Stats
                        let mut radio_stats = netsim_model::NetsimRadioStats::default();
                        radio_stats.id = id.0;
                        radio_stats.name = name;
                        radio_stats.kind = netsim_model::RadioKind::BluetoothClassic;
                        radio_stats.tx_count = stats.ll_packets_out_classic;
                        radio_stats.rx_count = stats.ll_packets_in_classic;
                        radio_stats.p2p_tx_count = stats.classic_p2p_tx_count;
                        radio_stats.p2p_rx_count = stats.classic_p2p_rx_count;
                        stats_list.push(radio_stats);
                    }
                }
                Ok(BluetoothActionResult::Statistics(stats_list.into_boxed_slice()))
            }
            BluetoothAction::GetCountForTesting => {
                let count = self.chips.read().len();
                Ok(BluetoothActionResult::Count(count))
            }
        }
    }

    async fn handle_list(
        &mut self,
        _ctx: &mut DynContext<Self>,
    ) -> Result<Vec<Self::Entity>, Self::Error> {
        let chips = self.chips.read();
        Ok(chips.values().cloned().collect())
    }
}

impl BluetoothActor {
    /// Asynchronously synchronize the device name to match the chip name.
    fn sync_device_name(&self, device_id: netsim_model::DeviceId, name: String) {
        let dc = self.device_client.clone();
        tokio::spawn(async move {
            let update = netsim_model::DeviceUpdate { name: Some(name), ..Default::default() };
            if let Err(e) = dc.update(device_id, update).await {
                warn!("Failed to sync device name for device {}: {:?}", device_id, e);
            }
        });
    }
}

/// Maps a string slice representation of a preset to its Protobuf Enum value.
fn parse_controller_preset(
    preset_name: &str,
) -> Result<netsim_proto::configuration::ControllerPreset, BluetoothError> {
    match preset_name {
        "default" => Ok(netsim_proto::configuration::ControllerPreset::DEFAULT),
        "laird_bl654" => Ok(netsim_proto::configuration::ControllerPreset::LAIRD_BL654),
        "csr_rck_pts_dongle" => {
            Ok(netsim_proto::configuration::ControllerPreset::CSR_RCK_PTS_DONGLE)
        }
        "intel_be200" => Ok(netsim_proto::configuration::ControllerPreset::INTEL_BE200),
        _ => Err(BluetoothError::invalid_arg(format!("Invalid preset: {preset_name}"))),
    }
}
