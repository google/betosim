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
    io::{self, ErrorKind},
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
    time::Duration,
};

use actor_framework::{FrameworkError, MockActorClient};
use bytes::Bytes;
use common::util::netsim_logger::{ErrorStatsLayer, ErrorStatsTable};
use device_actor::{DeviceActor, DeviceClient, DeviceError};
use device_api::{DeviceAction, DeviceActionResult};
use futures::{FutureExt, Sink, SinkExt};
use netsim_model::{
    Chip, ChipClient, ChipCreate, ChipError, ChipId, ChipUpdate, ClientError, DeviceId, Position,
};
use netsim_testing::{PinnedSink, mock_sink, mock_stream};
use parking_lot::Mutex;
use pdl_runtime::Packet;
use pica::packets::uci::{self, ShortAddressTwoWayRangingMeasurement};
use tokio::sync::mpsc::{self, Receiver, Sender};
use tracing::instrument::WithSubscriber;
use tracing_subscriber::layer::SubscriberExt;
use uwb_actor::{UwbActor, UwbClient};

#[derive(Clone, Copy, Debug, Default)]
pub(crate) enum NotifyBehavior {
    #[default]
    Success,
    DeviceNotFound,
    ActorClosed,
    ActorDropped,
}

enum SinkFault {
    FailWrite(ErrorKind),
    DisconnectOnNextWrite {
        packet_tx: Sender<Bytes>,
        packet_rx: Option<Receiver<Vec<u8>>>,
        use_grpc_disconnect: bool,
    },
}

#[derive(Default)]
struct FaultySinkState {
    fault: Option<SinkFault>,
    faulted: bool,
}

struct FaultySink {
    inner: PinnedSink,
    state: Arc<Mutex<FaultySinkState>>,
}

impl FaultySink {
    fn check_injected_fault(&self) -> Result<(), io::Error> {
        let taken_fault = {
            let mut state = self.state.lock();
            let fault = state.fault.take();
            if fault.is_some() {
                state.faulted = true;
            }
            fault
        };
        match taken_fault {
            None => Ok(()),
            Some(SinkFault::FailWrite(kind)) => {
                Err(io::Error::new(kind, "injected sink write error"))
            }
            Some(SinkFault::DisconnectOnNextWrite {
                packet_tx,
                packet_rx,
                use_grpc_disconnect,
            }) => {
                drop(packet_tx);
                drop(packet_rx);
                if use_grpc_disconnect {
                    Err(io::Error::new(ErrorKind::ConnectionReset, "Remote is stopped"))
                } else {
                    Err(io::Error::new(ErrorKind::BrokenPipe, "guest disconnected"))
                }
            }
        }
    }
}

impl Sink<Bytes> for FaultySink {
    type Error = io::Error;

    fn poll_ready(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        if let Err(err) = self.check_injected_fault() {
            return Poll::Ready(Err(err));
        }
        self.inner.as_mut().poll_ready(cx)
    }

    fn start_send(mut self: Pin<&mut Self>, item: Bytes) -> Result<(), Self::Error> {
        self.inner.as_mut().start_send(item)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.as_mut().poll_flush(cx)
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        let already_faulted = self.state.lock().faulted;
        if already_faulted {
            return Poll::Ready(Err(io::Error::other("sink closed after write fault")));
        }
        self.inner.as_mut().poll_close(cx)
    }
}

fn mock_actor_client(
    behavior: Arc<Mutex<NotifyBehavior>>,
    notify_tx: mpsc::UnboundedSender<()>,
) -> MockActorClient<DeviceActor> {
    let mut mock = MockActorClient::new();
    let action_behavior = Arc::clone(&behavior);
    let action_tx = notify_tx.clone();
    mock.expect_perform_action().returning(move |_, action| {
        if matches!(action, DeviceAction::NotifyChipRemoved(..)) {
            let _ = action_tx.send(());
        }
        let current_behavior = *action_behavior.lock();
        match current_behavior {
            NotifyBehavior::Success => Ok(DeviceActionResult::Success),
            NotifyBehavior::DeviceNotFound => {
                Err(FrameworkError::ServiceError(DeviceError::DeviceNotFound("test".to_string())))
            }
            NotifyBehavior::ActorClosed => Err(FrameworkError::ActorClosed(Box::new(
                io::Error::new(ErrorKind::BrokenPipe, "closed"),
            ))),
            NotifyBehavior::ActorDropped => {
                let (tx, rx) = tokio::sync::oneshot::channel::<()>();
                drop(tx);
                let recv_err = rx.now_or_never().unwrap().unwrap_err();
                Err(FrameworkError::ActorDropped(recv_err))
            }
        }
    });
    mock.expect_shutdown().returning(|| Ok(()));
    mock.expect_clone_box()
        .returning(move || Box::new(mock_actor_client(Arc::clone(&behavior), notify_tx.clone())));
    mock
}

