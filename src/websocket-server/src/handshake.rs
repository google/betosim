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

use common::util::websocket::{
    HandshakeError, WEBSOCKET_VERSION, accept_key, validate_upgrade_request,
};
use http::{
    Response, StatusCode,
    header::{CONNECTION, SEC_WEBSOCKET_ACCEPT, SEC_WEBSOCKET_VERSION, UPGRADE},
};
use tokio::io::{AsyncWrite, AsyncWriteExt};

use crate::error::ServerError;

pub(crate) fn handle_websocket_handshake(
    req: &httparse::Request,
) -> Result<Response<Vec<u8>>, ServerError> {
    let key = validate_upgrade_request(req.headers.iter().map(|h| (h.name, h.value)))?;
    let websocket_accept = accept_key(key);

    Ok(Response::builder()
        .status(StatusCode::SWITCHING_PROTOCOLS)
        .header(UPGRADE, "websocket")
        .header(SEC_WEBSOCKET_ACCEPT, websocket_accept)
        .header(CONNECTION, "Upgrade")
        .body(Vec::new())?)
}

/// Builds an HTTP error response corresponding to a WebSocket handshake failure.
pub(crate) fn handshake_error_response(
    err: &HandshakeError,
) -> Result<Response<Vec<u8>>, ServerError> {
    let builder = match err {
        HandshakeError::UnsupportedVersion => Response::builder()
            .status(StatusCode::UPGRADE_REQUIRED)
            .header(SEC_WEBSOCKET_VERSION, WEBSOCKET_VERSION),
        HandshakeError::MissingUpgrade
        | HandshakeError::MissingConnection
        | HandshakeError::MissingKey
        | HandshakeError::InvalidKey
        | HandshakeError::DuplicateHeader => Response::builder().status(StatusCode::BAD_REQUEST),
    };
    Ok(builder.header(CONNECTION, "close").body(Vec::new())?)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_and_handle(raw_request: &[u8]) -> Result<Response<Vec<u8>>, ServerError> {
        let mut headers = [httparse::EMPTY_HEADER; 16];
        let mut req = httparse::Request::new(&mut headers);
        assert!(matches!(req.parse(raw_request), Ok(httparse::Status::Complete(_))));
        handle_websocket_handshake(&req)
    }

    #[test]
    fn handle_websocket_handshake_accepts_valid_request() {
        let raw = b"GET /v1/websocket/bt HTTP/1.1\r\n\
Host: localhost:8080\r\n\
Upgrade: websocket\r\n\
Connection: keep-alive, Upgrade\r\n\
Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\
Sec-WebSocket-Version: 13\r\n\r\n";

        let resp = parse_and_handle(raw).unwrap();
        assert_eq!(resp.status(), StatusCode::SWITCHING_PROTOCOLS);
        assert_eq!(resp.headers().get(UPGRADE).unwrap(), "websocket");
        assert_eq!(resp.headers().get(CONNECTION).unwrap(), "Upgrade");
        assert_eq!(
            resp.headers().get(SEC_WEBSOCKET_ACCEPT).unwrap(),
            "s3pPLMBiTxaQ9kYGzzhZRbK+xOo="
        );
    }

    #[test]
    fn handle_websocket_handshake_rejects_bad_requests_with_400() {
        let cases = [
            // Missing Upgrade header
            b"GET /v1/websocket/bt HTTP/1.1\r\n\
Host: localhost\r\n\
Connection: Upgrade\r\n\
Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\
Sec-WebSocket-Version: 13\r\n\r\n"
                .as_slice(),
            // Missing Connection Upgrade token
            b"GET /v1/websocket/bt HTTP/1.1\r\n\
Host: localhost\r\n\
Upgrade: websocket\r\n\
Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\
Sec-WebSocket-Version: 13\r\n\r\n"
                .as_slice(),
            // Missing Sec-WebSocket-Key
            b"GET /v1/websocket/bt HTTP/1.1\r\n\
Host: localhost\r\n\
Upgrade: websocket\r\n\
Connection: Upgrade\r\n\
Sec-WebSocket-Version: 13\r\n\r\n"
                .as_slice(),
            // Invalid Sec-WebSocket-Key (not a base64 16-byte nonce)
            b"GET /v1/websocket/bt HTTP/1.1\r\n\
Host: localhost\r\n\
Upgrade: websocket\r\n\
Connection: Upgrade\r\n\
Sec-WebSocket-Key: c2hvcnQ=\r\n\
Sec-WebSocket-Version: 13\r\n\r\n"
                .as_slice(),
            // Duplicate Sec-WebSocket-Key
            b"GET /v1/websocket/bt HTTP/1.1\r\n\
Host: localhost\r\n\
Upgrade: websocket\r\n\
Connection: Upgrade\r\n\
Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\
Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\
Sec-WebSocket-Version: 13\r\n\r\n"
                .as_slice(),
            // Duplicate Sec-WebSocket-Version
            b"GET /v1/websocket/bt HTTP/1.1\r\n\
Host: localhost\r\n\
Upgrade: websocket\r\n\
Connection: Upgrade\r\n\
Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\
Sec-WebSocket-Version: 13\r\n\
Sec-WebSocket-Version: 13\r\n\r\n"
                .as_slice(),
        ];

        for raw in cases {
            let Err(ServerError::Handshake(err)) = parse_and_handle(raw) else {
                panic!("expected HandshakeError for request");
            };
            let resp = handshake_error_response(&err).unwrap();
            assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
            assert_eq!(resp.headers().get(CONNECTION).unwrap(), "close");
        }
    }

    #[test]
    fn handle_websocket_handshake_rejects_unsupported_version_with_426() {
        for raw in [
            b"GET /v1/websocket/bt HTTP/1.1\r\n\
Host: localhost\r\n\
Upgrade: websocket\r\n\
Connection: Upgrade\r\n\
Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\
Sec-WebSocket-Version: 8\r\n\r\n"
                .as_slice(),
            b"GET /v1/websocket/bt HTTP/1.1\r\n\
Host: localhost\r\n\
Upgrade: websocket\r\n\
Connection: Upgrade\r\n\
Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\r\n"
                .as_slice(),
        ] {
            let Err(ServerError::Handshake(err)) = parse_and_handle(raw) else {
                panic!("expected UnsupportedVersion");
            };
            assert_eq!(err, HandshakeError::UnsupportedVersion);
            let resp = handshake_error_response(&err).unwrap();
            assert_eq!(resp.status(), StatusCode::UPGRADE_REQUIRED);
            assert_eq!(resp.headers().get(SEC_WEBSOCKET_VERSION).unwrap(), WEBSOCKET_VERSION);
            assert_eq!(resp.headers().get(CONNECTION).unwrap(), "close");
        }
    }
}
