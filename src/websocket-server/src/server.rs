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

use std::{collections::HashMap, net::SocketAddr, str::FromStr, sync::Arc};

use bytes::BytesMut;
use device_actor::DeviceClient;
use http::{Response, StatusCode, header::CONNECTION};
use netsim_model::ChipKind;
use tokio::{
    io::AsyncReadExt,
    net::{TcpListener, TcpStream},
};
use tracing::{error, info, warn};

use crate::{
    error::ServerError,
    handshake::{handle_websocket_handshake, handshake_error_response, write_http_response},
    transport::{RegisteredDevice, run_loopback, run_websocket_transport, setup_virtual_chip},
};

const WEBSOCKET_PREFIX: &str = "/v1/websocket/";
const MAX_HTTP_BUFFER_SIZE: usize = 2048;
const CHANNEL_CAPACITY: usize = 100;

pub async fn run(listener: TcpListener, device_client: DeviceClient) {
    let port = listener.local_addr().map(|a| a.port()).unwrap_or(0);
    info!("WebSocket server is listening on: {port}");

    loop {
        match listener.accept().await {
            Ok((stream, addr)) => {
                info!("WebSocket client address: {addr}");
                let device_client = device_client.clone();
                tokio::spawn(async move {
                    handle_websocket_client(stream, addr, device_client).await;
                });
            }
            Err(error) => {
                warn!(%error, "Error accepting WebSocket connection");
            }
        }
    }
}

