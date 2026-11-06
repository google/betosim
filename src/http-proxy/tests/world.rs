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

//! BDD `World` test harness for `http-proxy` Tier 2 integration tests.

use std::{
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    sync::mpsc,
    time::Duration,
};

use bytes::Bytes;
use http_proxy::{Connector, DnsManager, Error, Manager};
use netsim_packets::{
    DnsFlags, DnsPacketBuilder, IP_P_TCP, IP_P_UDP, PacketBuilder, ether_type, parse,
};
use slirp::{ProxyConnect, ProxyManager};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::{TcpListener, TcpStream},
    sync::mpsc as tokio_mpsc,
    task::JoinHandle,
    time::timeout,
};

const TEST_TIMEOUT: Duration = Duration::from_secs(5);
const DST_MAC: [u8; 6] = [0x02, 0x00, 0x00, 0x00, 0x00, 0x01];
const SRC_MAC: [u8; 6] = [0x02, 0x00, 0x00, 0x00, 0x00, 0x02];
const DNS_SERVER_IP: [u8; 4] = [8, 8, 8, 8];
const GUEST_IP: [u8; 4] = [10, 0, 2, 15];

/// Behavior mode for the in-memory mock HTTP proxy server.
pub(crate) enum ProxyMode {
    /// Accepts `CONNECT` requests (optionally requiring an exact `Proxy-Authorization` line),
    /// replies with `200 Connection established`, and relays tunnel traffic bidirectionally.
    ConnectTunnel { expected_auth: Option<String> },
    /// Rejects incoming `CONNECT` requests with `407 Proxy Authentication Required`.
    Reject407,
    /// Accepts rewritten HTTP requests (port-80 path) and replies with a static HTTP response.
    HttpUpstream { response: Vec<u8> },
}

/// DNS header QR bit direction for constructing synthetic DNS wire packets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DnsQr {
    Query,
    Response,
}

/// DNS answer record descriptor for constructing synthetic DNS wire packets.
pub(crate) enum DnsAnswerRecord<'a> {
    /// Standard `A` record using an `0xC00C` pointer to the question name.
    A(Ipv4Addr),
    /// Standard `AAAA` record using an `0xC00C` pointer to the question name.
    Aaaa(Ipv6Addr),
    /// Record with a prefix subdomain label followed by an `0xC00C` pointer to the question name.
    PrefixedA { prefix_label: &'a str, ip: Ipv4Addr },
}

struct TestProxyConnect {
    tx: mpsc::Sender<(i32, SocketAddr)>,
}

impl ProxyConnect for TestProxyConnect {
    fn proxy_connect(&self, fd: i32, addr: SocketAddr) {
        let _ = self.tx.send((fd, addr));
    }
}

/// BDD integration test harness for `http-proxy`.
#[derive(Default)]
pub(crate) struct World {
    tasks: Vec<JoinHandle<()>>,
    proxy_addr: Option<SocketAddr>,
    target_addr: Option<SocketAddr>,
    connector: Option<Connector>,
    manager: Option<Manager>,
    _proxy_bytes_tx: Option<mpsc::Sender<Bytes>>,
    dns_manager: DnsManager,
    client_stream: Option<TcpStream>,
    connect_error: Option<Error>,
    manager_error: Option<Error>,
    proxy_requests_rx: Option<tokio_mpsc::UnboundedReceiver<String>>,
    target_requests_rx: Option<tokio_mpsc::UnboundedReceiver<Vec<u8>>>,
    last_response: Vec<u8>,
    callback_rx: Option<mpsc::Receiver<(i32, SocketAddr)>>,
    last_callback: Option<(i32, SocketAddr)>,
    last_try_connect_initiated: Option<bool>,
}

impl Drop for World {
    fn drop(&mut self) {
        for task in self.tasks.drain(..) {
            task.abort();
        }
        if let Some(manager) = self.manager.take() {
            let _ = std::thread::spawn(move || drop(manager)).join();
        }
    }
}

