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
    net::TcpStream as StdTcpStream,
    sync::{
        Arc,
        atomic::{AtomicU32, Ordering},
    },
    time::Duration,
};

use actor_framework::MockActorClient;
use bytes::{Bytes, BytesMut};
use device_actor::{DeviceActor, DeviceClient};
use device_api::{DeviceAction, DeviceActionResult, DeviceAddChip, DeviceId};
use futures::{SinkExt, StreamExt};
use netsim_model::{ChipId, PacketSink, PacketStream};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    sync::mpsc,
};
use tungstenite::{Message, WebSocket};
use websocket_server::server;

/// Barebones RFC 6455 compliant WebSocket client for integration testing.
pub struct ManualWebSocketClient {
    stream: StdTcpStream,
}

impl ManualWebSocketClient {
    pub async fn connect(host: &str, port: u16, path: &str) -> Result<Self, u16> {
        let mut stream = TcpStream::connect(format!("{host}:{port}"))
            .await
            .expect("Failed to connect to server");

        let key = "dGhlIHNhbXBsZSBub25jZQ==";
        let handshake = format!(
            "GET {path} HTTP/1.1\r\n\
             Host: {host}:{port}\r\n\
             Upgrade: websocket\r\n\
             Connection: Upgrade\r\n\
             Sec-WebSocket-Key: {key}\r\n\
             Sec-WebSocket-Version: 13\r\n\r\n",
        );

        stream.write_all(handshake.as_bytes()).await.unwrap();

        let mut buffer = BytesMut::with_capacity(1024);
        loop {
            if stream.read_buf(&mut buffer).await.expect("Failed to read handshake response") == 0 {
                return Err(0); // EOF
            }

            let mut headers = [httparse::EMPTY_HEADER; 16];
            let mut res = httparse::Response::new(&mut headers);
            match res.parse(&buffer) {
                Ok(httparse::Status::Complete(_)) => {
                    if res.code == Some(101) {
                        break;
                    } else {
                        return Err(res.code.unwrap_or(0));
                    }
                }
                Ok(httparse::Status::Partial) => continue,
                Err(_) => return Err(400),
            }
        }

        // Transition to blocking std stream for tungstenite
        let std_stream = stream.into_std().expect("Failed to convert to std stream");
        std_stream.set_nonblocking(false).expect("Failed to set blocking");

        Ok(Self { stream: std_stream })
    }

    pub async fn send_binary(&mut self, data: Bytes) {
        let std_stream = self.stream.try_clone().unwrap();
        tokio::task::spawn_blocking(move || {
            let mut ws =
                WebSocket::from_raw_socket(std_stream, tungstenite::protocol::Role::Client, None);
            ws.send(Message::Binary(data)).expect("Failed to write message")
        })
        .await
        .unwrap();
    }

    pub async fn read_binary(&mut self) -> Bytes {
        let std_stream = self.stream.try_clone().unwrap();
        tokio::task::spawn_blocking(move || {
            let mut ws =
                WebSocket::from_raw_socket(std_stream, tungstenite::protocol::Role::Client, None);
            loop {
                match ws.read().expect("Failed to read message") {
                    Message::Binary(bin) => return bin,
                    Message::Ping(data) => {
                        ws.send(Message::Pong(data)).expect("Failed to pong");
                    }
                    Message::Close(_) => return Bytes::new(),
                    _ => continue,
                }
            }
        })
        .await
        .unwrap()
    }

    pub async fn close(&mut self) {
        let std_stream = self.stream.try_clone().unwrap();
        tokio::task::spawn_blocking(move || {
            let mut ws =
                WebSocket::from_raw_socket(std_stream, tungstenite::protocol::Role::Client, None);
            let _ = ws.close(None);
            let _ = ws.flush();
        })
        .await
        .unwrap();
    }
}

