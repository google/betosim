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
use futures::SinkExt;
use modem_rs::ModemSink;
use netsim_model::{
    Cell, CellNetworkConfig, Chip, ChipCreate, ChipError, ChipId, ChipKind, ChipUpdate,
    ChipVariant, ChipVariantUpdate, MODEM_STATE_DOWN, MODEM_STATE_IDLE, MODEM_STATE_RINGING,
    ModemAction, Quirks, Radio,
};
use tracing::{debug, error, info};

use crate::{
    CellAction, CellActionResult,
    cell_actor::{CellActor, ChipState},
    error::CellError,
};

impl CellActor {
    fn lease_to_network_configs(lease: &slirp_actor::SlirpLease) -> Vec<CellNetworkConfig> {
        let mut network_configs = vec![CellNetworkConfig {
            ip_address: std::net::IpAddr::V4(lease.v4.ip_address),
            prefixlen: lease.v4.prefixlen,
            gateway: std::net::IpAddr::V4(lease.v4.gateway),
            dns: std::net::IpAddr::V4(lease.v4.dns),
        }];
        if let Some(v6) = lease.v6 {
            network_configs.push(CellNetworkConfig {
                ip_address: std::net::IpAddr::V6(v6.ip_address),
                prefixlen: v6.prefixlen,
                gateway: std::net::IpAddr::V6(v6.gateway),
                dns: std::net::IpAddr::V6(v6.dns),
            });
        }
        network_configs
    }
}

impl ActorService for CellActor {
    type Id = ChipId;
    type Create = ChipCreate;
    type Update = ChipUpdate;
    type Action = CellAction;
    type ActionResult = CellActionResult;
    type Error = CellError;
    type Entity = Chip;
    type TypedStream = modem_rs::HostEvent;

    async fn handle_create(
        &mut self,
        id: Option<Self::Id>,
        mut params: Self::Create,
        ctx: &mut DynContext<Self>,
    ) -> Result<Self::Id, Self::Error> {
        let chip_id = id.ok_or_else(|| ChipError::InvalidArguments("missing chip id".into()))?;
        let device_id = params.chip.device_id;

        if self.active_chips.contains_key(&chip_id) {
            return Err(CellError::Chip(ChipError::ChipExists(chip_id.0)));
        }

        let (sim_type, sim_profile, quirks, mut network_configs) = match params
            .chip
            .variant
            .as_ref()
        {
            Some(ChipVariant::Cell(cell)) => {
                (cell.sim_type, cell.sim_profile.clone(), cell.quirks, cell.network_configs.clone())
            }
            _ => (None, None, Quirks::default(), Vec::new()),
        };

        let mut sink = params.packet_sink.take().ok_or(CellError::MissingStreamSink)?;
        let stream = params.packet_stream.take().ok_or(CellError::MissingStreamSink)?;

        let mut slirp_allocated = false;
        if quirks.dynamic_slirp_lease && network_configs.is_empty() {
            match self.slirp_client.allocate_lease(chip_id).await {
                Ok(Some(lease)) => {
                    network_configs.extend(Self::lease_to_network_configs(&lease));
                    slirp_allocated = true;
                }
                Ok(None) => {
                    return Err(CellError::Chip(ChipError::Internal(
                        format!("Slirp static IP pool exhausted for chip {chip_id}").into(),
                    )));
                }
                Err(e) => {
                    return Err(CellError::Chip(ChipError::Internal(
                        format!("Slirp allocate_lease failed for chip {chip_id}: {e}").into(),
                    )));
                }
            }
        }

        // Bridge Async PacketSink to Sync ModemSink
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<bytes::Bytes>();
        let modem_sink = ModemSink::new(move |b| tx.send(b).map_err(|e| e.to_string()));

        // Spawn forwarder task
        ctx.spawn(
            chip_id,
            Box::pin(async move {
                while let Some(packet) = rx.recv().await {
                    debug!(
                        "CellActor forwarder: raw packet for chip {}: {:?}",
                        chip_id,
                        std::str::from_utf8(&packet)
                    );
                    // Note: If the underlying transport is a PTY, ensure ONLCR processing
                    // is disabled on the PTY descriptor (e.g. at PTY creation site when opened
                    // via openpty/tcsetattr for Casimir or emulator bridge) to prevent '\n' to
                    // '\r\n' expansion which corrupts the packet framing.
                    if let Err(e) = sink.send(packet).await {
                        error!("PacketSink send error: {}", e);
                    }
                }
                chip_id
            }),
        );

        // 1. Add Stream
        ctx.add_stream(chip_id, Box::pin(stream));

        // 2. Add to Controller directly (Sync)
        if let Err(e) = self.controller.add_modem(
            chip_id.0,
            modem_sink,
            sim_type,
            sim_profile,
            quirks,
            network_configs,
        ) {
            if slirp_allocated {
                let _ = self.slirp_client.release_lease(chip_id).await;
            }
            return Err(CellError::ModemError(e));
        }

        self.active_chips.insert(chip_id, ChipState { device_id, enabled: true });

        Ok(chip_id)
    }

