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

//! Shared RFC 6455 WebSocket handshake and framing utilities.
//!
//! Kept HTTP-crate-agnostic (`&str` / `&[u8]` / `Bytes` / `AsyncRead + AsyncWrite`)
//! so both `hyper` 1.x endpoints (`rest-api`) and `httparse` / `http` 0.2
//! endpoints (`websocket-server`) can share handshake validation,
//! `Sec-WebSocket-Accept` derivation, frame encoding, and control-frame handling.

use base64::Engine;
use bytes::{BufMut, Bytes, BytesMut};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    sync::mpsc,
};

/// GUID appended to `Sec-WebSocket-Key` to compute `Sec-WebSocket-Accept`
/// (RFC 6455, Section 1.3 and Section 4.2.2).
const WS_GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";

/// Supported WebSocket protocol version (`13`, RFC 6455, Section 4.2.1).
pub const WEBSOCKET_VERSION: &str = "13";

const OPCODE_BINARY: u8 = 0x2;
const OPCODE_CLOSE: u8 = 0x8;
const OPCODE_PING: u8 = 0x9;
const OPCODE_PONG: u8 = 0xA;

const MAX_CONTROL_PAYLOAD: usize = 125;
const CLOSE_NORMAL: u16 = 1000;
const WEBSOCKET_NONCE_LEN: usize = 16;

const FIN_BIT: u8 = 0x80;
const RSV_MASK: u8 = 0x70;
const MASK_BIT: u8 = 0x80;
const OPCODE_MASK: u8 = 0x0f;
const CONTROL_FRAME_BIT: u8 = 0x08;
const PAYLOAD_LEN_MASK: u8 = 0x7f;
const PAYLOAD_LEN_16: u8 = 126;
const PAYLOAD_LEN_64: u8 = 127;
const MAX_EXTENDED_FRAME_HEADER_LEN: usize = 10;
const CONTROL_CHANNEL_CAPACITY: usize = 4;

/// Errors returned when validating an HTTP/1.1 WebSocket opening handshake
/// request (RFC 6455, Section 4.2.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum HandshakeError {
    /// The `Upgrade` header is missing or does not contain the `websocket` token.
    #[error("Expected a WebSocket upgrade request")]
    MissingUpgrade,
    /// The `Connection` header is missing or does not contain the `upgrade` token.
    #[error("Expected a WebSocket upgrade request")]
    MissingConnection,
    /// The `Sec-WebSocket-Version` header is missing or not `"13"`.
    #[error("Unsupported WebSocket version")]
    UnsupportedVersion,
    /// The `Sec-WebSocket-Key` header is missing or empty.
    #[error("Missing Sec-WebSocket-Key")]
    MissingKey,
    /// The `Sec-WebSocket-Key` header is not a valid base64-encoded 16-byte nonce.
    #[error("Invalid Sec-WebSocket-Key")]
    InvalidKey,
    /// A singleton WebSocket handshake header (`Sec-WebSocket-Key` or
    /// `Sec-WebSocket-Version`) appeared more than once.
    #[error("Duplicate WebSocket handshake header")]
    DuplicateHeader,
}

/// Computes the `Sec-WebSocket-Accept` handshake response header value by
/// concatenating the trimmed `Sec-WebSocket-Key` with [`WS_GUID`], taking the
/// SHA-1 digest, and Base64-encoding the result (RFC 6455, Section 1.3).
pub fn accept_key(key: impl AsRef<[u8]>) -> String {
    let mut ctx = ring::digest::Context::new(&ring::digest::SHA1_FOR_LEGACY_USE_ONLY);
    ctx.update(key.as_ref().trim_ascii());
    ctx.update(WS_GUID.as_bytes());
    base64::engine::general_purpose::STANDARD.encode(ctx.finish())
}

