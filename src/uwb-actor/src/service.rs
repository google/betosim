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

use std::sync::atomic::{AtomicU64, Ordering};

use actor_framework::{ActorService, DynContext};
use bytes::Bytes;
use futures::{FutureExt, SinkExt};
use netsim_model::{Chip, ChipCreate, ChipError, ChipId, ChipUpdate, ChipVariant, UwbMode};
use pdl_runtime::Packet;
use pica::{Handle, MacAddress, packets::uci};
use tokio::sync::mpsc;

use crate::{
    UwbAction, UwbActionResult,
    error::UwbError,
    uwb_actor::{MAX_UNICAST_SHORT_MAC, UwbActor, UwbChipState},
};

const BROADCAST_SHORT_MAC: u16 = u16::MAX;

impl UwbActor {
    fn allocate_anchor_mac(&mut self) -> Result<(Handle, MacAddress), ChipError> {
        for _ in 0..=MAX_UNICAST_SHORT_MAC {
            let candidate = self.next_anchor_mac;
            self.next_anchor_mac = candidate.checked_sub(1).unwrap_or(MAX_UNICAST_SHORT_MAC);
            let mac = MacAddress::Short(candidate.to_be_bytes());
            if let Ok(handle) = self.pica.create_anchor(mac) {
                return Ok((handle, mac));
            }
        }
        Err(ChipError::InvalidArguments("No available UWB short MAC addresses".into()))
    }

    fn create_anchor(&mut self, address: &str) -> Result<(Handle, MacAddress), UwbError> {
        if address.is_empty() {
            return Ok(self.allocate_anchor_mac()?);
        }
        let mac = match MacAddress::try_from(address.to_owned()) {
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

        let handle = self.pica.create_anchor(mac).map_err(|_err| {
            ChipError::InvalidArguments(format!("MAC address '{address}' is already in use").into())
        })?;
        Ok((handle, mac))
    }

    pub(crate) fn inject_uci(&mut self, handle: Handle, packet: &[u8], ctx: &mut DynContext<Self>) {
        self.pica.handle_packet(handle, packet);
        self.drain_packets();
        self.reschedule_tick(ctx);
    }

    fn on_ranging_tick(&mut self, ctx: &mut DynContext<Self>) {
        self.ranging_timer = None;
        self.pica.tick();
        self.drain_packets();
        self.reschedule_tick(ctx);
    }

    fn reschedule_tick(&mut self, ctx: &mut DynContext<Self>) {
        if let Some(key) = self.ranging_timer.take() {
            ctx.cancel_timer(key);
        }
        if let Some(delay) = self.pica.next_tick() {
            self.ranging_timer =
                Some(ctx.run_later(delay, Box::new(|actor, ctx| actor.on_ranging_tick(ctx))));
        }
    }

    fn drain_packets(&mut self) {
        let chip_states = self.chip_states.read();
        for (handle, packet) in self.pica.drain_packets() {
            if let Some(state) = chip_states.get(&handle) {
                state.rx_count.fetch_add(1, Ordering::Relaxed);
                if let Ok(count) = count_p2p_ranging_measurements(&packet)
                    && count > 0
                {
                    // A single ranging cycle involves multiple packets transmitted and
                    // received. Incrementing both TX and RX by the same cycle count is
                    // an approximation of P2P activity.
                    state.p2p_tx_count.fetch_add(count, Ordering::Relaxed);
                    state.p2p_rx_count.fetch_add(count, Ordering::Relaxed);
                }
                parse_and_count_uci_data_receive(&packet, &self.uwb_stats);
                if let Some(tx) = &state.packet_tx {
                    let _ = tx.send(Bytes::from(packet));
                }
            }
        }
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
        ctx: &mut DynContext<Self>,
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

        let (handle, packet_tx) = match uwb.mode {
            UwbMode::Anchor => {
                if packet_stream.is_some() || packet_sink.is_some() {
                    return Err(ChipError::InvalidArguments(
                        "Anchors cannot have packet stream or sink".into(),
                    )
                    .into());
                }
                let (handle, mac) = self.create_anchor(&uwb.address)?;
                uwb.address = mac.to_string();
                (handle, None)
            }
            UwbMode::Device => {
                let stream = packet_stream.ok_or(UwbError::PacketStreamMissing)?;
                let mut sink = Box::pin(packet_sink.ok_or(UwbError::PacketSinkMissing)?);
                let handle = self.pica.add_device();
                let (sink_tx, mut sink_rx) = mpsc::unbounded_channel::<Bytes>();

                ctx.add_stream(chip_id, Box::pin(stream));
                ctx.spawn(
                    chip_id,
                    async move {
                        while let Some(p) = sink_rx.recv().await {
                            if sink.send(p).await.is_err() {
                                break;
                            }
                        }
                        let _ = sink.close().await;
                        chip_id
                    }
                    .boxed(),
                );

                (handle, Some(sink_tx))
            }
        };

        self.chip_states.write().insert(
            handle,
            UwbChipState {
                chip: chip.clone(),
                tx_count: AtomicU64::new(0),
                rx_count: AtomicU64::new(0),
                p2p_tx_count: AtomicU64::new(0),
                p2p_rx_count: AtomicU64::new(0),
                packet_tx,
            },
        );
        self.initial_chips.insert(chip_id, chip);
        self.chip_to_handle.insert(chip_id, handle);

        self.drain_packets();
        self.reschedule_tick(ctx);

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
            self.pica.destroy_anchor(handle);
        } else {
            self.pica.remove_device(handle);
        }
        ctx.remove_stream(id);
        ctx.abort(id);

        let device_client = self.device_client.clone();
        let device_id = state.chip.device_id;
        tokio::spawn(async move {
            let _ = device_client.notify_chip_removed(device_id, id).await;
        });

        self.drain_packets();
        self.reschedule_tick(ctx);

        Ok(())
    }

