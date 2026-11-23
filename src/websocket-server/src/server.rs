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

use std::{collections::HashMap, future::Future, net::SocketAddr, str::FromStr, sync::Arc};

use bytes::BytesMut;
use device_actor::DeviceClient;
use http::{Response, StatusCode, header::CONNECTION};
use netsim_model::ChipKind;
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite},
    net::{TcpListener, TcpStream},
};
use tracing::{debug, error, info, warn};

use crate::{
    err::Code,
    error::ServerError,
    handshake::{handle_websocket_handshake, handshake_error_response, write_http_response},
    transport::{RegisteredDevice, run_loopback, run_websocket_transport, setup_virtual_chip},
};

const WEBSOCKET_PREFIX: &str = "/v1/websocket/";
const MAX_HTTP_BUFFER_SIZE: usize = 2048;
const CHANNEL_CAPACITY: usize = 100;

type HandshakeSuccess = (
    tungstenite::WebSocket<std::net::TcpStream>,
    tungstenite::WebSocket<std::net::TcpStream>,
    ChipKind,
    HashMap<String, String>,
);

#[derive(Debug)]
enum HandshakeFailure {
    EarlyDisconnect,
    InvalidUpgrade(ServerError),
    StreamIntoStd(std::io::Error),
    SetStreamBlocking(std::io::Error),
    StreamClone(std::io::Error),
    Internal(ServerError),
    Transport(ServerError),
}

pub async fn run(listener: TcpListener, device_client: DeviceClient) {
    let port = listener.local_addr().map(|a| a.port()).unwrap_or(0);
    run_with_accept(port, device_client, || async { Some(listener.accept().await) }).await;
}

async fn run_with_accept<F, Fut>(port: u16, device_client: DeviceClient, mut accept: F)
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Option<std::io::Result<(TcpStream, SocketAddr)>>>,
{
    info!("WebSocket server is listening on: {port}");

    while let Some(accepted) = accept().await {
        match accepted {
            Ok((stream, addr)) => {
                info!("WebSocket client address: {addr}");
                let device_client = device_client.clone();
                tokio::spawn(async move {
                    handle_websocket_client(stream, addr, device_client).await;
                });
            }
            Err(error) => warn!(
                code = Code::ListenerAcceptFailed as u32,
                %error,
                "Error accepting WebSocket connection"
            ),
        }
    }
}

fn log_handshake_failure(addr: SocketAddr, failure: HandshakeFailure) {
    match failure {
        HandshakeFailure::EarlyDisconnect => {
            debug!("WebSocket client {addr} disconnected before completing handshake");
        }
        HandshakeFailure::InvalidUpgrade(error) => warn!(
            code = Code::InvalidUpgradeRequest as u32,
            %error,
            "Invalid WebSocket upgrade request from {addr}"
        ),
        HandshakeFailure::StreamIntoStd(error) => error!(
            code = Code::StreamIntoStdFailed as u32,
            %error,
            "WebSocket OS stream into_std failed for {addr}"
        ),
        HandshakeFailure::SetStreamBlocking(error) => error!(
            code = Code::SetStreamBlockingFailed as u32,
            %error,
            "WebSocket OS stream set_nonblocking failed for {addr}"
        ),
        HandshakeFailure::StreamClone(error) => error!(
            code = Code::StreamCloneFailed as u32,
            %error,
            "WebSocket OS stream try_clone failed for {addr}"
        ),
        HandshakeFailure::Internal(error) => error!(
            code = Code::HandshakeResponseBuildFailed as u32,
            %error,
            "WebSocket internal handshake response error for {addr}"
        ),
        HandshakeFailure::Transport(error) => warn!(
            code = Code::HandshakeFailed as u32,
            %error,
            "WebSocket handshake failed for {addr}"
        ),
    }
}

