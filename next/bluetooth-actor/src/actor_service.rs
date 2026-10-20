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
use netsim_model::{BluetoothMode, Chip, ChipCreate, ChipId, ChipUpdate, ChipVariant};
use tracing::{info, warn};

use crate::{
    BluetoothEvent,
    actions::{BluetoothAction, BluetoothActionResult},
    bluetooth_actor::BluetoothActor,
    error::BluetoothError,
    hci_callbacks::HciCallbacks,
    utils::ToChipError,
};

const SINK_CHANNEL_CAPACITY: usize = 100;

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

        // Construct the Protobuf configuration for Rootcanal
        let is_device = matches!(&mode, BluetoothMode::Device(_));
        let config_bytes = bluetooth
            .bt_properties
            .config
            .serialize_config(is_device)
            .map_err(BluetoothError::invalid_arg)?;

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
            // Bounded to SINK_CHANNEL_CAPACITY packets (~100ms of lag absorption at 2Mbps) to:
            // - Tolerate transient guest lag.
            // - Protect netsimd from OOM crashes if the client becomes unresponsive.
            let (tx, rx) = tokio::sync::mpsc::channel(SINK_CHANNEL_CAPACITY);

            // Spawn the sink task which forwards packets from the channel to the sink.
            let sink_id = chip_id;
            _ctx.spawn(sink_id, Box::pin(crate::hci_callbacks::sink_loop(sink, rx, sink_id)));

            if let BluetoothMode::Sniffer(_) = &mode {
                HciCallbacks::with_ll(chip_id, tx)
            } else {
                HciCallbacks::with_hci(chip_id, tx)
            }
        } else {
            HciCallbacks::new(chip_id)
        };

        // 3. Create Rootcanal Controller
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
        let should_update_properties = matches!(
            &update.variant,
            Some(netsim_model::ChipVariantUpdate::Bluetooth(bt_update))
                if bt_update.preset.is_some() || bt_update.bt_properties.is_some()
        );

        let mut chip = {
            let chips = self.chips.read();
            chips.get(&id).cloned().ok_or(BluetoothError::not_found(id))?
        };
        update.apply(&mut chip);

        if should_update_properties {
            self.apply_chip_properties(id, &chip)?;
        }

        self.chips.write().insert(id, chip.clone());
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
            self.initial_chips.remove(&id);

            // Notify DeviceService
            let dc = self.device_client.clone();
            if device_id.0 != 0 {
                tokio::spawn(async move {
                    let _ = dc.notify_chip_removed(device_id, chip_id).await;
                });
            }
            Ok(())
        } else {
            Err(BluetoothError::not_found(id))
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

                let initial_chip =
                    self.initial_chips.get(&id).ok_or(BluetoothError::not_found(id))?;

                self.apply_chip_properties(id, initial_chip)?;

                let chip = {
                    let mut chips = self.chips.write();
                    let chip = chips.get_mut(&id).ok_or(BluetoothError::not_found(id))?;
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
            if let Err(error) = dc.update(device_id, update).await {
                warn!(?error, "Failed to sync device name for device {}", device_id);
            }
        });
    }

    /// Serializes the chip's Bluetooth controller properties and applies them to RootCanal.
    fn apply_chip_properties(&self, id: ChipId, chip: &Chip) -> Result<(), BluetoothError> {
        if let Some(ChipVariant::Bluetooth(bt)) = &chip.variant {
            let is_device = matches!(&bt.mode, BluetoothMode::Device(_));
            let bytes = bt
                .bt_properties
                .config
                .serialize_config(is_device)
                .map_err(BluetoothError::invalid_arg)?;
            self.rootcanal
                .set_properties(id.0, &bytes)
                .map_err(|e| BluetoothError::Rootcanal(Box::new(e)))?;
        }
        Ok(())
    }
}
