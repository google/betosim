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
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};

use actor_framework::{ActorService, DynContext};
use bytes::{Bytes, BytesMut};
use device_actor::DeviceClient;
use futures::{SinkExt, StreamExt};
use netsim_model::{ChipCreate, ChipError, ChipId, ChipUpdate, DeviceId};
use parking_lot::Mutex;
use tokio::sync::mpsc::unbounded_channel;
use tokio_util::codec::{Decoder, FramedRead};
use tracing::{error, info};

use crate::{
    error::NfcError,
    nfc_actor::{ChipState, NciMessageType, NfcAction, NfcActor, NfcMode},
    stats::NfcApi,
};

/// NCI Frame Decoder for Casimir <-> Guest packet stream.
#[derive(Debug, Default, Clone, Copy)]
pub struct NciCodec;

impl Decoder for NciCodec {
    type Item = Bytes;
    type Error = std::io::Error;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        let Some(&payload_len) = src.get(2) else {
            return Ok(None);
        };
        let total_len = 3 + payload_len as usize;
        if src.len() < total_len {
            src.reserve(total_len - src.len());
            return Ok(None);
        }
        Ok(Some(src.split_to(total_len).freeze()))
    }
}

/// Maximum physical proximity threshold (in meters) for peer-to-peer NFC interactions.
const PROXIMITY_THRESHOLD_METERS: f32 = 0.04;

/// Helper to check if two Casimir NFC devices are within physical NFC proximity
/// (<= 0.04m). Returns true if within threshold or if either device is
/// unmapped/missing (fallback for non-spatial tests).
async fn is_within_nfc_proximity(
    sender_casimir_id: u16,
    receiver_device_id: DeviceId,
    casimir_to_device: &Arc<Mutex<HashMap<u16, DeviceId>>>,
    device_client: &DeviceClient,
) -> bool {
    let sender_dev_id = casimir_to_device.lock().get(&sender_casimir_id).copied();
    let Some(src_dev) = sender_dev_id else {
        return true; // Fallback: intentionally allow unmapped control/external test devices
    };

    let (src_res, dst_res) =
        tokio::join!(device_client.get(src_dev), device_client.get(receiver_device_id));

    match (src_res, dst_res) {
        (Ok(Some(src_ent)), Ok(Some(dst_ent))) => {
            src_ent.pose.position.distance(&dst_ent.pose.position) <= PROXIMITY_THRESHOLD_METERS
        }
        _ => true, // Fallback: allow missing positions to bypass spatial filtering
    }
}

impl ActorService for NfcActor {
    type Id = ChipId;
    type Create = ChipCreate;
    type Update = ChipUpdate;
    type Action = NfcAction;
    type ActionResult = crate::nfc_actor::NfcActionResult;
    type Error = NfcError;
    type Entity = netsim_model::Chip;
    type TypedStream = (); // No typed streams for now

