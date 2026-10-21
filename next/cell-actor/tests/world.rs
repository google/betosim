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
    io::Error as IoError,
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    pin::Pin,
    time::Duration,
};

use actor_framework::{ResourceClient, ResourceRequest, Response};
use bytes::Bytes;
use cell_actor::{CellAction, CellClient};
use device_actor::{DeviceActor, DeviceClient, DeviceError};
use device_api::{DeviceAction, DeviceActionResult};
use futures::{StreamExt, channel::mpsc as fmpsc, sink::SinkExt};
use netsim_model::{
    Cell, CellNetworkConfig, CellUpdate, Chip, ChipClient, ChipCreate,
    ChipError as NetsimChipError, ChipId, ChipUpdate, ChipVariant, ChipVariantUpdate, ClientError,
    DeviceId, PacketSink, PacketStream, Quirks, RegistrationStatus,
};
use tokio::{sync::mpsc, task::JoinHandle, time::timeout};

const DEVICE_ID: DeviceId = DeviceId(1);
const RECV_TIMEOUT: Duration = Duration::from_secs(2);
pub(crate) const SLIRP_FIRST_V4: Ipv4Addr = Ipv4Addr::new(10, 0, 2, 32);
pub(crate) const SLIRP_FIRST_V6: Ipv6Addr = Ipv6Addr::new(0xfec0, 0, 0, 0, 0, 0, 0, 0x20);

struct GuestChannel {
    stream_tx: fmpsc::Sender<Bytes>,
    sink_rx: fmpsc::Receiver<Bytes>,
}

pub(crate) struct World {
    client: CellClient,
    slirp_client: slirp_actor::SlirpClient,
    device_server_rx: mpsc::Receiver<ResourceRequest<DeviceActor>>,
    pending_device_reply: Option<Response<DeviceActionResult, DeviceError>>,
    pending_delete: Option<JoinHandle<Result<(), ClientError>>>,
    guests: HashMap<ChipId, GuestChannel>,
    event_tx: mpsc::UnboundedSender<modem_rs::HostEvent>,
    server_handle: JoinHandle<()>,
    slirp_handle: JoinHandle<()>,
    last_error: Option<ClientError>,
}

impl Drop for World {
    fn drop(&mut self) {
        if let Some(handle) = &self.pending_delete {
            handle.abort();
        }
        self.server_handle.abort();
        self.slirp_handle.abort();
    }
}

impl World {
    pub(crate) async fn new() -> Self {
        let _ = tracing_subscriber::fmt().with_test_writer().try_init();
        let (device_server_tx, device_server_rx) = mpsc::channel(100);
        let resource_client = ResourceClient::new(device_server_tx);
        let device_client = DeviceClient::new(Box::new(resource_client));
        let (slirp_runner, slirp_client) = slirp_actor::new();
        let slirp_actor = slirp_actor::SlirpActor::new(Default::default(), None, None).await;
        let slirp_handle = tokio::spawn(slirp_runner.run(slirp_actor));

        let (event_tx, event_rx) = mpsc::unbounded_channel::<modem_rs::HostEvent>();
        let controller = Box::new(modem_rs::ModemNetworkSimulator::new(event_tx.clone()));
        let (actor, client) = cell_actor::new();
        let service = cell_actor::CellActor {
            device_client,
            slirp_client: slirp_client.clone(),
            controller,
            active_chips: HashMap::new(),
            event_receiver: Some(event_rx),
        };
        let server_handle = tokio::spawn(actor.run(service));

        Self {
            client,
            slirp_client,
            device_server_rx,
            pending_device_reply: None,
            pending_delete: None,
            guests: HashMap::new(),
            event_tx,
            server_handle,
            slirp_handle,
            last_error: None,
        }
    }

    async fn read_cell(&self, id: u32) -> Cell {
        let chip = self.client.read(ChipId(id)).await.expect("read cell chip");
        match chip.variant {
            Some(ChipVariant::Cell(cell_chip)) => cell_chip,
            other => panic!("expected Cell variant for chip {id}, got {other:?}"),
        }
    }

    async fn recv_guest_packet(&mut self, id: u32) -> Bytes {
        let guest = self.guests.get_mut(&ChipId(id)).expect("guest not found");
        timeout(RECV_TIMEOUT, guest.sink_rx.next())
            .await
            .expect("timeout waiting for guest packet")
            .expect("guest sink channel closed")
    }

    fn ack_pending_device_reply(&mut self) {
        let respond_to = self.pending_device_reply.take().expect("no pending device reply to ack");
        let _ = respond_to.send(Ok(DeviceActionResult::Success));
    }

    pub(crate) async fn given_cell_chip(&mut self, id: u32) {
        self.given_cell_chip_with_quirks(id, Quirks::default(), Vec::new()).await;
    }

    pub(crate) async fn given_cell_chip_with_configs(
        &mut self,
        id: u32,
        configs: Vec<CellNetworkConfig>,
    ) {
        self.given_cell_chip_with_quirks(id, Quirks::default(), configs).await;
    }