async fn handle_websocket_client(stream: TcpStream, addr: SocketAddr, device_client: DeviceClient) {
    // 1. Perform the WebSocket Handshake (RFC 6455, Section 1.3)
    let (websocket_reader, websocket_writer, chip_kind, queries) =
        match perform_handshake(stream).await {
            Ok(res) => res,
            Err(failure) => {
                log_handshake_failure(addr, failure);
                return;
            }
        };

    let (stream_tx, stream_rx) =
        tokio::sync::mpsc::channel::<Result<bytes::Bytes, std::io::Error>>(CHANNEL_CAPACITY);
    let (sink_tx, sink_rx) = tokio::sync::mpsc::channel::<bytes::Bytes>(CHANNEL_CAPACITY);

    // 2. Register the virtual chip with the device actor, or echo packets
    //    straight back for a loopback stream (no device is created).
    let mut loopback_task = None;
    let device = if is_loopback(&queries) {
        info!("WebSocket loopback stream for {addr} ({chip_kind:?})");
        loopback_task = Some(tokio::spawn(run_loopback(addr, stream_rx, sink_tx)));
        None
    } else {
        match setup_virtual_chip(&device_client, chip_kind, queries, addr, stream_rx, sink_tx).await
        {
            Ok((device_id, chip_id)) => {
                Some(Arc::new(RegisteredDevice { device_id, chip_id, client: device_client }))
            }
            Err(ServerError::DeviceActor(ref error))
                if crate::transport::is_daemon_shutdown_client_error(&device_client, error) =>
            {
                debug!(%error, "Skipping virtual chip setup during shutdown for {addr}");
                return;
            }
            Err(error) => {
                warn!(
                    code = Code::SetupVirtualChipFailed as u32,
                    %error,
                    "Failed to setup virtual chip for {addr}"
                );
                return;
            }
        }
    };

    // 3. Start the WebSocket transport loops (Read/Write/Forward)
    run_websocket_transport(
        websocket_reader,
        websocket_writer,
        addr,
        chip_kind,
        stream_tx,
        sink_rx,
        device,
    )
    .await;
    if let Some(task) = loopback_task {
        task.abort();
    }
}

/// True when the client asked for a loopback stream (`?loopback=1`).
fn is_loopback(queries: &HashMap<String, String>) -> bool {
    matches!(queries.get("loopback").map(String::as_str), Some("1" | "true"))
}

/// Resolves the target `ChipKind` from `/v1/websocket/<kind>`.
fn resolve_chip_kind(base_path: &str) -> Option<ChipKind> {
    let kind_str = base_path.strip_prefix(WEBSOCKET_PREFIX)?;
    match ChipKind::from_str(kind_str) {
        Ok(kind) if kind != ChipKind::UNSPECIFIED => Some(kind),
        _ => None,
    }
}

async fn write_bad_request_close<S: AsyncWrite + Unpin>(
    stream: &mut S,
) -> Result<(), HandshakeFailure> {
    let response = Response::builder()
        .status(StatusCode::BAD_REQUEST)
        .header(CONNECTION, "close")
        .body(Vec::new())
        .map_err(|e| HandshakeFailure::Internal(e.into()))?;
    let _ = write_http_response(stream, response).await;
    Ok(())
}

/// Performs the initial HTTP handshake and upgrades to WebSocket.
///
/// Ref: RFC 6455, Section 1.3 - Opening Handshake
async fn perform_handshake(mut stream: TcpStream) -> Result<HandshakeSuccess, HandshakeFailure> {
    let (leftover, chip_kind, queries) = perform_handshake_io(&mut stream).await?;
    finish_handshake(stream, leftover, chip_kind, queries)
}

