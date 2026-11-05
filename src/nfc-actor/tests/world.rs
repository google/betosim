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

//! BDD World test harness for `nfc-actor` integration tests.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};

use actor_framework::{ActorClient, ActorLifecycle, ActorService, Context};
use bytes::Bytes;
use casimir::packets::nci::{
    CoreInitCommand, CoreInitResponse, CoreResetCommand, CoreResetNotification, CoreResetResponse,
    DeactivationType, DiscoverConfiguration, FeatureEnable, ResetType, RfDeactivateCommand,
    RfDeactivateResponse, RfDiscoverCommand, RfDiscoverResponse, RfSetListenModeRoutingCommand,
    RfSetListenModeRoutingResponse, RfTechnologyAndMode, Status,
};
use device_actor::DeviceActor;
use futures::{StreamExt, future::BoxFuture};
use netsim_model::{
    Chip, ChipClient, ChipCreate, ChipId, ChipUpdate, ChipVariantUpdate, DeviceId,
    NetsimRadioStats, NfcUpdate, Position, RadioUpdate,
};
use nfc_actor::{NfcActor, NfcApi, NfcClient};
use pdl_runtime::Packet;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

// --- Mock Helpers ---

#[derive(Clone, Default)]
struct MockDeviceActorClient {
    positions: Arc<Mutex<HashMap<DeviceId, Position>>>,
}

impl MockDeviceActorClient {
    fn set_position(&self, id: DeviceId, pos: Position) {
        self.positions.lock().unwrap().insert(id, pos);
    }
}

#[async_trait::async_trait]
impl ActorClient<DeviceActor> for MockDeviceActorClient {
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
        id: <DeviceActor as ActorService>::Id,
    ) -> Result<
        Option<<DeviceActor as ActorService>::Entity>,
        actor_framework::FrameworkError<<DeviceActor as ActorService>::Error>,
    > {
        let pos = self.positions.lock().unwrap().get(&id).copied().unwrap_or_default();
        Ok(Some(netsim_model::Device {
            id: id.0,
            name: format!("device-{}", id.0),
            pose: netsim_model::Pose { position: pos, ..Default::default() },
            ..Default::default()
        }))
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
    fn clone_box(&self) -> Box<dyn ActorClient<DeviceActor>> {
        Box::new(self.clone())
    }
}

#[derive(Default)]
pub(crate) struct TestContext {
    tasks: HashMap<ChipId, tokio::task::JoinHandle<ChipId>>,
    stream_tasks: HashMap<ChipId, tokio::task::JoinHandle<()>>,
    aborted_tasks: Vec<ChipId>,
    removed_streams: Vec<ChipId>,
}