    async fn handle_create(
        &mut self,
        id: Option<Self::Id>,
        mut params: Self::Create,
        ctx: &mut DynContext<Self>,
    ) -> Result<Self::Id, Self::Error> {
        let chip_id = id.ok_or_else(|| ChipError::InvalidArguments("missing chip id".into()))?;
        let device_id = params.chip.device_id;

        if self.is_chip_active(&chip_id) {
            return Err(NfcError::Chip(ChipError::ChipExists(chip_id.0)));
        }

        let packet_stream = params.packet_stream.take().ok_or(NfcError::MissingStreamSink)?;
        let mut packet_sink = params.packet_sink.take().ok_or(NfcError::MissingStreamSink)?;

        let scene_client = self
            .scene_client
            .as_ref()
            .ok_or_else(|| {
                NfcError::IoError(std::io::Error::other("Scene client not initialized"))
            })?
            .clone();

        // Create duplex stream for Casimir bridge
        let (nfc_io, casimir_io) = tokio::io::duplex(1024);

        let (casimir_rx, casimir_tx) = tokio::io::split(casimir_io);
        let casimir_to_device = self.casimir_to_device.clone();
        let device_client = self.device_client.clone();
        let nfc_stats_rf = self.nfc_stats.clone();
        let casimir_device_id = scene_client
            .add_device(move |id, scene_rf_tx| {
                casimir_to_device.lock().insert(id, device_id);
                let (egress_rf_tx, mut egress_rf_rx) = unbounded_channel();
                let mut device = casimir::Device::nci(id, casimir_rx, casimir_tx, egress_rf_tx);
                let nfc_stats_rf_tx = nfc_stats_rf.clone();
                tokio::spawn(async move {
                    while let Some(packet) = egress_rf_rx.recv().await {
                        nfc_stats_rf_tx.incr_rf_taps_tx();
                        if scene_rf_tx.send(packet).is_err() {
                            nfc_stats_rf_tx.incr_rf_error();
                            break;
                        }
                    }
                });
                let (ingress_rf_tx, mut ingress_rf_rx) = unbounded_channel();
                let controller_ingress_rf_tx = std::mem::replace(&mut device.rf_tx, ingress_rf_tx);
                let map_clone = casimir_to_device.clone();
                tokio::spawn(async move {
                    while let Some(packet) = ingress_rf_rx.recv().await {
                        if packet.sender() == id
                            || is_within_nfc_proximity(
                                packet.sender(),
                                device_id,
                                &map_clone,
                                &device_client,
                            )
                            .await
                        {
                            if packet.sender() != id {
                                nfc_stats_rf.incr_rf_taps_rx();
                            }
                            if controller_ingress_rf_tx.send(packet).is_err() {
                                nfc_stats_rf.incr_rf_error();
                                break;
                            }
                        }
                    }
                });
                device
            })
            .await
            .map_err(|e| {
                self.nfc_stats.incr_casimir_error();
                NfcError::IoError(std::io::Error::other(e))
            })?;

        info!("Added NFC device to Casimir scene with ID {}", casimir_device_id);

        // Bridge Casimir -> Guest
        let (nfc_reader, nfc_writer) = tokio::io::split(nfc_io);
        let mut stream = FramedRead::new(nfc_reader, NciCodec);
        let enabled = Arc::new(AtomicBool::new(true));
        let enabled_clone = enabled.clone();
        let chip_id_clone = chip_id;
        let nfc_stats_clone = self.nfc_stats.clone();
        let tx_count = Arc::new(AtomicU64::new(0));
        let rx_count = Arc::new(AtomicU64::new(0));
        let tx_count_clone = tx_count.clone();
        let rx_count_clone = rx_count.clone();
        let mode = Arc::new(std::sync::atomic::AtomicU8::new(NfcMode::Idle.as_u8()));
        let mode_clone = mode.clone();

        let task_1 = Box::pin(async move {
            while let Some(item) = stream.next().await {
                match item {
                    Ok(bytes) => {
                        if !enabled_clone.load(Ordering::Acquire) {
                            continue;
                        }
                        if let &[b0, _, _, ..] = bytes.as_ref() {
                            match NciMessageType::from_header_byte(b0) {
                                Some(NciMessageType::Data) => {
                                    nfc_stats_clone.incr_nci_data_tx();
                                    match NfcMode::try_from(mode_clone.load(Ordering::Relaxed)) {
                                        Ok(NfcMode::Listen) => {
                                            nfc_stats_clone
                                                .incr(NfcApi::CardEmulationProcessCommandApdu);
                                            nfc_stats_clone.incr_card_emulation_count();
                                        }
                                        Ok(NfcMode::Poll) => {
                                            nfc_stats_clone.incr_tag_emulation_count();
                                        }
                                        _ => {}
                                    }
                                    rx_count_clone.fetch_add(1, Ordering::Relaxed);
                                }
                                Some(NciMessageType::Response) => {
                                    nfc_stats_clone.incr_nci_responses_tx();
                                    rx_count_clone.fetch_add(1, Ordering::Relaxed);
                                }
                                Some(NciMessageType::Notification) => {
                                    nfc_stats_clone.incr_nci_notifications_tx();
                                    rx_count_clone.fetch_add(1, Ordering::Relaxed);
                                }
                                _ => {}
                            }
                        }
                        if let Err(error) = packet_sink.send(bytes).await {
                            error!(?error, "Failed to send packet to guest");
                            break;
                        }
                    }
                    Err(error) => {
                        error!(?error, "NFC reader error");
                        nfc_stats_clone.incr_nci_error();
                        break;
                    }
                }
            }
            info!("NFC forward loop finished");
            chip_id_clone
        });
        ctx.spawn(chip_id, task_1);

        // Bridge Guest -> Casimir
        let nfc_stats_rx = self.nfc_stats.clone();
        let packet_stream = packet_stream.inspect(move |bytes| {
            if let &[b0, _, _, ..] = bytes.as_ref() {
                match NciMessageType::from_header_byte(b0) {
                    Some(NciMessageType::Data) => {
                        nfc_stats_rx.incr_nci_data_rx();
                        tx_count_clone.fetch_add(1, Ordering::Relaxed);
                    }
                    Some(NciMessageType::Command) => {
                        nfc_stats_rx.incr_nci_commands_rx();
                        tx_count_clone.fetch_add(1, Ordering::Relaxed);
                    }
                    _ => {}
                }
            }
        });
        ctx.add_stream(chip_id, Box::pin(packet_stream));

        let mut chip = params.chip;
        chip.kind = netsim_model::ChipKind::NFC;
        chip.id = chip_id.0;
        if chip.name.is_empty() {
            chip.name = format!("nfc-{}", chip_id.0);
        }
        chip.device_id = device_id;
        chip.enabled = true;
        chip.variant = Some(netsim_model::ChipVariant::Nfc(netsim_model::Nfc {
            radio: netsim_model::Radio { state: Some(true), ..Default::default() },
        }));

        self.active_chips.insert(
            chip_id,
            ChipState {
                id: chip_id,
                device_id,
                chip,
                enabled,
                casimir_device_id,
                nfc_writer,
                tx_count,
                rx_count,
                mode,
            },
        );

        info!("NFC chip {} created for device {}", chip_id, device_id);

        Ok(chip_id)
    }