/// The BDD World for UWB Actor tests.
pub struct World {
    pub client: UwbClient,
    device_client: DeviceClient,
    pub packet_txs: HashMap<ChipId, Sender<Bytes>>,
    pub packet_rxs: HashMap<ChipId, Receiver<Vec<u8>>>,
    sink_states: HashMap<ChipId, Arc<Mutex<FaultySinkState>>>,
    error_stats: Arc<ErrorStatsTable>,
    notify_behavior: Arc<Mutex<NotifyBehavior>>,
    notify_called_rx: mpsc::UnboundedReceiver<()>,
    actor_task: tokio::task::JoinHandle<()>,
    // Keeps >= 2 live Dispatchers so tracing_core Rebuilder::JustOne cannot
    // cache Interest::never() when parallel NoSubscriber tests run.
    _multi_dispatch_guard: Option<tracing::Dispatch>,
}

impl Drop for World {
    fn drop(&mut self) {
        self.actor_task.abort();
    }
}

impl World {
    pub async fn new() -> Self {
        Self::build(false).await
    }

    pub(crate) async fn with_error_stats() -> Self {
        Self::build(true).await
    }

    async fn build(record_error_stats: bool) -> Self {
        let (runner, client) = uwb_actor::new();
        let notify_behavior = Arc::new(Mutex::new(NotifyBehavior::default()));
        let (notify_tx, notify_called_rx) = mpsc::unbounded_channel();
        let mock_device_client = mock_actor_client(Arc::clone(&notify_behavior), notify_tx);
        let device_client = DeviceClient::new(Box::new(mock_device_client));

        // Spawn actor
        let actor = UwbActor::new(device_client.clone());
        let error_stats = Arc::new(ErrorStatsTable::new());
        let (actor_task, multi_dispatch_guard) = if record_error_stats {
            let guard = tracing::Dispatch::new(tracing_subscriber::registry());
            let subscriber = tracing_subscriber::registry()
                .with(ErrorStatsLayer::with_table(Arc::clone(&error_stats)));
            (tokio::spawn(runner.run(actor).with_subscriber(subscriber)), Some(guard))
        } else {
            (tokio::spawn(runner.run(actor)), None)
        };

        World {
            client,
            device_client,
            packet_txs: HashMap::new(),
            packet_rxs: HashMap::new(),
            sink_states: HashMap::new(),
            error_stats,
            notify_behavior,
            notify_called_rx,
            actor_task,
            _multi_dispatch_guard: multi_dispatch_guard,
        }
    }

    pub(crate) fn given_device_notify_behavior(&self, behavior: NotifyBehavior) {
        *self.notify_behavior.lock() = behavior;
    }

    pub(crate) fn given_packet_sink_write_error(
        &self,
        chip_id: impl Into<ChipId>,
        kind: ErrorKind,
    ) {
        let chip_id = chip_id.into();
        let sink_state = self.sink_states.get(&chip_id).expect("Chip sink_state not found");
        sink_state.lock().fault = Some(SinkFault::FailWrite(kind));
    }

    pub(crate) fn when_guest_disconnects_on_next_packet(
        &mut self,
        chip_id: impl Into<ChipId>,
        use_grpc_disconnect: bool,
    ) {
        let chip_id = chip_id.into();
        let packet_tx =
            self.packet_txs.remove(&chip_id).expect("Chip packet_tx not found or already closed");
        let packet_rx = self.packet_rxs.remove(&chip_id);
        let sink_state = self.sink_states.get(&chip_id).expect("Chip sink_state not found");
        sink_state.lock().fault =
            Some(SinkFault::DisconnectOnNextWrite { packet_tx, packet_rx, use_grpc_disconnect });
    }