fn finish_handshake(
    stream: TcpStream,
    leftover: Vec<u8>,
    chip_kind: ChipKind,
    queries: HashMap<String, String>,
) -> Result<HandshakeSuccess, HandshakeFailure> {
    // Transition to standard blocking stream for tungstenite
    let std_stream = stream.into_std().map_err(HandshakeFailure::StreamIntoStd)?;
    std_stream.set_nonblocking(false).map_err(HandshakeFailure::SetStreamBlocking)?;

    // Initialize WebSocket readers and writers
    let websocket_writer = tungstenite::WebSocket::from_raw_socket(
        std_stream.try_clone().map_err(HandshakeFailure::StreamClone)?,
        tungstenite::protocol::Role::Server,
        None,
    );
    let websocket_reader = tungstenite::WebSocket::from_partially_read(
        std_stream,
        leftover,
        tungstenite::protocol::Role::Server,
        None,
    );

    Ok((websocket_reader, websocket_writer, chip_kind, queries))
}

async fn perform_handshake_io<S>(
    stream: &mut S,
) -> Result<(Vec<u8>, ChipKind, HashMap<String, String>), HandshakeFailure>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let mut buffer = BytesMut::with_capacity(MAX_HTTP_BUFFER_SIZE);
    // Wait until we have a full HTTP request or hit MAX_HTTP_BUFFER_SIZE
    let header_len = loop {
        let n = stream.read_buf(&mut buffer).await.map_err(|e| {
            if crate::transport::is_expected_io_close(&e) {
                HandshakeFailure::EarlyDisconnect
            } else {
                HandshakeFailure::Transport(e.into())
            }
        })?;
        if n == 0 {
            return Err(HandshakeFailure::EarlyDisconnect);
        }

        let mut headers = [httparse::EMPTY_HEADER; 64];
        let mut req = httparse::Request::new(&mut headers);
        match req.parse(&buffer) {
            Ok(httparse::Status::Complete(header_len)) => {
                break header_len;
            }
            Err(err) => {
                write_bad_request_close(stream).await?;
                return Err(HandshakeFailure::InvalidUpgrade(err.into()));
            }
            Ok(httparse::Status::Partial) if buffer.len() >= MAX_HTTP_BUFFER_SIZE => {
                write_bad_request_close(stream).await?;
                return Err(HandshakeFailure::InvalidUpgrade(ServerError::HandshakeFailed(
                    "Request too large".to_string(),
                )));
            }
            Ok(httparse::Status::Partial) => {}
        }
    };

    let mut headers = [httparse::EMPTY_HEADER; 64];
    let mut req = httparse::Request::new(&mut headers);
    req.parse(&buffer).expect("validated parse is complete");

    // 1. Path validation and query extraction
    let path_str = req.path.ok_or_else(|| {
        HandshakeFailure::InvalidUpgrade(ServerError::HandshakeFailed(
            "Missing HTTP request path".to_string(),
        ))
    })?;

    let (base_path, query_string_opt) = match path_str.split_once('?') {
        Some((path, query)) => (path, Some(query)),
        None => (path_str, None),
    };

    let queries: HashMap<String, String> = query_string_opt
        .map(|query| form_urlencoded::parse(query.as_bytes()).into_owned().collect())
        .unwrap_or_default();

    let Some(chip_kind) = resolve_chip_kind(base_path) else {
        let response = Response::builder()
            .status(StatusCode::NOT_FOUND)
            .header(CONNECTION, "close")
            .body(Vec::new())
            .map_err(|e| HandshakeFailure::Internal(e.into()))?;
        let _ = write_http_response(stream, response).await;
        return Err(HandshakeFailure::InvalidUpgrade(ServerError::InvalidPath(
            path_str.to_string(),
        )));
    };

    let response = match handle_websocket_handshake(&req) {
        Ok(response) => response,
        Err(ServerError::Handshake(err)) => {
            if let Ok(error_response) = handshake_error_response(&err) {
                let _ = write_http_response(stream, error_response).await;
            }
            return Err(HandshakeFailure::InvalidUpgrade(ServerError::Handshake(err)));
        }
        Err(err) => return Err(HandshakeFailure::Internal(err)),
    };

    // Write the upgrade response
    write_http_response(stream, response).await.map_err(|err| match &err {
        ServerError::Io(io_err) if crate::transport::is_expected_io_close(io_err) => {
            HandshakeFailure::EarlyDisconnect
        }
        _ => HandshakeFailure::Transport(err),
    })?;

    let leftover = buffer.split_off(header_len).to_vec();
    Ok((leftover, chip_kind, queries))
}

