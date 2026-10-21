// Copyright 2025 The Android Open Source Project
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
//=============================================================================
// tests/world.rs - BDD World test harness for packet-stream integration tests
//=============================================================================

#[cfg(unix)]
use std::path::PathBuf;
#[cfg(unix)]
use std::sync::atomic::{AtomicU64, Ordering};
use std::{collections::HashMap, time::Duration};

use bytes::Bytes;
use futures::{SinkExt, StreamExt};
use packet_stream::{
    ChipInfo, ChipKind, StreamAddress, Streams, TransportType,
    transport::traits::{PacketSink, PacketStream},
};
use tokio::{io::AsyncWriteExt, net::TcpStream, time::timeout};

#[cfg(unix)]
static UDS_COUNTER: AtomicU64 = AtomicU64::new(0);

const TEST_TIMEOUT: Duration = Duration::from_secs(5);
const SHORT_TIMEOUT: Duration = Duration::from_millis(500);

pub(crate) fn create_chip_info(name: &str) -> ChipInfo {
    ChipInfo::new(name, ChipKind::WIFI)
}

struct Endpoint {
    stream: PacketStream,
    sink: Option<PacketSink>,
    chip_info: Option<ChipInfo>,
}

impl Endpoint {
    async fn send_packet(&mut self, packet: Bytes) {
        let sink = self.sink.as_mut().expect("sink is closed or missing");
        timeout(TEST_TIMEOUT, sink.send(packet))
            .await
            .expect("send timed out")
            .expect("send failed");
    }

    async fn recv_packet(&mut self, expected: &[u8]) {
        let packet = timeout(TEST_TIMEOUT, self.stream.next())
            .await
            .expect("recv timed out")
            .expect("stream reached EOF")
            .expect("stream read error");
        assert_eq!(&packet[..], expected);
    }

    async fn close_sink(&mut self) {
        let mut sink = self.sink.take().expect("sink already closed");
        timeout(TEST_TIMEOUT, sink.close()).await.expect("close timed out").expect("close failed");
    }

    async fn assert_closed(&mut self) {
        let res = timeout(TEST_TIMEOUT, self.stream.next()).await;
        assert!(matches!(res, Ok(None | Some(Err(_)))), "expected EOF or error, got {res:?}");
    }
}

pub(crate) struct World {
    streams: Streams,
    listener_targets: HashMap<String, TransportType>,
    #[cfg(unix)]
    uds_paths: Vec<PathBuf>,
    clients: HashMap<String, Endpoint>,
    servers: HashMap<String, Endpoint>,
    raw_tcp_streams: HashMap<String, TcpStream>,
    last_connect_succeeded: Option<bool>,
}

impl World {
    pub(crate) fn new() -> Self {
        Self {
            streams: Streams::new(),
            listener_targets: HashMap::new(),
            #[cfg(unix)]
            uds_paths: Vec::new(),
            clients: HashMap::new(),
            servers: HashMap::new(),
            raw_tcp_streams: HashMap::new(),
            last_connect_succeeded: None,
        }
    }

    fn client_mut(&mut self, name: &str) -> &mut Endpoint {
        self.clients.get_mut(name).unwrap_or_else(|| panic!("client endpoint not found: {name}"))
    }

    fn server_mut(&mut self, name: &str) -> &mut Endpoint {
        self.servers.get_mut(name).unwrap_or_else(|| panic!("server endpoint not found: {name}"))
    }

    fn listener_target(&self, listener_name: &str) -> TransportType {
        self.listener_targets
            .get(listener_name)
            .cloned()
            .unwrap_or_else(|| panic!("listener target not found for {listener_name}"))
    }

    pub(crate) async fn given_tcp_listener(&mut self, name: &str) {
        self.streams
            .start_listener(name, TransportType::tcp("localhost", 0))
            .await
            .expect("start TCP listener");
        let listener_addr = self.streams.listener_address(name).expect("listener address");
        let StreamAddress::Tcp(addr) = listener_addr else {
            panic!("Expected TCP address for listener {name}");
        };
        self.listener_targets
            .insert(name.to_string(), TransportType::tcp("localhost", addr.port()));
    }