/// Returns `true` if a comma-separated HTTP header value contains `token`
/// (matched ASCII case-insensitively with optional whitespace trimmed).
fn header_value_contains_token(value: &[u8], token: &str) -> bool {
    std::str::from_utf8(value)
        .is_ok_and(|v| v.split(',').any(|part| part.trim().eq_ignore_ascii_case(token)))
}

/// Returns `true` if any header matching `name` (ASCII case-insensitively)
/// contains `token` in its comma-separated token list.
pub fn headers_contain_token<'a, I>(headers: I, name: &str, token: &str) -> bool
where
    I: IntoIterator<Item = (&'a str, &'a [u8])>,
{
    headers.into_iter().any(|(header_name, value)| {
        header_name.eq_ignore_ascii_case(name) && header_value_contains_token(value, token)
    })
}

/// Validates an HTTP/1.1 WebSocket opening handshake request's headers
/// (RFC 6455, Section 4.2.1) and returns the trimmed `Sec-WebSocket-Key`
/// string on success.
///
/// # Errors
///
/// Returns [`HandshakeError::MissingUpgrade`] or
/// [`HandshakeError::MissingConnection`] if the `Upgrade: websocket` or
/// `Connection: Upgrade` tokens are absent,
/// [`HandshakeError::DuplicateHeader`] if `Sec-WebSocket-Version` or
/// `Sec-WebSocket-Key` appears more than once,
/// [`HandshakeError::UnsupportedVersion`] if `Sec-WebSocket-Version` is not
/// `13`, [`HandshakeError::MissingKey`] if `Sec-WebSocket-Key` is missing or
/// empty, or [`HandshakeError::InvalidKey`] if `Sec-WebSocket-Key` is not a
/// valid base64-encoded 16-byte nonce.
pub fn validate_upgrade_request<'a, I>(headers: I) -> Result<&'a str, HandshakeError>
where
    I: IntoIterator<Item = (&'a str, &'a [u8])>,
{
    let mut has_upgrade = false;
    let mut has_connection = false;
    let mut version: Option<&'a [u8]> = None;
    let mut key: Option<&'a [u8]> = None;

    for (name, value) in headers {
        if name.eq_ignore_ascii_case("upgrade") {
            has_upgrade |= header_value_contains_token(value, "websocket");
        } else if name.eq_ignore_ascii_case("connection") {
            has_connection |= header_value_contains_token(value, "upgrade");
        } else if name.eq_ignore_ascii_case("sec-websocket-version") {
            if version.is_some() {
                return Err(HandshakeError::DuplicateHeader);
            }
            version = Some(value.trim_ascii());
        } else if name.eq_ignore_ascii_case("sec-websocket-key") {
            if key.is_some() {
                return Err(HandshakeError::DuplicateHeader);
            }
            key = Some(value.trim_ascii());
        }
    }

    if !has_upgrade {
        return Err(HandshakeError::MissingUpgrade);
    }
    if !has_connection {
        return Err(HandshakeError::MissingConnection);
    }
    if version != Some(WEBSOCKET_VERSION.as_bytes()) {
        return Err(HandshakeError::UnsupportedVersion);
    }
    let Some(raw_key) = key.filter(|k| !k.is_empty()) else {
        return Err(HandshakeError::MissingKey);
    };
    let Ok(key_str) = std::str::from_utf8(raw_key) else {
        return Err(HandshakeError::InvalidKey);
    };
    if !base64::engine::general_purpose::STANDARD
        .decode(key_str)
        .is_ok_and(|decoded| decoded.len() == WEBSOCKET_NONCE_LEN)
    {
        return Err(HandshakeError::InvalidKey);
    }
    Ok(key_str)
}