    async fn handle_delete(
        &mut self,
        id: Self::Id,
        ctx: &mut DynContext<Self>,
    ) -> Result<(), Self::Error> {
        if let Some(state) = self.active_chips.remove(&id) {
            info!("Deleting NFC chip {}", id);
            self.nfc_stats.incr(NfcApi::AdapterDisable);
            // Notify DeviceClient asynchronously
            let dc = self.device_client.clone();
            let device_id = state.device_id;
            tokio::spawn(async move {
                let _ = dc.notify_chip_removed(device_id, id).await;
            });

            // Abort the Casimir -> Guest task
            ctx.abort(id);

            // Remove the guest stream
            ctx.remove_stream(id);

            // Remove device mapping and Casimir scene device
            self.casimir_to_device.lock().remove(&state.casimir_device_id);
            if let Some(ref scene_client) = self.scene_client
                && let Err(error) = scene_client.remove_device(state.casimir_device_id).await
            {
                error!(
                    ?error,
                    "Failed to remove device {} from Casimir scene", state.casimir_device_id
                );
            }
        }
        Ok(())
    }

    async fn handle_get(
        &self,
        id: Self::Id,
        _ctx: &mut DynContext<Self>,
    ) -> Result<Option<Self::Entity>, Self::Error> {
        Ok(self.get_chip(&id).map(|s| s.chip.clone()))
    }

    async fn handle_update(
        &mut self,
        id: Self::Id,
        update: Self::Update,
        _ctx: &mut DynContext<Self>,
    ) -> Result<Self::Entity, Self::Error> {
        let (should_disable_adapter, updated_chip) = {
            let state = self
                .get_chip_mut(&id)
                .ok_or_else(|| NfcError::Chip(ChipError::ChipNotFound(id)))?;

            update.apply(&mut state.chip);

            let variant_enabled = match &update.variant {
                Some(netsim_model::ChipVariantUpdate::Nfc(netsim_model::NfcUpdate {
                    radio: netsim_model::RadioUpdate { state, .. },
                })) => *state,
                _ => None,
            };
            let target_enabled = variant_enabled.or(update.enabled);
            let should_disable = target_enabled
                .is_some_and(|enabled| state.enabled.swap(enabled, Ordering::AcqRel) && !enabled);

            let is_enabled = state.enabled.load(Ordering::Acquire);
            if let Some(netsim_model::ChipVariant::Nfc(ref mut nfc)) = state.chip.variant {
                nfc.radio.state = Some(is_enabled);
            }
            state.chip.enabled = is_enabled;

            (should_disable, state.chip.clone())
        };

        if should_disable_adapter {
            self.nfc_stats.incr(NfcApi::AdapterDisable);
        }

        Ok(updated_chip)
    }

