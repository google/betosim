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

use std::sync::atomic::Ordering;

use actor_framework::{ActorLifecycle, DynContext};
use bytes::Bytes;
use casimir::packets::nci::{
    ControlPacket, ControlPacketChild, CorePacketChild, DeactivationType, RfPacketChild,
};
use pdl_runtime::Packet;
use tokio::io::AsyncWriteExt;
use tracing::{error, info};

use crate::{
    nfc_actor::{NFC_MODE_IDLE, NFC_MODE_LISTEN, NFC_MODE_POLL, NfcActor},
    stats::NfcApi,
};

impl ActorLifecycle for NfcActor {
    async fn on_start(&mut self, _ctx: &mut DynContext<Self>) {
        info!("NfcActor started");
        self.start_casimir();
    }

    async fn on_shutdown(&mut self) {
        if let Some(task) = self.scene_task.take() {
            info!("Shutting down Casimir scene task");
            task.abort();
        }
    }

    async fn on_stream(&mut self, id: Self::Id, message: Bytes, _ctx: &mut DynContext<Self>) {
        let Some(&b0) = message.first() else {
            return;
        };
        let mt = b0 >> 5;
        if mt == 0 {
            // Data Packet
            if message.len() < 3 {
                tracing::warn!(
                    "Failed to decode DataPacket (invalid length), bytes: {:?}",
                    message
                );
            } else if let Some(state) = self.active_chips.get(&id) {
                match state.mode.load(Ordering::Relaxed) {
                    NFC_MODE_POLL => {
                        self.nfc_stats.incr(NfcApi::TagTransceive);
                    }
                    NFC_MODE_LISTEN => {
                        self.nfc_stats.incr(NfcApi::CardEmulationSendResponseApdu);
                    }
                    _ => {}
                }
            }
        } else {
            // Control Packet
            if let Ok(packet) = ControlPacket::decode_full(&message) {
                match packet.specialize() {
                    Ok(ControlPacketChild::CorePacket(core_pkt)) => {
                        if let Ok(CorePacketChild::CoreInitCommand(_)) = core_pkt.specialize() {
                            self.nfc_stats.incr(NfcApi::AdapterEnable);
                        }
                    }
                    Ok(ControlPacketChild::RfPacket(rf_pkt)) => match rf_pkt.specialize() {
                        Ok(RfPacketChild::RfDiscoverCommand(_)) => {
                            self.nfc_stats.incr(NfcApi::AdapterEnableReaderMode);
                            if let Some(state) = self.active_chips.get(&id) {
                                state.mode.store(NFC_MODE_POLL, Ordering::Relaxed);
                            }
                        }
                        Ok(RfPacketChild::RfDiscoverSelectCommand(_)) => {
                            self.nfc_stats.incr(NfcApi::TagConnect);
                            if let Some(state) = self.active_chips.get(&id) {
                                state.mode.store(NFC_MODE_POLL, Ordering::Relaxed);
                            }
                        }
                        Ok(RfPacketChild::RfDeactivateCommand(cmd)) => {
                            match cmd.deactivation_type {
                                DeactivationType::IdleMode => {
                                    self.nfc_stats.incr(NfcApi::AdapterDisableReaderMode);
                                    if let Some(state) = self.active_chips.get(&id) {
                                        state.mode.store(NFC_MODE_IDLE, Ordering::Relaxed);
                                    }
                                }
                                DeactivationType::SleepMode
                                | DeactivationType::SleepAfMode
                                | DeactivationType::Discovery => {
                                    self.nfc_stats.incr(NfcApi::TagClose);
                                    if let Some(state) = self.active_chips.get(&id) {
                                        if cmd.deactivation_type == DeactivationType::Discovery {
                                            state.mode.store(NFC_MODE_POLL, Ordering::Relaxed);
                                        } else {
                                            state.mode.store(NFC_MODE_IDLE, Ordering::Relaxed);
                                        }
                                    }
                                }
                            }
                        }
                        Ok(RfPacketChild::RfSetListenModeRoutingCommand(_)) => {
                            self.nfc_stats.incr(NfcApi::AdapterSetListenModeRouting);
                            if let Some(state) = self.active_chips.get(&id) {
                                state.mode.store(NFC_MODE_LISTEN, Ordering::Relaxed);
                            }
                        }
                        _ => {}
                    },
                    _ => {}
                }
            } else {
                tracing::warn!("Failed to decode ControlPacket, bytes: {:?}", message);
            }
        }

        if let Some(state) = self.active_chips.get_mut(&id)
            && let Err(e) = state.nfc_writer.write_all(&message).await
        {
            error!("Failed to write guest packet to Casimir: {:?}", e);
        }
    }