/// Encodes one unmasked, unfragmented server-to-client WebSocket frame
/// (RFC 6455, Section 5.1 and Section 5.2).
fn encode_frame(opcode: u8, payload: &[u8]) -> Bytes {
    let mut frame = BytesMut::with_capacity(payload.len() + MAX_EXTENDED_FRAME_HEADER_LEN);
    frame.put_u8(FIN_BIT | (opcode & OPCODE_MASK));
    if payload.len() <= MAX_CONTROL_PAYLOAD
        && let Ok(len) = u8::try_from(payload.len())
    {
        frame.put_u8(len);
    } else if let Ok(len) = u16::try_from(payload.len()) {
        frame.put_u8(PAYLOAD_LEN_16);
        frame.put_u16(len);
    } else {
        frame.put_u8(PAYLOAD_LEN_64);
        frame.put_u64(u64::try_from(payload.len()).unwrap_or(u64::MAX));
    }
    frame.put_slice(payload);
    frame.freeze()
}

struct AbortOnDrop(tokio::task::JoinHandle<()>);

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

enum Control {
    Pong(Bytes),
    Close,
}

/// Streams each binary payload in `packets` as an unmasked binary WebSocket frame
/// (`OPCODE_BINARY`) until the channel closes or the client closes/errors.
pub async fn forward_binary(
    io: impl AsyncRead + AsyncWrite + Send + 'static,
    mut packets: mpsc::Receiver<Bytes>,
) {
    let (reader, mut writer) = tokio::io::split(io);
    let (control_tx, mut control_rx) = mpsc::channel(CONTROL_CHANNEL_CAPACITY);
    let _reader_guard = AbortOnDrop(tokio::spawn(read_client_frames(reader, control_tx)));

    loop {
        let (opcode, payload, close) = tokio::select! {
            item = packets.recv() => match item {
                Some(payload) => (OPCODE_BINARY, payload, false),
                None => (OPCODE_CLOSE, Bytes::copy_from_slice(&CLOSE_NORMAL.to_be_bytes()), true),
            },
            control = control_rx.recv() => match control {
                Some(Control::Pong(payload)) => (OPCODE_PONG, payload, false),
                Some(Control::Close) => {
                    (OPCODE_CLOSE, Bytes::copy_from_slice(&CLOSE_NORMAL.to_be_bytes()), true)
                }
                None => break,
            },
        };
        if writer.write_all(&encode_frame(opcode, &payload)).await.is_err() || close {
            break;
        }
    }
    let _ = writer.shutdown().await;
}

/// Returns `true` if `code` is a valid WebSocket Close frame status code on
/// the wire (RFC 6455, Section 7.4.1 and Section 7.4.2).
fn is_valid_close_code(code: u16) -> bool {
    matches!(code, 1000..=1003 | 1007..=1014 | 3000..=4999)
}