    async fn handle_action(
        &mut self,
        _id: Option<Self::Id>,
        action: Self::Action,
        _ctx: &mut DynContext<Self>,
    ) -> Result<Self::ActionResult, Self::Error> {
        match action {
            NfcAction::Generic(_req) => Ok(crate::nfc_actor::NfcActionResult::Ok),
            NfcAction::GetStatistics => {
                let stats: Box<[netsim_model::NetsimRadioStats]> = self
                    .active_chips
                    .iter()
                    .map(|(id, state)| {
                        let mut radio_stats = netsim_model::NetsimRadioStats::default();
                        radio_stats.id = id.0;
                        radio_stats.name = format!("nfc-{}", id.0);
                        radio_stats.kind = netsim_model::RadioKind::Nfc;
                        radio_stats.tx_count = state.tx_count.load(Ordering::Relaxed);
                        radio_stats.rx_count = state.rx_count.load(Ordering::Relaxed);
                        radio_stats
                    })
                    .collect();
                Ok(crate::nfc_actor::NfcActionResult::Statistics(stats))
            }
            NfcAction::CreateControlChannel { respond_to } => {
                info!("NfcActor: Received CreateControlChannel action");
                let (grpc_io, casimir_io) = tokio::io::duplex(1024);
                if let Some(ref scene_client) = self.scene_client {
                    info!("NfcActor: Calling scene_client.add_device...");
                    let add_result = scene_client
                        .add_device(move |id, rf_tx| {
                            let (rx, tx) = tokio::io::split(casimir_io);
                            casimir::Device::rf(id, rx, tx, rf_tx)
                        })
                        .await;
                    info!("NfcActor: scene_client.add_device returned: {:?}", add_result);
                    match add_result {
                        Ok(id) => {
                            info!(
                                "NfcActor: Sending success response to grpc-server with ID {}",
                                id
                            );
                            let _ = respond_to.send(Ok((grpc_io, id)));
                            Ok(crate::nfc_actor::NfcActionResult::Ok)
                        }
                        Err(error) => {
                            let err_msg = format!("Failed to add RF device to Casimir: {error}");
                            error!(%error, "NfcActor: Failed to add RF device to Casimir");
                            self.nfc_stats.incr_casimir_error();
                            let _ = respond_to.send(Err(NfcError::Internal(err_msg)));
                            Ok(crate::nfc_actor::NfcActionResult::Ok)
                        }
                    }
                } else {
                    let err_msg = "Casimir scene not started".to_string();
                    error!("NfcActor: {}", err_msg);
                    self.nfc_stats.incr_casimir_error();
                    let _ = respond_to.send(Err(NfcError::Internal(err_msg)));
                    Ok(crate::nfc_actor::NfcActionResult::Ok)
                }
            }
        }
    }

    async fn handle_list(
        &mut self,
        _ctx: &mut DynContext<Self>,
    ) -> Result<Vec<Self::Entity>, Self::Error> {
        Ok(self.active_chips.values().map(|s| s.chip.clone()).collect())
    }
}

#[cfg(test)]
mod tests {
    use bytes::BytesMut;
    use tokio_util::codec::Decoder;

    use super::*;

    #[test]
    fn test_nci_codec_single_packet() {
        let mut codec = NciCodec;
        let mut buf = BytesMut::from(&[0x40, 0x00, 0x01, 0xAA][..]);

        let frame = codec.decode(&mut buf).unwrap().expect("Frame must decode");
        assert_eq!(frame.len(), 4);
        assert_eq!(frame, Bytes::from_static(&[0x40, 0x00, 0x01, 0xAA]));
        assert!(buf.is_empty());
    }