#[cfg(test)]
mod tests {
    use std::{
        io::{self, ErrorKind},
        pin::Pin,
        task::{Context, Poll},
        time::Duration,
    };

    use actor_framework::{FrameworkError, MockActorClient};
    use device_actor::{DeviceActor, DeviceError};
    use tokio::io::AsyncWriteExt;

    use super::*;
    use crate::err::{Code, EXPECTED_IO_CLOSE_KINDS, assert_error_stats_async};

    const HANDSHAKE_TEST_TIMEOUT: Duration = Duration::from_secs(2);
    const SOCKET_READ_TIMEOUT: Duration = Duration::from_secs(2);
    const RESPONSE_READ_CHUNK_SIZE: usize = 1024;
    const HTTP_HEADER_END: &[u8] = b"\r\n\r\n";
    const PREMATURE_EOF_REQUEST: &[u8] = b"GET /v1/websocket/bt HTTP/1.1\r\nHost: localhost";
    const MALFORMED_HTTP_REQUEST: &[u8] = b"NOT_HTTP_AT_ALL\r\n\r\n";
    const UNKNOWN_KIND_REQUEST: &[u8] =
        b"GET /v1/websocket/invalid HTTP/1.1\r\nHost: localhost\r\n\r\n";
    const MISSING_KEY_REQUEST: &[u8] = b"GET /v1/websocket/bt HTTP/1.1\r\nHost: localhost\r\n\
        Upgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Version: 13\r\n\r\n";
    const PIPELINED_REQUEST_HEAD: &[u8] = concat!(
        "GET /v1/websocket/bt HTTP/1.1\r\n",
        "Host: localhost\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n",
        "Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n",
        "Sec-WebSocket-Version: 13\r\n\r\n",
    )
    .as_bytes();
    const PIPELINED_PAYLOAD: &[u8] = &[0xAA, 0xBB, 0xCC, 0xDD];
    // Masked RFC 6455 binary frame for PIPELINED_PAYLOAD using mask [0x11, 0x22, 0x33, 0x44].
    const PIPELINED_MASKED_FRAME: &[u8] =
        &[0x82, 0x84, 0x11, 0x22, 0x33, 0x44, 0xBB, 0x99, 0xFF, 0x99];
    const OVERSIZED_HEADER_BYTES: usize = MAX_HTTP_BUFFER_SIZE + 1024;
    const VALID_UPGRADE_HEADERS: &[u8] = concat!(
        "Host: localhost\r\n",
        "Upgrade: websocket\r\n",
        "Connection: Upgrade\r\n",
        "Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n",
        "Sec-WebSocket-Version: 13\r\n\r\n",
    )
    .as_bytes();

    async fn connected_stream_pair() -> (TcpStream, TcpStream, SocketAddr) {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind loopback listener");
        let addr = listener.local_addr().expect("listener local addr");
        let (client_res, accept_res) = tokio::join!(TcpStream::connect(addr), listener.accept());
        let client = client_res.expect("connect test client");
        let (server_stream, client_addr) = accept_res.expect("accept server stream");
        (client, server_stream, client_addr)
    }

    async fn run_handshake_with_bytes(
        request_bytes: &[u8],
    ) -> (Result<HandshakeSuccess, HandshakeFailure>, Vec<u8>) {
        let (result, resp_bytes, _saw_reset) =
            run_handshake_with_options(request_bytes, true).await;
        (result, resp_bytes)
    }

