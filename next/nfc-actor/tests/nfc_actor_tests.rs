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
    sync::{Arc, Mutex},
};

use actor_framework::{ActorLifecycle, ActorService, Context};
use bytes::Bytes;
use device_actor::DeviceActor;
use futures::{StreamExt, future::BoxFuture};
use netsim_model::{ChipCreate, ChipId, DeviceId};
use nfc_actor::NfcActor;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

// --- Mock Helpers ---

#[derive(Clone)]
struct MockDeviceActorClient;

#[async_trait::async_trait]
impl actor_framework::ActorClient<DeviceActor> for MockDeviceActorClient {
    async fn create(
        &self,
        _params: <DeviceActor as ActorService>::Create,
    ) -> Result<
        <DeviceActor as ActorService>::Id,
        actor_framework::FrameworkError<<DeviceActor as ActorService>::Error>,
    > {
        panic!("not implemented");
    }
    async fn create_with_id(
        &self,
        _id: <DeviceActor as ActorService>::Id,
        _params: <DeviceActor as ActorService>::Create,
    ) -> Result<
        <DeviceActor as ActorService>::Id,
        actor_framework::FrameworkError<<DeviceActor as ActorService>::Error>,
    > {
        panic!("not implemented");
    }
    async fn get(
        &self,
        _id: <DeviceActor as ActorService>::Id,
    ) -> Result<
        Option<<DeviceActor as ActorService>::Entity>,
        actor_framework::FrameworkError<<DeviceActor as ActorService>::Error>,
    > {
        panic!("not implemented");
    }
    async fn update(
        &self,
        _id: <DeviceActor as ActorService>::Id,
        _update: <DeviceActor as ActorService>::Update,
    ) -> Result<
        <DeviceActor as ActorService>::Entity,
        actor_framework::FrameworkError<<DeviceActor as ActorService>::Error>,
    > {
        panic!("not implemented");
    }
    async fn delete(
        &self,
        _id: <DeviceActor as ActorService>::Id,
    ) -> Result<(), actor_framework::FrameworkError<<DeviceActor as ActorService>::Error>> {
        panic!("not implemented");
    }
    async fn perform_action(
        &self,
        _id: Option<<DeviceActor as ActorService>::Id>,
        _action: <DeviceActor as ActorService>::Action,
    ) -> Result<
        <DeviceActor as ActorService>::ActionResult,
        actor_framework::FrameworkError<<DeviceActor as ActorService>::Error>,
    > {
        Ok(device_api::DeviceActionResult::Success)
    }
    async fn list(
        &self,
    ) -> Result<
        Vec<<DeviceActor as ActorService>::Entity>,
        actor_framework::FrameworkError<<DeviceActor as ActorService>::Error>,
    > {
        panic!("not implemented");
    }
    async fn shutdown(
        &self,
    ) -> Result<(), actor_framework::FrameworkError<<DeviceActor as ActorService>::Error>> {
        panic!("not implemented");
    }
    fn clone_box(&self) -> Box<dyn actor_framework::ActorClient<DeviceActor>> {
        Box::new(self.clone())
    }
}

struct TestContext {
    tasks: Arc<Mutex<HashMap<ChipId, tokio::task::JoinHandle<ChipId>>>>,
    stream_tasks: Arc<Mutex<HashMap<ChipId, tokio::task::JoinHandle<()>>>>,
    aborted_tasks: Arc<Mutex<Vec<ChipId>>>,
    removed_streams: Arc<Mutex<Vec<ChipId>>>,
}

impl Context<NfcActor> for TestContext {
    fn set_interval(&mut self, _duration: std::time::Duration) {}
    fn add_stream(&mut self, id: ChipId, mut stream: actor_framework::BoxStream) {
        let handle = tokio::spawn(async move { while stream.next().await.is_some() {} });
        self.stream_tasks.lock().unwrap().insert(id, handle);
    }
    fn remove_stream(&mut self, id: ChipId) {
        self.removed_streams.lock().unwrap().push(id);
        let value = self.stream_tasks.lock().unwrap().remove(&id);
        if let Some(handle) = value {
            handle.abort();
        }
    }
    fn add_typed_stream(&mut self, _id: usize, _stream: actor_framework::BoxTypedStream<()>) {}
    fn remove_typed_stream(&mut self, _id: usize) {}
    fn spawn(&mut self, id: ChipId, task: BoxFuture<'static, ChipId>) {
        let handle = tokio::spawn(task);
        self.tasks.lock().unwrap().insert(id, handle);
    }
    fn abort(&mut self, id: ChipId) {
        self.aborted_tasks.lock().unwrap().push(id);
        let handle = self.tasks.lock().unwrap().remove(&id);
        if let Some(handle) = handle {
            handle.abort();
        }
    }
    fn shutdown(&mut self) {}
    fn run_later(
        &mut self,
        _duration: std::time::Duration,
        _f: actor_framework::TimerCallback<NfcActor>,
    ) -> actor_framework::TimerKey {
        panic!("not implemented");
    }
    fn cancel_timer(&mut self, _key: actor_framework::TimerKey) {}
}

