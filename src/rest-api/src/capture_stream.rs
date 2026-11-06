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

//! Live packet capture over WebSocket: `GET /v1/capture?chip_id=N`.
//!
//! After the upgrade the server sends the chip's packets as a PCAP stream:
//! the first binary message is the PCAP global header (same link type as the
//! capture file), then one binary message per PCAP record (record header and
//! packet). This is independent of file capture: streaming works whether or
//! not capture is enabled, and file capture is not affected by subscribers.

use bytes::Bytes;
use capture_actor::CaptureClient;
use common::util::websocket::{
    HandshakeError, WEBSOCKET_VERSION, accept_key, forward_binary, validate_upgrade_request,
};
use http_body_util::Full;
use hyper::{
    Request, Response, StatusCode,
    header::{
        CONNECTION, HeaderMap, HeaderValue, SEC_WEBSOCKET_ACCEPT, SEC_WEBSOCKET_VERSION, UPGRADE,
    },
};
use hyper_util::rt::TokioIo;
use netsim_model::ChipId;
use tracing::info;

use crate::server::error_response;

fn parse_chip_id(query: Option<&str>) -> Option<u32> {
    form_urlencoded::parse(query?.as_bytes())
        .find(|(key, _)| key == "chip_id")
        .and_then(|(_, value)| value.parse().ok())
}

/// Handles `GET /v1/capture`: validates the RFC 6455 Section 4.2.1 opening
/// handshake, subscribes to the chip's capture stream, and spawns the task
/// that forwards PCAP records over the upgraded connection.
pub(super) async fn upgrade(
    capture_client: &CaptureClient,
    mut req: Request<hyper::body::Incoming>,
) -> Response<Full<Bytes>> {
    let resp = match switching_protocols(req.headers()) {
        Handshake::Upgrade(resp) => resp,
        Handshake::Reject(resp) => return resp,
    };
    let Some(chip_id) = parse_chip_id(req.uri().query()) else {
        return error_response(StatusCode::BAD_REQUEST, "Missing or invalid chip_id");
    };

    // Subscribe before answering 101 so an unknown chip gets a real HTTP error.
    let Ok(packets) = capture_client.stream_capture(ChipId(chip_id)).await else {
        return error_response(StatusCode::NOT_FOUND, &format!("No capture for chip {chip_id}"));
    };

    let on_upgrade = hyper::upgrade::on(&mut req);
    tokio::spawn(async move {
        if let Ok(upgraded) = on_upgrade.await {
            info!("Capture stream opened for chip {chip_id}");
            forward_binary(TokioIo::new(upgraded), packets).await;
            info!("Capture stream closed for chip {chip_id}");
        }
    });

    resp
}

pub(super) enum Handshake {
    Upgrade(Response<Full<Bytes>>),
    Reject(Response<Full<Bytes>>),
}

pub(super) fn switching_protocols(headers: &HeaderMap) -> Handshake {
    let key = match validate_upgrade_request(
        headers.iter().map(|(name, value)| (name.as_str(), value.as_bytes())),
    ) {
        Ok(key) => key,
        Err(HandshakeError::UnsupportedVersion) => {
            let mut resp =
                error_response(StatusCode::UPGRADE_REQUIRED, "Unsupported WebSocket version");
            resp.headers_mut()
                .insert(SEC_WEBSOCKET_VERSION, HeaderValue::from_static(WEBSOCKET_VERSION));
            return Handshake::Reject(resp);
        }
        Err(err) => {
            return Handshake::Reject(error_response(StatusCode::BAD_REQUEST, &err.to_string()));
        }
    };
    Handshake::Upgrade(
        Response::builder()
            .status(StatusCode::SWITCHING_PROTOCOLS)
            .header(UPGRADE, "websocket")
            .header(CONNECTION, "Upgrade")
            .header(SEC_WEBSOCKET_ACCEPT, accept_key(key))
            .body(Full::new(Bytes::new()))
            .unwrap_or_else(|_err| Response::new(Full::new(Bytes::new()))),
    )
}

#[cfg(test)]
mod tests {
    use hyper::header::{SEC_WEBSOCKET_KEY, SEC_WEBSOCKET_VERSION};

    use super::*;

    #[test]
    fn switching_protocols_validates_headers() {
        let mut headers = HeaderMap::new();
        headers.insert(UPGRADE, HeaderValue::from_static("websocket"));
        headers.insert(CONNECTION, HeaderValue::from_static("Upgrade"));
        headers.insert(SEC_WEBSOCKET_VERSION, HeaderValue::from_static("13"));
        headers.insert(SEC_WEBSOCKET_KEY, HeaderValue::from_static("dGhlIHNhbXBsZSBub25jZQ=="));

        let Handshake::Upgrade(resp) = switching_protocols(&headers) else {
            panic!("expected 101 Switching Protocols");
        };
        assert_eq!(resp.status(), StatusCode::SWITCHING_PROTOCOLS);
        assert_eq!(
            resp.headers().get(SEC_WEBSOCKET_ACCEPT).unwrap(),
            "s3pPLMBiTxaQ9kYGzzhZRbK+xOo="
        );

        headers.insert(SEC_WEBSOCKET_VERSION, HeaderValue::from_static("8"));
        let Handshake::Reject(resp) = switching_protocols(&headers) else {
            panic!("expected 426 Upgrade Required");
        };
        assert_eq!(resp.status(), StatusCode::UPGRADE_REQUIRED);
        assert_eq!(resp.headers().get(SEC_WEBSOCKET_VERSION).unwrap(), "13");
    }

    #[test]
    fn parse_chip_id_from_query() {
        assert_eq!(parse_chip_id(Some("chip_id=5")), Some(5));
        assert_eq!(parse_chip_id(Some("x=1&chip_id=42")), Some(42));
        assert_eq!(parse_chip_id(Some("chip_id=abc")), None);
        assert_eq!(parse_chip_id(Some("x=1")), None);
        assert_eq!(parse_chip_id(None), None);
    }
}