async fn handle_websocket_client(stream: TcpStream, addr: SocketAddr, device_client: DeviceClient) {
    // 1. Perform the WebSocket Handshake (RFC 6455, Section 1.3)
    let (websocket_reader, websocket_writer, chip_kind, queries) =
        match perform_handshake(stream).await {
            Ok(res) => res,
            Err(error) => {
                warn!(%error, "WebSocket handshake failed for {addr}");
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
            Err(error) => {
                warn!(%error, "Failed to setup virtual chip for {addr}");
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

/// Performs the initial HTTP handshake and upgrades to WebSocket.
///
/// Ref: RFC 6455, Section 1.3 - Opening Handshake
async fn perform_handshake(
    mut stream: TcpStream,
) -> Result<
    (
        tungstenite::WebSocket<std::net::TcpStream>,
        tungstenite::WebSocket<std::net::TcpStream>,
        ChipKind,
        HashMap<String, String>,
    ),
    ServerError,
> {
    let mut buffer = BytesMut::with_capacity(MAX_HTTP_BUFFER_SIZE);
    // Wait until we have a full HTTP request or hit MAX_HTTP_BUFFER_SIZE
    let header_len = loop {
        let n = stream.read_buf(&mut buffer).await?;
        if n == 0 {
            if buffer.len() == buffer.capacity() {
                let response = Response::builder()
                    .status(StatusCode::BAD_REQUEST)
                    .header(CONNECTION, "close")
                    .body(Vec::new())?;
                let _ = write_http_response(&mut stream, response).await;
                return Err(ServerError::HandshakeFailed("Request too large".to_string()));
            } else {
                return Err(ServerError::HandshakeFailed(
                    "Client disconnected before sending full request".to_string(),
                ));
            }
        }

        let mut headers = [httparse::EMPTY_HEADER; 64];
        let mut req = httparse::Request::new(&mut headers);
        match req.parse(&buffer) {
            Ok(httparse::Status::Complete(header_len)) => {
                break header_len;
            }
            Err(err) => {
                let response = Response::builder()
                    .status(StatusCode::BAD_REQUEST)
                    .header(CONNECTION, "close")
                    .body(Vec::new())?;
                let _ = write_http_response(&mut stream, response).await;
                return Err(err.into());
            }
            Ok(httparse::Status::Partial) => {}
        }
    };

    let mut headers = [httparse::EMPTY_HEADER; 64];
    let mut req = httparse::Request::new(&mut headers);
    req.parse(&buffer).expect("validated parse is complete");

    // 1. Path validation and query extraction
    let path_str = req
        .path
        .ok_or_else(|| ServerError::HandshakeFailed("Missing HTTP request path".to_string()))?;

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
            .body(Vec::new())?;
        let _ = write_http_response(&mut stream, response).await;
        return Err(ServerError::InvalidPath(path_str.to_string()));
    };

    let response = match handle_websocket_handshake(&req) {
        Ok(response) => response,
        Err(ServerError::Handshake(err)) => {
            if let Ok(error_response) = handshake_error_response(&err) {
                let _ = write_http_response(&mut stream, error_response).await;
            }
            return Err(ServerError::Handshake(err));
        }
        Err(err) => return Err(err),
    };

    // Write the upgrade response
    write_http_response(&mut stream, response).await?;

    let leftover = buffer.split_off(header_len).to_vec();

    // Transition to standard blocking stream for tungstenite
    let std_stream = stream.into_std()?;
    if let Err(error) = std_stream.set_nonblocking(false) {
        error!(%error, "Failed to set std stream to blocking");
        return Err(ServerError::Io(error));
    }

    // Initialize WebSocket readers and writers
    let websocket_writer = tungstenite::WebSocket::from_raw_socket(
        std_stream.try_clone()?,
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

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use tokio::io::AsyncWriteExt;

    use super::*;

    type HandshakeSuccess = (
        tungstenite::WebSocket<std::net::TcpStream>,
        tungstenite::WebSocket<std::net::TcpStream>,
        ChipKind,
        HashMap<String, String>,
    );

    const HANDSHAKE_TEST_TIMEOUT: Duration = Duration::from_secs(2);
    const SOCKET_READ_TIMEOUT: Duration = Duration::from_secs(2);
    const RESPONSE_READ_CHUNK_SIZE: usize = 1024;
    const HTTP_HEADER_END: &[u8] = b"\r\n\r\n";
    const PREMATURE_EOF_REQUEST: &[u8] = b"GET /v1/websocket/bt HTTP/1.1\r\nHost: localhost";
    const MALFORMED_HTTP_REQUEST: &[u8] = b"NOT_HTTP_AT_ALL\r\n\r\n";
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

    async fn run_handshake_with_bytes(
        request_bytes: &[u8],
    ) -> (Result<HandshakeSuccess, ServerError>, Vec<u8>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind loopback listener");
        let addr = listener.local_addr().expect("listener local addr");
        let (client_res, accept_res) = tokio::join!(TcpStream::connect(addr), listener.accept());
        let mut client = client_res.expect("connect test client");
        let (server_stream, _) = accept_res.expect("accept server stream");

        client.write_all(request_bytes).await.expect("write request bytes");
        client.shutdown().await.expect("shutdown client write half");

        let result = perform_handshake(server_stream).await;

        let mut resp_bytes = Vec::new();
        tokio::time::timeout(HANDSHAKE_TEST_TIMEOUT, async {
            let mut chunk = [0u8; RESPONSE_READ_CHUNK_SIZE];
            while !resp_bytes.windows(HTTP_HEADER_END.len()).any(|w| w == HTTP_HEADER_END) {
                let n = client.read(&mut chunk).await.expect("read handshake response");
                if n == 0 {
                    break;
                }
                resp_bytes.extend_from_slice(&chunk[..n]);
            }
        })
        .await
        .expect("timed out reading handshake response");

        (result, resp_bytes)
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
    async fn test_perform_handshake_premature_eof_returns_handshake_failed() {
        for partial_bytes in [b"".as_slice(), PREMATURE_EOF_REQUEST] {
            let (result, resp_bytes) = run_handshake_with_bytes(partial_bytes).await;
            assert!(
                matches!(
                    result,
                    Err(ServerError::HandshakeFailed(ref msg))
                        if msg.contains("Client disconnected before sending full request")
                ),
                "expected HandshakeFailed on premature EOF for input={partial_bytes:?}, got: {:?}",
                result.err()
            );
            assert!(
                resp_bytes.is_empty(),
                "expected empty response on premature EOF for input={partial_bytes:?}"
            );
        }
    }

    #[tokio::test]
    async fn test_perform_handshake_malformed_http_writes_400_and_returns_http_parse() {
        let (result, resp_bytes) = run_handshake_with_bytes(MALFORMED_HTTP_REQUEST).await;
        assert!(
            matches!(result, Err(ServerError::HttpParse(_))),
            "expected HttpParse on malformed HTTP request, got: {:?}",
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
}