/// Reads client-to-server frames (RFC 6455, Section 5.2), forwarding Ping and
/// Close control frames to the writer task and draining client data frames.
/// Returns on Close, EOF, or any framing/protocol violation.
async fn read_client_frames(
    reader: impl AsyncRead + Unpin + Send,
    control_tx: mpsc::Sender<Control>,
) {
    let mut reader = tokio::io::BufReader::new(reader);
    let mut head = [0u8; 2];
    while reader.read_exact(&mut head).await.is_ok() {
        let [b0, b1] = head;
        // RFC 6455, Section 5.2: RSV1..3 MUST be 0 when no extension is negotiated.
        if b0 & RSV_MASK != 0 {
            return;
        }
        let opcode = b0 & OPCODE_MASK;
        // RFC 6455, Section 5.2: The minimal number of bytes MUST be used to encode length.
        let len = match b1 & PAYLOAD_LEN_MASK {
            PAYLOAD_LEN_16 => match reader.read_u16().await {
                Ok(len) if len >= u16::from(PAYLOAD_LEN_16) => u64::from(len),
                Ok(_) | Err(_) => return,
            },
            PAYLOAD_LEN_64 => match reader.read_u64().await {
                Ok(len) if len > u64::from(u16::MAX) && len >> 63 == 0 => len,
                Ok(_) | Err(_) => return,
            },
            len => u64::from(len),
        };
        // RFC 6455, Section 5.1 & Section 5.3: Client-to-server frames MUST be masked.
        let mut mask = [0u8; 4];
        if b1 & MASK_BIT == 0 || reader.read_exact(&mut mask).await.is_err() {
            return;
        }
        if opcode & CONTROL_FRAME_BIT == 0 {
            let mut discard = (&mut reader).take(len);
            match tokio::io::copy(&mut discard, &mut tokio::io::sink()).await {
                Ok(copied) if copied == len => continue,
                Ok(_) | Err(_) => return,
            }
        }
        // RFC 6455, Section 5.4 & Section 5.5: Control frames MUST NOT be fragmented
        // and MUST have a payload length of 125 bytes or less.
        let Ok(payload_len) = usize::try_from(len) else {
            return;
        };
        if b0 & FIN_BIT == 0 || payload_len > MAX_CONTROL_PAYLOAD {
            return;
        }
        let mut payload = vec![0u8; payload_len];
        if reader.read_exact(&mut payload).await.is_err() {
            return;
        }
        for (byte, m) in payload.iter_mut().zip(mask.iter().cycle()) {
            *byte ^= m;
        }
        match opcode {
            OPCODE_PING => {
                if control_tx.send(Control::Pong(payload.into())).await.is_err() {
                    return;
                }
            }
            OPCODE_CLOSE => {
                if !payload.is_empty() {
                    let Some((code_bytes, reason)) = payload.split_first_chunk::<2>() else {
                        return;
                    };
                    let code = u16::from_be_bytes(*code_bytes);
                    if !is_valid_close_code(code) || std::str::from_utf8(reason).is_err() {
                        return;
                    }
                }
                let _ = control_tx.send(Control::Close).await;
                return;
            }
            OPCODE_PONG => {}
            _ => return,
        }
    }
}

#[cfg(test)]
mod tests {
    use tokio::time::{Duration, timeout};

    use super::*;

    const IO_TIMEOUT: Duration = Duration::from_secs(2);
    const OPCODE_TEXT: u8 = 0x1;

    #[test]
    fn accept_key_matches_rfc_6455_vector() {
        // RFC 6455, Section 1.3.
        assert_eq!(accept_key("dGhlIHNhbXBsZSBub25jZQ=="), "s3pPLMBiTxaQ9kYGzzhZRbK+xOo=");
        assert_eq!(accept_key(b"  dGhlIHNhbXBsZSBub25jZQ==\t "), "s3pPLMBiTxaQ9kYGzzhZRbK+xOo=");
    }

    #[test]
    fn validate_upgrade_request_accepts_valid_headers() {
        let headers = [
            ("Host", b"localhost:8080".as_slice()),
            ("Upgrade", b"websocket".as_slice()),
            ("Connection", b"keep-alive, Upgrade".as_slice()),
            ("Sec-WebSocket-Key", b"dGhlIHNhbXBsZSBub25jZQ==".as_slice()),
            ("Sec-WebSocket-Version", b"13".as_slice()),
        ];
        assert_eq!(validate_upgrade_request(headers), Ok("dGhlIHNhbXBsZSBub25jZQ=="));
    }