    #[test]
    fn test_nci_codec_multi_packet_stream() {
        let mut codec = NciCodec;
        // Packet 1: 3-byte header [0x00, 0x00, 0x03] + 3-byte payload [0x01, 0x02,
        // 0x03] (Len = 6) Packet 2: 3-byte header [0x20, 0x00, 0x01] + 1-byte
        // payload [0xAA]             (Len = 4)
        let mut buf =
            BytesMut::from(&[0x00, 0x00, 0x03, 0x01, 0x02, 0x03, 0x20, 0x00, 0x01, 0xAA][..]);

        let frame1 = codec.decode(&mut buf).unwrap().expect("Frame 1 must decode");
        assert_eq!(frame1.len(), 6, "Frame 1 must be exact length 6 (no over-reading)");
        assert_eq!(frame1, Bytes::from_static(&[0x00, 0x00, 0x03, 0x01, 0x02, 0x03]));

        let frame2 = codec.decode(&mut buf).unwrap().expect("Frame 2 must decode");
        assert_eq!(frame2.len(), 4, "Frame 2 must be exact length 4");
        assert_eq!(frame2, Bytes::from_static(&[0x20, 0x00, 0x01, 0xAA]));

        assert!(buf.is_empty());
        assert!(codec.decode(&mut buf).unwrap().is_none());
    }

    #[test]
    fn test_nci_codec_partial_chunks() {
        let mut codec = NciCodec;
        let mut buf = BytesMut::new();

        // 1. Partial header (2 bytes)
        buf.extend_from_slice(&[0x61, 0x05]);
        assert!(codec.decode(&mut buf).unwrap().is_none());

        // 2. Length byte (declares 2 bytes payload)
        buf.extend_from_slice(&[0x02]);
        assert!(codec.decode(&mut buf).unwrap().is_none());

        // 3. Partial payload (1 byte)
        buf.extend_from_slice(&[0x10]);
        assert!(codec.decode(&mut buf).unwrap().is_none());

        // 4. Final payload byte
        buf.extend_from_slice(&[0x20]);
        let frame = codec.decode(&mut buf).unwrap().expect("Frame must decode when complete");
        assert_eq!(frame.len(), 5);
        assert_eq!(frame, Bytes::from_static(&[0x61, 0x05, 0x02, 0x10, 0x20]));
        assert!(buf.is_empty());
    }

    #[test]
    fn test_nci_codec_empty_payload() {
        let mut codec = NciCodec;
        let mut buf = BytesMut::from(&[0x20, 0x00, 0x00][..]);

        let frame = codec.decode(&mut buf).unwrap().expect("0-payload frame must decode");
        assert_eq!(frame.len(), 3);
        assert_eq!(frame, Bytes::from_static(&[0x20, 0x00, 0x00]));
        assert!(buf.is_empty());
    }

    #[tokio::test]
    async fn test_nci_codec_with_framed_read() -> Result<(), Box<dyn std::error::Error>> {
        let packet = &[0x40, 0x00, 1, 0xaa];
        let cursor = std::io::Cursor::new(packet.to_vec());
        let mut stream = FramedRead::new(cursor, NciCodec);

        let bytes = stream.next().await.ok_or("Stream ended prematurely")??;
        assert_eq!(bytes.len(), 4);
        assert_eq!(bytes.as_ref(), packet);
        assert!(stream.next().await.is_none());
        Ok(())
    }

    struct DummyContext;
    impl actor_framework::Context<NfcActor> for DummyContext {
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
    async fn test_nfc_actor_service_operations() {
        use actor_framework::ActorService;
        let (_runner, device_client) = device_actor::new();
        let stats = Arc::new(crate::stats::NfcStats::new());
        let mut actor = NfcActor::new(device_client, stats.clone());
        let mut ctx = DummyContext;

        // handle_list on empty
        let list = actor.handle_list(&mut ctx).await.unwrap();
        assert!(list.is_empty());

        // handle_get non-existent chip
        let get_res = actor.handle_get(ChipId(999), &mut ctx).await.unwrap();
        assert!(get_res.is_none());

        // handle_action GetStatistics
        let action_res =
            actor.handle_action(None, NfcAction::GetStatistics, &mut ctx).await.unwrap();
        match action_res {
            crate::nfc_actor::NfcActionResult::Statistics(s) => assert!(s.is_empty()),
            _ => panic!("Expected Statistics result"),
        }

        // handle_delete non-existent chip
        let del_res = actor.handle_delete(ChipId(999), &mut ctx).await;
        assert!(del_res.is_ok());
    }
}