pub struct TestClientContext {
    pub client: ManualWebSocketClient,
    pub packet_stream: Option<PacketStream>,
    pub packet_sink: Option<PacketSink>,
    pub add_request: Box<DeviceAddChip>,
    pub device_id: DeviceId,
}

pub struct TestWorld {
    port: u16,
    add_chip_rx: mpsc::Receiver<(Box<DeviceAddChip>, DeviceId)>,
    delete_chip_rx: mpsc::Receiver<DeviceId>,
    clients: Vec<TestClientContext>,
    last_handshake_error: Option<u16>,
}

impl TestWorld {
    pub async fn new() -> Self {
        let (add_tx, add_rx) = mpsc::channel(10);
        let (del_tx, del_rx) = mpsc::channel(10);

        let device_id_counter = Arc::new(AtomicU32::new(1000));

        let mut mock = MockActorClient::<DeviceActor>::new();

        let add_tx_clone = add_tx.clone();
        let del_tx_clone = del_tx.clone();
        let id_counter_clone = Arc::clone(&device_id_counter);
        mock.expect_clone_box().returning(move || {
            let mut inner_mock = MockActorClient::<DeviceActor>::new();
            let add_tx = add_tx_clone.clone();
            let del_tx = del_tx_clone.clone();
            let id_counter = Arc::clone(&id_counter_clone);

            inner_mock.expect_perform_action().returning(move |_, action| {
                let add_tx = add_tx.clone();
                match action {
                    DeviceAction::AddChipByGuid { params } => {
                        let device_id = DeviceId(id_counter.fetch_add(1, Ordering::SeqCst));
                        let _ = add_tx.try_send((params, device_id));
                        Ok(DeviceActionResult::AddChipByGuidSuccess {
                            device_id,
                            chip_id: ChipId(0),
                        })
                    }
                    _ => panic!("Unexpected action in mock"),
                }
            });

            inner_mock.expect_delete().returning(move |id| {
                let _ = del_tx.try_send(id);
                Ok(())
            });

            Box::new(inner_mock)
        });

        let device_client = DeviceClient::new(Box::new(mock));

        let listener = common::util::net::bind_tcp_loopback(0).expect("Failed to bind");
        let port = listener.local_addr().unwrap().port();

        tokio::spawn(async move {
            server::run(listener, device_client).await;
        });

        tokio::time::sleep(Duration::from_millis(100)).await;

        Self {
            port,
            add_chip_rx: add_rx,
            delete_chip_rx: del_rx,
            clients: Vec::new(),
            last_handshake_error: None,
        }
    }

    async fn connect_internal(&mut self, host: &str, path: &str) -> Result<usize, u16> {
        match ManualWebSocketClient::connect(host, self.port, path).await {
            Ok(client) => {
                let (mut add_request, device_id) =
                    self.add_chip_rx.recv().await.expect("Did not receive add_chip request");

                let packet_stream = add_request.packet_stream.take().unwrap();
                let packet_sink = add_request.packet_sink.take().unwrap();

                let context = TestClientContext {
                    client,
                    packet_stream: Some(packet_stream),
                    packet_sink: Some(packet_sink),
                    add_request,
                    device_id,
                };
                self.clients.push(context);
                Ok(self.clients.len() - 1)
            }
            Err(e) => {
                self.last_handshake_error = Some(e);
                Err(e)
            }
        }
    }

    pub async fn given_a_websocket_client_connected(
        &mut self,
        name: Option<&str>,
        address: Option<&str>,
    ) -> usize {
        let path = build_path(name, address);
        self.connect_internal("127.0.0.1", &path).await.expect("Failed to connect client")
    }

    pub async fn when_client_attempts_to_connect_to_invalid_path(&mut self) {
        let _ = self.connect_internal("127.0.0.1", "/v1/websocket/invalid").await;
    }

    pub fn then_the_connection_fails_with_status(&self, expected_status: u16) {
        assert_eq!(self.last_handshake_error, Some(expected_status));
    }

