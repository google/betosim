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

use std::{net::SocketAddr, time::Duration};

use http_proxy::{Connector, Error};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::{TcpListener, TcpStream},
    sync::mpsc as tokio_mpsc,
    task::JoinHandle,
    time::timeout,
};

const TEST_TIMEOUT: Duration = Duration::from_secs(5);

/// Behavior mode for the in-memory mock HTTP proxy server.
pub(crate) enum ProxyMode {
    /// Accepts `CONNECT` requests (optionally requiring an exact `Proxy-Authorization` line),
    /// replies with `200 Connection established`, and relays tunnel traffic bidirectionally.
    ConnectTunnel { expected_auth: Option<String> },
    /// Rejects incoming `CONNECT` requests with `407 Proxy Authentication Required`.
    Reject407,
}

/// BDD integration test harness for `http-proxy`.
#[derive(Default)]
pub(crate) struct World {
    tasks: Vec<JoinHandle<()>>,
    proxy_addr: Option<SocketAddr>,
    target_addr: Option<SocketAddr>,
    connector: Option<Connector>,
    client_stream: Option<TcpStream>,
    connect_error: Option<Error>,
    proxy_requests_rx: Option<tokio_mpsc::UnboundedReceiver<String>>,
    target_requests_rx: Option<tokio_mpsc::UnboundedReceiver<Vec<u8>>>,
    last_response: Vec<u8>,
}

impl Drop for World {
    fn drop(&mut self) {
        for task in self.tasks.drain(..) {
            task.abort();
        }
    }
}

impl World {
    /// Creates a new `World` harness.
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Returns the bound mock proxy address.
    fn proxy_addr(&self) -> SocketAddr {
        self.proxy_addr.expect("mock proxy address not initialized")
    }

    /// Returns the bound mock target server address.
    fn target_addr(&self) -> SocketAddr {
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

    /// Configures a `Connector` pointing at the mock proxy with optional credentials.
    pub(crate) fn given_connector(&mut self, username: Option<&str>, password: Option<&str>) {
        let proxy_addr = self.proxy_addr();
        let connector =
            Connector::new(proxy_addr, username.map(String::from), password.map(String::from));
        self.connector = Some(connector);
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

    /// Asserts the proxy received the expected HTTP/1.1 `CONNECT` handshake request.
    async fn then_manager_connect_handshake_succeeded(
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
}
