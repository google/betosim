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

use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

use actor_framework::{ActorService, DynContext};
use bytes::Bytes;
use futures::{FutureExt, SinkExt, StreamExt};
use netsim_model::{
    Chip, ChipCreate, ChipError, ChipId, ChipUpdate, ChipVariant, PacketSink, PacketStream, UwbMode,
};
use pdl_runtime::Packet;
use pica::{Handle, MacAddress, PicaCommand, PicaEvent, packets::uci};
use tokio::sync::oneshot;

use crate::{
    UwbAction, UwbActionResult,
    error::UwbError,
    uwb_actor::{MAX_UNICAST_SHORT_MAC, UwbActor, UwbChipState},
};

const BROADCAST_SHORT_MAC: u16 = u16::MAX;

impl UwbActor {
    fn is_mac_in_use(&self, mac: MacAddress) -> bool {
        self.chip_states.read().values().any(|s| s.mac_address == mac)
    }

    fn allocate_anchor_mac(&mut self) -> Result<MacAddress, ChipError> {
        for _ in 0..=MAX_UNICAST_SHORT_MAC {
            let candidate = self.next_anchor_mac;
            self.next_anchor_mac = candidate.checked_sub(1).unwrap_or(MAX_UNICAST_SHORT_MAC);
            let mac = MacAddress::Short(candidate.to_be_bytes());
            if !self.is_mac_in_use(mac) {
                return Ok(mac);
            }
        }
        Err(ChipError::InvalidArguments("No available UWB short MAC addresses".into()))
    }

    async fn create_anchor(&mut self, address: &str) -> Result<(Handle, MacAddress), UwbError> {
        let mac = if address.is_empty() {
            self.allocate_anchor_mac()?
        } else {
            let parsed = match MacAddress::try_from(address.to_owned()) {
                Ok(mac @ MacAddress::Short(bytes)) => {
                    if bytes == BROADCAST_SHORT_MAC.to_be_bytes() {
                        return Err(ChipError::InvalidArguments(
                            "Broadcast address 0xFFFF cannot be used as anchor MAC".into(),
                        )
                        .into());
                    }
                    mac
                }
                _ => {
                    return Err(ChipError::InvalidArguments(
                        format!("Invalid UWB short anchor address '{address}'").into(),
                    )
                    .into());
                }
            };
            if self.is_mac_in_use(parsed) {
                return Err(ChipError::InvalidArguments(
                    format!("MAC address '{address}' is already in use").into(),
                )
                .into());
            }
            parsed
        };

        let (tx, rx) = oneshot::channel();
        let _ = self.pica_commands.send(PicaCommand::CreateAnchor(mac, tx)).await;
        let handle = rx.await.map_err(|err| UwbError::PicaShutdown(Box::new(err)))??;
        Ok((handle, mac))
    }