struct MockSink(mpsc::Sender<Bytes>);
impl futures::Sink<Bytes> for MockSink {
    type Error = std::io::Error;
    fn poll_ready(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        std::task::Poll::Ready(Ok(()))
    }
    fn start_send(self: std::pin::Pin<&mut Self>, item: Bytes) -> Result<(), Self::Error> {
        let _ = self.0.try_send(item);
        Ok(())
    }
    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        std::task::Poll::Ready(Ok(()))
    }
    fn poll_close(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        std::task::Poll::Ready(Ok(()))
    }
}

// --- BDD World ---

struct NfcWorld {
    actor: NfcActor,
    ctx: TestContext,
    tasks: Arc<Mutex<HashMap<ChipId, tokio::task::JoinHandle<ChipId>>>>,
    aborted_tasks: Arc<Mutex<Vec<ChipId>>>,
    removed_streams: Arc<Mutex<Vec<ChipId>>>,
    packet_receivers: HashMap<ChipId, mpsc::Receiver<Bytes>>,
    packet_senders: HashMap<ChipId, mpsc::Sender<Bytes>>,
}

impl NfcWorld {
    async fn new() -> Self {
        let _ = tracing_subscriber::fmt().with_test_writer().try_init();

        let device_client = device_actor::DeviceClient::new(Box::new(MockDeviceActorClient));
        let mut actor = NfcActor::new(device_client, Arc::new(nfc_actor::NfcStats::new()));
        actor.start_casimir();

        let tasks = Arc::new(Mutex::new(HashMap::new()));
        let stream_tasks = Arc::new(Mutex::new(HashMap::new()));
        let aborted_tasks = Arc::new(Mutex::new(Vec::new()));
        let removed_streams = Arc::new(Mutex::new(Vec::new()));
        let ctx = TestContext {
            tasks: tasks.clone(),
            stream_tasks: stream_tasks.clone(),
            aborted_tasks: aborted_tasks.clone(),
            removed_streams: removed_streams.clone(),
        };
        let packet_receivers = HashMap::new();
        let packet_senders = HashMap::new();

        Self { actor, ctx, tasks, aborted_tasks, removed_streams, packet_receivers, packet_senders }
    }

    async fn given_a_chip(&mut self, id_val: u32) {
        let chip_id = ChipId(id_val);
        let device_id = DeviceId(id_val);

        let (tx_stream, rx_stream) = mpsc::channel(10);
        let (tx_sink, rx_sink) = mpsc::channel(10);
        let packet_stream = ReceiverStream::new(rx_stream);
        let packet_sink = MockSink(tx_sink);

        let params = ChipCreate {
            packet_stream: Some(Box::new(packet_stream)),
            packet_sink: Some(Box::pin(packet_sink)),
            chip: netsim_model::Chip { device_id, ..Default::default() },
        };

        let create_res = self.actor.handle_create(Some(chip_id), params, &mut self.ctx).await;
        assert!(create_res.is_ok());
        assert_eq!(create_res.unwrap(), chip_id);
        assert!(
            self.actor
                .active_chips
                .get(&chip_id)
                .unwrap()
                .enabled
                .load(std::sync::atomic::Ordering::Relaxed),
            "Chip must be enabled by default upon boot across all platforms during transition to Netsim"
        );
        self.packet_receivers.insert(chip_id, rx_sink);
        self.packet_senders.insert(chip_id, tx_stream);
    }