    pub(crate) async fn given_cell_chip_with_quirks(
        &mut self,
        id: u32,
        quirks: Quirks,
        configs: Vec<CellNetworkConfig>,
    ) {
        let chip_id = ChipId(id);
        let (stream, sink, stream_tx, sink_rx) = create_dummy_stream_sink();
        let params = create_params(chip_id, stream, sink, quirks, configs);
        self.client.create(chip_id, params).await.expect("given_cell_chip create");
        self.guests.insert(chip_id, GuestChannel { stream_tx, sink_rx });
    }

    pub(crate) async fn given_voice_registered_home(&self, id: u32) {
        self.client
            .perform_action(
                ChipId(id),
                CellAction::SetVoiceRegistration { status: RegistrationStatus::RegisteredHome },
            )
            .await
            .expect("set voice registration");
    }

    pub(crate) async fn when_delete_chip(&self, id: u32) {
        timeout(RECV_TIMEOUT, self.client.delete(ChipId(id)))
            .await
            .expect("delete chip timeout")
            .expect("delete chip");
    }

    pub(crate) fn when_start_delete_chip(&mut self, id: u32) {
        let client = self.client.clone();
        self.pending_delete = Some(tokio::spawn(async move { client.delete(ChipId(id)).await }));
    }

    pub(crate) fn when_device_acks_chip_removed(&mut self) {
        self.ack_pending_device_reply();
    }

    pub(crate) fn when_guest_stream_closes(&mut self, id: u32) {
        let guest = self.guests.get_mut(&ChipId(id)).expect("guest not found");
        guest.stream_tx.close_channel();
    }

    pub(crate) fn when_host_emits_sink_error(&self, id: u32) {
        self.event_tx.send(modem_rs::HostEvent::SinkError(id)).expect("send HostEvent::SinkError");
    }

    pub(crate) async fn when_set_signal_strength(&self, id: u32, rssi: u32, ber: u32) {
        self.client
            .perform_action(ChipId(id), CellAction::SetSignalStrength { rssi, ber })
            .await
            .expect("set signal strength");
    }

    pub(crate) async fn when_set_registrations(
        &self,
        id: u32,
        voice: RegistrationStatus,
        data: RegistrationStatus,
    ) {
        self.client
            .perform_action(ChipId(id), CellAction::SetVoiceRegistration { status: voice })
            .await
            .expect("set voice registration");
        self.client
            .perform_action(ChipId(id), CellAction::SetDataRegistration { status: data })
            .await
            .expect("set data registration");
    }

    pub(crate) async fn when_receive_pdu(&self, id: u32, pdu: &str) {
        self.client
            .perform_action(ChipId(id), CellAction::ReceivePdu { pdu: pdu.to_string() })
            .await
            .expect("receive pdu");
    }

    pub(crate) async fn when_try_receive_pdu(&mut self, id: u32, pdu: &str) {
        let res = self
            .client
            .perform_action(ChipId(id), CellAction::ReceivePdu { pdu: pdu.to_string() })
            .await;
        self.last_error = res.err();
    }

    pub(crate) async fn when_receive_sms(&self, id: u32, sender: &str, text: &str) {
        self.client
            .perform_action(
                ChipId(id),
                CellAction::ReceiveSms { sender: sender.to_string(), text: text.to_string() },
            )
            .await
            .expect("receive sms");
    }

    pub(crate) async fn when_try_incoming_call(&mut self, id: u32, number: &str) {
        let res = self
            .client
            .perform_action(ChipId(id), CellAction::IncomingCall { number: number.to_string() })
            .await;
        self.last_error = res.err();
    }

    pub(crate) async fn when_update_network_configs(
        &self,
        id: u32,
        configs: Vec<CellNetworkConfig>,
    ) {
        let update = ChipUpdate {
            variant: Some(ChipVariantUpdate::Cell(CellUpdate {
                network_configs: Some(configs),
                ..Default::default()
            })),
            ..Default::default()
        };
        self.client.update(ChipId(id), update).await.expect("update network configs");
    }

    pub(crate) fn then_last_error_contains(&self, substr: &str) {
        let err = self.last_error.as_ref().expect("expected error, got Ok");
        let msg = err.to_string();
        assert!(msg.contains(substr), "expected error containing '{substr}', got '{msg}'");
    }

    pub(crate) async fn then_device_notified_chip_removed(&mut self, expected_id: u32) {
        self.then_device_receives_chip_removed_unacked(expected_id).await;
        self.ack_pending_device_reply();
    }

    pub(crate) async fn then_device_receives_chip_removed_unacked(&mut self, expected_id: u32) {
        let msg = timeout(RECV_TIMEOUT, self.device_server_rx.recv())
            .await
            .expect("timed out waiting for device message")
            .expect("device channel closed");
        let ResourceRequest::Action {
            action: DeviceAction::NotifyChipRemoved(device_id, id),
            respond_to,
            ..
        } = msg
        else {
            panic!("unexpected device message: {msg:?}");
        };
        assert_eq!(device_id, DEVICE_ID);
        assert_eq!(id, ChipId(expected_id));
        self.pending_device_reply = Some(respond_to);
    }