    async fn connect_device(
        &mut self,
        packet_stream: Option<PacketStream>,
        packet_sink: Option<PacketSink>,
        tx_count: &Arc<AtomicU64>,
        rx_count: &Arc<AtomicU64>,
        p2p_tx_count: &Arc<AtomicU64>,
        p2p_rx_count: &Arc<AtomicU64>,
    ) -> Result<(Handle, MacAddress), UwbError> {
        let stats_clone = self.uwb_stats.clone();
        let tx_stream_clone = tx_count.clone();
        let stream = packet_stream
            .ok_or(UwbError::PacketStreamMissing)?
            .map(move |b| {
                let v = b.to_vec();
                parse_and_count_uci_command(&v, &stats_clone);
                tx_stream_clone.fetch_add(1, Ordering::Relaxed);
                v
            })
            .boxed();

        let tx_clone = p2p_tx_count.clone();
        let rx_clone = p2p_rx_count.clone();
        let rx_sink_clone = rx_count.clone();

        // Pica wants a Sink<Vec<u8>>.
        let stats_clone_sink = self.uwb_stats.clone();
        let sink =
            Box::pin(packet_sink.ok_or(UwbError::PacketSinkMissing)?.sink_map_err(|_| ()).with(
                move |v: Vec<u8>| {
                    if let Some(count) = count_p2p_ranging_measurements(&v).ok().filter(|&c| c > 0)
                    {
                        // A single ranging cycle involves multiple packets transmitted and
                        // received. Incrementing both TX and RX by the same
                        // cycle count is a reasonable proxy for activity,
                        // but it's an approximation of P2P activity.
                        tx_clone.fetch_add(count, Ordering::Relaxed);
                        rx_clone.fetch_add(count, Ordering::Relaxed);
                    }
                    rx_sink_clone.fetch_add(1, Ordering::Relaxed);
                    parse_and_count_uci_data_receive(&v, &stats_clone_sink);
                    async move { Ok(Bytes::from(v)) }
                },
            ));

        // Clear unrelated events to prevent a lagged error
        self.pica_connect_events = self.pica_connect_events.resubscribe();
        let _ = self.pica_commands.send(PicaCommand::Connect(stream, sink)).await;

        // Wait for add to complete. This guarantees the chip exists by the time any
        // actions are performed on it.
        let (handle, mac_address) = loop {
            if let PicaEvent::Connected { handle, mac_address } = self
                .pica_connect_events
                .recv()
                .await
                .map_err(|err| UwbError::PicaShutdown(Box::new(err)))?
            {
                break (handle, mac_address);
            }
        };

        Ok((handle, mac_address))
    }

    #[cfg(any(test, feature = "testing"))]
    fn check_not_anchor(&self, handle: Handle) -> Result<(), UwbError> {
        if self.chip_states.read().get(&handle).is_some_and(UwbChipState::is_anchor) {
            return Err(ChipError::Unsupported.into());
        }
        Ok(())
    }
}

impl ActorService for UwbActor {
    type Id = ChipId;
    type Create = ChipCreate;
    type Update = ChipUpdate;
    type Action = UwbAction;
    type ActionResult = UwbActionResult;
    type Error = UwbError;
    type Entity = Chip;
    type TypedStream = ();

    async fn handle_create(
        &mut self,
        id: Option<Self::Id>,
        params: Self::Create,
        _ctx: &mut DynContext<Self>,
    ) -> Result<Self::Id, Self::Error> {
        let chip_id = id.ok_or(ChipError::InvalidArguments(Box::from("missing chip id")))?;
        if self.chip_to_handle.contains_key(&chip_id) {
            return Err(ChipError::ChipExists(chip_id.0).into());
        }

        let ChipCreate { mut chip, packet_stream, packet_sink } = params;
        chip.id = chip_id.0;

        let Some(ChipVariant::Uwb(ref mut uwb)) = chip.variant else {
            return Err(ChipError::InvalidArguments("Missing UWB chip variant".into()).into());
        };

        let tx_count = Arc::new(AtomicU64::new(0));
        let rx_count = Arc::new(AtomicU64::new(0));
        let p2p_tx_count = Arc::new(AtomicU64::new(0));
        let p2p_rx_count = Arc::new(AtomicU64::new(0));

        let (handle, mac_address) = match uwb.mode {
            UwbMode::Anchor => {
                if packet_stream.is_some() || packet_sink.is_some() {
                    return Err(ChipError::InvalidArguments(
                        "Anchors cannot have packet stream or sink".into(),
                    )
                    .into());
                }
                self.create_anchor(&uwb.address).await?
            }
            UwbMode::Device => {
                self.connect_device(
                    packet_stream,
                    packet_sink,
                    &tx_count,
                    &rx_count,
                    &p2p_tx_count,
                    &p2p_rx_count,
                )
                .await?
            }
        };

        uwb.address = mac_address.to_string();

        self.chip_states.write().insert(
            handle,
            UwbChipState {
                chip: chip.clone(),
                tx_count,
                rx_count,
                p2p_tx_count,
                p2p_rx_count,
                mac_address,
            },
        );
        self.initial_chips.insert(chip_id, chip);
        self.chip_to_handle.insert(chip_id, handle);

        Ok(chip_id)
    }