    async fn when_casimir_disconnects(&mut self, id_val: u32) {
        let chip_id = ChipId(id_val);
        let casimir_device_id = self.actor.active_chips.get(&chip_id).unwrap().casimir_device_id;

        let scene_client = self.actor.scene_client.as_ref().unwrap().clone();
        let remove_res = scene_client.remove_device(casimir_device_id).await;
        assert!(remove_res.is_ok());

        let handle = {
            let mut map = self.tasks.lock().unwrap();
            map.remove(&chip_id)
        };
        if let Some(handle) = handle {
            let res = handle.await;
            assert!(res.is_ok());
            assert_eq!(res.unwrap(), chip_id);

            self.actor.on_task_closed(chip_id, &mut self.ctx).await;
        } else {
            panic!("Task handle NOT found for chip {}", id_val);
        }
    }

    async fn then_chip_is_cleaned_up(&mut self, id_val: u32) {
        let chip_id = ChipId(id_val);
        assert!(!self.actor.active_chips.contains_key(&chip_id), "Chip still active in NfcActor");
        assert!(self.aborted_tasks.lock().unwrap().contains(&chip_id), "Task was not aborted");
        assert!(self.removed_streams.lock().unwrap().contains(&chip_id), "Stream was not removed");
        assert!(
            !self.actor.casimir_to_device.lock().values().any(|&dev_id| dev_id.0 == id_val),
            "Casimir mapping was not cleaned up!"
        );
    }
}

// --- Tests ---

// Feature: Casimir Disconnect Cleanup
// Scenario: Casimir disconnects and triggers clean EOF
//
//   Given a new NFC world with a registered chip
//   When Casimir disconnects the device
//   Then the chip is cleanly removed and tasks are aborted
#[tokio::test]
async fn test_casimir_eof_cleanup() {
    // Given
    let mut world = NfcWorld::new().await;
    let chip_id = 1;
    world.given_a_chip(chip_id).await;

    // When
    let timeout = tokio::time::timeout(std::time::Duration::from_secs(2), async {
        world.when_casimir_disconnects(chip_id).await;
    })
    .await;
    assert!(timeout.is_ok(), "Test timed out! Possible hang detected.");

    // Then
    world.then_chip_is_cleaned_up(chip_id).await;
}

// Feature: NFC Enablement Gating
// Scenario: Chips can be dynamically enabled/disabled via handle_update
//
//   Given a new NFC world with a registered chip
//   When handle_update sets enabled = false
//   Then the chip state transitions to disabled
//   When handle_update sets enabled = true
//   Then the chip state transitions back to enabled
#[tokio::test]
async fn test_nfc_enablement_toggle() {
    // Given
    let mut world = NfcWorld::new().await;
    let chip_id = 1;
    world.given_a_chip(chip_id).await;

    // When (Android OS calls NfcAdapter.disable())
    let update_disable = netsim_model::ChipUpdate { enabled: Some(false), ..Default::default() };
    let update_res1 =
        world.actor.handle_update(ChipId(chip_id), update_disable, &mut world.ctx).await;
    assert!(update_res1.is_ok());

    // Then (Verify chip is now disabled)
    let state_disabled =
        world.actor.handle_get(ChipId(chip_id), &mut world.ctx).await.unwrap().unwrap();
    assert!(!state_disabled.enabled, "Chip should be disabled after update!");

    // When (Android OS calls NfcAdapter.enable())
    let update_enable = netsim_model::ChipUpdate { enabled: Some(true), ..Default::default() };
    let update_res2 =
        world.actor.handle_update(ChipId(chip_id), update_enable, &mut world.ctx).await;
    assert!(update_res2.is_ok());

    // Then (Verify chip is enabled again)
    let state_enabled_again =
        world.actor.handle_get(ChipId(chip_id), &mut world.ctx).await.unwrap().unwrap();
    assert!(state_enabled_again.enabled, "Chip should be enabled after second update!");
}