    pub(crate) async fn then_read_does_not_block(&self, id: u32) {
        let read_res = timeout(RECV_TIMEOUT, self.client.read(ChipId(id))).await;
        assert!(read_res.is_ok(), "read call was blocked by pending delete notification!");
    }

    pub(crate) async fn then_pending_delete_succeeds(&mut self) {
        let handle = self.pending_delete.take().expect("no pending delete task");
        timeout(RECV_TIMEOUT, handle)
            .await
            .expect("timed out waiting for delete task")
            .expect("delete task panicked")
            .expect("delete chip should succeed");
    }

    pub(crate) async fn then_chip_not_found(&self, expected_id: u32) {
        let res = self.client.read(ChipId(expected_id)).await;
        match res {
            Err(ClientError::Chip(NetsimChipError::ChipNotFound(id))) => {
                assert_eq!(id, ChipId(expected_id));
            }
            other => panic!("expected ChipNotFound({expected_id}), got {other:?}"),
        }
    }

    pub(crate) async fn then_cell_state_is(&self, id: u32, expected_state: &str) {
        assert_eq!(self.read_cell(id).await.state, expected_state);
    }

    pub(crate) async fn then_cell_sms_count_is(&self, id: u32, expected: u32) {
        assert_eq!(self.read_cell(id).await.sms_count, expected);
    }

    pub(crate) async fn then_cell_status_is(
        &self,
        id: u32,
        rssi: u32,
        ber: u32,
        voice: RegistrationStatus,
        data: RegistrationStatus,
    ) {
        let cell = self.read_cell(id).await;
        assert_eq!(cell.rssi, rssi);
        assert_eq!(cell.ber, ber);
        assert_eq!(cell.voice_registration, voice);
        assert_eq!(cell.data_registration, data);
    }

    pub(crate) async fn then_guest_receives_exact_packet(&mut self, id: u32, expected: &str) {
        let packet = self.recv_guest_packet(id).await;
        assert_eq!(packet.as_ref(), expected.as_bytes());
    }

    pub(crate) async fn then_guest_receives_lowercase_packet_framing(
        &mut self,
        id: u32,
        prefix: &str,
        suffix: &str,
    ) {
        let packet = self.recv_guest_packet(id).await;
        let packet_str = std::str::from_utf8(&packet).expect("utf8 decode").to_lowercase();
        assert!(
            packet_str.starts_with(prefix),
            "packet '{packet_str}' does not start with '{prefix}'"
        );
        assert!(packet_str.ends_with(suffix), "packet '{packet_str}' does not end with '{suffix}'");
    }

    pub(crate) async fn then_cell_network_configs_are(
        &self,
        id: u32,
        expected: &[CellNetworkConfig],
    ) {
        assert_eq!(self.read_cell(id).await.network_configs, expected);
    }

    pub(crate) async fn then_cell_has_dual_stack_lease(
        &self,
        id: u32,
        expected_v4: Ipv4Addr,
        expected_v6: Ipv6Addr,
    ) {
        let cell = self.read_cell(id).await;
        assert_eq!(cell.network_configs.len(), 2, "must allocate dual-stack v4 and v6 configs");
        assert_eq!(cell.network_configs[0].ip_address, IpAddr::V4(expected_v4));
        assert_eq!(cell.network_configs[1].ip_address, IpAddr::V6(expected_v6));
    }

    pub(crate) async fn then_slirp_allocates_ipv4_lease(&self, id: u32, expected_v4: Ipv4Addr) {
        let lease = self
            .slirp_client
            .allocate_lease(ChipId(id))
            .await
            .expect("allocate lease")
            .expect("lease returned");
        assert_eq!(lease.v4.ip_address, expected_v4);
    }
}

fn create_dummy_stream_sink()
-> (PacketStream, PacketSink, fmpsc::Sender<Bytes>, fmpsc::Receiver<Bytes>) {
    let (stream_tx, stream_rx) = fmpsc::channel::<Bytes>(64);
    let (sink_tx, sink_rx) = fmpsc::channel::<Bytes>(64);

    let stream: PacketStream = Box::new(stream_rx);
    let sink: PacketSink = Pin::from(Box::new(sink_tx.sink_map_err(IoError::other)));

    (stream, sink, stream_tx, sink_rx)
}

fn create_params(
    chip_id: ChipId,
    stream: PacketStream,
    sink: PacketSink,
    quirks: Quirks,
    network_configs: Vec<CellNetworkConfig>,
) -> ChipCreate {
    let mut chip = Chip::new_test_cell(format!("cell-{chip_id}"));
    chip.id = chip_id.0;
    chip.device_id = DEVICE_ID;
    chip.variant = Some(ChipVariant::Cell(Cell { quirks, network_configs, ..Default::default() }));

    ChipCreate { packet_stream: Some(stream), packet_sink: Some(sink), chip }
}