    async fn handle_get(
        &self,
        id: Self::Id,
        _ctx: &mut DynContext<Self>,
    ) -> Result<Option<Self::Entity>, Self::Error> {
        let handle = self.chip_to_handle.get(&id).ok_or(ChipError::ChipNotFound(id))?;
        Ok(self.chip_states.read().get(handle).map(|state| state.chip.clone()))
    }

    async fn handle_update(
        &mut self,
        id: Self::Id,
        update: Self::Update,
        _ctx: &mut DynContext<Self>,
    ) -> Result<Self::Entity, Self::Error> {
        let handle = self.chip_to_handle.get(&id).ok_or(ChipError::ChipNotFound(id))?;
        let mut chips = self.chip_states.write();
        let state = chips.get_mut(handle).ok_or(ChipError::ChipNotFound(id))?;

        update.apply(&mut state.chip);

        Ok(state.chip.clone())
    }

    async fn handle_delete(
        &mut self,
        id: Self::Id,
        ctx: &mut DynContext<Self>,
    ) -> Result<(), Self::Error> {
        let handle = self.chip_to_handle.remove(&id).ok_or(ChipError::ChipNotFound(id))?;
        self.initial_chips.remove(&id);
        let state = self.chip_states.write().remove(&handle).ok_or(ChipError::ChipNotFound(id))?;

        if state.is_anchor() {
            let (tx, rx) = oneshot::channel();
            let _ =
                self.pica_commands.send(PicaCommand::DestroyAnchor(state.mac_address, tx)).await;
            match rx.await {
                Ok(Ok(_)) => {}
                Ok(Err(e)) => {
                    tracing::warn!("Failed to destroy Pica anchor {}: {e}", state.mac_address)
                }
                Err(e) => tracing::warn!(
                    "Failed to receive Pica DestroyAnchor response for {}: {e}",
                    state.mac_address
                ),
            }
        } else {
            // Disconnect from Pica. This completes asynchronously as we'll receive a
            // `PicaEvent::Disconnected` event in the lifecycle `on_tick`.
            let _ = self.pica_commands.send(PicaCommand::Disconnect(handle)).await;
        }

        let device_client = self.device_client.clone();
        let device_id = state.chip.device_id;
        ctx.spawn(
            id,
            async move {
                let _ = device_client.notify_chip_removed(device_id, id).await;
                id
            }
            .boxed(),
        );

        Ok(())
    }
    async fn handle_action(
        &mut self,
        _id: Option<Self::Id>,
        action: Self::Action,
        _ctx: &mut DynContext<Self>,
    ) -> Result<Self::ActionResult, Self::Error> {
        match action {
            UwbAction::Reset { id } => {
                let handle = self.chip_to_handle.get(&id).ok_or(ChipError::ChipNotFound(id))?;
                let initial_chip =
                    self.initial_chips.get(&id).ok_or(ChipError::ChipNotFound(id))?;
                let (chip, is_anchor) = {
                    let mut chips = self.chip_states.write();
                    let state = chips.get_mut(handle).ok_or(ChipError::ChipNotFound(id))?;
                    state.chip = initial_chip.clone();
                    (state.chip.clone(), state.is_anchor())
                };
                if !is_anchor {
                    let reset_cmd =
                        uci::CoreDeviceResetCmd { reset_config: uci::ResetConfig::UwbsReset };
                    let _ = self
                        .pica_commands
                        .send(PicaCommand::UciPacket(
                            *handle,
                            reset_cmd.encode_to_vec().expect("encoding succeeds"),
                        ))
                        .await;
                }

                Ok(UwbActionResult::Chip(Box::new(chip)))
            }
            UwbAction::GetStatistics => {
                let stats = self
                    .chip_states
                    .read()
                    .values()
                    .map(|state| {
                        let mut radio_stats = netsim_model::NetsimRadioStats::default();
                        radio_stats.id = state.chip.id;
                        radio_stats.name = state.chip.name.clone();
                        radio_stats.kind = netsim_model::RadioKind::Uwb;
                        radio_stats.tx_count =
                            state.tx_count.load(std::sync::atomic::Ordering::Relaxed);
                        radio_stats.rx_count =
                            state.rx_count.load(std::sync::atomic::Ordering::Relaxed);
                        radio_stats.p2p_tx_count =
                            state.p2p_tx_count.load(std::sync::atomic::Ordering::Relaxed);
                        radio_stats.p2p_rx_count =
                            state.p2p_rx_count.load(std::sync::atomic::Ordering::Relaxed);
                        radio_stats
                    })
                    .collect();
                Ok(UwbActionResult::Statistics(stats))
            }
            #[cfg(any(test, feature = "testing"))]
            UwbAction::StartRanging { id, session_id } => {
                let handle = self.chip_to_handle.get(&id).ok_or(ChipError::ChipNotFound(id))?;
                self.check_not_anchor(*handle)?;
                let _ = self.pica_commands.send(PicaCommand::Ranging(*handle, session_id)).await;
                Ok(UwbActionResult::Success)
            }
            #[cfg(any(test, feature = "testing"))]
            UwbAction::StopRanging { id, session_id } => {
                let handle = self.chip_to_handle.get(&id).ok_or(ChipError::ChipNotFound(id))?;
                self.check_not_anchor(*handle)?;
                let stop_cmd = uci::SessionStopCmd { session_id };
                let _ = self
                    .pica_commands
                    .send(PicaCommand::UciPacket(
                        *handle,
                        stop_cmd.encode_to_vec().expect("encoding succeeds"),
                    ))
                    .await;
                Ok(UwbActionResult::Success)
            }
            UwbAction::GetGlobalStats => {
                use netsim_proto::protobuf::Message;
                let proto = self.uwb_stats.to_proto();
                let bytes = proto.write_to_bytes().map_err(|e| {
                    UwbError::Internal(Box::from(format!("Failed to serialize UwbStats: {}", e)))
                })?;
                Ok(UwbActionResult::GlobalStats(bytes))
            }
        }
    }