// variant update path
//
// Given a new NFC world with a registered chip
// When handle_update sets state via variant = Some(false)
// Then the chip state transitions to disabled
// When handle_update sets state via variant = Some(true)
// Then the chip state transitions back to enabled
#[tokio::test]
async fn test_nfc_update_via_variant() {
    // Given
    let mut world = NfcWorld::new().await;
    let chip_id = 1;
    world.given_a_chip(chip_id).await;

    // When (Update via variant field to disable)
    let update_disable = netsim_model::ChipUpdate {
        variant: Some(netsim_model::ChipVariantUpdate::Nfc(netsim_model::NfcUpdate {
            radio: netsim_model::RadioUpdate { state: Some(false) },
        })),
        ..Default::default()
    };
    let update_res1 =
        world.actor.handle_update(ChipId(chip_id), update_disable, &mut world.ctx).await;
    assert!(update_res1.is_ok());

    // Then (Verify chip is now disabled)
    let state_disabled =
        world.actor.handle_get(ChipId(chip_id), &mut world.ctx).await.unwrap().unwrap();
    assert!(!state_disabled.enabled, "Chip should be disabled after variant update!");

    // When (Update via variant field to enable)
    let update_enable = netsim_model::ChipUpdate {
        variant: Some(netsim_model::ChipVariantUpdate::Nfc(netsim_model::NfcUpdate {
            radio: netsim_model::RadioUpdate { state: Some(true) },
        })),
        ..Default::default()
    };
    let update_res2 =
        world.actor.handle_update(ChipId(chip_id), update_enable, &mut world.ctx).await;
    assert!(update_res2.is_ok());

    // Then (Verify chip is enabled again)
    let state_enabled_again =
        world.actor.handle_get(ChipId(chip_id), &mut world.ctx).await.unwrap().unwrap();
    assert!(state_enabled_again.enabled, "Chip should be enabled after second variant update!");
}

// Feature: NFC Proximity Threshold and Mapping
// Scenario: Verify spatial distance calculation and threshold gating
//
//   Given two positions in 3D space
//   When distance is calculated
//   Then distance <= 0.04m is within NFC proximity threshold
#[test]
fn test_nfc_spatial_proximity_threshold() {
    let p1 = netsim_model::Position { x: 0.0, y: 0.0, z: 0.0 };
    let p2 = netsim_model::Position { x: 0.0, y: 0.0, z: 0.035 }; // 3.5cm
    let p3 = netsim_model::Position { x: 0.0, y: 0.0, z: 0.045 }; // 4.5cm
    assert!(p1.distance(&p2) <= 0.04, "3.5cm should be within 4cm NFC threshold!");
    assert!(p1.distance(&p3) > 0.04, "4.5cm should be outside 4cm NFC threshold!");
}

#[tokio::test]
async fn test_nfc_initial_enablement_universal() {
    let mut world = NfcWorld::new().await;
    let chip_id = 1;
    world.given_a_chip(chip_id).await;

    let chip = world.actor.handle_get(ChipId(chip_id), &mut world.ctx).await.unwrap().unwrap();
    assert!(
        chip.enabled,
        "Chip must be enabled upon creation across all platforms during transition to Netsim"
    );
}