    async fn run_handshake_with_options(
        request_bytes: &[u8],
        close_write_half: bool,
    ) -> (Result<HandshakeSuccess, HandshakeFailure>, Vec<u8>, bool) {
        let (mut client, server_stream, _) = connected_stream_pair().await;

        client.write_all(request_bytes).await.expect("write request bytes");
        if close_write_half {
            client.shutdown().await.expect("shutdown client write half");
        }

        let handshake_fut = async {
            tokio::time::timeout(HANDSHAKE_TEST_TIMEOUT, perform_handshake(server_stream))
                .await
                .expect("timed out waiting for perform_handshake")
        };

        let read_fut = async {
            let mut resp_bytes = Vec::new();
            let mut saw_reset = false;
            tokio::time::timeout(HANDSHAKE_TEST_TIMEOUT, async {
                let mut chunk = [0u8; RESPONSE_READ_CHUNK_SIZE];
                while !resp_bytes.windows(HTTP_HEADER_END.len()).any(|w| w == HTTP_HEADER_END) {
                    match client.read(&mut chunk).await {
                        Ok(0) => break,
                        Ok(n) => resp_bytes.extend_from_slice(&chunk[..n]),
                        Err(err) if err.kind() == std::io::ErrorKind::ConnectionReset => {
                            saw_reset = true;
                            break;
                        }
                        Err(err) => panic!("read handshake response: {err}"),
                    }
                }
            })
            .await
            .expect("timed out reading handshake response");
            (resp_bytes, saw_reset)
        };

        let (result, (resp_bytes, saw_reset)) = tokio::join!(handshake_fut, read_fut);
        (result, resp_bytes, saw_reset)
    }

    #[test]
    fn test_is_loopback_accepts_only_one_and_true() {
        assert!(!is_loopback(&HashMap::new()));
        for (value, expected) in
            [("1", true), ("true", true), ("0", false), ("false", false), ("TRUE", false)]
        {
            let queries = HashMap::from([("loopback".to_string(), value.to_string())]);
            assert_eq!(is_loopback(&queries), expected, "unexpected result for loopback={value}");
        }
    }

    #[test]
    fn test_resolve_chip_kind_valid_unspecified_and_invalid_paths() {
        for (path, expected) in [
            ("/v1/websocket/bt", Some(ChipKind::BLUETOOTH)),
            ("/v1/websocket/wifi", Some(ChipKind::WIFI)),
            ("/v1/websocket/unspecified", None),
            ("/other/bt", None),
            ("/v1/websocket/unknown", None),
        ] {
            assert_eq!(resolve_chip_kind(path), expected, "unexpected result for path={path}");
        }
    }

    #[tokio::test]
    async fn test_perform_handshake_premature_eof_returns_early_disconnect() {
        for partial_bytes in [b"".as_slice(), PREMATURE_EOF_REQUEST] {
            let (result, resp_bytes) = run_handshake_with_bytes(partial_bytes).await;
            assert!(
                matches!(result, Err(HandshakeFailure::EarlyDisconnect)),
                "expected EarlyDisconnect on premature EOF for input={partial_bytes:?}, got: {:?}",
                result.err()
            );
            assert!(
                resp_bytes.is_empty(),
                "expected empty response on premature EOF for input={partial_bytes:?}"
            );
        }
    }

    async fn assert_client_error_stats(request: &[u8], dc: DeviceClient, expected: Option<Code>) {
        let (mut client, stream, addr) = connected_stream_pair().await;
        client.write_all(request).await.unwrap();
        client.shutdown().await.unwrap();
        let fut =
            assert_error_stats_async(Box::pin(handle_websocket_client(stream, addr, dc)), expected);
        tokio::time::timeout(HANDSHAKE_TEST_TIMEOUT, fut).await.unwrap();
    }