    #[cfg(unix)]
    pub(crate) async fn given_uds_listener(&mut self, name: &str) {
        let count = UDS_COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "netsim_test_{}_{}_{}.sock",
            std::process::id(),
            count,
            name
        ));
        let transport = TransportType::uds(path.to_string_lossy());
        self.uds_paths.push(path);
        self.streams.start_listener(name, transport.clone()).await.expect("start UDS listener");
        self.listener_targets.insert(name.to_string(), transport);
    }

    pub(crate) async fn when_connect_client(
        &mut self,
        client_name: &str,
        listener_name: &str,
        chip_info: ChipInfo,
    ) {
        let transport = self.listener_target(listener_name);
        let (stream, sink) = timeout(TEST_TIMEOUT, self.streams.connect(transport, chip_info))
            .await
            .expect("timeout connecting client")
            .expect("connect client");

        assert!(
            self.clients
                .insert(
                    client_name.to_string(),
                    Endpoint { stream, sink: Some(sink), chip_info: None },
                )
                .is_none(),
            "duplicate client endpoint for {client_name}"
        );
    }

    pub(crate) async fn when_accept_connection(&mut self) {
        let (_listener_name, (stream, sink, chip_info, _guid)) =
            timeout(TEST_TIMEOUT, self.streams.accept_any())
                .await
                .expect("timeout accepting connection")
                .expect("accept connection");

        assert!(
            self.servers
                .insert(
                    chip_info.name.clone(),
                    Endpoint { stream, sink: Some(sink), chip_info: Some(chip_info) },
                )
                .is_none(),
            "duplicate server endpoint for chip name"
        );
    }

    pub(crate) async fn when_connect_raw_tcp(&mut self, stream_name: &str, listener_name: &str) {
        let transport = self.listener_target(listener_name);
        let TransportType::Tcp { port, .. } = transport else {
            panic!("Expected TCP transport for {listener_name}");
        };
        let stream = timeout(TEST_TIMEOUT, TcpStream::connect(format!("localhost:{port}")))
            .await
            .expect("timeout connecting raw TCP")
            .expect("connect raw TCP");
        self.raw_tcp_streams.insert(stream_name.to_string(), stream);
    }

    pub(crate) fn when_drop_raw_tcp(&mut self, stream_name: &str) {
        assert!(
            self.raw_tcp_streams.remove(stream_name).is_some(),
            "raw tcp stream not found: {stream_name}"
        );
    }

    pub(crate) async fn when_send_raw_tcp_bytes(&mut self, stream_name: &str, bytes: &[u8]) {
        let stream = self
            .raw_tcp_streams
            .get_mut(stream_name)
            .unwrap_or_else(|| panic!("raw tcp stream not found: {stream_name}"));
        stream.write_all(bytes).await.expect("write raw tcp bytes");
    }

    pub(crate) async fn when_send_packet_from_client(&mut self, client_name: &str, packet: Bytes) {
        self.client_mut(client_name).send_packet(packet).await;
    }

    pub(crate) async fn when_send_packet_from_server(&mut self, server_name: &str, packet: Bytes) {
        self.server_mut(server_name).send_packet(packet).await;
    }

    pub(crate) async fn when_close_client_sink(&mut self, client_name: &str) {
        self.client_mut(client_name).close_sink().await;
    }

    pub(crate) fn when_drop_client(&mut self, client_name: &str) {
        assert!(
            self.clients.remove(client_name).is_some(),
            "client endpoint not found: {client_name}"
        );
    }

    pub(crate) fn when_drop_server(&mut self, server_name: &str) {
        assert!(
            self.servers.remove(server_name).is_some(),
            "server endpoint not found: {server_name}"
        );
    }

    pub(crate) async fn when_shutdown_all(&mut self) {
        self.streams.shutdown_all().await.expect("shutdown all listeners");
    }

    pub(crate) async fn when_try_connect_client(
        &mut self,
        listener_name: &str,
        chip_info: ChipInfo,
    ) {
        self.when_try_connect_to_transport(self.listener_target(listener_name), chip_info).await;
    }

    pub(crate) async fn when_try_connect_to_transport(
        &mut self,
        transport: TransportType,
        chip_info: ChipInfo,
    ) {
        let res = timeout(SHORT_TIMEOUT, self.streams.connect(transport, chip_info)).await;
        self.last_connect_succeeded = Some(matches!(res, Ok(Ok(_))));
    }

    pub(crate) async fn then_client_receives_packet(&mut self, client_name: &str, expected: &[u8]) {
        self.client_mut(client_name).recv_packet(expected).await;
    }

    pub(crate) async fn then_server_receives_packet(&mut self, server_name: &str, expected: &[u8]) {
        self.server_mut(server_name).recv_packet(expected).await;
    }

    pub(crate) async fn then_server_stream_closes(&mut self, server_name: &str) {
        self.server_mut(server_name).assert_closed().await;
    }

    pub(crate) fn then_connection_accepted(&self, server_name: &str) {
        let server = self
            .servers
            .get(server_name)
            .unwrap_or_else(|| panic!("expected accepted server connection for {server_name}"));
        assert_eq!(server.chip_info.as_ref().map(|c| c.name.as_str()), Some(server_name));
    }

    pub(crate) async fn then_no_connection_accepted(&mut self) {
        let res = timeout(SHORT_TIMEOUT, self.streams.accept_any()).await;
        assert!(matches!(res, Err(_) | Ok(Err(_))), "expected no connection accepted");
    }

    pub(crate) fn then_connect_succeeded(&self) {
        assert_eq!(self.last_connect_succeeded, Some(true), "expected connect to succeed");
    }

    pub(crate) fn then_connect_failed(&self) {
        assert_eq!(self.last_connect_succeeded, Some(false), "expected connect to fail");
    }

    pub(crate) fn then_init_info_matches(
        &self,
        server_name: &str,
        expected_device_name: &str,
        expected_mfr: &str,
    ) {
        let server = self
            .servers
            .get(server_name)
            .unwrap_or_else(|| panic!("server endpoint not found: {server_name}"));
        let chip_info = server.chip_info.as_ref().expect("server chip info");
        assert_eq!(
            chip_info.device_info.as_ref().map(|d| d.name.as_str()),
            Some(expected_device_name)
        );
        assert_eq!(chip_info.chip.as_ref().map(|c| c.manufacturer.as_str()), Some(expected_mfr));
    }

    pub(crate) fn then_listener_is_running(&self, name: &str, expected: bool) {
        assert_eq!(self.streams.is_listener_running(name), expected);
    }
}

#[cfg(unix)]
impl Drop for World {
    fn drop(&mut self) {
        for path in &self.uds_paths {
            let _ = std::fs::remove_file(path);
        }
    }
}