#[tokio::test]
async fn test_nfc_stats_counters() {
    let mut world = NfcWorld::new().await;
    let chip_id = ChipId(1);
    world.given_a_chip(chip_id.0).await;

    // Enable the chip so that it can receive packets from Casimir
    let update_enable = netsim_model::ChipUpdate { enabled: Some(true), ..Default::default() };
    let update_res = world.actor.handle_update(chip_id, update_enable, &mut world.ctx).await;
    assert!(update_res.is_ok());

    use casimir::packets::nci::{
        ControlPacket, ControlPacketChild, CoreInitCommand, CoreResetCommand, DeactivationType,
        DiscoverConfiguration, FeatureEnable, ResetType, RfDeactivateCommand, RfDiscoverCommand,
        RfDiscoverSelectCommand, RfDiscoveryId, RfInterfaceType, RfPacketChild, RfProtocolType,
        RfSetListenModeRoutingCommand, RfTechnologyAndMode,
    };
    use nfc_actor::{NfcAction, NfcApi};
    use pdl_runtime::Packet;

    async fn send_and_verify(
        actor: &mut NfcActor,
        ctx: &mut TestContext,
        chip_id: ChipId,
        mut packet_bytes: Vec<u8>,
        api: NfcApi,
        expected_count: u32,
    ) {
        if packet_bytes.len() > 3 {
            packet_bytes[2] = (packet_bytes.len() - 3) as u8;
        }
        let msg = Bytes::from(packet_bytes);
        actor.on_stream(chip_id, msg, ctx).await;
        assert_eq!(actor.nfc_stats.get(api), expected_count, "Assertion failed for {:?}", api);
    }
    // 1. CoreReset -> does NOT increment AdapterDisable
    let cmd = CoreResetCommand { reset_type: ResetType::KeepConfig };
    send_and_verify(
        &mut world.actor,
        &mut world.ctx,
        chip_id,
        cmd.encode_to_vec().unwrap(),
        NfcApi::AdapterDisable,
        0,
    )
    .await;

    // 2. CoreInit -> AdapterEnable
    let cmd = CoreInitCommand { feature_enable: FeatureEnable {} };
    send_and_verify(
        &mut world.actor,
        &mut world.ctx,
        chip_id,
        cmd.encode_to_vec().unwrap(),
        NfcApi::AdapterEnable,
        1,
    )
    .await;

    // Setup Mock Peer on RF channel
    let (tx, rx) = tokio::sync::oneshot::channel();
    let action = NfcAction::CreateControlChannel { respond_to: tx };
    world.actor.handle_action(None, action, &mut world.ctx).await.unwrap();
    let (rf_io, rf_device_id) = rx.await.unwrap().unwrap();

    let (rf_rx, rf_tx) = tokio::io::split(rf_io);
    let mut rf_reader = ::casimir::RfReader::new(rf_rx);
    let mut rf_writer = ::casimir::RfWriter::new(rf_tx);

    let casimir_device_id = world.actor.active_chips.get(&chip_id).unwrap().casimir_device_id;
    let (mock_peer_cmd_tx, mut mock_peer_cmd_rx) = mpsc::channel::<Vec<u8>>(10);

    let mock_peer_handle = tokio::spawn(async move {
        use ::casimir::packets::rf;
        use pdl_runtime::Packet;

        loop {
            tokio::select! {
                res = rf_reader.read() => {
                    let bytes = match res {
                        Ok(b) => b,
                        Err(_) => break, // EOF
                    };
                    let pkt = match rf::RfPacket::decode_full(&bytes) {
                        Ok(p) => p,
                        Err(_) => continue,
                    };
                    if pkt.packet_type() == rf::RfPacketType::SelectCommand
                        && pkt.protocol() == rf::Protocol::IsoDep
                        && pkt.technology() == rf::Technology::NfcA
                    {
                        if let Ok(cmd) = rf::IsoDepT4ATSelectCommand::decode_full(&bytes) {
                            let resp = rf::IsoDepT4ATSelectResponse {
                                sender: rf_device_id,
                                receiver: cmd.sender(),
                                bitrate: rf::BitRate::BitRate106KbitS,
                                power_level: 10,
                                rats_response: vec![0x78, 0x80, 0x00, 0x00],
                            };
                            let _ = rf_writer.write(&resp.encode_to_vec().unwrap()).await;
                        }
                    } else {
                        if let Ok(specialized) = pkt.specialize() {
                            match specialized {
                                rf::RfPacketChild::PollCommand(cmd) => {
                                     let resp = rf::NfcAPollResponse {
                                         sender: rf_device_id,
                                         receiver: cmd.sender(),
                                         protocol: rf::Protocol::IsoDep,
                                         bitrate: rf::BitRate::BitRate106KbitS,
                                         power_level: 10,
                                         nfcid1: vec![1, 2, 3, 4],
                                         int_protocol: 0b01, // Type 4A Tag
                                         bit_frame_sdd: 0,
                                     };
                                     let _ = rf_writer.write(&resp.encode_to_vec().unwrap()).await;
                                }
                                _ => {}
                            }
                        }
                    }
                }
                Some(data_bytes) = mock_peer_cmd_rx.recv() => {
                    let pkt = rf::Data {
                        sender: rf_device_id,
                        receiver: casimir_device_id,
                        technology: rf::Technology::NfcA,
                        protocol: rf::Protocol::IsoDep,
                        bitrate: rf::BitRate::BitRate106KbitS,
                        power_level: 10,
                        data: data_bytes,
                    };
                    let _ = rf_writer.write(&pkt.encode_to_vec().unwrap()).await;
                }
            }
        }
    });

    // 3. RfDiscover -> AdapterEnableReaderMode
    let cmd = RfDiscoverCommand {
        configurations: vec![DiscoverConfiguration {
            technology_and_mode: RfTechnologyAndMode::NfcAPassivePollMode,
            discovery_frequency: 1,
        }],
    };
    send_and_verify(
        &mut world.actor,
        &mut world.ctx,
        chip_id,
        cmd.encode_to_vec().unwrap(),
        NfcApi::AdapterEnableReaderMode,
        1,
    )
    .await;

    // Wait for mock peer to activate Casimir (poll + select)
    let rx_sink = world.packet_receivers.get_mut(&chip_id).unwrap();
    loop {
        let bytes = rx_sink.recv().await.unwrap();
        if let Ok(pkt) = ControlPacket::decode_full(&bytes) {
            if let Ok(ControlPacketChild::RfPacket(rf_pkt)) = pkt.specialize() {
                if let Ok(RfPacketChild::RfIntfActivatedNotification(_)) = rf_pkt.specialize() {
                    break;
                }
            }
        }
    }

    // 4. RfDiscoverSelect -> TagConnect
    let cmd = RfDiscoverSelectCommand {
        rf_discovery_id: RfDiscoveryId::try_from(1).unwrap(),
        rf_protocol: RfProtocolType::IsoDep,
        rf_interface: RfInterfaceType::Frame,
    };
    send_and_verify(
        &mut world.actor,
        &mut world.ctx,
        chip_id,
        cmd.encode_to_vec().unwrap(),
        NfcApi::TagConnect,
        1,
    )
    .await;

    // 5. DataPacket (Guest -> Casimir in Poll Mode) -> TagTransceive
    let packet_bytes = vec![0, 0, 3, 1, 2, 3]; // NCI Data Packet: MT=0, ConnID=0, RFU=0, Len=3, Payload=[1,2,3]
    world
        .packet_senders
        .get(&chip_id)
        .unwrap()
        .send(Bytes::from(packet_bytes.clone()))
        .await
        .unwrap();
    send_and_verify(
        &mut world.actor,
        &mut world.ctx,
        chip_id,
        packet_bytes,
        NfcApi::TagTransceive,
        1,
    )
    .await;
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    assert_eq!(world.actor.nfc_stats.to_proto().nci_data_rx(), 1);
    assert_eq!(world.actor.nfc_stats.to_proto().tag_emulation_count(), 1);
    assert!(world.actor.nfc_stats.to_proto().rf_taps_tx() >= 2);
    assert!(world.actor.nfc_stats.to_proto().rf_taps_rx() >= 2);

    // 6. DataPacket (Casimir -> Guest while in Listen Mode via mock peer) ->
    //    CardEmulationProcessCommandApdu
    world.actor.active_chips.get(&chip_id).unwrap().set_mode(nfc_actor::NfcMode::Listen);
    let nci_data_pkt = vec![0, 0, 3, 1, 2, 3]; // NCI Data Packet
    mock_peer_cmd_tx.send(nci_data_pkt).await.unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    assert_eq!(world.actor.nfc_stats.get(NfcApi::CardEmulationProcessCommandApdu), 1);
    assert_eq!(world.actor.nfc_stats.to_proto().nci_data_tx(), 1);
    assert_eq!(world.actor.nfc_stats.to_proto().card_emulation_count(), 1);

    // 7. RfDeactivate(SleepMode) -> TagClose
    let cmd = RfDeactivateCommand { deactivation_type: DeactivationType::SleepMode };
    send_and_verify(
        &mut world.actor,
        &mut world.ctx,
        chip_id,
        cmd.encode_to_vec().unwrap(),
        NfcApi::TagClose,
        1,
    )
    .await;

    // 8. RfSetListenModeRouting -> AdapterSetListenModeRouting (switches mode to
    //    LISTEN)
    let cmd = RfSetListenModeRoutingCommand { more_to_follow: 0, routing_entries: vec![] };
    send_and_verify(
        &mut world.actor,
        &mut world.ctx,
        chip_id,
        cmd.encode_to_vec().unwrap(),
        NfcApi::AdapterSetListenModeRouting,
        1,
    )
    .await;

    // 9. DataPacket (Guest -> Casimir in Listen Mode) ->
    //    CardEmulationSendResponseApdu
    send_and_verify(
        &mut world.actor,
        &mut world.ctx,
        chip_id,
        vec![0, 0, 2, 0x90, 0x00],
        NfcApi::CardEmulationSendResponseApdu,
        1,
    )
    .await;
    assert_eq!(world.actor.nfc_stats.to_proto().card_emulation_count(), 2);
    let ipc = world.actor.nfc_stats.to_ipc_proto();
    assert_eq!(ipc.nfc_stats.card_emulation_count(), 2);
    assert_eq!(ipc.nfc_stats.tag_emulation_count(), 1);
    assert!(ipc.nfc_api_stats.nci_data.send() >= 1);
    assert_eq!(ipc.nfc_api_stats.nci_data.receive(), 1);

    // 10. RfDeactivate(IdleMode) -> AdapterDisableReaderMode
    let cmd = RfDeactivateCommand { deactivation_type: DeactivationType::IdleMode };
    send_and_verify(
        &mut world.actor,
        &mut world.ctx,
        chip_id,
        cmd.encode_to_vec().unwrap(),
        NfcApi::AdapterDisableReaderMode,
        1,
    )
    .await;

    // 11. Chip disable -> AdapterDisable
    let update_disable = netsim_model::ChipUpdate { enabled: Some(false), ..Default::default() };
    let update_res = world.actor.handle_update(chip_id, update_disable, &mut world.ctx).await;
    assert!(update_res.is_ok());
    assert_eq!(world.actor.nfc_stats.get(NfcApi::AdapterDisable), 1);

    mock_peer_handle.abort();
}

