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
//!
//! Only the server side of RFC 6455 needed here is implemented: unmasked
//! server frames, and client frames read just enough to answer ping and close.

use base64::Engine;
use bytes::{BufMut, Bytes, BytesMut};
use capture_actor::CaptureClient;
use http_body_util::Full;
use hyper::{
    Request, Response, StatusCode,
    header::{
        CONNECTION, HeaderMap, HeaderValue, SEC_WEBSOCKET_ACCEPT, SEC_WEBSOCKET_KEY,
        SEC_WEBSOCKET_VERSION, UPGRADE,
    },
};
use hyper_util::rt::TokioIo;
use netsim_model::ChipId;
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    sync::mpsc,
};
use tracing::info;

use crate::server::error_response;

/// GUID appended to `Sec-WebSocket-Key` to compute `Sec-WebSocket-Accept`
/// (RFC 6455, Section 1.3 and Section 4.2.2).
const WS_GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";
/// Binary data frame opcode (`%x2`, RFC 6455, Section 5.2).
const OPCODE_BINARY: u8 = 0x2;
/// Connection Close control frame opcode (`%x8`, RFC 6455, Section 5.5.1).
const OPCODE_CLOSE: u8 = 0x8;
/// Ping control frame opcode (`%x9`, RFC 6455, Section 5.5.2).
const OPCODE_PING: u8 = 0x9;
/// Pong control frame opcode (`%xA`, RFC 6455, Section 5.5.3).
const OPCODE_PONG: u8 = 0xA;
/// Maximum payload length for control frames (125 bytes; RFC 6455, Section 5.5).
/// Also the threshold for single-byte payload length encoding (`0..=125`, Section 5.2).
const MAX_CONTROL_PAYLOAD: u64 = 125;
/// Normal closure status code (`1000`, RFC 6455, Section 5.5.1 and Section 7.4.1).
const CLOSE_NORMAL: u16 = 1000;

// TODO(b/573234188): Centralize WebSocket handshake and framing logic with websocket-server.
/// Computes the `Sec-WebSocket-Accept` handshake response header value by
/// concatenating the client's `Sec-WebSocket-Key` with [`WS_GUID`], taking the
/// SHA-1 digest, and Base64-encoding the result (RFC 6455, Section 1.3).
fn accept_key(key: &[u8]) -> String {
    let mut ctx = ring::digest::Context::new(&ring::digest::SHA1_FOR_LEGACY_USE_ONLY);
    ctx.update(key);
    ctx.update(WS_GUID.as_bytes());
    base64::engine::general_purpose::STANDARD.encode(ctx.finish())
}

/// Encodes one unmasked, unfragmented server-to-client WebSocket frame
/// (RFC 6455, Section 5.1 and Section 5.2):
///
/// ```text
///  0                   1                   2                   3
///  0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1
/// +-+-+-+-+-------+-+-------------+-------------------------------+
/// |F|R|R|R| opcode|M| Payload len |    Extended payload length    |
/// |I|S|S|S|  (4)  |A|     (7)     |             (16/64)           |
/// |N|V|V|V|       |S|             |   (if payload len==126/127)   |
/// | |1|2|3|       |K|             |                               |
/// +-+-+-+-+-------+-+-------------+-------------------------------+
/// ```
fn encode_frame(opcode: u8, payload: &[u8]) -> Bytes {
    let mut frame = BytesMut::with_capacity(payload.len() + 10);
    // Byte 0: FIN=1 (0x80, final fragment), RSV1..3=0, and 4-bit opcode (Section 5.2).
    frame.put_u8(0x80 | opcode);
    // Byte 1 (+ optional extended length): MASK=0 because a server MUST NOT mask
    // any frames that it sends to the client (RFC 6455, Section 5.1).
    // Payload length encoding (RFC 6455, Section 5.2):
    // - 0..=125: 7-bit length in byte 1.
    // - 126..=65535: byte 1 = 126, followed by 16-bit unsigned big-endian length.
    // - > 65535: byte 1 = 127, followed by 64-bit unsigned big-endian length.
    if payload.len() <= MAX_CONTROL_PAYLOAD as usize {
        frame.put_u8(payload.len() as u8);
    } else if let Ok(len) = u16::try_from(payload.len()) {
        frame.put_u8(126);
        frame.put_u16(len);
    } else {
        frame.put_u8(127);
        frame.put_u64(payload.len() as u64);
    }
    frame.put_slice(payload);
    frame.freeze()
}

