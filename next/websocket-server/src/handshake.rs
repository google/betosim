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

use base64::Engine;
use http::{
    Response, StatusCode,
    header::{CONNECTION, SEC_WEBSOCKET_ACCEPT, SEC_WEBSOCKET_KEY, UPGRADE},
};
use tokio::io::{AsyncWrite, AsyncWriteExt};

use crate::error::ServerError;

const WS_MAGIC_GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";

/// Generates the `Sec-WebSocket-Accept` header value for the server handshake.
///
/// Ref: RFC 6455, Section 4.2.2 - Server Opening Handshake
pub(crate) fn generate_websocket_accept(websocket_key: &str) -> String {
    let concat = format!("{websocket_key}{WS_MAGIC_GUID}");
    let digest = ring::digest::digest(&ring::digest::SHA1_FOR_LEGACY_USE_ONLY, concat.as_bytes());
    base64::engine::general_purpose::STANDARD.encode(digest)
}

pub(crate) fn handle_websocket_handshake(
    req: &httparse::Request,
) -> Result<Response<Vec<u8>>, ServerError> {
    let header = req
        .headers
        .iter()
        .find(|header| header.name.eq_ignore_ascii_case(SEC_WEBSOCKET_KEY.as_str()))
        .ok_or(ServerError::MissingHeader(SEC_WEBSOCKET_KEY))?;
    let key_str = str::from_utf8(header.value)?;
    let websocket_accept = generate_websocket_accept(key_str);

    Ok(Response::builder()
        .status(StatusCode::SWITCHING_PROTOCOLS)
        .header(UPGRADE, "websocket")
        .header(SEC_WEBSOCKET_ACCEPT, websocket_accept)
        .header(CONNECTION, "Upgrade")
        .body(Vec::new())?)
}

/// Helper to write an http::Response to a tokio TcpStream.
pub(crate) async fn write_http_response(
    stream: &mut (impl AsyncWrite + Unpin),
    response: Response<Vec<u8>>,
) -> Result<(), ServerError> {
    let status = response.status();
    let status_line =
        format!("HTTP/1.1 {} {}\r\n", status.as_u16(), status.canonical_reason().unwrap_or(""));
    stream.write_all(status_line.as_bytes()).await?;
    for (name, value) in response.headers().iter() {
        let header_line = format!("{}: {}\r\n", name, value.to_str()?);
        stream.write_all(header_line.as_bytes()).await?;
    }
    stream.write_all(b"\r\n").await?;
    stream.write_all(response.body()).await?;
    stream.flush().await?;
    Ok(())
}