#[tokio::test]
async fn test_multiple_chip_stats_separation() {
    let mut world = NfcWorld::new().await;

    // Given two active chips
    world.given_a_chip(1).await;
    world.given_a_chip(2).await;

    // Send packets through the streams to naturally increment counters
    let chip1_tx = world.packet_senders.get(&ChipId(1)).unwrap();
    let chip2_tx = world.packet_senders.get(&ChipId(2)).unwrap();

    // Send 3 DATA packets (Guest -> Casimir) for Chip 1
    // MT=0x00 (DATA)
    for _ in 0..3 {
        chip1_tx.send(Bytes::from(vec![0x00, 0x00, 0x00])).await.unwrap();
    }

    // Send 5 CMD packets (Guest -> Casimir) for Chip 2
    // MT=0x20 (CMD) -> 0x20 >> 5 == 1 (NCI_MT_CMD)
    for _ in 0..5 {
        chip2_tx.send(Bytes::from(vec![0x20, 0x00, 0x00])).await.unwrap();
    }

    // Yield to allow the background stream tasks to process the items
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    // Call GetStatistics
    use nfc_actor::{NfcAction, NfcActionResult};

    let res = world.actor.handle_action(None, NfcAction::GetStatistics, &mut world.ctx).await;
    assert!(res.is_ok());

    match res.unwrap() {
        NfcActionResult::Statistics(stats) => {
            assert_eq!(stats.len(), 2);
            let mut chip1_found = false;
            let mut chip2_found = false;
            for s in stats.iter() {
                if s.id == 1 {
                    assert_eq!(s.tx_count, 3);
                    assert_eq!(s.rx_count, 0);
                    chip1_found = true;
                } else if s.id == 2 {
                    assert_eq!(s.tx_count, 5);
                    assert_eq!(s.rx_count, 0);
                    chip2_found = true;
                }
            }
            assert!(chip1_found);
            assert!(chip2_found);
        }
        _ => panic!("Expected Statistics"),
    }
}