enum Control {
    Pong(Bytes),
    Close,
}

fn header_contains(
    headers: &HeaderMap,
    name: impl hyper::header::AsHeaderName,
    token: &str,
) -> bool {
    headers.get_all(name).iter().any(|value| {
        value.to_str().is_ok_and(|v| v.split(',').any(|t| t.trim().eq_ignore_ascii_case(token)))
    })
}

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
    let headers = req.headers();
    if !header_contains(headers, UPGRADE, "websocket")
        || !header_contains(headers, CONNECTION, "upgrade")
    {
        return error_response(StatusCode::BAD_REQUEST, "Expected a WebSocket upgrade request");
    }
    if headers.get(SEC_WEBSOCKET_VERSION).is_none_or(|v| v != "13") {
        let mut resp =
            error_response(StatusCode::UPGRADE_REQUIRED, "Unsupported WebSocket version");
        resp.headers_mut().insert(SEC_WEBSOCKET_VERSION, HeaderValue::from_static("13"));
        return resp;
    }
    let Some(key) = headers.get(SEC_WEBSOCKET_KEY).filter(|k| !k.is_empty()) else {
        return error_response(StatusCode::BAD_REQUEST, "Missing Sec-WebSocket-Key");
    };
    let accept = accept_key(key.as_bytes());
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
            forward(TokioIo::new(upgraded), packets).await;
            info!("Capture stream closed for chip {chip_id}");
        }
    });

    Response::builder()
        .status(StatusCode::SWITCHING_PROTOCOLS)
        .header(UPGRADE, "websocket")
        .header(CONNECTION, "Upgrade")
        .header(SEC_WEBSOCKET_ACCEPT, accept)
        .body(Full::new(Bytes::new()))
        .unwrap_or_else(|_| Response::new(Full::new(Bytes::new())))
}

/// Sends each PCAP chunk as a binary message (`OPCODE_BINARY`) until the
/// capture ends or the client closes (sending a `CLOSE_NORMAL` 1000 status
/// code per RFC 6455, Section 5.5.1). Dropping `packets` on return unsubscribes.
async fn forward(
    io: impl AsyncRead + AsyncWrite + Send + 'static,
    mut packets: mpsc::Receiver<Bytes>,
) {
    let (reader, mut writer) = tokio::io::split(io);
    let (control_tx, mut control_rx) = mpsc::channel(4);
    let reader_task = tokio::spawn(read_client_frames(reader, control_tx));

    loop {
        let (opcode, payload, close) = tokio::select! {
            packet = packets.recv() => match packet {
                Some(packet) => (OPCODE_BINARY, packet, false),
                None => (OPCODE_CLOSE, Bytes::copy_from_slice(&CLOSE_NORMAL.to_be_bytes()), true),
            },
            control = control_rx.recv() => match control {
                Some(Control::Pong(payload)) => (OPCODE_PONG, payload, false),
                Some(Control::Close) => (OPCODE_CLOSE, Bytes::copy_from_slice(&CLOSE_NORMAL.to_be_bytes()), true),
                None => break,
            },
        };
        if writer.write_all(&encode_frame(opcode, &payload)).await.is_err() || close {
            break;
        }
    }
    let _ = writer.shutdown().await;
    reader_task.abort();
}

