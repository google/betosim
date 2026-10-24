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

//! BDD World test harness for `hci-server` integration tests.

use std::{
    net::SocketAddr,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU32, Ordering},
    },
    time::Duration,
};

use actor_framework::MockActorClient;
use bytes::Bytes;
use device_actor::{DeviceActor, DeviceClient};
use device_api::{DeviceAction, DeviceActionResult, DeviceAddChip, DeviceId};
use futures::{SinkExt, StreamExt};
use netsim_model::{ChipId, ChipKind, ChipVariant, PacketSink, PacketStream};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::mpsc,
    task::JoinHandle,
    time::timeout,
};

const STEP_TIMEOUT: Duration = Duration::from_secs(2);

struct ConnectedHciClient {
    stream: TcpStream,
    client_addr: SocketAddr,
    registration: DeviceAddChip,
    packet_stream: PacketStream,
    packet_sink: PacketSink,
}

pub struct World {
    server_addr: SocketAddr,
    server_task: JoinHandle<()>,
    registered_rx: mpsc::Receiver<DeviceAddChip>,
    rejected_rx: mpsc::Receiver<String>,
    fail_next_add_chip: Arc<AtomicBool>,
    clients: Vec<ConnectedHciClient>,
}

impl Drop for World {
    fn drop(&mut self) {
        self.server_task.abort();
    }
}

impl World {
    pub async fn given_running_server() -> Self {
        let (registered_tx, registered_rx) = mpsc::channel(16);
        let (rejected_tx, rejected_rx) = mpsc::channel(16);
        let fail_next_add_chip = Arc::new(AtomicBool::new(false));
        let next_id = Arc::new(AtomicU32::new(1));

        let mut mock_device = MockActorClient::<DeviceActor>::new();
        let fail_flag = Arc::clone(&fail_next_add_chip);
        let id_seq = Arc::clone(&next_id);

        mock_device.expect_clone_box().returning(move || {
            let mut per_conn_mock = MockActorClient::<DeviceActor>::new();
            let reg_tx = registered_tx.clone();
            let rej_tx = rejected_tx.clone();
            let should_fail = Arc::clone(&fail_flag);
            let seq = Arc::clone(&id_seq);

            per_conn_mock.expect_perform_action().returning(move |target_id, action| {
                assert!(target_id.is_none());
                let DeviceAction::AddChipByGuid { params } = action else {
                    panic!("Unexpected DeviceAction in hci-server test: {action:?}");
                };
                if should_fail.swap(false, Ordering::SeqCst) {
                    let _ = rej_tx.try_send(params.device_guid);
                    // Returning `Success` instead of `AddChipByGuidSuccess` causes
                    // `DeviceClient::add_chip` to return `Err(ClientError::Recv(...))`.
                    return Ok(DeviceActionResult::Success);
                }
                let assigned = seq.fetch_add(1, Ordering::SeqCst);
                let _ = reg_tx.try_send(*params);
                Ok(DeviceActionResult::AddChipByGuidSuccess {
                    device_id: DeviceId(assigned),
                    chip_id: ChipId(assigned),
                })
            });

            Box::new(per_conn_mock)
        });

        let listener = TcpListener::bind("127.0.0.1:0").await.expect("failed to bind TCP listener");
        let server_addr = listener.local_addr().expect("failed to read listener local_addr");
        let device_client = DeviceClient::new(Box::new(mock_device));
        let server_task = tokio::spawn(hci_server::run(listener, device_client));

        Self {
            server_addr,
            server_task,
            registered_rx,
            rejected_rx,
            fail_next_add_chip,
            clients: Vec::new(),
        }
    }

    pub fn given_next_add_chip_will_fail(&self) {
        self.fail_next_add_chip.store(true, Ordering::SeqCst);
    }