    async fn handle_list(
        &mut self,
        _ctx: &mut DynContext<Self>,
    ) -> Result<Vec<Self::Entity>, Self::Error> {
        Ok(self.chip_states.read().values().map(|state| state.chip.clone()).collect())
    }
}

fn count_p2p_ranging_measurements(bytes: &[u8]) -> Result<u64, ()> {
    if bytes.len() < 4 {
        return Err(());
    }
    use pica::packets::uci::{
        ControlPacket, ControlPacketChild, ExtendedMacOwrAoaSessionInfoNtf,
        ExtendedMacTwoWaySessionInfoNtf, SessionControlPacketChild, ShortMacOwrAoaSessionInfoNtf,
        ShortMacTwoWaySessionInfoNtf,
    };

    let Ok((packet, _)) = ControlPacket::decode(bytes) else {
        return Err(());
    };
    let Ok(ControlPacketChild::SessionControlPacket(ntf)) = packet.specialize() else {
        return Err(());
    };
    let Ok(SessionControlPacketChild::SessionInfoNtf(info)) = ntf.specialize() else {
        return Err(());
    };

    if let Ok(m) = ShortMacTwoWaySessionInfoNtf::try_from(&info) {
        return Ok(m.two_way_ranging_measurements.len() as u64);
    }
    if let Ok(m) = ExtendedMacTwoWaySessionInfoNtf::try_from(&info) {
        return Ok(m.two_way_ranging_measurements.len() as u64);
    }
    if let Ok(m) = ShortMacOwrAoaSessionInfoNtf::try_from(&info) {
        return Ok(m.owr_aoa_ranging_measurements.len() as u64);
    }
    if let Ok(m) = ExtendedMacOwrAoaSessionInfoNtf::try_from(&info) {
        return Ok(m.owr_aoa_ranging_measurements.len() as u64);
    }
    Err(())
}