    async fn handle_delete(
        &mut self,
        id: Self::Id,
        _ctx: &mut DynContext<Self>,
    ) -> Result<(), Self::Error> {
        // Remove from local state
        if let Some(state) = self.active_chips.remove(&id) {
            info!("Deleting chip {}", id);
            let _ = self.slirp_client.release_lease(id).await;
            // Remove from controller
            if let Err(e) = self.controller.remove_modem(id.0) {
                error!("Failed to remove modem: {:?}", e);
            }

            // Notify DeviceClient asynchronously
            let dc = self.device_client.clone();
            let device_id = state.device_id;
            tokio::spawn(async move {
                let _ = dc.notify_chip_removed(device_id, id).await;
            });
        }
        Ok(())
    }

    async fn handle_get(
        &self,
        id: Self::Id,
        _ctx: &mut DynContext<Self>,
    ) -> Result<Option<Self::Entity>, Self::Error> {
        if let Ok(info) = self.controller.get_modem_info(id.0) {
            let enabled = self.active_chips.get(&id).map(|s| s.enabled).unwrap_or(true);
            Ok(Some(Chip {
                kind: ChipKind::CELLULAR,
                id: info.id,
                name: format!("modem-{}", info.id),
                enabled,
                variant: Some(ChipVariant::Cell(Cell {
                    radio: Radio { state: Some(enabled), ..Default::default() },
                    state: if !enabled {
                        MODEM_STATE_DOWN.to_string()
                    } else if info.ringing {
                        MODEM_STATE_RINGING.to_string()
                    } else {
                        MODEM_STATE_IDLE.to_string()
                    },
                    sim_type: None,
                    sim_profile: None,
                    quirks: info.quirks,
                    sms_count: info.sms_count,
                    rssi: info.rssi,
                    ber: info.ber,
                    voice_registration: info.voice_registration,
                    data_registration: info.data_registration,
                    active_calls: info.calls,
                    network_configs: info.network_configs,
                })),
                ..Default::default()
            }))
        } else {
            Ok(None)
        }
    }

    async fn handle_update(
        &mut self,
        id: Self::Id,
        update: Self::Update,
        ctx: &mut DynContext<Self>,
    ) -> Result<Self::Entity, Self::Error> {
        let state = self
            .active_chips
            .get_mut(&id)
            .ok_or_else(|| CellError::Chip(ChipError::ChipNotFound(id)))?;

        if let Some(enabled) = update.enabled {
            state.enabled = enabled;
        }
        if let Some(ChipVariantUpdate::Cell(cell_update)) = &update.variant {
            if let Some(configs) = &cell_update.network_configs
                && let Err(e) = self.controller.update_network_configs(id.0, configs.clone())
            {
                error!("Failed to update network configs for chip {id}: {e:?}");
                return Err(CellError::ModemError(e));
            }
            if let Some(s) = &cell_update.state {
                state.enabled = s != "down";
            }
            if let Some(enabled) = cell_update.radio.state {
                state.enabled = enabled;
            }
        }

        self.handle_get(id, ctx)
            .await?
            .ok_or_else(|| CellError::Chip(netsim_model::ChipError::ChipNotFound(id)))
    }