impl Context<NfcActor> for TestContext {
    fn set_interval(&mut self, _duration: std::time::Duration) {}
    fn add_stream(&mut self, id: ChipId, mut stream: actor_framework::BoxStream) {
        let handle = tokio::spawn(async move { while stream.next().await.is_some() {} });
        self.stream_tasks.insert(id, handle);
    }
    fn remove_stream(&mut self, id: ChipId) {
        self.removed_streams.push(id);
        if let Some(handle) = self.stream_tasks.remove(&id) {
            handle.abort();
        }
    }
    fn add_typed_stream(&mut self, _id: usize, _stream: actor_framework::BoxTypedStream<()>) {}
    fn remove_typed_stream(&mut self, _id: usize) {}
    fn spawn(&mut self, id: ChipId, task: BoxFuture<'static, ChipId>) {
        let handle = tokio::spawn(task);
        self.tasks.insert(id, handle);
    }
    fn abort(&mut self, id: ChipId) {
        self.aborted_tasks.push(id);
        if let Some(handle) = self.tasks.remove(&id) {
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

pub(crate) struct NfcWorld {
    pub(crate) actor: NfcActor,
    pub(crate) ctx: TestContext,
    pub(crate) packet_receivers: HashMap<ChipId, mpsc::Receiver<Bytes>>,
    pub(crate) packet_senders: HashMap<ChipId, mpsc::Sender<Bytes>>,
    mock_device_client: MockDeviceActorClient,
}

impl Drop for NfcWorld {
    fn drop(&mut self) {
        if let Some(task) = self.actor.scene_task.take() {
            task.abort();
        }
        for (_, handle) in self.ctx.tasks.drain() {
            handle.abort();
        }
        for (_, handle) in self.ctx.stream_tasks.drain() {
            handle.abort();
        }
    }
}

impl NfcWorld {
    pub(crate) async fn new() -> Self {
        let _ = tracing_subscriber::fmt().with_test_writer().try_init();

        let mock_device_client = MockDeviceActorClient::default();
        let device_client = device_actor::DeviceClient::new(Box::new(mock_device_client.clone()));
        let mut actor = NfcActor::new(device_client, Arc::new(nfc_actor::NfcStats::new()));
        actor.start_casimir();

        Self {
            actor,
            ctx: TestContext::default(),
            packet_receivers: HashMap::new(),
            packet_senders: HashMap::new(),
            mock_device_client,
        }
    }

    pub(crate) fn set_device_position(&self, id: DeviceId, x: f32, y: f32, z: f32) {
        self.mock_device_client.set_position(id, Position { x, y, z });
    }

    pub(crate) async fn given_a_chip(&mut self, id_val: u32) {
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

    pub(crate) async fn when_casimir_disconnects(&mut self, id_val: u32) {
        let chip_id = ChipId(id_val);
        let casimir_device_id = self.actor.active_chips.get(&chip_id).unwrap().casimir_device_id;

        let scene_client = self.actor.scene_client.as_ref().unwrap().clone();
        let remove_res = scene_client.remove_device(casimir_device_id).await;
        assert!(remove_res.is_ok());

        if let Some(handle) = self.ctx.tasks.remove(&chip_id) {
            let res = handle.await;
            assert!(res.is_ok());
            assert_eq!(res.unwrap(), chip_id);

            self.actor.on_task_closed(chip_id, &mut self.ctx).await;
        } else {
            panic!("Task handle NOT found for chip {}", id_val);
        }
    }

    pub(crate) async fn then_chip_is_cleaned_up(&mut self, id_val: u32) {
        let chip_id = ChipId(id_val);
        assert!(!self.actor.active_chips.contains_key(&chip_id), "Chip still active in NfcActor");
        assert!(self.ctx.aborted_tasks.contains(&chip_id), "Task was not aborted");
        assert!(self.ctx.removed_streams.contains(&chip_id), "Stream was not removed");
        assert!(
            !self.actor.casimir_to_device.lock().values().any(|&dev_id| dev_id.0 == id_val),
            "Casimir mapping was not cleaned up!"
        );
    }
}

// --- Tier 2 Client BDD World (`ResourceActor<NfcActor>` + `NfcClient`) ---

const CLIENT_STREAM_CHANNEL_CAPACITY: usize = 64;
const NCI_HEADER_LEN: usize = 3;
const PACKET_TIMEOUT: Duration = Duration::from_secs(2);
const POLL_INTERVAL: Duration = Duration::from_millis(10);
const RF_FLUSH_SETTLE_DELAY: Duration = Duration::from_millis(30);
const NCI_DROP_OBSERVATION_WINDOW: Duration = Duration::from_millis(100);
const CASIMIR_POLL_OBSERVATION_WINDOW: Duration = Duration::from_millis(250);
const APDU_OK_RESPONSE_DATA_PACKET: [u8; 5] = [0x00, 0x00, 0x02, 0x90, 0x00];

pub(crate) struct NfcClientWorld {
    client: NfcClient,
    packet_receivers: HashMap<ChipId, mpsc::Receiver<Bytes>>,
    packet_senders: HashMap<ChipId, mpsc::Sender<Bytes>>,
    mock_device_client: MockDeviceActorClient,
    last_reset_chip: Option<Chip>,
    actor_task: Option<tokio::task::JoinHandle<()>>,
}

impl Drop for NfcClientWorld {
    fn drop(&mut self) {
        if let Some(task) = self.actor_task.take() {
            task.abort();
        }
    }
}

impl NfcClientWorld {
    pub(crate) async fn new() -> Self {
        let _ = tracing_subscriber::fmt().with_test_writer().try_init();

        let mock_device_client = MockDeviceActorClient::default();
        let device_client = device_actor::DeviceClient::new(Box::new(mock_device_client.clone()));
        let (runner, client) = nfc_actor::new();
        let actor = NfcActor::new(device_client, client.stats());
        let actor_task = tokio::spawn(runner.run(actor));

        Self {
            client,
            packet_receivers: HashMap::new(),
            packet_senders: HashMap::new(),
            mock_device_client,
            last_reset_chip: None,
            actor_task: Some(actor_task),
        }
    }

    pub(crate) async fn shutdown(mut self) {
        let _ = self.client.shutdown().await;
        if let Some(task) = self.actor_task.take() {
            let _ = task.await;
        }
    }

    fn encode_nci_packet<P: Packet>(packet: P) -> Bytes {
        let mut bytes = packet.encode_to_vec().expect("encode NCI packet");
        if bytes.len() > NCI_HEADER_LEN {
            bytes[2] = u8::try_from(bytes.len() - NCI_HEADER_LEN).expect("NCI payload fits in u8");
        }
        Bytes::from(bytes)
    }

    async fn create_chip(&mut self, chip_id: ChipId, chip: Chip) {
        let (tx_stream, rx_stream) = mpsc::channel(CLIENT_STREAM_CHANNEL_CAPACITY);
        let (tx_sink, rx_sink) = mpsc::channel(CLIENT_STREAM_CHANNEL_CAPACITY);

        let params = ChipCreate {
            packet_stream: Some(Box::new(ReceiverStream::new(rx_stream))),
            packet_sink: Some(Box::pin(MockSink(tx_sink))),
            chip,
        };

        self.client.create(chip_id, params).await.expect("create NFC chip via NfcClient");
        self.packet_senders.insert(chip_id, tx_stream);
        self.packet_receivers.insert(chip_id, rx_sink);
    }

    async fn send_raw_nci(&self, chip_id: ChipId, bytes: Bytes) {
        let tx = self.packet_senders.get(&chip_id).expect("chip sender must exist");
        tx.send(bytes).await.expect("send NCI packet to actor stream");
    }

    async fn recv_nci_packet(&mut self, chip_id: ChipId) -> Bytes {
        let rx = self.packet_receivers.get_mut(&chip_id).expect("chip receiver must exist");
        tokio::time::timeout(PACKET_TIMEOUT, rx.recv())
            .await
            .expect("timed out waiting for NCI packet from Casimir")
            .expect("NCI packet sink closed unexpectedly")
    }

    async fn radio_stats_for(&self, chip_id: ChipId) -> NetsimRadioStats {
        let stats = self.client.read_statistics().await.expect("read NFC radio statistics");
        stats
            .into_vec()
            .into_iter()
            .find(|entry| entry.id == chip_id.0)
            .expect("statistics entry for chip must exist")
    }

    pub(crate) async fn given_chip_at(
        &mut self,
        chip_id: ChipId,
        device_id: DeviceId,
        x: f32,
        y: f32,
        z: f32,
    ) {
        self.mock_device_client.set_position(device_id, Position { x, y, z });
        self.create_chip(chip_id, Chip { device_id, ..Default::default() }).await;
    }

    pub(crate) async fn given_named_chip_at(
        &mut self,
        chip_id: ChipId,
        device_id: DeviceId,
        name: &str,
        x: f32,
        y: f32,
        z: f32,
    ) {
        self.mock_device_client.set_position(device_id, Position { x, y, z });
        self.create_chip(chip_id, Chip { name: name.to_string(), device_id, ..Default::default() })
            .await;
    }

    pub(crate) async fn given_chip_with_metadata(
        &mut self,
        chip_id: ChipId,
        device_id: DeviceId,
        name: &str,
        manufacturer: &str,
        product_name: &str,
    ) {
        self.create_chip(
            chip_id,
            Chip {
                name: name.to_string(),
                manufacturer: manufacturer.to_string(),
                product_name: product_name.to_string(),
                device_id,
                ..Default::default()
            },
        )
        .await;
    }

    pub(crate) async fn given_nci_core_initialized(&mut self, chip_id: ChipId) {
        let reset_cmd = CoreResetCommand { reset_type: ResetType::KeepConfig };
        self.send_raw_nci(chip_id, Self::encode_nci_packet(reset_cmd)).await;

        let reset_rsp_bytes = self.recv_nci_packet(chip_id).await;
        let reset_rsp =
            CoreResetResponse::decode_full(&reset_rsp_bytes).expect("decode CoreResetResponse");
        assert_eq!(reset_rsp.status, Status::Ok);

        let reset_ntf_bytes = self.recv_nci_packet(chip_id).await;
        CoreResetNotification::decode_full(&reset_ntf_bytes).expect("decode CoreResetNotification");

        self.when_nci_core_init_completed(chip_id).await;
    }

    pub(crate) async fn given_nci_listen_mode_routing_configured(&mut self, chip_id: ChipId) {
        let routing_cmd =
            RfSetListenModeRoutingCommand { more_to_follow: 0, routing_entries: vec![] };
        self.send_raw_nci(chip_id, Self::encode_nci_packet(routing_cmd)).await;

        let rsp_bytes = self.recv_nci_packet(chip_id).await;
        let rsp = RfSetListenModeRoutingResponse::decode_full(&rsp_bytes)
            .expect("decode RfSetListenModeRoutingResponse");
        assert_eq!(rsp.status, Status::Ok);
    }

    pub(crate) async fn when_chip_enabled_set(&mut self, chip_id: ChipId, enabled: bool) {
        let updated = self
            .client
            .update(chip_id, ChipUpdate { enabled: Some(enabled), ..Default::default() })
            .await
            .expect("update chip enabled flag");
        assert_eq!(updated.enabled, enabled);
        assert_eq!(updated.is_nfc_enabled(), enabled);
    }

    pub(crate) async fn when_chip_metadata_and_radio_updated(
        &mut self,
        chip_id: ChipId,
        name: &str,
        manufacturer: &str,
        product_name: &str,
        radio_enabled: bool,
    ) {
        let updated = self
            .client
            .update(
                chip_id,
                ChipUpdate {
                    name: Some(name.to_string()),
                    manufacturer: Some(manufacturer.to_string()),
                    product_name: Some(product_name.to_string()),
                    enabled: Some(radio_enabled),
                    variant: Some(ChipVariantUpdate::Nfc(NfcUpdate {
                        radio: RadioUpdate { state: Some(radio_enabled) },
                    })),
                    ..Default::default()
                },
            )
            .await
            .expect("update chip metadata and NFC radio state");
        assert_eq!(updated.enabled, radio_enabled);
        assert_eq!(updated.is_nfc_enabled(), radio_enabled);
    }

    pub(crate) async fn when_nci_core_init_sent(&mut self, chip_id: ChipId) {
        let init_cmd = CoreInitCommand { feature_enable: FeatureEnable {} };
        self.send_raw_nci(chip_id, Self::encode_nci_packet(init_cmd)).await;
    }

    pub(crate) async fn when_nci_apdu_response_data_sent(&mut self, chip_id: ChipId) {
        self.send_raw_nci(chip_id, Bytes::from_static(&APDU_OK_RESPONSE_DATA_PACKET)).await;
    }

    pub(crate) async fn when_nci_core_init_completed(&mut self, chip_id: ChipId) {
        self.when_nci_core_init_sent(chip_id).await;
        let init_rsp_bytes = self.recv_nci_packet(chip_id).await;
        let init_rsp =
            CoreInitResponse::decode_full(&init_rsp_bytes).expect("decode CoreInitResponse");
        assert_eq!(init_rsp.status, Status::Ok);
    }

    pub(crate) async fn when_nci_discovery_started(&mut self, chip_id: ChipId) {
        let discover_cmd = RfDiscoverCommand {
            configurations: vec![DiscoverConfiguration {
                technology_and_mode: RfTechnologyAndMode::NfcAPassivePollMode,
                discovery_frequency: 1,
            }],
        };
        self.send_raw_nci(chip_id, Self::encode_nci_packet(discover_cmd)).await;

        let rsp_bytes = self.recv_nci_packet(chip_id).await;
        let rsp = RfDiscoverResponse::decode_full(&rsp_bytes).expect("decode RfDiscoverResponse");
        assert_eq!(rsp.status, Status::Ok);
    }

    pub(crate) async fn when_nci_discovery_stopped(&mut self, chip_id: ChipId) {
        let deactivate_cmd = RfDeactivateCommand { deactivation_type: DeactivationType::IdleMode };
        self.send_raw_nci(chip_id, Self::encode_nci_packet(deactivate_cmd)).await;

        let rsp_bytes = self.recv_nci_packet(chip_id).await;
        let rsp =
            RfDeactivateResponse::decode_full(&rsp_bytes).expect("decode RfDeactivateResponse");
        assert_eq!(rsp.status, Status::Ok);
    }

    pub(crate) async fn when_chip_is_reset(&mut self, chip_id: ChipId) {
        let chip = self.client.reset(chip_id).await.expect("reset NFC chip");
        self.last_reset_chip = Some(chip);
    }

    pub(crate) async fn then_no_nci_packet_is_received(&mut self, chip_id: ChipId) {
        let rx = self.packet_receivers.get_mut(&chip_id).expect("chip receiver must exist");
        let recv_res = tokio::time::timeout(NCI_DROP_OBSERVATION_WINDOW, rx.recv()).await;
        assert!(recv_res.is_err(), "disabled NFC chip must not emit NCI packets");
    }

    pub(crate) async fn then_chip_packet_counts_are(
        &self,
        chip_id: ChipId,
        expected_tx: u64,
        expected_rx: u64,
    ) {
        let stats = self.radio_stats_for(chip_id).await;
        assert_eq!(stats.tx_count, expected_tx, "unexpected tx_count for chip {chip_id}");
        assert_eq!(stats.rx_count, expected_rx, "unexpected rx_count for chip {chip_id}");
    }

    pub(crate) async fn then_chip_tx_count_at_least(&self, chip_id: ChipId, min_tx: u64) {
        let stats = self.radio_stats_for(chip_id).await;
        assert!(
            stats.tx_count >= min_tx,
            "expected tx_count >= {min_tx} for chip {chip_id}, got {}",
            stats.tx_count
        );
    }

    pub(crate) fn then_api_stat_count_is(&self, api: NfcApi, expected: u32) {
        assert_eq!(self.client.stats().get(api), expected, "unexpected count for {api:?}");
    }

    pub(crate) async fn then_rf_polls_transmitted_while_peer_receives_none(&self) {
        tokio::time::timeout(PACKET_TIMEOUT, async {
            while self.client.stats().to_proto().rf_taps_tx() < 1 {
                tokio::time::sleep(POLL_INTERVAL).await;
            }
        })
        .await
        .expect("enabled polling chip must transmit RF poll packets");

        tokio::time::sleep(RF_FLUSH_SETTLE_DELAY).await;
        assert_eq!(
            self.client.stats().to_proto().rf_taps_rx(),
            0,
            "disabled peer chip must drop ingress RF poll packets"
        );
    }

    pub(crate) async fn then_rf_tx_remains_unchanged_over_poll_window(&self) {
        tokio::time::sleep(RF_FLUSH_SETTLE_DELAY).await;
        let baseline_rf_taps_tx = self.client.stats().to_proto().rf_taps_tx();
        tokio::time::sleep(CASIMIR_POLL_OBSERVATION_WINDOW).await;
        assert_eq!(
            self.client.stats().to_proto().rf_taps_tx(),
            baseline_rf_taps_tx,
            "disabled polling chip must drop egress RF poll packets across Casimir poll ticks"
        );
    }

    pub(crate) async fn then_rf_rx_resumes(&self) {
        tokio::time::timeout(PACKET_TIMEOUT, async {
            while self.client.stats().to_proto().rf_taps_rx() < 1 {
                tokio::time::sleep(POLL_INTERVAL).await;
            }
        })
        .await
        .expect("re-enabled chips in proximity must resume RF packet delivery");
    }

    pub(crate) async fn then_chip_read_matches(
        &self,
        chip_id: ChipId,
        name: &str,
        manufacturer: &str,
        product_name: &str,
        enabled: bool,
    ) {
        let chip = self.client.read(chip_id).await.expect("read NFC chip");
        assert_eq!(chip.name, name);
        assert_eq!(chip.manufacturer, manufacturer);
        assert_eq!(chip.product_name, product_name);
        assert_eq!(chip.enabled, enabled);
        assert_eq!(chip.is_nfc_enabled(), enabled);
    }

    pub(crate) async fn then_reset_restored_initial_chip(
        &self,
        chip_id: ChipId,
        name: &str,
        manufacturer: &str,
        product_name: &str,
    ) {
        let restored = self.last_reset_chip.as_ref().expect("reset result chip must be recorded");
        assert_eq!(restored.name, name);
        assert_eq!(restored.manufacturer, manufacturer);
        assert_eq!(restored.product_name, product_name);
        assert!(restored.enabled);
        assert!(restored.is_nfc_enabled());

        self.then_chip_read_matches(chip_id, name, manufacturer, product_name, true).await;
    }

    pub(crate) async fn when_chip_name_updated(&mut self, chip_id: ChipId, name: &str) {
        self.client
            .update(chip_id, ChipUpdate { name: Some(name.to_string()), ..Default::default() })
            .await
            .expect("update chip name");
    }

    pub(crate) async fn then_p2p_traffic_exchanged(&self, chip_a: ChipId, chip_b: ChipId) {
        tokio::time::timeout(PACKET_TIMEOUT, async {
            loop {
                let stats = self.client.read_statistics().await.expect("read NFC radio statistics");
                let stats_a = stats.iter().find(|s| s.id == chip_a.0);
                let stats_b = stats.iter().find(|s| s.id == chip_b.0);
                let ready = match (stats_a, stats_b) {
                    (Some(a), Some(b)) => {
                        a.p2p_tx_count >= 1
                            && a.p2p_rx_count >= 1
                            && b.p2p_tx_count >= 1
                            && b.p2p_rx_count >= 1
                    }
                    _ => false,
                };
                if ready {
                    break;
                }
                tokio::time::sleep(POLL_INTERVAL).await;
            }
        })
        .await
        .expect("timed out waiting for p2p traffic between chips");
    }

    pub(crate) async fn then_chip_radio_stats(
        &self,
        chip_id: ChipId,
        expected_name: &str,
        min_tx: u64,
        min_rx: u64,
        min_p2p_tx: u64,
        min_p2p_rx: u64,
    ) {
        let stats = self.radio_stats_for(chip_id).await;
        assert_eq!(stats.name, expected_name, "unexpected radio stats name for chip {chip_id}");
        assert!(
            stats.tx_count >= min_tx,
            "expected tx_count >= {min_tx} for chip {chip_id}, got {}",
            stats.tx_count
        );
        assert!(
            stats.rx_count >= min_rx,
            "expected rx_count >= {min_rx} for chip {chip_id}, got {}",
            stats.rx_count
        );
        assert!(
            stats.p2p_tx_count >= min_p2p_tx,
            "expected p2p_tx_count >= {min_p2p_tx} for chip {chip_id}, got {}",
            stats.p2p_tx_count
        );
        assert!(
            stats.p2p_rx_count >= min_p2p_rx,
            "expected p2p_rx_count >= {min_p2p_rx} for chip {chip_id}, got {}",
            stats.p2p_rx_count
        );
    }

    pub(crate) async fn then_radio_stats_name_is(&self, chip_id: ChipId, expected_name: &str) {
        let stats = self.radio_stats_for(chip_id).await;
        assert_eq!(stats.name, expected_name);
    }

    pub(crate) async fn then_chip_radio_stats_zeroed(&self, chip_id: ChipId, expected_name: &str) {
        let stats = self.radio_stats_for(chip_id).await;
        assert_eq!(stats.name, expected_name, "unexpected radio stats name for chip {chip_id}");
        assert_eq!(
            stats.tx_count, 0,
            "expected tx_count == 0 for chip {chip_id}, got {}",
            stats.tx_count
        );
        assert_eq!(
            stats.rx_count, 0,
            "expected rx_count == 0 for chip {chip_id}, got {}",
            stats.rx_count
        );
        assert_eq!(
            stats.p2p_tx_count, 0,
            "expected p2p_tx_count == 0 for chip {chip_id}, got {}",
            stats.p2p_tx_count
        );
        assert_eq!(
            stats.p2p_rx_count, 0,
            "expected p2p_rx_count == 0 for chip {chip_id}, got {}",
            stats.p2p_rx_count
        );
    }
}