#[tokio::test]
async fn test_nci_packet_framing_codec() {
    use bytes::Bytes;
    use futures::StreamExt;
    use nfc_actor::NciCodec;
    use tokio_util::codec::FramedRead;

    // Continuous NCI stream:
    // Packet 1: Header [0x00, 0x00, 0x03] + Payload [0x01, 0x02, 0x03] (Len = 6)
    // Packet 2: Header [0x20, 0x00, 0x01] + Payload [0xAA]             (Len = 4)
    let raw_nci_bytes = vec![0x00, 0x00, 0x03, 0x01, 0x02, 0x03, 0x20, 0x00, 0x01, 0xAA];
    let cursor = std::io::Cursor::new(raw_nci_bytes);
    let mut stream = FramedRead::new(cursor, NciCodec);

    let frame1 = stream.next().await.unwrap().unwrap();
    assert_eq!(
        frame1.len(),
        6,
        "NCI Frame 1 length must be exactly 6 bytes (3 header + 3 payload)! No trailing bytes allowed."
    );
    assert_eq!(frame1, Bytes::from_static(&[0x00, 0x00, 0x03, 0x01, 0x02, 0x03]));

    let frame2 = stream.next().await.unwrap().unwrap();
    assert_eq!(
        frame2.len(),
        4,
        "NCI Frame 2 length must be exactly 4 bytes (3 header + 1 payload)!"
    );
    assert_eq!(frame2, Bytes::from_static(&[0x20, 0x00, 0x01, 0xAA]));

    assert!(stream.next().await.is_none());
}