    pub fn then_backend_receives_add_chip_request(
        &self,
        client_idx: usize,
        expected_name: &str,
        expected_address: &str,
    ) {
        let req = &self.clients[client_idx].add_request;
        assert_eq!(req.device_config.name, expected_name);
        if let Some(netsim_model::ChipVariant::Bluetooth(ref bt)) = req.chip.variant {
            assert_eq!(bt.address, expected_address);
        } else {
            panic!("Expected Bluetooth chip params");
        }
    }

    pub fn then_backend_receives_default_add_chip_request(&self, client_idx: usize) {
        // The server should use a default name containing the address (port) and empty
        // address. We don't know the exact port easily here without more inspection,
        // but we can at least check that it starts with "websocket-device-".
        let req = &self.clients[client_idx].add_request;
        assert!(req.device_config.name.starts_with("websocket-device-"));
    }

    pub async fn when_client_sends_packet(&mut self, client_idx: usize, packet: &[u8]) {
        self.clients[client_idx].client.send_binary(Bytes::copy_from_slice(packet)).await;
    }

    pub async fn then_backend_receives_packet(&mut self, client_idx: usize, expected: &[u8]) {
        let received = self.clients[client_idx]
            .packet_stream
            .as_mut()
            .unwrap()
            .next()
            .await
            .expect("Stream closed");
        assert_eq!(received.as_ref(), expected);
    }

    pub async fn when_backend_sends_packet(&mut self, client_idx: usize, packet: &[u8]) {
        self.clients[client_idx]
            .packet_sink
            .as_mut()
            .unwrap()
            .send(Bytes::from(packet.to_vec()))
            .await
            .expect("Failed to send to sink");
    }

    pub async fn then_client_receives_packet(&mut self, client_idx: usize, expected: &[u8]) {
        let received = self.clients[client_idx].client.read_binary().await;
        assert_eq!(received, expected);
    }

    pub async fn then_client_receives_eof(&mut self, client_idx: usize) {
        let data = self.clients[client_idx].client.read_binary().await;
        assert!(data.is_empty());
    }

    pub async fn when_client_closes_connection(&mut self, client_idx: usize) {
        self.clients[client_idx].client.close().await;
    }

    pub async fn when_backend_closes_connection(&mut self, client_idx: usize) {
        self.clients[client_idx].packet_stream.take();
        self.clients[client_idx].packet_sink.as_mut().unwrap().close().await.unwrap();
        self.clients[client_idx].packet_sink.take();
    }

    pub async fn then_backend_deletes_chip(&mut self, client_idx: usize) {
        let expected_id = self.clients[client_idx].device_id;
        let deleted_id = self.delete_chip_rx.recv().await.expect("Delete channel closed");
        assert_eq!(deleted_id, expected_id);
    }

    /// Connects a client with `?loopback=1`. No add_chip request is expected,
    /// so the client is returned directly instead of being tracked.
    pub async fn given_a_loopback_client_connected(&mut self) -> ManualWebSocketClient {
        ManualWebSocketClient::connect("127.0.0.1", self.port, "/v1/websocket/bt?loopback=1")
            .await
            .expect("Failed to connect loopback client")
    }

    pub fn then_backend_receives_no_add_chip_request(&mut self) {
        assert!(self.add_chip_rx.try_recv().is_err(), "loopback must not add a chip");
    }

    pub fn then_backend_deletes_nothing(&mut self) {
        assert!(self.delete_chip_rx.try_recv().is_err(), "loopback must not delete a device");
    }
}

fn build_path(name: Option<&str>, address: Option<&str>) -> String {
    let mut path = String::from("/v1/websocket/bt");
    let mut query = Vec::new();
    if let Some(n) = name {
        query.push(format!("name={n}"));
    }
    if let Some(a) = address {
        query.push(format!("address={a}"));
    }
    if !query.is_empty() {
        path.push('?');
        path.push_str(&query.join("&"));
    }
    path
}