/// Reads client-to-server frames (RFC 6455, Section 5.2), forwarding Ping and
/// Close control frames to the writer task. Client data frames are discarded.
/// Returns on Close, EOF, or any framing/protocol violation.
async fn read_client_frames(reader: impl AsyncRead + Unpin, control_tx: mpsc::Sender<Control>) {
    let mut reader = tokio::io::BufReader::new(reader);
    let mut head = [0u8; 2];
    while reader.read_exact(&mut head).await.is_ok() {
        // Base 2-byte frame header (RFC 6455, Section 5.2):
        // - b0: bit 7 is FIN (0x80), bits 4..6 are RSV1..3, bits 0..3 are opcode (0x0f).
        // - b1: bit 7 is MASK (0x80), bits 0..6 are the 7-bit payload length indicator (0x7f).
        let [b0, b1] = head;
        let opcode = b0 & 0x0f;
        // Decode payload length (RFC 6455, Section 5.2):
        // - 0..=125: literal payload length in bytes.
        // - 126: followed by 2-byte unsigned big-endian extended payload length.
        // - 127: followed by 8-byte unsigned big-endian extended payload length.
        let len = match b1 & 0x7f {
            126 => match reader.read_u16().await {
                Ok(len) => u64::from(len),
                Err(_) => return,
            },
            127 => match reader.read_u64().await {
                Ok(len) => len,
                Err(_) => return,
            },
            len => u64::from(len),
        };
        // RFC 6455, Section 5.1 & Section 5.3: All frames sent from client to
        // server MUST have the MASK bit (0x80) set and carry a 4-byte masking key;
        // the server MUST close the connection upon receiving an unmasked frame.
        let mut mask = [0u8; 4];
        if b1 & 0x80 == 0 || reader.read_exact(&mut mask).await.is_err() {
            return;
        }
        // RFC 6455, Section 5.2 & Section 5.5: Control frames are identified by
        // the most significant bit of the opcode being 1 (`opcode & 0x8 != 0`).
        // Data frames (`opcode & 0x8 == 0`, i.e. 0x0..=0x7) are ignored on this
        // unidirectional capture stream, so drain and discard their payload bytes.
        if opcode & 0x8 == 0 {
            let mut discard = (&mut reader).take(len);
            if tokio::io::copy(&mut discard, &mut tokio::io::sink()).await.is_err() {
                return;
            }
            continue;
        }
        // RFC 6455, Section 5.4 & Section 5.5: Control frames MUST NOT be
        // fragmented (`FIN` bit `b0 & 0x80` must be set) and MUST have a payload
        // length of 125 bytes or less (`MAX_CONTROL_PAYLOAD`).
        if b0 & 0x80 == 0 || len > MAX_CONTROL_PAYLOAD {
            return;
        }
        let mut payload = vec![0u8; len as usize];
        if reader.read_exact(&mut payload).await.is_err() {
            return;
        }
        // RFC 6455, Section 5.3: Unmask payload octets in place by XORing byte `i`
        // with `mask[i % 4]`.
        for (byte, m) in payload.iter_mut().zip(mask.iter().cycle()) {
            *byte ^= m;
        }
        // Dispatch control frames (RFC 6455, Section 5.5.1 - Section 5.5.3):
        // - Ping (0x9, Section 5.5.2 & 5.5.3): Respond with a Pong frame carrying
        //   the identical Application data payload.
        // - Close (0x8, Section 5.5.1): Echo a Close frame and terminate the reader.
        match opcode {
            OPCODE_PING if control_tx.send(Control::Pong(payload.into())).await.is_ok() => {}
            OPCODE_CLOSE => {
                let _ = control_tx.send(Control::Close).await;
                return;
            }
            OPCODE_PING => return,
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accept_key_matches_rfc_example() {
        // RFC 6455, section 1.3.
        assert_eq!(accept_key(b"dGhlIHNhbXBsZSBub25jZQ=="), "s3pPLMBiTxaQ9kYGzzhZRbK+xOo=");
    }

    #[test]
    fn encode_frame_lengths() {
        assert_eq!(&encode_frame(OPCODE_BINARY, &[7; 3])[..2], &[0x82, 3]);
        let medium = encode_frame(OPCODE_BINARY, &[0; 300]);
        assert_eq!(&medium[..4], &[0x82, 126, 0x01, 0x2c]);
        assert_eq!(medium.len(), 4 + 300);
        let large = encode_frame(OPCODE_BINARY, &vec![0; 70000]);
        assert_eq!(&large[..10], &[0x82, 127, 0, 0, 0, 0, 0, 0x01, 0x11, 0x70]);
        assert_eq!(large.len(), 10 + 70000);
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