    pub(crate) async fn when_device_actor_shuts_down(&self) {
        self.given_device_notify_behavior(NotifyBehavior::ActorClosed);
        self.device_client.shutdown().await.unwrap();
    }

    pub(crate) async fn then_device_notify_completed(&mut self) {
        tokio::time::timeout(Duration::from_secs(1), self.notify_called_rx.recv())
            .await
            .expect("Timed out waiting for notify_chip_removed")
            .expect("notify channel closed unexpectedly");
        tokio::task::yield_now().await;
    }

    pub(crate) async fn then_error_stats_are(&self, expected: &[(u32, u32)]) {
        if expected.is_empty() {
            assert_eq!(self.error_stats.snapshot(), expected);
            return;
        }
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        loop {
            let actual = self.error_stats.snapshot();
            if actual == expected || std::time::Instant::now() >= deadline {
                assert_eq!(actual, expected);
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    pub async fn when_create_chip_with_name(
        &mut self,
        chip_id: u32,
        name: &str,
    ) -> Result<(), ClientError> {
        let id = ChipId(chip_id);
        let (stream, packet_tx) = mock_stream();
        let (inner_sink, packet_rx) = mock_sink();
        let sink_state = Arc::new(Mutex::new(FaultySinkState::default()));
        let sink = Box::pin(
            FaultySink { inner: inner_sink, state: Arc::clone(&sink_state) }
                .sink_map_err(io::Error::other),
        );
        let mut chip = Chip::new_test_uwb(name.to_string());
        chip.id = id.0;
        chip.device_id = DeviceId(1);

        let params = ChipCreate { packet_stream: Some(stream), packet_sink: Some(sink), chip };

        self.client.create(id, params).await?;

        self.packet_txs.insert(id, packet_tx);
        self.packet_rxs.insert(id, packet_rx);
        self.sink_states.insert(id, sink_state);

        Ok(())
    }

    pub async fn when_create_chip(&mut self, chip_id: u32) -> Result<(), ClientError> {
        self.when_create_chip_with_name(chip_id, &format!("uwb_chip_{chip_id}")).await
    }

    pub async fn when_delete_chip(&self, chip_id: u32) -> Result<(), ClientError> {
        self.client.delete(ChipId(chip_id)).await
    }

    pub async fn when_get_chip(&self, chip_id: u32) -> Result<Chip, ClientError> {
        self.client.read(ChipId(chip_id)).await
    }

    pub fn and_packet_stream_is_closed(&mut self, chip_id: u32) {
        let chip_id = ChipId(chip_id);
        self.packet_txs.remove(&chip_id);
    }

    pub fn and_packet_sink_is_closed(&mut self, chip_id: u32) {
        let chip_id = ChipId(chip_id);
        self.packet_rxs.remove(&chip_id);
    }

    pub async fn then_chip_does_not_exist(&self, chip_id: u32) {
        let err = tokio::time::timeout(std::time::Duration::from_secs(1), async {
            loop {
                if let Err(err) = self.when_get_chip(chip_id).await {
                    break err;
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("Timed out waiting for chip deletion");
        let Some(ChipError::ChipNotFound(ChipId(actual_chip_id))) = err.as_chip_error() else {
            panic!("Unexpected error: {err}");
        };
        assert_eq!(*actual_chip_id, chip_id);
    }

    pub async fn when_packet_is_sent(&mut self, chip_id: u32, packet: &[u8]) {
        let chip_id = ChipId(chip_id);
        let tx = self.packet_txs.get_mut(&chip_id).expect("Chip not found or already closed");
        tx.send(Bytes::copy_from_slice(packet)).await.expect("Failed to send packet");
    }

    pub async fn then_packet_is_received(&mut self, chip_id: u32) -> Vec<u8> {
        let chip_id = ChipId(chip_id);
        let rx = self.packet_rxs.get_mut(&chip_id).expect("Chip not found or already closed");
        tokio::time::timeout(std::time::Duration::from_secs(1), rx.recv())
            .await
            .expect("Timed out waiting for packet")
            .expect("Packet stream closed unexpectedly")
    }

    pub async fn then_chip_exists(&self, chip_id: u32) {
        self.when_get_chip(chip_id).await.expect("Chip should exist");
    }

    pub async fn given_a_chip(&mut self, chip_id: u32) {
        self.when_create_chip(chip_id).await.expect("GIVEN: Failed to create chip");
    }

    pub async fn given_chip_created(&mut self, name: &str) -> ChipId {
        let chip_id = 1;
        self.when_create_chip_with_name(chip_id, name).await.expect("GIVEN: Failed to create chip");
        ChipId(chip_id)
    }

    pub async fn given_initialized_chip(&mut self, name: &str) -> ChipId {
        let chip_id = self.given_chip_created(name).await;
        self.then_status_notification_is_received(chip_id.0, uci::DeviceState::DeviceStateReady)
            .await;
        chip_id
    }

    pub async fn given_chip_with_nonzero_packet_counters(&mut self, chip_id: impl Into<ChipId>) {
        let chip_id = chip_id.into();
        self.when_uci_is_reset(chip_id.0).await;
        let stats = self.client.read_statistics().await.expect("Failed to read statistics");
        let chip_stats = stats
            .iter()
            .find(|s| s.id == chip_id.0)
            .unwrap_or_else(|| panic!("chip {} not found in statistics", chip_id.0));
        assert!(chip_stats.tx_count > 0, "tx_count was 0");
        assert!(chip_stats.rx_count > 0, "rx_count was 0");
    }

    pub async fn when_chip_is_reset(&self, chip_id: impl Into<ChipId>) {
        self.client.reset(chip_id.into()).await.expect("WHEN: Failed to reset chip");
    }

    pub async fn then_reset_response_is_received(&mut self, chip_id: u32, status: uci::Status) {
        let packet = self.then_packet_is_received(chip_id).await;
        let (rsp, _) = uci::CoreDeviceResetRsp::decode(&packet).expect("reset response");
        assert_eq!(rsp.status, status);
    }

    pub async fn then_status_notification_is_received(
        &mut self,
        chip_id: u32,
        state: uci::DeviceState,
    ) {
        let packet = self.then_packet_is_received(chip_id).await;
        let (ntf, _) = uci::CoreDeviceStatusNtf::decode(&packet).expect("status notification");
        assert_eq!(ntf.device_state, state);
    }

    pub async fn when_uci_is_reset(&mut self, chip_id: u32) {
        let reset_cmd = uci::CoreDeviceResetCmd { reset_config: uci::ResetConfig::UwbsReset };
        self.when_packet_is_sent(chip_id, &reset_cmd.encode_to_vec().unwrap()).await;

        let p1 = self.then_packet_is_received(chip_id).await;
        let (rsp, _) = uci::CoreDeviceResetRsp::decode(&p1).expect("reset response");
        assert_eq!(rsp.status, uci::Status::Ok);

        let p2 = self.then_packet_is_received(chip_id).await;
        let (ntf, _) = uci::CoreDeviceStatusNtf::decode(&p2).expect("status notification (reset)");
        assert_eq!(ntf.device_state, uci::DeviceState::DeviceStateReady);
    }

    pub async fn given_a_chip_at(&mut self, chip_id: u32, pos: netsim_model::Position) {
        self.given_a_chip(chip_id).await;
        self.client
            .update(
                ChipId(chip_id),
                ChipUpdate {
                    pose: netsim_model::PoseUpdate { position: Some(pos), orientation: None },
                    ..Default::default()
                },
            )
            .await
            .expect("Failed to set chip position");
    }

    pub async fn when_uci_session_is_established(
        &mut self,
        chip_id: u32,
        session_id: u32,
        device_type: uci::DeviceType,
        device_role: uci::DeviceRole,
        device_mac: [u8; 2],
        peer_mac: [u8; 2],
    ) {
        self.when_uci_session_with_type_is_established(
            chip_id,
            session_id,
            uci::SessionType::FiraRangingSession,
            (device_type, device_role),
            device_mac,
            peer_mac,
        )
        .await;
    }

    pub async fn when_uci_session_with_type_is_established(
        &mut self,
        chip_id: u32,
        session_id: u32,
        session_type: uci::SessionType,
        (device_type, device_role): (uci::DeviceType, uci::DeviceRole),
        device_mac: [u8; 2],
        peer_mac: [u8; 2],
    ) {
        // 1. Session Init
        let init_cmd = uci::SessionInitCmd { session_id, session_type };
        self.when_packet_is_sent(chip_id, &init_cmd.encode_to_vec().unwrap()).await;

        let p1 = self.then_packet_is_received(chip_id).await;
        let (init_rsp, _) = uci::SessionInitRsp::decode(&p1).expect("session init response");
        assert_eq!(init_rsp.status, uci::Status::Ok);

        let p2 = self.then_packet_is_received(chip_id).await;
        let (status_ntf, _) =
            uci::SessionStatusNtf::decode(&p2).expect("session status notification (init)");
        assert_eq!(status_ntf.session_state, uci::SessionState::SessionStateInit);

        // 2. Set App Config
        let tlvs = vec![
            uci::AppConfigTlv {
                cfg_id: uci::AppConfigTlvType::DeviceType,
                v: vec![device_type as u8],
            },
            uci::AppConfigTlv {
                cfg_id: uci::AppConfigTlvType::DeviceRole,
                v: vec![device_role as u8],
            },
            uci::AppConfigTlv {
                cfg_id: uci::AppConfigTlvType::DeviceMacAddress,
                v: device_mac.to_vec(),
            },
            uci::AppConfigTlv {
                cfg_id: uci::AppConfigTlvType::DstMacAddress,
                v: peer_mac.to_vec(),
            },
            uci::AppConfigTlv {
                cfg_id: uci::AppConfigTlvType::MultiNodeMode,
                v: vec![uci::MultiNodeMode::OneToOne as u8],
            },
            uci::AppConfigTlv {
                cfg_id: uci::AppConfigTlvType::RangingRoundUsage,
                v: vec![uci::RangingRoundUsage::DsTwrDeferredMode as u8],
            },
            uci::AppConfigTlv {
                cfg_id: uci::AppConfigTlvType::ScheduleMode,
                v: vec![uci::ScheduleMode::TimeScheduled as u8],
            },
            // Disable independent ranging ticks on the controlee.
            uci::AppConfigTlv {
                cfg_id: uci::AppConfigTlvType::RangingDuration,
                v: if device_type == uci::DeviceType::Controller { 200u32 } else { u32::MAX }
                    .to_le_bytes()
                    .to_vec(),
            },
        ];
        let config_cmd = uci::SessionSetAppConfigCmd { session_token: session_id, tlvs };
        self.when_packet_is_sent(chip_id, &config_cmd.encode_to_vec().unwrap()).await;

        let p3 = self.then_packet_is_received(chip_id).await;
        let (config_rsp, _) =
            uci::SessionSetAppConfigRsp::decode(&p3).expect("session set app config response");
        assert_eq!(config_rsp.status, uci::Status::Ok);

        let p4 = self.then_packet_is_received(chip_id).await;
        let (status_ntf2, _) =
            uci::SessionStatusNtf::decode(&p4).expect("session status notification (config)");
        assert_eq!(status_ntf2.session_state, uci::SessionState::SessionStateIdle);
    }

    async fn then_session_started_is_received(&mut self, chip_id: u32) {
        let p1 = self.then_packet_is_received(chip_id).await;
        let (start_rsp, _) = uci::SessionStartRsp::decode(&p1).expect("session start response");
        assert_eq!(start_rsp.status, uci::Status::Ok);

        let p2 = self.then_packet_is_received(chip_id).await;
        let (status_ntf, _) =
            uci::SessionStatusNtf::decode(&p2).expect("session status notification (start)");
        assert_eq!(status_ntf.session_state, uci::SessionState::SessionStateActive);

        let p3 = self.then_packet_is_received(chip_id).await;
        let (device_status_ntf, _) =
            uci::CoreDeviceStatusNtf::decode(&p3).expect("device status notification (active)");
        assert_eq!(device_status_ntf.device_state, uci::DeviceState::DeviceStateActive);
    }

    pub async fn when_ranging_is_started(&mut self, chip_id: u32, session_id: u32) {
        let start_cmd = uci::SessionStartCmd { session_id };
        self.when_packet_is_sent(chip_id, &start_cmd.encode_to_vec().unwrap()).await;
        self.then_session_started_is_received(chip_id).await;
    }

    pub async fn when_ranging_is_triggered(&mut self, chip_id: u32, session_id: u32) {
        self.client
            .start_ranging(ChipId(chip_id), session_id)
            .await
            .expect("WHEN: Failed to start ranging via client");
        self.then_session_started_is_received(chip_id).await;
    }

    pub async fn when_ranging_is_stopped(&mut self, chip_id: u32, session_id: u32) -> u64 {
        self.client
            .stop_ranging(ChipId(chip_id), session_id)
            .await
            .expect("WHEN: Failed to stop ranging session");

        let mut extra_measurements = 0;
        loop {
            let p = self.then_packet_is_received(chip_id).await;
            if let Ok((stop_rsp, _)) = uci::SessionStopRsp::decode(&p) {
                assert_eq!(stop_rsp.status, uci::Status::Ok);
                break;
            } else if let Ok((ntf, _)) = uci::ShortMacTwoWaySessionInfoNtf::decode(&p) {
                extra_measurements += ntf.two_way_ranging_measurements.len() as u64;
            } else {
                panic!("Unexpected packet received while stopping ranging: {p:?}");
            }
        }

        let p2 = self.then_packet_is_received(chip_id).await;
        let (status_ntf, _) =
            uci::SessionStatusNtf::decode(&p2).expect("session status notification (stop)");
        assert_eq!(status_ntf.session_state, uci::SessionState::SessionStateIdle);

        let p3 = self.then_packet_is_received(chip_id).await;
        let (device_status_ntf, _) =
            uci::CoreDeviceStatusNtf::decode(&p3).expect("device status notification (ready)");
        assert_eq!(device_status_ntf.device_state, uci::DeviceState::DeviceStateReady);

        extra_measurements
    }

    pub async fn then_ranging_measurement_is_received(
        &mut self,
        chip_id: u32,
        expected_range: u16,
    ) {
        let packet = self.then_packet_is_received(chip_id).await;
        let (ntf, _) =
            uci::ShortMacTwoWaySessionInfoNtf::decode(&packet).expect("ranging info notification");

        assert_eq!(ntf.two_way_ranging_measurements.len(), 1, "Expected exactly one measurement");
        let tolerance = (6.0 * (f32::from(expected_range) / 500.0).max(1.0)).ceil() as u16;
        assert!(
            (expected_range.saturating_sub(tolerance)..=expected_range.saturating_add(tolerance))
                .contains(&ntf.two_way_ranging_measurements[0].distance)
        );
    }

    pub async fn when_create_anchor(
        &mut self,
        chip_id: u32,
        address: Option<&str>,
        pos: Position,
    ) -> Result<(), ClientError> {
        let id = ChipId(chip_id);
        let mut chip =
            Chip::new_test_uwb_anchor(format!("uwb_anchor_{id}"), address.map(str::to_owned));
        chip.id = id.0;
        chip.device_id = DeviceId(1);
        chip.pose.position = pos;

        let params = ChipCreate { packet_stream: None, packet_sink: None, chip };
        self.client.create(id, params).await
    }

    pub async fn given_an_anchor_at(&mut self, chip_id: u32, address: Option<&str>, pos: Position) {
        self.when_create_anchor(chip_id, address, pos)
            .await
            .expect("GIVEN: Failed to create anchor");
    }

    pub async fn then_single_ranging_measurement_is_received(
        &mut self,
        chip_id: u32,
    ) -> ShortAddressTwoWayRangingMeasurement {
        let packet = self.then_packet_is_received(chip_id).await;
        let (ntf, _) =
            uci::ShortMacTwoWaySessionInfoNtf::decode(&packet).expect("ranging info notification");

        assert_eq!(ntf.two_way_ranging_measurements.len(), 1, "Expected exactly one measurement");
        ntf.two_way_ranging_measurements[0].clone()
    }

    pub async fn then_packet_counters_are(
        &self,
        chip_id: impl Into<ChipId>,
        expected_tx: u64,
        expected_rx: u64,
    ) {
        let chip_id = chip_id.into();
        let stats = self.client.read_statistics().await.expect("Failed to read statistics");
        let chip_stats = stats
            .iter()
            .find(|s| s.id == chip_id.0)
            .unwrap_or_else(|| panic!("chip {} not found in statistics", chip_id.0));
        assert_eq!(chip_stats.tx_count, expected_tx, "tx_count mismatch");
        assert_eq!(chip_stats.rx_count, expected_rx, "rx_count mismatch");
    }

    pub async fn given_active_session_between_two_chips(
        &mut self,
        session_type: uci::SessionType,
        initiator_chip: u32,
        initiator_mac: [u8; 2],
        responder_chip: u32,
        responder_mac: [u8; 2],
        session_id: u32,
    ) {
        for chip_id in [initiator_chip, responder_chip] {
            self.given_a_chip(chip_id).await;
            self.then_status_notification_is_received(chip_id, uci::DeviceState::DeviceStateReady)
                .await;
            self.when_uci_is_reset(chip_id).await;
        }
        self.when_uci_session_with_type_is_established(
            initiator_chip,
            session_id,
            session_type,
            (uci::DeviceType::Controller, uci::DeviceRole::Initiator),
            initiator_mac,
            responder_mac,
        )
        .await;
        self.when_uci_session_with_type_is_established(
            responder_chip,
            session_id,
            session_type,
            (uci::DeviceType::Controlee, uci::DeviceRole::Responder),
            responder_mac,
            initiator_mac,
        )
        .await;
        self.when_ranging_is_started(responder_chip, session_id).await;
        self.when_ranging_is_started(initiator_chip, session_id).await;
    }

    pub async fn when_data_message_is_sent(
        &mut self,
        chip_id: u32,
        session_handle: u32,
        peer_mac: [u8; 2],
        data_sequence_number: u16,
        payload: &[u8],
    ) {
        let snd = uci::DataMessageSnd {
            pbf: uci::PacketBoundaryFlag::Complete,
            session_handle,
            destination_address: u64::from(u16::from_le_bytes(peer_mac)),
            data_sequence_number,
            application_data: payload.to_vec(),
        };
        self.when_packet_is_sent(chip_id, &snd.encode_to_vec().unwrap()).await;

        // Skip any initiator ShortMacTwoWaySessionInfoNtf if the 200ms ranging tick fires first.
        let credit_ntf = tokio::time::timeout(std::time::Duration::from_secs(1), async {
            loop {
                let pkt = self.then_packet_is_received(chip_id).await;
                if let Ok((ntf, _)) = uci::SessionDataCreditNtf::decode(&pkt) {
                    return ntf;
                }
                uci::ShortMacTwoWaySessionInfoNtf::decode(&pkt)
                    .expect("expected SessionDataCreditNtf or ShortMacTwoWaySessionInfoNtf");
            }
        })
        .await
        .expect("Timed out waiting for SessionDataCreditNtf");
        assert_eq!(credit_ntf.session_token, session_handle);
        assert_eq!(credit_ntf.credit_availability, uci::CreditAvailability::CreditNotAvailable);
    }

    pub async fn then_data_message_is_received(
        &mut self,
        chip_id: u32,
        expected_session_handle: u32,
        expected_sender_mac: [u8; 2],
        expected_sequence_number: u16,
        expected_payload: &[u8],
    ) {
        let pkt = self.then_packet_is_received(chip_id).await;
        let (rcv, _) = uci::DataMessageRcv::decode(&pkt).expect("DataMessageRcv notification");

        assert_eq!(rcv.session_handle, expected_session_handle);
        assert_eq!(rcv.status, uci::DataRcvStatusCode::UciStatusSuccess);
        assert_eq!(rcv.source_address, u64::from(u16::from_le_bytes(expected_sender_mac)));
        assert_eq!(rcv.data_sequence_number, expected_sequence_number);
        assert_eq!(rcv.application_data, expected_payload);
    }

    pub async fn then_ranging_session_data_stats_are(
        &self,
        expected_send_data: i32,
        expected_on_data_received: i32,
    ) {
        use netsim_proto::{protobuf::Message, stats::UwbApiStats};

        let stats_bytes = self.client.get_global_stats().await.unwrap().unwrap();
        let stats = UwbApiStats::parse_from_bytes(&stats_bytes).unwrap();
        assert_eq!(stats.ranging_session.send_data(), expected_send_data);
        assert_eq!(stats.ranging_session.on_data_received(), expected_on_data_received);
    }
}