const UCI_MT_SHIFT: u8 = 5;
const UCI_MT_MASK: u8 = 0x07;
const UCI_GID_OR_DPF_MASK: u8 = 0x0F;
const UCI_MT_DATA: u8 = 0;
const UCI_MT_COMMAND: u8 = 1;
const UCI_DPF_DATA_MESSAGE_SND: u8 = 1;
const UCI_DPF_DATA_MESSAGE_RCV: u8 = 2;

fn parse_and_count_uci_command(bytes: &[u8], stats: &crate::stats::UwbStats) {
    use pica::packets::uci::{
        ControlPacket, ControlPacketChild, DataPacket, DataPacketChild, SessionConfigPacketChild,
        SessionControlPacketChild, UpdateMulticastListAction,
    };

    use crate::stats::UwbApi;

    let Some(&b0) = bytes.first() else {
        return;
    };

    let mt = (b0 >> UCI_MT_SHIFT) & UCI_MT_MASK;
    let dpf_or_gid = b0 & UCI_GID_OR_DPF_MASK;

    if mt == UCI_MT_COMMAND {
        if let Ok((packet, _)) = ControlPacket::decode(bytes) {
            match packet.specialize() {
                Ok(ControlPacketChild::SessionConfigPacket(cfg_pkt)) => {
                    match cfg_pkt.specialize() {
                        Ok(SessionConfigPacketChild::SessionInitCmd(_)) => {
                            stats.incr(UwbApi::Open);
                        }
                        Ok(SessionConfigPacketChild::SessionDeinitCmd(_)) => {
                            stats.incr(UwbApi::Close);
                        }
                        Ok(SessionConfigPacketChild::SessionSetAppConfigCmd(_)) => {
                            stats.incr(UwbApi::Reconfigure);
                        }
                        Ok(SessionConfigPacketChild::SessionGetAppConfigCmd(_)) => {
                            stats.incr(UwbApi::SessionGetAppConfig);
                        }
                        Ok(SessionConfigPacketChild::SessionUpdateControllerMulticastListCmd(
                            cmd,
                        )) => match cmd.action {
                            UpdateMulticastListAction::AddControlee
                            | UpdateMulticastListAction::AddControleeWithShortSubSessionKey
                            | UpdateMulticastListAction::AddControleeWithExtendedSubSessionKey => {
                                stats.incr(UwbApi::AddControlee)
                            }
                            UpdateMulticastListAction::RemoveControlee => {
                                stats.incr(UwbApi::RemoveControlee)
                            }
                        },
                        _ => {}
                    }
                }
                Ok(ControlPacketChild::SessionControlPacket(ctrl_pkt)) => {
                    match ctrl_pkt.specialize() {
                        Ok(SessionControlPacketChild::SessionStartCmd(_)) => {
                            stats.incr(UwbApi::Start);
                        }
                        Ok(SessionControlPacketChild::SessionStopCmd(_)) => {
                            stats.incr(UwbApi::Stop);
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
        }
    } else if mt == UCI_MT_DATA
        && dpf_or_gid == UCI_DPF_DATA_MESSAGE_SND
        && let Ok((packet, _)) = DataPacket::decode(bytes)
        && let Ok(DataPacketChild::DataMessageSnd(_)) = packet.specialize()
    {
        stats.incr(UwbApi::SendData);
    }
}

fn parse_and_count_uci_data_receive(bytes: &[u8], stats: &crate::stats::UwbStats) {
    use pica::packets::uci::{DataPacket, DataPacketChild};

    use crate::stats::UwbApi;

    let Some(&b0) = bytes.first() else {
        return;
    };

    let mt = (b0 >> UCI_MT_SHIFT) & UCI_MT_MASK;
    let dpf = b0 & UCI_GID_OR_DPF_MASK;

    if mt == UCI_MT_DATA
        && dpf == UCI_DPF_DATA_MESSAGE_RCV
        && let Ok((packet, _)) = DataPacket::decode(bytes)
        && let Ok(DataPacketChild::DataMessageRcv(_)) = packet.specialize()
    {
        stats.incr(UwbApi::OnDataReceived);
    }
}

#[cfg(test)]
mod tests {
    use pica::packets::uci::{
        ControlPacket, SessionControlPacket, SessionInfoNtf, SessionInitCmd, SessionType,
        ShortAddressTwoWayRangingMeasurement, ShortMacTwoWaySessionInfoNtf, Status,
    };

    use super::*;

    #[tokio::test]
    async fn test_allocate_anchor_mac_wraps_after_zero() {
        let (_device_actor, device_client) = device_actor::new();
        let mut actor = UwbActor::new(device_client);
        actor.next_anchor_mac = 0;
        assert_eq!(actor.allocate_anchor_mac().unwrap(), MacAddress::Short([0x00, 0x00]));
        assert_eq!(actor.allocate_anchor_mac().unwrap(), MacAddress::Short([0xFF, 0xFE]));
    }

    #[test]
    fn test_parsing_empty() {
        assert_eq!(count_p2p_ranging_measurements(&[]), Err(()));
        assert_eq!(count_p2p_ranging_measurements(&[1, 2]), Err(()));
        assert_eq!(count_p2p_ranging_measurements(&[1, 2, 3]), Err(()));
        assert_eq!(count_p2p_ranging_measurements(&[0; 10]), Err(()));
    }

    #[test]
    fn test_parsing_valid_packet() {
        let ntf = ShortMacTwoWaySessionInfoNtf {
            sequence_number: 0,
            session_token: 0,
            rcr_indicator: 0,
            current_ranging_interval: 0,
            two_way_ranging_measurements: vec![
                ShortAddressTwoWayRangingMeasurement {
                    mac_address: 0,
                    status: Status::Ok,
                    nlos: 0,
                    distance: 0,
                    aoa_azimuth: 0,
                    aoa_azimuth_fom: 0,
                    aoa_elevation: 0,
                    aoa_elevation_fom: 0,
                    aoa_destination_azimuth: 0,
                    aoa_destination_azimuth_fom: 0,
                    aoa_destination_elevation: 0,
                    aoa_destination_elevation_fom: 0,
                    slot_index: 0,
                    rssi: 0,
                },
                ShortAddressTwoWayRangingMeasurement {
                    mac_address: 1,
                    status: Status::Ok,
                    nlos: 0,
                    distance: 10,
                    aoa_azimuth: 0,
                    aoa_azimuth_fom: 0,
                    aoa_elevation: 0,
                    aoa_elevation_fom: 0,
                    aoa_destination_azimuth: 0,
                    aoa_destination_azimuth_fom: 0,
                    aoa_destination_elevation: 0,
                    aoa_destination_elevation_fom: 0,
                    slot_index: 1,
                    rssi: 0,
                },
            ],
            vendor_data: vec![],
        };

        let parent: SessionInfoNtf = ntf.try_into().unwrap();
        let root: SessionControlPacket = parent.try_into().unwrap();
        let final_pkt: ControlPacket = root.try_into().unwrap();

        let mut bytes = Vec::new();
        final_pkt.encode(&mut bytes).unwrap();

        assert_eq!(count_p2p_ranging_measurements(&bytes), Ok(2));
    }

    #[test]
    fn test_parsing_invalid_packet() {
        // Construct a SessionInitCmd (valid ControlPacket, but not a SessionInfoNtf)
        let cmd = SessionInitCmd { session_id: 1, session_type: SessionType::FiraRangingSession };
        let final_pkt: ControlPacket = cmd.try_into().unwrap();

        let mut bytes = Vec::new();
        final_pkt.encode(&mut bytes).unwrap();

        assert_eq!(count_p2p_ranging_measurements(&bytes), Err(()));
    }
}