    pub async fn when_client_connects(&mut self) -> usize {
        let stream =
            TcpStream::connect(self.server_addr).await.expect("failed to connect HCI TCP client");
        let client_addr = stream.local_addr().expect("failed to read client local_addr");

        let mut registration = timeout(STEP_TIMEOUT, self.registered_rx.recv())
            .await
            .expect("timed out waiting for AddChipByGuid registration")
            .expect("registration channel closed");

        let packet_stream =
            registration.packet_stream.take().expect("registered chip missing packet_stream");
        let packet_sink =
            registration.packet_sink.take().expect("registered chip missing packet_sink");

        let idx = self.clients.len();
        self.clients.push(ConnectedHciClient {
            stream,
            client_addr,
            registration,
            packet_stream,
            packet_sink,
        });
        idx
    }

    pub async fn when_client_connects_and_registration_fails(&mut self) -> SocketAddr {
        let stream =
            TcpStream::connect(self.server_addr).await.expect("failed to connect HCI TCP client");
        let client_addr = stream.local_addr().expect("failed to read client local_addr");
        let rejected_guid = timeout(STEP_TIMEOUT, self.rejected_rx.recv())
            .await
            .expect("timed out waiting for rejected AddChipByGuid")
            .expect("rejected channel closed");
        assert_eq!(rejected_guid, format!("socket-{client_addr}"));
        client_addr
    }

    pub fn then_bluetooth_chip_is_registered(&self, client_idx: usize) {
        let client = &self.clients[client_idx];
        let expected_name = format!("socket-{}", client.client_addr);
        assert_eq!(client.registration.device_guid, expected_name);
        assert_eq!(client.registration.device_config.name, expected_name);
        assert!(client.registration.device_config.visible);
        assert!(!client.registration.device_config.builtin);
        assert_eq!(client.registration.chip.name, expected_name);
        assert_eq!(client.registration.chip.kind, ChipKind::BLUETOOTH);
        assert!(matches!(client.registration.chip.variant, Some(ChipVariant::Bluetooth(_))));
    }

    pub async fn when_client_sends_h4_bytes(&mut self, client_idx: usize, raw_bytes: &[u8]) {
        let stream = &mut self.clients[client_idx].stream;
        stream.write_all(raw_bytes).await.expect("failed to write bytes to HCI client socket");
        stream.flush().await.expect("failed to flush HCI client socket");
    }

    pub async fn then_packet_stream_receives(&mut self, client_idx: usize, expected: &[u8]) {
        let frame = timeout(STEP_TIMEOUT, self.clients[client_idx].packet_stream.next())
            .await
            .expect("timed out waiting for frame on packet_stream")
            .expect("packet_stream closed before expected frame");
        assert_eq!(frame.as_ref(), expected);
    }

    pub async fn when_client_disconnects(&mut self, client_idx: usize) {
        self.clients[client_idx]
            .stream
            .shutdown()
            .await
            .expect("failed to shutdown HCI client socket");
    }

    pub async fn then_packet_stream_ends(&mut self, client_idx: usize) {
        let next = timeout(STEP_TIMEOUT, self.clients[client_idx].packet_stream.next())
            .await
            .expect("timed out waiting for packet_stream EOF");
        assert!(next.is_none(), "expected packet_stream to terminate with None");
    }

    pub async fn when_backend_sends_to_packet_sink(&mut self, client_idx: usize, payload: &[u8]) {
        self.clients[client_idx]
            .packet_sink
            .send(Bytes::copy_from_slice(payload))
            .await
            .expect("failed to send downlink packet to packet_sink");
    }

    pub async fn then_client_socket_receives(&mut self, client_idx: usize, expected: &[u8]) {
        let mut received = vec![0u8; expected.len()];
        timeout(STEP_TIMEOUT, self.clients[client_idx].stream.read_exact(&mut received))
            .await
            .expect("timed out reading downlink bytes on client socket")
            .expect("failed to read exact bytes from client socket");
        assert_eq!(received.as_slice(), expected);
    }
}