    async fn handle_action(
        &mut self,
        _id: Option<Self::Id>,
        action: Self::Action,
        ctx: &mut DynContext<Self>,
    ) -> Result<Self::ActionResult, Self::Error> {
        match action {
            UwbAction::Reset { id } => {
                let &handle = self.chip_to_handle.get(&id).ok_or(ChipError::ChipNotFound(id))?;
                let initial_chip =
                    self.initial_chips.get(&id).ok_or(ChipError::ChipNotFound(id))?;
                let (chip, is_anchor) = {
                    let mut chips = self.chip_states.write();
                    let state = chips.get_mut(&handle).ok_or(ChipError::ChipNotFound(id))?;
                    state.chip = initial_chip.clone();
                    (state.chip.clone(), state.is_anchor())
                };
                if !is_anchor {
                    let reset_cmd =
                        uci::CoreDeviceResetCmd { reset_config: uci::ResetConfig::UwbsReset };
                    let bytes = reset_cmd.encode_to_vec().expect("encoding succeeds");
                    self.inject_uci(handle, &bytes, ctx);
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
                let &handle = self.chip_to_handle.get(&id).ok_or(ChipError::ChipNotFound(id))?;
                self.check_not_anchor(handle)?;
                let start_cmd = uci::SessionStartCmd { session_id };
                let bytes = start_cmd.encode_to_vec().expect("encoding succeeds");
                self.inject_uci(handle, &bytes, ctx);
                Ok(UwbActionResult::Success)
            }
            #[cfg(any(test, feature = "testing"))]
            UwbAction::StopRanging { id, session_id } => {
                let &handle = self.chip_to_handle.get(&id).ok_or(ChipError::ChipNotFound(id))?;
                self.check_not_anchor(handle)?;
                let stop_cmd = uci::SessionStopCmd { session_id };
                let bytes = stop_cmd.encode_to_vec().expect("encoding succeeds");
                self.inject_uci(handle, &bytes, ctx);
                Ok(UwbActionResult::Success)
            }
            UwbAction::GetGlobalStats => {
                use netsim_proto::protobuf::Message;
                let proto = self.uwb_stats.to_proto();
                let bytes = proto.write_to_bytes().map_err(|e| {
                    UwbError::Internal(Box::from(format!("Failed to serialize UwbStats: {e}")))
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
        ControlPacket, ControlPacketChild, SessionControlPacketChild, SessionInfoNtfChild,
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

    match info.specialize() {
        Ok(SessionInfoNtfChild::ShortMacTwoWaySessionInfoNtf(m)) => {
            Ok(m.two_way_ranging_measurements.len() as u64)
        }
        Ok(SessionInfoNtfChild::ExtendedMacTwoWaySessionInfoNtf(m)) => {
            Ok(m.two_way_ranging_measurements.len() as u64)
        }
        Ok(SessionInfoNtfChild::ShortMacOwrAoaSessionInfoNtf(m)) => {
            Ok(m.owr_aoa_ranging_measurements.len() as u64)
        }
        Ok(SessionInfoNtfChild::ExtendedMacOwrAoaSessionInfoNtf(m)) => {
            Ok(m.owr_aoa_ranging_measurements.len() as u64)
        }
        Ok(SessionInfoNtfChild::ShortMacDlTDoASessionInfoNtf(m)) => {
            Ok(m.dl_tdoa_measurements.len() as u64)
        }
        Ok(SessionInfoNtfChild::ExtendedMacDlTDoASessionInfoNtf(m)) => {
            Ok(m.dl_tdoa_measurements.len() as u64)
        }
        _ => Err(()),
    }
}

const UCI_MT_SHIFT: u8 = 5;
const UCI_MT_MASK: u8 = 0x07;
const UCI_GID_OR_DPF_MASK: u8 = 0x0F;
const UCI_MT_DATA: u8 = 0;
const UCI_MT_COMMAND: u8 = 1;
const UCI_DPF_DATA_MESSAGE_SND: u8 = 1;
const UCI_DPF_DATA_MESSAGE_RCV: u8 = 2;

pub(crate) fn parse_and_count_uci_command(bytes: &[u8], stats: &crate::stats::UwbStats) {
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
        assert_eq!(actor.allocate_anchor_mac().unwrap().1, MacAddress::Short([0x00, 0x00]));
        assert_eq!(actor.allocate_anchor_mac().unwrap().1, MacAddress::Short([0xFF, 0xFE]));
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

    fn mock_dl_tdoa_measurement() -> uci::DlTdoaRangingMeasurement {
        uci::DlTdoaRangingMeasurement {
            tx_timestamp_40: Some(0),
            rx_timestamp_40: Some(0),
            ..Default::default()
        }
    }

    #[test]
    fn test_parsing_dl_tdoa_short_mac() {
        let ntf = uci::ShortMacDlTDoASessionInfoNtf {
            sequence_number: 0,
            session_token: 0,
            rcr_indicator: 0,
            current_ranging_interval: 0,
            dl_tdoa_measurements: vec![
                uci::ShortAddressDlTdoaRangingMeasurement {
                    mac_address: 0,
                    measurement: mock_dl_tdoa_measurement(),
                },
                uci::ShortAddressDlTdoaRangingMeasurement {
                    mac_address: 1,
                    measurement: mock_dl_tdoa_measurement(),
                },
            ],
        };
        let parent: SessionInfoNtf = ntf.try_into().unwrap();
        let root: SessionControlPacket = parent.try_into().unwrap();
        let final_pkt: ControlPacket = root.try_into().unwrap();

        let mut bytes = Vec::new();
        final_pkt.encode(&mut bytes).unwrap();

        assert_eq!(count_p2p_ranging_measurements(&bytes), Ok(2));
    }

    #[test]
    fn test_parsing_dl_tdoa_extended_mac() {
        let ntf = uci::ExtendedMacDlTDoASessionInfoNtf {
            sequence_number: 0,
            session_token: 0,
            rcr_indicator: 0,
            current_ranging_interval: 0,
            dl_tdoa_measurements: vec![
                uci::ExtendedAddressDlTdoaRangingMeasurement {
                    mac_address: 0,
                    measurement: mock_dl_tdoa_measurement(),
                },
                uci::ExtendedAddressDlTdoaRangingMeasurement {
                    mac_address: 1,
                    measurement: mock_dl_tdoa_measurement(),
                },
                uci::ExtendedAddressDlTdoaRangingMeasurement {
                    mac_address: 2,
                    measurement: mock_dl_tdoa_measurement(),
                },
            ],
        };
        let parent: SessionInfoNtf = ntf.try_into().unwrap();
        let root: SessionControlPacket = parent.try_into().unwrap();
        let final_pkt: ControlPacket = root.try_into().unwrap();

        let mut bytes = Vec::new();
        final_pkt.encode(&mut bytes).unwrap();

        assert_eq!(count_p2p_ranging_measurements(&bytes), Ok(3));
    }
}