impl World {
    /// Creates a new `World` harness.
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Returns the bound mock proxy address.
    pub(crate) fn proxy_addr(&self) -> SocketAddr {
        self.proxy_addr.expect("mock proxy address not initialized")
    }

    /// Returns the bound mock target server address.
    pub(crate) fn target_addr(&self) -> SocketAddr {
        self.target_addr.expect("mock target address not initialized")
    }

    // --- Given Steps ---

    /// Spawns a mock target TCP server on an ephemeral non-80 port that records
    /// incoming request bytes and replies with `response_payload`.
    async fn given_mock_target_server(&mut self, response_payload: &'static [u8]) {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind mock target listener");
        let addr = listener.local_addr().expect("mock target local_addr");
        assert_ne!(addr.port(), 80, "ephemeral target port must not be 80");
        self.target_addr = Some(addr);

        let (tx, rx) = tokio_mpsc::unbounded_channel();
        self.target_requests_rx = Some(rx);

        let handle = tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener.accept().await {
                let mut buf = vec![0u8; 1024];
                if let Ok(n) = stream.read(&mut buf).await
                    && n > 0
                {
                    buf.truncate(n);
                    let _ = tx.send(buf);
                    let _ = stream.write_all(response_payload).await;
                }
            }
        });
        self.tasks.push(handle);
    }

    /// Spawns a mock HTTP proxy server with the specified [`ProxyMode`].
    pub(crate) async fn given_mock_proxy(&mut self, mode: ProxyMode) {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind mock proxy listener");
        self.proxy_addr = Some(listener.local_addr().expect("mock proxy local_addr"));

        let (req_tx, req_rx) = tokio_mpsc::unbounded_channel();
        self.proxy_requests_rx = Some(req_rx);
        let relay_target = self.target_addr;

        let handle = tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                let mut reader = BufReader::new(stream);
                let mut raw_request = String::new();
                let mut content_length = 0usize;

                loop {
                    let mut line = String::new();
                    let n = reader.read_line(&mut line).await.unwrap_or(0);
                    if n == 0 {
                        break;
                    }
                    if let Some((key, val)) = line.split_once(':')
                        && key.trim().eq_ignore_ascii_case("content-length")
                    {
                        content_length = val.trim().parse().unwrap_or(0);
                    }
                    raw_request.push_str(&line);
                    if line == "\r\n" {
                        break;
                    }
                }

                if raw_request.is_empty() {
                    continue;
                }

                if content_length > 0 {
                    let mut body = vec![0u8; content_length];
                    if reader.read_exact(&mut body).await.is_ok() {
                        raw_request.push_str(&String::from_utf8_lossy(&body));
                    }
                }

                let _ = req_tx.send(raw_request.clone());
                let mut client_stream = reader.into_inner();

                match &mode {
                    ProxyMode::Reject407 => {
                        let _ = client_stream
                            .write_all(b"HTTP/1.1 407 Proxy Authentication Required\r\n\r\n")
                            .await;
                    }
                    ProxyMode::ConnectTunnel { expected_auth } => {
                        let auth_matches = match expected_auth {
                            Some(expected) => raw_request.contains(expected.as_str()),
                            None => true,
                        };
                        if !auth_matches {
                            let _ = client_stream
                                .write_all(b"HTTP/1.1 407 Proxy Authentication Required\r\n\r\n")
                                .await;
                            continue;
                        }
                        let _ = client_stream
                            .write_all(b"HTTP/1.1 200 Connection established\r\n\r\n")
                            .await;
                        if let Some(target_addr) = relay_target
                            && let Ok(mut target_stream) = TcpStream::connect(target_addr).await
                        {
                            let _ = tokio::io::copy_bidirectional(
                                &mut client_stream,
                                &mut target_stream,
                            )
                            .await;
                        }
                    }
                    ProxyMode::HttpUpstream { response } => {
                        let _ = client_stream.write_all(response).await;
                    }
                }
            }
        });
        self.tasks.push(handle);
    }

    /// Spawns a mock target server and an HTTP/1.1 `ConnectTunnel` mock proxy.
    pub(crate) async fn given_connect_tunnel(&mut self, reply: &'static [u8], auth: Option<&str>) {
        self.given_mock_target_server(reply).await;
        self.given_mock_proxy(ProxyMode::ConnectTunnel { expected_auth: auth.map(String::from) })
            .await;
    }

    /// Spawns a mock proxy in `HttpUpstream` mode and configures a `Connector`.
    pub(crate) async fn given_http_upstream_proxy_and_connector(
        &mut self,
        upstream_reply: &[u8],
        username: Option<&str>,
        password: Option<&str>,
    ) {
        self.given_mock_proxy(ProxyMode::HttpUpstream { response: upstream_reply.to_vec() }).await;
        self.given_connector(username, password);
    }

    /// Configures a `Connector` pointing at the mock proxy with optional credentials.
    pub(crate) fn given_connector(&mut self, username: Option<&str>, password: Option<&str>) {
        let proxy_addr = self.proxy_addr();
        let connector =
            Connector::new(proxy_addr, username.map(String::from), password.map(String::from));
        self.connector = Some(connector);
    }

    /// Configures a `Manager` from a proxy configuration string.
    pub(crate) fn given_manager_with_config(&mut self, config_str: &str) {
        if let Some(prev) = self.manager.take() {
            let _ = std::thread::spawn(move || drop(prev)).join();
        }
        let (tx, rx) = mpsc::channel();
        match Manager::new(config_str, rx) {
            Ok(manager) => {
                self.manager = Some(manager);
                self._proxy_bytes_tx = Some(tx);
                self.manager_error = None;
            }
            Err(err) => {
                self.manager_error = Some(err);
            }
        }
    }

    /// Initializes a fresh `DnsManager`.
    pub(crate) fn given_dns_manager(&mut self) {
        self.dns_manager = DnsManager::new();
    }

    // --- When Steps ---

    /// Connects to the mock target server through `Connector::connect` and exchanges `payload`.
    pub(crate) async fn when_connector_tunnels_payload(
        &mut self,
        payload: &[u8],
        expected_len: usize,
    ) {
        let target = self.target_addr();
        self.when_connector_connects_to(target).await;
        self.when_client_sends_and_receives(payload, expected_len).await;
    }

    /// Connects to port 80 (`127.0.0.1:80`) through `Connector::connect` and exchanges `request`.
    pub(crate) async fn when_port_80_request_sent(&mut self, request: &[u8], expected_len: usize) {
        let port_80_target: SocketAddr = "127.0.0.1:80".parse().expect("valid port 80 addr");
        self.when_connector_connects_to(port_80_target).await;
        self.when_client_sends_and_receives(request, expected_len).await;
    }

    /// Connects to an arbitrary `SocketAddr` through `Connector::connect`.
    pub(crate) async fn when_connector_connects_to(&mut self, addr: SocketAddr) {
        let connector = self.connector.as_ref().expect("connector not configured");
        let res = timeout(TEST_TIMEOUT, connector.connect(addr)).await.expect("connect timed out");
        match res {
            Ok(stream) => {
                self.client_stream = Some(stream);
                self.connect_error = None;
            }
            Err(err) => {
                self.client_stream = None;
                self.connect_error = Some(err);
            }
        }
    }

    /// Sends raw payload bytes over the client `TcpStream` and reads `expected_len` bytes.
    async fn when_client_sends_and_receives(&mut self, payload: &[u8], expected_len: usize) {
        let stream = self.client_stream.as_mut().expect("client stream not connected");
        timeout(TEST_TIMEOUT, stream.write_all(payload))
            .await
            .expect("write timed out")
            .expect("write failed");

        let mut buf = vec![0u8; expected_len];
        timeout(TEST_TIMEOUT, stream.read_exact(&mut buf))
            .await
            .expect("read timed out")
            .expect("read_exact failed");
        self.last_response = buf;
    }

    /// Calls `ProxyManager::try_connect` on `Manager` and waits for the `ProxyConnect` callback.
    pub(crate) async fn when_manager_try_connect(
        &mut self,
        target_addr: SocketAddr,
        connect_id: usize,
    ) {
        let manager = self.manager.as_ref().expect("manager not configured");
        let (tx, rx) = mpsc::channel();
        self.callback_rx = Some(rx);
        let initiated =
            manager.try_connect(target_addr, connect_id, Box::new(TestProxyConnect { tx }));
        self.last_try_connect_initiated = Some(initiated);

        let rx_ref = self.callback_rx.take().expect("callback_rx missing");
        let (fd, addr) = tokio::task::spawn_blocking(move || {
            rx_ref.recv_timeout(TEST_TIMEOUT).expect("timed out waiting for ProxyConnect callback")
        })
        .await
        .expect("spawn_blocking failed");

        self.last_callback = Some((fd, addr));
    }

    /// Calls `ProxyManager::remove` on the configured `Manager`.
    pub(crate) fn when_manager_removes_connection(&self, connect_id: usize) {
        let manager = self.manager.as_ref().expect("manager not configured");
        manager.remove(connect_id);
    }

    /// Feeds a synthetic DNS Ethernet frame into `DnsManager::add_from_ethernet_slice`.
    pub(crate) fn when_dns_ethernet_slice_received(
        &self,
        src_port: u16,
        qr: DnsQr,
        qname: &str,
        answers: &[DnsAnswerRecord<'_>],
    ) {
        let frame = build_dns_ethernet_frame(src_port, qr, qname, answers);
        self.dns_manager.add_from_ethernet_slice(&frame);
    }

    /// Parses a synthetic port-53 DNS response frame and feeds `DnsManager::add_from_packet`.
    pub(crate) fn when_dns_parsed_response_packet_received(
        &self,
        qname: &str,
        answer: DnsAnswerRecord<'_>,
    ) {
        let answers = [answer];
        let frame = build_dns_ethernet_frame(53, DnsQr::Response, qname, &answers);
        let packet = parse(&frame).expect("valid Ethernet/IPv4/UDP packet");
        self.dns_manager.add_from_packet(&packet);
    }

    /// Feeds a non-UDP (TCP) Ethernet frame and a truncated slice into `DnsManager`.
    pub(crate) fn when_non_udp_and_truncated_frames_received(&self) {
        let tcp_frame = PacketBuilder::new(DST_MAC, SRC_MAC, ether_type::IPV4)
            .ipv4(DNS_SERVER_IP, GUEST_IP, IP_P_TCP)
            .tcp(53, 49152, 1, 0, 0x02, 65535)
            .payload(b"not-dns");
        self.dns_manager.add_from_ethernet_slice(&tcp_frame);
        let parsed_tcp = parse(&tcp_frame).expect("valid TCP frame");
        self.dns_manager.add_from_packet(&parsed_tcp);
        self.dns_manager.add_from_ethernet_slice(&[0x01, 0x02, 0x03]);
    }

    // --- Then Steps ---

    /// Asserts `Connector::proxy_addr` matches the bound mock proxy address.
    pub(crate) fn then_connector_proxy_addr_matches(&self) {
        let connector = self.connector.as_ref().expect("connector not configured");
        assert_eq!(connector.proxy_addr(), self.proxy_addr());
    }

    /// Asserts the next raw HTTP request captured by the mock proxy equals `expected`.
    async fn then_proxy_received_exact_request(&mut self, expected: &str) {
        let rx = self.proxy_requests_rx.as_mut().expect("proxy_requests_rx not initialized");
        let actual = timeout(TEST_TIMEOUT, rx.recv())
            .await
            .expect("timed out waiting for proxy request")
            .expect("proxy request channel closed");
        assert_eq!(actual, expected);
    }

    /// Asserts the proxy received `expected_rewritten` and the client received `expected_response`.
    pub(crate) async fn then_port_80_exchange_succeeded(
        &mut self,
        expected_rewritten: &str,
        expected_response: &[u8],
    ) {
        self.then_proxy_received_exact_request(expected_rewritten).await;
        self.then_client_received_response(expected_response);
    }

    /// Asserts the proxy received the expected HTTP/1.1 `CONNECT` handshake request.
    pub(crate) async fn then_manager_connect_handshake_succeeded(
        &mut self,
        expected_auth_header: Option<&str>,
    ) {
        let target = self.target_addr();
        let expected_connect = match expected_auth_header {
            Some(auth) => format!("CONNECT {target} HTTP/1.1\r\n{auth}\r\n"),
            None => format!("CONNECT {target} HTTP/1.1\r\n\r\n"),
        };
        self.then_proxy_received_exact_request(&expected_connect).await;
    }

    /// Asserts the proxy received the `CONNECT` request and the tunnel round-trip succeeded.
    pub(crate) async fn then_connect_tunnel_round_trip_succeeded(
        &mut self,
        expected_auth_header: Option<&str>,
        expected_target_payload: &[u8],
        expected_client_response: &[u8],
    ) {
        self.then_manager_connect_handshake_succeeded(expected_auth_header).await;
        self.then_target_received_payload(expected_target_payload).await;
        self.then_client_received_response(expected_client_response);
    }

    /// Asserts the mock target server received `expected` bytes through the tunnel.
    async fn then_target_received_payload(&mut self, expected: &[u8]) {
        let rx = self.target_requests_rx.as_mut().expect("target_requests_rx not initialized");
        let actual = timeout(TEST_TIMEOUT, rx.recv())
            .await
            .expect("timed out waiting for target payload")
            .expect("target request channel closed");
        assert_eq!(actual, expected);
    }

    /// Asserts the client received `expected` response bytes.
    fn then_client_received_response(&self, expected: &[u8]) {
        assert_eq!(self.last_response, expected);
    }

    /// Asserts `Connector::connect` failed with `Error::ConnectionError(expected_addr, status)`.
    pub(crate) fn then_connect_failed_with_connection_error(
        &self,
        expected_addr: SocketAddr,
        expected_status: &str,
    ) {
        let err = self.connect_error.as_ref().expect("expected Connector::connect error");
        match err {
            Error::ConnectionError(addr, status) => {
                assert_eq!(*addr, expected_addr);
                assert_eq!(status, expected_status);
            }
            other => panic!(
                "expected Error::ConnectionError({expected_addr}, {expected_status}), got {other:?}"
            ),
        }
    }

    /// Asserts `ProxyManager::try_connect` delivered a valid `fd >= 0` for `expected_addr`.
    pub(crate) fn then_callback_succeeded_for(&self, expected_addr: SocketAddr) {
        assert_eq!(self.last_try_connect_initiated, Some(true));
        let (fd, addr) = self.last_callback.expect("missing ProxyConnect callback result");
        assert!(fd >= 0, "expected valid raw descriptor fd >= 0, got {fd}");
        assert_eq!(addr, expected_addr);
    }

    /// Asserts `ProxyManager::try_connect` delivered `fd == -1` for `expected_addr`.
    pub(crate) fn then_callback_failed_for(&self, expected_addr: SocketAddr) {
        assert_eq!(self.last_try_connect_initiated, Some(true));
        let (fd, addr) = self.last_callback.expect("missing ProxyConnect callback result");
        assert_eq!(fd, -1, "expected fd == -1 on failed proxy connection, got {fd}");
        assert_eq!(addr, expected_addr);
    }

    /// Asserts `Manager::new` failed with `Error::MalformedConfigString`.
    pub(crate) fn then_manager_failed_with_malformed_config(&self) {
        let err = self.manager_error.as_ref().expect("expected Manager::new error");
        assert!(
            matches!(err, Error::MalformedConfigString),
            "expected MalformedConfigString, got {err:?}"
        );
    }

    /// Asserts `Manager::new` failed with `Error::InvalidPortNumber`.
    pub(crate) fn then_manager_failed_with_invalid_port(&self) {
        let err = self.manager_error.as_ref().expect("expected Manager::new error");
        assert!(
            matches!(err, Error::InvalidPortNumber(_)),
            "expected InvalidPortNumber, got {err:?}"
        );
    }

    /// Asserts `Manager::new` failed with `Error::InvalidHost`.
    pub(crate) fn then_manager_failed_with_invalid_host(&self) {
        let err = self.manager_error.as_ref().expect("expected Manager::new error");
        assert!(matches!(err, Error::InvalidHost(_)), "expected InvalidHost, got {err:?}");
    }

    /// Asserts `DnsManager::len` and `DnsManager::is_empty` match `expected_len`.
    pub(crate) fn then_dns_cache_len_is(&self, expected_len: usize) {
        assert_eq!(self.dns_manager.len(), expected_len);
        assert_eq!(self.dns_manager.is_empty(), expected_len == 0);
    }

    /// Asserts `DnsManager::get(&ip)` returns `expected_fqdn`.
    pub(crate) fn then_dns_lookup_is(&self, ip: IpAddr, expected_fqdn: Option<&str>) {
        assert_eq!(self.dns_manager.get(&ip).as_deref(), expected_fqdn);
    }
}

