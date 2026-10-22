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
    handshake::{handle_websocket_handshake, write_http_response},
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

    let response = handle_websocket_handshake(&req)?;

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