    async fn on_stream_closed(&mut self, id: Self::Id, ctx: &mut DynContext<Self>) {
        info!("Stream closed for NFC chip {}", id);
        use actor_framework::ActorService;
        let _ = self.handle_delete(id, ctx).await;
    }

    async fn on_task_closed(&mut self, id: Self::Id, ctx: &mut DynContext<Self>) {
        info!("Task closed for NFC chip {}", id);
        use actor_framework::ActorService;
        let _ = self.handle_delete(id, ctx).await;
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc,
        atomic::{AtomicBool, AtomicU8, AtomicU64},
    };

    use actor_framework::Context;
    use casimir::packets::nci::{
        CoreGetConfigCommand, CoreInitCommand, CoreResetCommand, DeactivationType,
        DiscoverConfiguration, FeatureEnable, ResetType, RfDeactivateCommand, RfDiscoverCommand,
        RfDiscoverSelectCommand, RfDiscoveryId, RfInterfaceType, RfProtocolType,
        RfSetListenModeRoutingCommand, RfTechnologyAndMode,
    };
    use netsim_model::{ChipId, DeviceId};

    use super::*;
    use crate::{nfc_actor::ChipState, stats::NfcStats};

    pub(crate) struct DummyContext;
    impl Context<NfcActor> for DummyContext {
        fn set_interval(&mut self, _duration: std::time::Duration) {}
        fn add_stream(&mut self, _id: ChipId, _stream: actor_framework::BoxStream) {}
        fn remove_stream(&mut self, _id: ChipId) {}
        fn add_typed_stream(&mut self, _id: usize, _stream: actor_framework::BoxTypedStream<()>) {}
        fn remove_typed_stream(&mut self, _id: usize) {}
        fn spawn(&mut self, _id: ChipId, _task: futures::future::BoxFuture<'static, ChipId>) {}
        fn abort(&mut self, _id: ChipId) {}
        fn shutdown(&mut self) {}
        fn run_later(
            &mut self,
            _duration: std::time::Duration,
            _f: actor_framework::TimerCallback<NfcActor>,
        ) -> actor_framework::TimerKey {
            unimplemented!()
        }
        fn cancel_timer(&mut self, _key: actor_framework::TimerKey) {}
    }