    #[test]
    fn validate_upgrade_request_rejects_invalid_headers() {
        let missing_upgrade = [
            ("Connection", b"Upgrade".as_slice()),
            ("Sec-WebSocket-Key", b"dGhlIHNhbXBsZSBub25jZQ==".as_slice()),
            ("Sec-WebSocket-Version", b"13".as_slice()),
        ];
        assert_eq!(validate_upgrade_request(missing_upgrade), Err(HandshakeError::MissingUpgrade));

        let missing_connection = [
            ("Upgrade", b"websocket".as_slice()),
            ("Connection", b"keep-alive".as_slice()),
            ("Sec-WebSocket-Key", b"dGhlIHNhbXBsZSBub25jZQ==".as_slice()),
            ("Sec-WebSocket-Version", b"13".as_slice()),
        ];
        assert_eq!(
            validate_upgrade_request(missing_connection),
            Err(HandshakeError::MissingConnection)
        );

        let wrong_version = [
            ("Upgrade", b"websocket".as_slice()),
            ("Connection", b"Upgrade".as_slice()),
            ("Sec-WebSocket-Key", b"dGhlIHNhbXBsZSBub25jZQ==".as_slice()),
            ("Sec-WebSocket-Version", b"8".as_slice()),
        ];
        assert_eq!(
            validate_upgrade_request(wrong_version),
            Err(HandshakeError::UnsupportedVersion)
        );

        let empty_key = [
            ("Upgrade", b"websocket".as_slice()),
            ("Connection", b"Upgrade".as_slice()),
            ("Sec-WebSocket-Key", b"   ".as_slice()),
            ("Sec-WebSocket-Version", b"13".as_slice()),
        ];
        assert_eq!(validate_upgrade_request(empty_key), Err(HandshakeError::MissingKey));

        let invalid_key_len = [
            ("Upgrade", b"websocket".as_slice()),
            ("Connection", b"Upgrade".as_slice()),
            ("Sec-WebSocket-Key", b"c2hvcnQ=".as_slice()),
            ("Sec-WebSocket-Version", b"13".as_slice()),
        ];
        assert_eq!(validate_upgrade_request(invalid_key_len), Err(HandshakeError::InvalidKey));

        let non_base64_key = [
            ("Upgrade", b"websocket".as_slice()),
            ("Connection", b"Upgrade".as_slice()),
            ("Sec-WebSocket-Key", b"not-valid-base64!!!".as_slice()),
            ("Sec-WebSocket-Version", b"13".as_slice()),
        ];
        assert_eq!(validate_upgrade_request(non_base64_key), Err(HandshakeError::InvalidKey));

        let duplicate_key = [
            ("Upgrade", b"websocket".as_slice()),
            ("Connection", b"Upgrade".as_slice()),
            ("Sec-WebSocket-Key", b"dGhlIHNhbXBsZSBub25jZQ==".as_slice()),
            ("Sec-WebSocket-Key", b"dGhlIHNhbXBsZSBub25jZQ==".as_slice()),
            ("Sec-WebSocket-Version", b"13".as_slice()),
        ];
        assert_eq!(validate_upgrade_request(duplicate_key), Err(HandshakeError::DuplicateHeader));

        let duplicate_version = [
            ("Upgrade", b"websocket".as_slice()),
            ("Connection", b"Upgrade".as_slice()),
            ("Sec-WebSocket-Key", b"dGhlIHNhbXBsZSBub25jZQ==".as_slice()),
            ("Sec-WebSocket-Version", b"13".as_slice()),
            ("Sec-WebSocket-Version", b"13".as_slice()),
        ];
        assert_eq!(
            validate_upgrade_request(duplicate_version),
            Err(HandshakeError::DuplicateHeader)
        );
    }

    #[test]
    fn encode_frame_lengths_and_opcodes() {
        assert_eq!(encode_frame(OPCODE_TEXT, b"hi").as_ref(), &[0x81, 2, b'h', b'i']);
        assert_eq!(&encode_frame(OPCODE_BINARY, &[7; 3])[..2], &[0x82, 3]);
        assert_eq!(&encode_frame(OPCODE_BINARY, &[0; 125])[..2], &[0x82, 125]);

        let medium = encode_frame(OPCODE_BINARY, &[0; 300]);
        assert_eq!(&medium[..4], &[0x82, 126, 0x01, 0x2c]);
        assert_eq!(medium.len(), 4 + 300);

        let large = encode_frame(OPCODE_BINARY, &vec![0; 70_000]);
        assert_eq!(&large[..10], &[0x82, 127, 0, 0, 0, 0, 0, 0x01, 0x11, 0x70]);
        assert_eq!(large.len(), 10 + 70_000);
    }