    struct TestUpgradeStream {
        read_bytes: Option<&'static [u8]>,
        read_err: Option<ErrorKind>,
        write_err: Option<ErrorKind>,
    }
    impl TestUpgradeStream {
        fn with_read_err(bytes: &'static [u8], err: ErrorKind) -> Self {
            Self { read_bytes: Some(bytes), read_err: Some(err), write_err: None }
        }
        fn with_write_err(bytes: &'static [u8], err: ErrorKind) -> Self {
            Self { read_bytes: Some(bytes), read_err: None, write_err: Some(err) }
        }
    }
    impl AsyncRead for TestUpgradeStream {
        fn poll_read(
            mut self: Pin<&mut Self>,
            _: &mut Context<'_>,
            buf: &mut tokio::io::ReadBuf<'_>,
        ) -> Poll<io::Result<()>> {
            if let Some(chunk) = self.read_bytes.take() {
                buf.put_slice(chunk);
                return Poll::Ready(Ok(()));
            }
            Poll::Ready(self.read_err.map_or(Ok(()), |k| Err(io::Error::from(k))))
        }
    }
    impl AsyncWrite for TestUpgradeStream {
        fn poll_write(
            self: Pin<&mut Self>,
            _: &mut Context<'_>,
            d: &[u8],
        ) -> Poll<io::Result<usize>> {
            Poll::Ready(self.write_err.map_or(Ok(d.len()), |k| Err(io::Error::from(k))))
        }
        fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Ready(Ok(()))
        }
        fn poll_shutdown(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Ready(Ok(()))
        }
    }

    #[tokio::test]
    async fn test_perform_handshake_mid_handshake_disconnect_returns_early_disconnect() {
        for kind in EXPECTED_IO_CLOSE_KINDS {
            let mut s = TestUpgradeStream::with_read_err(PREMATURE_EOF_REQUEST, kind);
            assert!(matches!(
                perform_handshake_io(&mut s).await,
                Err(HandshakeFailure::EarlyDisconnect)
            ));
        }
    }

    #[tokio::test]
    async fn test_perform_handshake_io_unexpected_read_error_returns_transport() {
        let mut s = TestUpgradeStream::with_read_err(PREMATURE_EOF_REQUEST, ErrorKind::Other);
        assert!(matches!(perform_handshake_io(&mut s).await, Err(HandshakeFailure::Transport(_))));
    }

    #[tokio::test]
    async fn test_perform_handshake_malformed_http_writes_400_and_returns_http_parse() {
        let (result, resp_bytes) = run_handshake_with_bytes(MALFORMED_HTTP_REQUEST).await;
        assert!(
            matches!(result, Err(HandshakeFailure::InvalidUpgrade(ServerError::HttpParse(_)))),
            "expected InvalidUpgrade(HttpParse) on malformed HTTP, got: {:?}",
            result.err()
        );
        let resp_text = std::str::from_utf8(&resp_bytes).expect("valid UTF-8 400 response");
        assert!(
            resp_text.starts_with("HTTP/1.1 400 Bad Request\r\n"),
            "expected HTTP 400 status line, got: {resp_text:?}"
        );
        assert!(
            resp_text.to_ascii_lowercase().contains("connection: close\r\n"),
            "expected connection: close header, got: {resp_text:?}"
        );
    }

    #[tokio::test]
    async fn test_perform_handshake_preserves_pipelined_binary_frame() {
        let mut request_bytes =
            Vec::with_capacity(PIPELINED_REQUEST_HEAD.len() + PIPELINED_MASKED_FRAME.len());
        request_bytes.extend_from_slice(PIPELINED_REQUEST_HEAD);
        request_bytes.extend_from_slice(PIPELINED_MASKED_FRAME);

        let (result, _resp_bytes) = run_handshake_with_bytes(&request_bytes).await;
        let (mut websocket_reader, _websocket_writer, _chip_kind, _queries) =
            result.expect("valid upgrade with pipelined frame should succeed");

        websocket_reader
            .get_ref()
            .set_read_timeout(Some(SOCKET_READ_TIMEOUT))
            .expect("set socket read timeout");
        let msg = tokio::task::spawn_blocking(move || websocket_reader.read())
            .await
            .expect("reader task panicked")
            .expect("read pipelined binary frame");
        let tungstenite::Message::Binary(data) = msg else {
            panic!("expected pipelined Binary frame, got: {msg:?}");
        };
        assert_eq!(data.as_ref(), PIPELINED_PAYLOAD);
    }

    fn assert_request_too_large(result: Result<HandshakeSuccess, HandshakeFailure>) {
        assert!(
            matches!(
                result,
                Err(HandshakeFailure::InvalidUpgrade(ServerError::HandshakeFailed(ref msg)))
                    if msg == "Request too large"
            ),
            "expected InvalidUpgrade(HandshakeFailed(\"Request too large\")), got: {:?}",
            result.err()
        );
    }

    fn assert_oversized_400_response(resp_bytes: &[u8], saw_reset: bool) {
        // Closing the server socket with unread request bytes triggers TCP RST, which on
        // non-Linux stacks can purge the already-sent 400 response before the client reads it.
        if cfg!(target_os = "linux") || !saw_reset || !resp_bytes.is_empty() {
            assert!(
                resp_bytes.starts_with(b"HTTP/1.1 400"),
                "expected HTTP 400 response, got: {:?}",
                String::from_utf8_lossy(resp_bytes)
            );
        }
    }

    #[tokio::test]
    async fn test_perform_handshake_oversized_partial_writes_400_and_returns_request_too_large() {
        const PREFIX: &[u8] = b"GET /v1/websocket/bt HTTP/1.1\r\nHost: localhost\r\nX-Pad: ";
        let mut exact_max_partial = Vec::from(PREFIX);
        exact_max_partial.resize(MAX_HTTP_BUFFER_SIZE, b'a');
        assert_eq!(exact_max_partial.len(), MAX_HTTP_BUFFER_SIZE);

        let (result, resp_bytes, _saw_reset) =
            run_handshake_with_options(&exact_max_partial, false).await;
        assert_request_too_large(result);
        assert!(
            resp_bytes.starts_with(b"HTTP/1.1 400"),
            "expected HTTP 400 response, got: {:?}",
            String::from_utf8_lossy(&resp_bytes)
        );

        let mut over_max_partial = Vec::from(PREFIX);
        over_max_partial.resize(OVERSIZED_HEADER_BYTES, b'a');

        let (result, resp_bytes, saw_reset) =
            run_handshake_with_options(&over_max_partial, false).await;
        assert_request_too_large(result);
        assert_oversized_400_response(&resp_bytes, saw_reset);
    }

    #[tokio::test]
    async fn test_perform_handshake_oversized_header_with_valid_tail_writes_400() {
        let mut request_bytes = Vec::from(b"GET /v1/websocket/bt HTTP/1.1\r\nX-Pad: ".as_slice());
        request_bytes.resize(OVERSIZED_HEADER_BYTES, b'a');
        request_bytes.extend_from_slice(b"\r\n");
        request_bytes.extend_from_slice(VALID_UPGRADE_HEADERS);

        let (result, resp_bytes, saw_reset) =
            run_handshake_with_options(&request_bytes, false).await;
        assert_request_too_large(result);
        assert_oversized_400_response(&resp_bytes, saw_reset);
    }

    #[tokio::test]
    async fn test_perform_handshake_exact_max_buffer_size_succeeds() {
        const PREFIX: &[u8] = b"GET /v1/websocket/bt HTTP/1.1\r\nX-Pad: ";
        const CRLF: &[u8] = b"\r\n";
        let pad_len =
            MAX_HTTP_BUFFER_SIZE - PREFIX.len() - CRLF.len() - VALID_UPGRADE_HEADERS.len();
        let mut request_bytes = Vec::with_capacity(MAX_HTTP_BUFFER_SIZE);
        request_bytes.extend_from_slice(PREFIX);
        request_bytes.resize(request_bytes.len() + pad_len, b'a');
        request_bytes.extend_from_slice(CRLF);
        request_bytes.extend_from_slice(VALID_UPGRADE_HEADERS);
        assert_eq!(request_bytes.len(), MAX_HTTP_BUFFER_SIZE);

        let (result, resp_bytes, _saw_reset) =
            run_handshake_with_options(&request_bytes, false).await;
        assert!(
            result.is_ok(),
            "expected exact MAX_HTTP_BUFFER_SIZE handshake to succeed, got: {:?}",
            result.err()
        );
        assert!(
            resp_bytes.starts_with(b"HTTP/1.1 101"),
            "expected HTTP 101 response, got: {:?}",
            String::from_utf8_lossy(&resp_bytes)
        );
    }

    #[tokio::test]
    async fn test_handle_websocket_client_invalid_upgrade_stats() {
        for req in [MALFORMED_HTTP_REQUEST, UNKNOWN_KIND_REQUEST, MISSING_KEY_REQUEST] {
            let (_runner, dc) = device_actor::new();
            assert_client_error_stats(req, dc, Some(Code::InvalidUpgradeRequest)).await;
        }
    }

    #[tokio::test]
    async fn test_perform_handshake_io_write_error_stats() {
        let addr: SocketAddr = "127.0.0.1:12345".parse().unwrap();
        let cases = std::iter::once((ErrorKind::Other, Some(Code::HandshakeFailed)))
            .chain(EXPECTED_IO_CLOSE_KINDS.into_iter().map(|k| (k, None)));
        for (write_err, expected) in cases {
            let fut = async move {
                let mut s = TestUpgradeStream::with_write_err(PIPELINED_REQUEST_HEAD, write_err);
                log_handshake_failure(addr, perform_handshake_io(&mut s).await.unwrap_err());
            };
            assert_error_stats_async(fut, expected).await;
        }
    }

    #[tokio::test]
    async fn test_log_handshake_failure_records_error_stats() {
        use Code::*;
        use HandshakeFailure::*;
        let addr: SocketAddr = "127.0.0.1:12345".parse().unwrap();
        let build_err = Internal(ServerError::HandshakeFailed("b".into()));
        for (failure, expected) in [
            (EarlyDisconnect, None),
            (StreamIntoStd(io::Error::other("into")), Some(StreamIntoStdFailed)),
            (SetStreamBlocking(io::Error::other("blk")), Some(SetStreamBlockingFailed)),
            (StreamClone(io::Error::other("clone")), Some(StreamCloneFailed)),
            (build_err, Some(HandshakeResponseBuildFailed)),
        ] {
            assert_error_stats_async(async move { log_handshake_failure(addr, failure) }, expected)
                .await;
        }
    }

    async fn assert_chip_err(shut_down: bool, is_dev: bool, expected: Option<Code>) {
        let mut mock = MockActorClient::<DeviceActor>::new();
        mock.expect_shutdown().returning(|| Ok(()));
        mock.expect_perform_action().returning(move |_, _| {
            Err(if is_dev {
                FrameworkError::ServiceError(DeviceError::ChipKindNotSupported("bt".into()))
            } else {
                FrameworkError::ActorClosed(Box::new(io::Error::other("closed")))
            })
        });
        let dc = DeviceClient::new(Box::new(mock));
        if shut_down {
            dc.shutdown().await.unwrap();
        }
        assert_client_error_stats(PIPELINED_REQUEST_HEAD, dc, expected).await;
    }

    #[tokio::test]
    async fn test_handle_websocket_client_setup_virtual_chip_failed_records_error_stat() {
        assert_chip_err(false, true, Some(Code::SetupVirtualChipFailed)).await;
    }

    #[tokio::test]
    async fn test_handle_websocket_client_shutdown_records_no_error_stat() {
        assert_chip_err(true, false, None).await;
    }

    #[tokio::test]
    async fn test_run_with_accept_records_listener_accept_failed() {
        let (_runner, dc) = device_actor::new();
        let mut emitted = false;
        let accept = || {
            let next = (!std::mem::replace(&mut emitted, true)).then(|| Err(io::Error::other("a")));
            std::future::ready(next)
        };
        let fut = assert_error_stats_async(
            run_with_accept(0, dc, accept),
            Some(Code::ListenerAcceptFailed),
        );
        tokio::time::timeout(HANDSHAKE_TEST_TIMEOUT, fut).await.unwrap();
    }
}