    async fn handle_action(
        &mut self,
        id: Option<Self::Id>,
        action: Self::Action,
        _ctx: &mut DynContext<Self>,
    ) -> Result<Self::ActionResult, Self::Error> {
        let chip_id = id.ok_or_else(|| {
            CellError::Chip(ChipError::InvalidArguments("missing chip id".into()))
        })?;

        if !self.active_chips.contains_key(&chip_id) {
            return Err(CellError::Chip(ChipError::ChipNotFound(chip_id)));
        }

        action.validate().map_err(CellError::Chip)?;

        let modem_action = match action {
            CellAction::IncomingCall { number } => {
                ModemAction::IncomingCall { target_id: chip_id, number }
            }
            CellAction::UpdateCall => ModemAction::UpdatePhysicalChannelConfigs { id: chip_id },
            CellAction::EndCall => ModemAction::RemoteHangup { id: chip_id },
            CellAction::ReceiveSms { sender, text } => {
                ModemAction::IncomingSms { id: chip_id, sender, text }
            }
            CellAction::ReceivePdu { pdu } => ModemAction::IncomingPdu { id: chip_id, pdu },
            CellAction::InjectCellBroadcast { serial_number, message_id, dcs, text } => {
                ModemAction::InjectCellBroadcast {
                    id: chip_id,
                    serial_number,
                    message_id,
                    dcs,
                    text,
                }
            }
            CellAction::SetSignalStrength { rssi, ber } => {
                let rssi_u8 = u8::try_from(rssi).map_err(|e| {
                    CellError::Chip(ChipError::InvalidArguments(
                        format!("rssi out of range ({}): {}", rssi, e).into(),
                    ))
                })?;
                let ber_u8 = u8::try_from(ber).map_err(|e| {
                    CellError::Chip(ChipError::InvalidArguments(
                        format!("ber out of range ({}): {}", ber, e).into(),
                    ))
                })?;
                ModemAction::SetSignalStrength { id: chip_id, rssi: rssi_u8, ber: ber_u8 }
            }
            CellAction::SetVoiceRegistration { status } => {
                ModemAction::SetVoiceRegistration { id: chip_id, status }
            }
            CellAction::SetDataRegistration { status } => {
                ModemAction::SetDataRegistration { id: chip_id, status }
            }
            CellAction::RemoteAnswer => ModemAction::RemoteAnswer { id: chip_id },
            CellAction::RemoteHold { on_hold } => ModemAction::RemoteHold { id: chip_id, on_hold },
            CellAction::SetSimStatus { present } => {
                ModemAction::SetSimStatus { id: chip_id, present }
            }
            CellAction::SetNetworkTechnology { tech } => {
                ModemAction::SetNetworkTechnology { id: chip_id, tech }
            }
            CellAction::SetOperator { operator } => {
                ModemAction::SetOperator { id: chip_id, operator }
            }
        };

        self.controller.perform_action(modem_action)?;

        Ok(CellActionResult::Success)
    }

    async fn handle_list(
        &mut self,
        _ctx: &mut DynContext<Self>,
    ) -> Result<Vec<Self::Entity>, Self::Error> {
        let mut chips = Vec::new();
        for (id, state) in &self.active_chips {
            if let Ok(info) = self.controller.get_modem_info(id.0) {
                chips.push(Chip {
                    kind: ChipKind::CELLULAR,
                    id: info.id,
                    name: format!("modem-{}", info.id),
                    enabled: state.enabled,
                    variant: Some(ChipVariant::Cell(Cell {
                        radio: Radio { state: Some(state.enabled), ..Default::default() },
                        state: if !state.enabled {
                            MODEM_STATE_DOWN.to_string()
                        } else if info.ringing {
                            MODEM_STATE_RINGING.to_string()
                        } else {
                            MODEM_STATE_IDLE.to_string()
                        },
                        sim_type: None,
                        sim_profile: None,
                        quirks: info.quirks,
                        sms_count: info.sms_count,
                        rssi: info.rssi,
                        ber: info.ber,
                        voice_registration: info.voice_registration,
                        data_registration: info.data_registration,
                        active_calls: info.calls,
                        network_configs: info.network_configs,
                    })),
                    ..Default::default()
                });
            }
        }
        Ok(chips)
    }
}