    fn build_masked_client_frame(fin: bool, rsv: u8, opcode: u8, payload: &[u8]) -> Vec<u8> {
        let mask = [0x12, 0x34, 0x56, 0x78];
        let b0 = (if fin { FIN_BIT } else { 0 }) | (rsv << 4) | (opcode & OPCODE_MASK);
        let mut frame = vec![b0];
        if payload.len() <= MAX_CONTROL_PAYLOAD
            && let Ok(len) = u8::try_from(payload.len())
        {
            frame.push(MASK_BIT | len);
        } else if let Ok(len) = u16::try_from(payload.len()) {
            frame.push(MASK_BIT | PAYLOAD_LEN_16);
            frame.extend_from_slice(&len.to_be_bytes());
        } else {
            frame.push(MASK_BIT | PAYLOAD_LEN_64);
            frame
                .extend_from_slice(&u64::try_from(payload.len()).unwrap_or(u64::MAX).to_be_bytes());
        }
        frame.extend_from_slice(&mask);
        frame.extend(payload.iter().enumerate().map(|(i, b)| b ^ mask[i % 4]));
        frame
    }

    async fn read_server_frame(stream: &mut tokio::io::DuplexStream) -> (u8, Vec<u8>) {
        timeout(IO_TIMEOUT, async {
            let b0 = stream.read_u8().await.unwrap();
            let b1 = stream.read_u8().await.unwrap();
            assert_eq!(b0 & FIN_BIT, FIN_BIT);
            assert_eq!(b1 & MASK_BIT, 0);
            let len = match b1 & PAYLOAD_LEN_MASK {
                PAYLOAD_LEN_16 => u64::from(stream.read_u16().await.unwrap()),
                PAYLOAD_LEN_64 => stream.read_u64().await.unwrap(),
                len => u64::from(len),
            };
            let mut payload = vec![0; usize::try_from(len).unwrap()];
            stream.read_exact(&mut payload).await.unwrap();
            (b0 & OPCODE_MASK, payload)
        })
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn forward_binary_handles_data_ping_pong_and_close() {
        let (server_io, mut client_io) = tokio::io::duplex(4096);
        let (tx, rx) = mpsc::channel(4);
        let task = tokio::spawn(forward_binary(server_io, rx));

        tx.send(Bytes::from_static(b"pkt")).await.unwrap();
        assert_eq!(read_server_frame(&mut client_io).await, (OPCODE_BINARY, b"pkt".to_vec()));

        // Client data frames (7-bit and 16-bit) are discarded without closing.
        client_io
            .write_all(&build_masked_client_frame(true, 0, OPCODE_TEXT, b"ignored"))
            .await
            .unwrap();
        client_io
            .write_all(&build_masked_client_frame(true, 0, OPCODE_BINARY, &[0xAB; 200]))
            .await
            .unwrap();

        // Client Ping is answered with Pong carrying the identical payload.
        let ping_payload = vec![0x5a; MAX_CONTROL_PAYLOAD];
        client_io
            .write_all(&build_masked_client_frame(true, 0, OPCODE_PING, &ping_payload))
            .await
            .unwrap();
        assert_eq!(read_server_frame(&mut client_io).await, (OPCODE_PONG, ping_payload));

        // Client Close triggers a Close(1000) reply and terminates the task.
        client_io
            .write_all(&build_masked_client_frame(
                true,
                0,
                OPCODE_CLOSE,
                &CLOSE_NORMAL.to_be_bytes(),
            ))
            .await
            .unwrap();
        assert_eq!(
            read_server_frame(&mut client_io).await,
            (OPCODE_CLOSE, CLOSE_NORMAL.to_be_bytes().to_vec())
        );
        timeout(IO_TIMEOUT, task).await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn forward_binary_closes_on_sender_drop() {
        let (server_io, mut client_io) = tokio::io::duplex(4096);
        let (tx, rx) = mpsc::channel(4);
        let task = tokio::spawn(forward_binary(server_io, rx));

        tx.send(Bytes::from_static(&[1, 2, 3])).await.unwrap();
        assert_eq!(read_server_frame(&mut client_io).await, (OPCODE_BINARY, vec![1, 2, 3]));

        drop(tx);
        assert_eq!(
            read_server_frame(&mut client_io).await,
            (OPCODE_CLOSE, CLOSE_NORMAL.to_be_bytes().to_vec())
        );
        timeout(IO_TIMEOUT, task).await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn forward_binary_aborts_reader_when_dropped() {
        let (server_io, mut client_io) = tokio::io::duplex(4096);
        let (_tx, rx) = mpsc::channel::<Bytes>(1);
        let mut forward_fut = Box::pin(forward_binary(server_io, rx));

        // Poll once so `forward_binary` spawns `read_client_frames` and suspends.
        let _ = timeout(Duration::from_millis(10), &mut forward_fut).await;
        drop(forward_fut);

        // Dropping `forward_binary` aborts `read_client_frames` and releases `server_io`.
        let mut eof_buf = [0u8; 1];
        assert_eq!(timeout(IO_TIMEOUT, client_io.read(&mut eof_buf)).await.unwrap().unwrap(), 0);
    }

    #[tokio::test]
    async fn forward_rejects_protocol_violations() {
        let mask = [0x12, 0x34, 0x56, 0x78];
        // Non-minimal 16-bit extended length (100 < 126)
        let mut non_minimal_16 = vec![FIN_BIT | OPCODE_BINARY, MASK_BIT | PAYLOAD_LEN_16, 0, 100];
        non_minimal_16.extend_from_slice(&mask);
        non_minimal_16.extend(vec![0; 100]);

        // Non-minimal 64-bit extended length (300 <= 65535)
        let mut non_minimal_64 = vec![FIN_BIT | OPCODE_BINARY, MASK_BIT | PAYLOAD_LEN_64];
        non_minimal_64.extend_from_slice(&300u64.to_be_bytes());
        non_minimal_64.extend_from_slice(&mask);
        non_minimal_64.extend(vec![0; 300]);

        // Invalid UTF-8 in Close reason
        let mut bad_utf8_close = CLOSE_NORMAL.to_be_bytes().to_vec();
        bad_utf8_close.extend_from_slice(&[0xff, 0xfe]);

        for bad_frame in [
            // Unmasked client frame
            vec![FIN_BIT | OPCODE_PING, 0],
            // Fragmented control frame (FIN = 0)
            build_masked_client_frame(false, 0, OPCODE_PING, b"hi"),
            // Oversized control frame (> 125 bytes)
            build_masked_client_frame(true, 0, OPCODE_PING, &[0; MAX_CONTROL_PAYLOAD + 1]),
            // Non-zero RSV1 bit
            build_masked_client_frame(true, 1, OPCODE_PING, b"hi"),
            // Non-minimal payload length encodings
            non_minimal_16,
            non_minimal_64,
            // 1-byte Close payload (RFC 6455 §5.5.1)
            build_masked_client_frame(true, 0, OPCODE_CLOSE, &[0x03]),
            // Reserved/pseudo Close status code 1005
            build_masked_client_frame(true, 0, OPCODE_CLOSE, &1005u16.to_be_bytes()),
            // Invalid UTF-8 Close reason
            build_masked_client_frame(true, 0, OPCODE_CLOSE, &bad_utf8_close),
        ] {
            let (server_io, mut client_io) = tokio::io::duplex(4096);
            let (_tx, rx) = mpsc::channel::<Bytes>(1);
            let task = tokio::spawn(forward_binary(server_io, rx));
            client_io.write_all(&bad_frame).await.unwrap();
            let mut eof_buf = [0u8; 1];
            assert_eq!(
                timeout(IO_TIMEOUT, client_io.read(&mut eof_buf)).await.unwrap().unwrap(),
                0
            );
            timeout(IO_TIMEOUT, task).await.unwrap().unwrap();
        }
    }
}