    #[tokio::test]
    async fn test_lifecycle_on_stream_and_stats() {
        let (_runner, device_client) = device_actor::new();
        let stats = Arc::new(NfcStats::new());
        let mut actor = NfcActor::new(device_client, stats.clone());
        let mut ctx = DummyContext;
        let chip_id = ChipId(1);

        // Setup active chip with duplex stream for writer testing
        let (nfc_io, _peer) = tokio::io::duplex(1024);
        let (_reader, nfc_writer) = tokio::io::split(nfc_io);
        let chip_mode = Arc::new(AtomicU8::new(NFC_MODE_IDLE));
        actor.active_chips.insert(
            chip_id,
            ChipState {
                id: chip_id,
                device_id: DeviceId(1),
                enabled: Arc::new(AtomicBool::new(true)),
                casimir_device_id: 1,
                nfc_writer,
                tx_count: Arc::new(AtomicU64::new(0)),
                rx_count: Arc::new(AtomicU64::new(0)),
                mode: chip_mode.clone(),
            },
        );

        // 1. Lifecycle start and shutdown hooks
        actor.on_start(&mut ctx).await;
        actor.on_shutdown().await;

        // 2. Empty message -> returns early
        actor.on_stream(chip_id, Bytes::new(), &mut ctx).await;

        // 3. Data packets in idle mode (len < 3 and len >= 3)
        actor.on_stream(chip_id, Bytes::from_static(&[0x00, 0x00]), &mut ctx).await;
        actor.on_stream(chip_id, Bytes::from_static(&[0x00, 0x00, 0x03, 1, 2, 3]), &mut ctx).await;
        assert_eq!(stats.get(NfcApi::TagTransceive), 0);
        assert_eq!(stats.get(NfcApi::CardEmulationSendResponseApdu), 0);

        // 4. CoreResetCommand -> does not increment AdapterDisable (standard
        //    boot/enable sequence)
        let cmd = CoreResetCommand { reset_type: ResetType::KeepConfig };
        actor.on_stream(chip_id, Bytes::from(cmd.encode_to_vec().unwrap()), &mut ctx).await;
        assert_eq!(stats.get(NfcApi::AdapterDisable), 0);

        // 5. CoreInitCommand -> AdapterEnable
        let cmd = CoreInitCommand { feature_enable: FeatureEnable {} };
        actor.on_stream(chip_id, Bytes::from(cmd.encode_to_vec().unwrap()), &mut ctx).await;
        assert_eq!(stats.get(NfcApi::AdapterEnable), 1);

        // 6. RfDiscoverCommand -> AdapterEnableReaderMode (sets mode to NFC_MODE_POLL)
        let cmd = RfDiscoverCommand {
            configurations: vec![DiscoverConfiguration {
                technology_and_mode: RfTechnologyAndMode::NfcAPassivePollMode,
                discovery_frequency: 1,
            }],
        };
        actor.on_stream(chip_id, Bytes::from(cmd.encode_to_vec().unwrap()), &mut ctx).await;
        assert_eq!(stats.get(NfcApi::AdapterEnableReaderMode), 1);
        assert_eq!(chip_mode.load(Ordering::Relaxed), NFC_MODE_POLL);

        // 7. Data packet in Poll mode -> TagTransceive
        actor
            .on_stream(chip_id, Bytes::from_static(&[0x00, 0x00, 0x03, 0x00, 0xa4, 0x04]), &mut ctx)
            .await;
        assert_eq!(stats.get(NfcApi::TagTransceive), 1);

        // 8. RfDiscoverSelectCommand -> TagConnect (maintains NFC_MODE_POLL)
        let cmd = RfDiscoverSelectCommand {
            rf_discovery_id: RfDiscoveryId::try_from(1).unwrap(),
            rf_protocol: RfProtocolType::IsoDep,
            rf_interface: RfInterfaceType::Frame,
        };
        actor.on_stream(chip_id, Bytes::from(cmd.encode_to_vec().unwrap()), &mut ctx).await;
        assert_eq!(stats.get(NfcApi::TagConnect), 1);
        assert_eq!(chip_mode.load(Ordering::Relaxed), NFC_MODE_POLL);

        // 9. RfDeactivateCommand with SleepMode / Discovery -> TagClose
        let cmd = RfDeactivateCommand { deactivation_type: DeactivationType::SleepMode };
        actor.on_stream(chip_id, Bytes::from(cmd.encode_to_vec().unwrap()), &mut ctx).await;
        assert_eq!(stats.get(NfcApi::TagClose), 1);
        assert_eq!(chip_mode.load(Ordering::Relaxed), NFC_MODE_IDLE);

        let cmd = RfDeactivateCommand { deactivation_type: DeactivationType::Discovery };
        actor.on_stream(chip_id, Bytes::from(cmd.encode_to_vec().unwrap()), &mut ctx).await;
        assert_eq!(stats.get(NfcApi::TagClose), 2);
        assert_eq!(chip_mode.load(Ordering::Relaxed), NFC_MODE_POLL);

        // 10. RfDeactivateCommand with IdleMode -> AdapterDisableReaderMode
        let cmd = RfDeactivateCommand { deactivation_type: DeactivationType::IdleMode };
        actor.on_stream(chip_id, Bytes::from(cmd.encode_to_vec().unwrap()), &mut ctx).await;
        assert_eq!(stats.get(NfcApi::AdapterDisableReaderMode), 1);
        assert_eq!(chip_mode.load(Ordering::Relaxed), NFC_MODE_IDLE);

        // 11. RfSetListenModeRoutingCommand -> AdapterSetListenModeRouting (sets mode
        //     to NFC_MODE_LISTEN)
        let cmd = RfSetListenModeRoutingCommand { more_to_follow: 0, routing_entries: vec![] };
        actor.on_stream(chip_id, Bytes::from(cmd.encode_to_vec().unwrap()), &mut ctx).await;
        assert_eq!(stats.get(NfcApi::AdapterSetListenModeRouting), 1);
        assert_eq!(chip_mode.load(Ordering::Relaxed), NFC_MODE_LISTEN);

        // 12. Data packet in Listen mode -> CardEmulationSendResponseApdu
        actor
            .on_stream(chip_id, Bytes::from_static(&[0x00, 0x00, 0x02, 0x90, 0x00]), &mut ctx)
            .await;
        assert_eq!(stats.get(NfcApi::CardEmulationSendResponseApdu), 1);

        // 13. Other unhandled Core / Rf packets
        let get_cfg_cmd = CoreGetConfigCommand { parameters: vec![] };
        actor.on_stream(chip_id, Bytes::from(get_cfg_cmd.encode_to_vec().unwrap()), &mut ctx).await;

        // 14. Malformed control packet
        actor.on_stream(chip_id, Bytes::from_static(&[0x20, 0xff, 0xff]), &mut ctx).await;

        // 15. Stream and task closed
        actor.on_stream_closed(chip_id, &mut ctx).await;
        actor.on_task_closed(chip_id, &mut ctx).await;
    }
}