fn append_dns_rr(payload: &mut Vec<u8>, prefix_label: Option<&str>, rtype: u16, rdata: &[u8]) {
    if let Some(label) = prefix_label {
        payload.push(label.len() as u8);
        payload.extend_from_slice(label.as_bytes());
    }
    // Pointer to question name at offset 12 (0xC00C).
    payload.extend_from_slice(&[0xC0, 0x0C]);
    payload.extend_from_slice(&rtype.to_be_bytes());
    payload.extend_from_slice(&1u16.to_be_bytes()); // CLASS IN
    payload.extend_from_slice(&60u32.to_be_bytes()); // TTL
    payload.extend_from_slice(&(rdata.len() as u16).to_be_bytes());
    payload.extend_from_slice(rdata);
}

fn build_dns_ethernet_frame(
    src_port: u16,
    qr: DnsQr,
    qname: &str,
    answers: &[DnsAnswerRecord<'_>],
) -> Vec<u8> {
    let mut dns_buf = [0u8; 256];
    let mut builder = DnsPacketBuilder::new(&mut dns_buf).expect("DnsPacketBuilder::new");
    builder.transaction_id(0x1234);
    let flags = match qr {
        DnsQr::Response => DnsFlags::RESPONSE | DnsFlags::RECURSION_DESIRED,
        DnsQr::Query => DnsFlags::RECURSION_DESIRED,
    };
    builder.flags(flags);
    builder.add_question(qname, 1, 1).expect("add_question");
    let base_len = builder.build();

    let mut payload = dns_buf[..base_len].to_vec();
    // Set ANCOUNT in DNS header (bytes 6..8).
    let ancount = (answers.len() as u16).to_be_bytes();
    payload[6] = ancount[0];
    payload[7] = ancount[1];

    for answer in answers {
        match answer {
            DnsAnswerRecord::A(ipv4) => append_dns_rr(&mut payload, None, 1, &ipv4.octets()),
            DnsAnswerRecord::Aaaa(ipv6) => append_dns_rr(&mut payload, None, 28, &ipv6.octets()),
            DnsAnswerRecord::PrefixedA { prefix_label, ip } => {
                append_dns_rr(&mut payload, Some(prefix_label), 1, &ip.octets());
            }
        }
    }

    PacketBuilder::new(DST_MAC, SRC_MAC, ether_type::IPV4)
        .ipv4(DNS_SERVER_IP, GUEST_IP, IP_P_UDP)
        .udp(src_port, 54321)
        .payload(&payload)
}
