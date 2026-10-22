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

//! WebSocket packet stream client.
//!
//! Connects to a packet stream route served by [`crate::server`] (for example
//! `/v1/websocket/bt`) and carries one packet per binary message in each
//! direction, so forwarders can stream packets without gRPC or protobuf.

use std::{io, net::TcpStream as StdTcpStream, pin::Pin, time::Duration};

use base64::Engine;
use bytes::{Bytes, BytesMut};
use futures::{Sink, Stream, sink::unfold};
use http::{
    StatusCode,
    header::{CONNECTION, SEC_WEBSOCKET_ACCEPT, UPGRADE},
};
use netsim_model::ChipInfo;
use ring::rand::{SecureRandom, SystemRandom};
use thiserror::Error;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    sync::mpsc,
};
use tokio_stream::wrappers::ReceiverStream;
use tracing::{info, warn};
use tungstenite::{Message, WebSocket, protocol::Role};

use crate::handshake::generate_websocket_accept;

const MAX_HTTP_RESPONSE_SIZE: usize = 2048;
const MAX_RESPONSE_HEADERS: usize = 64;
const CHANNEL_CAPACITY: usize = 100;
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);
const READ_POLL_TIMEOUT: Duration = Duration::from_secs(1);
const WRITE_TIMEOUT: Duration = Duration::from_secs(30);

/// Read-only wrapper around [`StdTcpStream`] for the reader [`WebSocket`].
///
/// `tungstenite` requires `Read + Write` on its stream type so it can queue
/// automatic Pong/Close replies during `read()`. Discarding writes on the
/// reader handle and forwarding `Ping`/`Close` frames over the shared outbound
/// channel ensures only the writer thread writes to the TCP socket.
struct ReaderStream(StdTcpStream);

impl io::Read for ReaderStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.0.read(buf)
    }
}

impl io::Write for ReaderStream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Packets received from the server, one per binary message.
pub type ClientStream = Pin<Box<dyn Stream<Item = io::Result<Bytes>> + Send + Sync>>;

/// Packets sent to the server, one per binary message. Dropping the sink
/// closes the WebSocket.
pub type ClientSink = Pin<Box<dyn Sink<Bytes, Error = io::Error> + Send + Sync>>;

#[derive(Debug, Error)]
pub enum ClientError {
    #[error("IO error: {0}")]
    Io(#[from] io::Error),
    #[error("HTTP parse error: {0}")]
    HttpParse(#[from] httparse::Error),
    #[error("Server rejected the WebSocket upgrade with HTTP status {0}")]
    Rejected(u16),
    #[error("Missing or invalid Sec-WebSocket-Accept header")]
    InvalidAccept,
    #[error("Handshake response exceeds {MAX_HTTP_RESPONSE_SIZE} bytes")]
    ResponseTooLarge,
}

/// Builds the `/v1/websocket/<kind>?...` request path and query string for
/// `chip_info` and `device_guid`, matching the query parameters parsed by
/// [`crate::server`].
pub fn build_packet_stream_path(chip_info: &ChipInfo, device_guid: &str) -> String {
    let kind = chip_info.chip_kind().as_str().to_ascii_lowercase();
    let mut query = form_urlencoded::Serializer::new(String::new());
    if !device_guid.is_empty() {
        query.append_pair("device_guid", device_guid);
    }
    let device_name = chip_info
        .device_info
        .as_ref()
        .map(|d| d.name.as_str())
        .filter(|s| !s.is_empty())
        .unwrap_or(chip_info.name.as_str());
    if !device_name.is_empty() {
        query.append_pair("name", device_name);
    }
    if let Some(info) = &chip_info.device_info {
        for (key, value) in [
            ("device_kind", info.kind.as_str()),
            ("version", info.version.as_str()),
            ("sdk_version", info.sdk_version.as_str()),
            ("build_id", info.build_id.as_str()),
            ("variant", info.variant.as_str()),
            ("arch", info.arch.as_str()),
            ("avd_path", info.avd_path.as_str()),
        ] {
            if !value.is_empty() {
                query.append_pair(key, value);
            }
        }
    }
    if let Some(chip) = &chip_info.chip {
        let chip_id = if !chip.id.is_empty() { chip.id.as_str() } else { chip.name.as_str() };
        for (key, value) in [
            ("chip_id", chip_id),
            ("manufacturer", chip.manufacturer.as_str()),
            ("product_name", chip.product_name.as_str()),
            ("address", chip.address.as_str()),
        ] {
            if !value.is_empty() {
                query.append_pair(key, value);
            }
        }
        if chip.loopback {
            query.append_pair("loopback", "1");
        }
    }
    let query_str = query.finish();
    if query_str.is_empty() {
        format!("/v1/websocket/{kind}")
    } else {
        format!("/v1/websocket/{kind}?{query_str}")
    }
}

/// Opens a WebSocket packet stream to `addr` (`host:port`) at
/// `path_and_query` (for example `/v1/websocket/bt?name=cvd-1`).
///
/// Ref: RFC 6455, Section 4.1 - Client Requirements
pub async fn connect(
    addr: &str,
    path_and_query: &str,
) -> Result<(ClientStream, ClientSink), ClientError> {
    let mut stream = TcpStream::connect(addr).await?;
    let key = generate_websocket_key()?;
    let request = format!(
        "GET {path_and_query} HTTP/1.1\r\n\
         Host: {addr}\r\n\
         Upgrade: websocket\r\n\
         Connection: Upgrade\r\n\
         Sec-WebSocket-Key: {key}\r\n\
         Sec-WebSocket-Version: 13\r\n\r\n"
    );
    stream.write_all(request.as_bytes()).await?;
    let leftover = read_handshake_response(&mut stream, &key).await?;
    info!("WebSocket packet stream connected to {addr}{path_and_query}");

    // tungstenite is blocking, so the reader and writer each get a blocking
    // thread. Only the writer thread writes to the TCP socket; the reader uses
    // ReaderStream and forwards Pong/Close replies over outbound_tx.
    let std_stream = stream.into_std()?;
    std_stream.set_nonblocking(false)?;
    std_stream.set_read_timeout(Some(READ_POLL_TIMEOUT))?;
    std_stream.set_write_timeout(Some(WRITE_TIMEOUT))?;
    let reader = WebSocket::from_partially_read(
        ReaderStream(std_stream.try_clone()?),
        leftover,
        Role::Client,
        None,
    );
    let writer = WebSocket::from_raw_socket(std_stream, Role::Client, None);
    let (outbound_tx, outbound_rx) = mpsc::channel::<Message>(CHANNEL_CAPACITY);
    Ok((
        spawn_reader(reader, outbound_tx.downgrade()),
        spawn_writer(writer, outbound_tx, outbound_rx),
    ))
}

/// Ref: RFC 6455, Section 4.1 - a randomly selected 16-byte nonce, base64
/// encoded.
pub(crate) fn generate_websocket_key() -> Result<String, ClientError> {
    let mut nonce = [0u8; 16];
    SystemRandom::new().fill(&mut nonce).map_err(|e| io::Error::other(e.to_string()))?;
    Ok(base64::engine::general_purpose::STANDARD.encode(nonce))
}

/// Reads the server's opening handshake response and checks the upgrade.
///
/// Returns any bytes read past the response; they already belong to the
/// WebSocket stream.
pub(crate) async fn read_handshake_response(
    stream: &mut TcpStream,
    key: &str,
) -> Result<Vec<u8>, ClientError> {
    tokio::time::timeout(HANDSHAKE_TIMEOUT, async {
        let mut buffer = BytesMut::with_capacity(MAX_HTTP_RESPONSE_SIZE);
        loop {
            if stream.read_buf(&mut buffer).await? == 0 {
                return Err(io::Error::from(io::ErrorKind::UnexpectedEof).into());
            }
            let mut headers = [httparse::EMPTY_HEADER; MAX_RESPONSE_HEADERS];
            let mut response = httparse::Response::new(&mut headers);
            match response.parse(&buffer)? {
                httparse::Status::Complete(len) => {
                    verify_upgrade(&response, key)?;
                    return Ok(buffer.split_off(len).to_vec());
                }
                httparse::Status::Partial if buffer.len() >= MAX_HTTP_RESPONSE_SIZE => {
                    return Err(ClientError::ResponseTooLarge);
                }
                httparse::Status::Partial => {}
            }
        }
    })
    .await
    .map_err(|e| io::Error::new(io::ErrorKind::TimedOut, e))?
}

/// Ref: RFC 6455, Section 4.1 - the client must fail the connection unless
/// the status is 101, `Upgrade` is `websocket`, `Connection` includes
/// `Upgrade`, and `Sec-WebSocket-Accept` matches the key.
fn verify_upgrade(response: &httparse::Response, key: &str) -> Result<(), ClientError> {
    let code = response.code.unwrap_or_default();
    if code != StatusCode::SWITCHING_PROTOCOLS.as_u16() {
        return Err(ClientError::Rejected(code));
    }
    if !header_contains_token(response.headers, UPGRADE.as_str(), "websocket")
        || !header_contains_token(response.headers, CONNECTION.as_str(), "upgrade")
    {
        return Err(ClientError::InvalidAccept);
    }
    let expected = generate_websocket_accept(key);
    response
        .headers
        .iter()
        .any(|header| {
            header.name.eq_ignore_ascii_case(SEC_WEBSOCKET_ACCEPT.as_str())
                && std::str::from_utf8(header.value).map(str::trim).is_ok_and(|val| val == expected)
        })
        .then_some(())
        .ok_or(ClientError::InvalidAccept)
}

fn header_contains_token(headers: &[httparse::Header], name: &str, token: &str) -> bool {
    headers.iter().filter(|h| h.name.eq_ignore_ascii_case(name)).any(|h| {
        std::str::from_utf8(h.value)
            .is_ok_and(|val| val.split(',').any(|part| part.trim().eq_ignore_ascii_case(token)))
    })
}

/// Reads binary messages on a blocking thread until the server closes or the
/// stream receiver is dropped.
fn spawn_reader(
    mut reader: WebSocket<ReaderStream>,
    outbound_tx: mpsc::WeakSender<Message>,
) -> ClientStream {
    let (tx, rx) = mpsc::channel(CHANNEL_CAPACITY);
    tokio::task::spawn_blocking(move || {
        loop {
            // Ref: RFC 6455, Section 5.2 - Base Framing Protocol
            let packet = match reader.read() {
                Ok(Message::Binary(data)) => Ok(data),
                Ok(Message::Ping(data)) => {
                    if let Some(outbound_tx) = outbound_tx.upgrade() {
                        let _ = outbound_tx.blocking_send(Message::Pong(data));
                    }
                    continue;
                }
                Ok(Message::Close(frame)) => {
                    if let Some(outbound_tx) = outbound_tx.upgrade() {
                        let _ = outbound_tx.blocking_send(Message::Close(frame));
                    }
                    break;
                }
                Ok(_) => continue,
                Err(tungstenite::Error::Io(ref e))
                    if matches!(e.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut) =>
                {
                    if tx.is_closed() {
                        break;
                    }
                    continue;
                }
                Err(e) => Err(io::Error::other(e)),
            };
            let failed = packet.is_err();
            if tx.blocking_send(packet).is_err() || failed {
                break;
            }
        }
    });
    Box::pin(ReceiverStream::new(rx))
}

/// Writes outbound frames on a single blocking thread and sends a Close frame
/// once the sink is dropped.
fn spawn_writer(
    mut writer: WebSocket<StdTcpStream>,
    outbound_tx: mpsc::Sender<Message>,
    mut outbound_rx: mpsc::Receiver<Message>,
) -> ClientSink {
    tokio::task::spawn_blocking(move || {
        let mut close_sent = false;
        while let Some(msg) = outbound_rx.blocking_recv() {
            let is_close = matches!(msg, Message::Close(_));
            if let Err(error) = writer.send(msg) {
                warn!(%error, "WebSocket packet stream write error");
                return;
            }
            if is_close {
                close_sent = true;
                break;
            }
        }
        if !close_sent {
            // Ref: RFC 6455, Section 5.5.1 - Send Close frame without calling
            // writer.close(), which would read from the socket and race with
            // spawn_reader.
            let _ = writer.send(Message::Close(None));
        }
        let _ = writer.flush();
    });
    Box::pin(unfold(outbound_tx, |tx, packet: Bytes| async move {
        tx.send(Message::Binary(packet))
            .await
            .map_err(|e| io::Error::new(io::ErrorKind::BrokenPipe, e))?;
        Ok(tx)
    }))
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc,
        atomic::{AtomicU32, Ordering},
    };

    use actor_framework::MockActorClient;
    use device_actor::{DeviceActor, DeviceClient};
    use device_api::{DeviceAction, DeviceActionResult, DeviceId};
    use futures::{SinkExt, StreamExt};
    use netsim_model::{ChipId, ChipKind};
    use tokio::net::TcpListener;

    use super::*;

    /// Sample nonce and accept value from RFC 6455, Section 1.3.
    const RFC_KEY: &str = "dGhlIHNhbXBsZSBub25jZQ==";
    const RFC_ACCEPT: &str = "s3pPLMBiTxaQ9kYGzzhZRbK+xOo=";

    fn parse_and_verify(raw: &[u8]) -> Result<(), ClientError> {
        let mut headers = [httparse::EMPTY_HEADER; MAX_RESPONSE_HEADERS];
        let mut response = httparse::Response::new(&mut headers);
        assert!(response.parse(raw).unwrap().is_complete());
        verify_upgrade(&response, RFC_KEY)
    }

    #[test]
    fn test_verify_upgrade_accepts_matching_accept_header() {
        let raw = format!(
            "HTTP/1.1 101 Switching Protocols\r\n\
             Upgrade: websocket\r\n\
             Connection: Upgrade\r\n\
             sec-websocket-accept:  {RFC_ACCEPT} \r\n\r\n"
        );
        assert!(parse_and_verify(raw.as_bytes()).is_ok());
    }

    #[test]
    fn test_verify_upgrade_rejects_wrong_accept_header() {
        let raw = b"HTTP/1.1 101 Switching Protocols\r\n\
                    Upgrade: websocket\r\n\
                    Connection: Upgrade\r\n\
                    Sec-WebSocket-Accept: AAAAAAAAAAAAAAAAAAAAAAAAAAA=\r\n\r\n";
        assert!(matches!(parse_and_verify(raw), Err(ClientError::InvalidAccept)));
    }

    #[test]
    fn test_verify_upgrade_rejects_missing_accept_header() {
        let raw = b"HTTP/1.1 101 Switching Protocols\r\n\
                    Upgrade: websocket\r\n\
                    Connection: Upgrade\r\n\r\n";
        assert!(matches!(parse_and_verify(raw), Err(ClientError::InvalidAccept)));
    }

    #[test]
    fn test_verify_upgrade_rejects_missing_upgrade_or_connection_header() {
        let missing_upgrade = format!(
            "HTTP/1.1 101 Switching Protocols\r\n\
             Connection: Upgrade\r\n\
             Sec-WebSocket-Accept: {RFC_ACCEPT}\r\n\r\n"
        );
        assert!(matches!(
            parse_and_verify(missing_upgrade.as_bytes()),
            Err(ClientError::InvalidAccept)
        ));

        let missing_connection = format!(
            "HTTP/1.1 101 Switching Protocols\r\n\
             Upgrade: websocket\r\n\
             Sec-WebSocket-Accept: {RFC_ACCEPT}\r\n\r\n"
        );
        assert!(matches!(
            parse_and_verify(missing_connection.as_bytes()),
            Err(ClientError::InvalidAccept)
        ));
    }

    #[test]
    fn test_verify_upgrade_rejects_non_101_status() {
        let raw = b"HTTP/1.1 404 Not Found\r\nConnection: close\r\n\r\n";
        assert!(matches!(parse_and_verify(raw), Err(ClientError::Rejected(404))));
    }

    #[test]
    fn test_generate_websocket_key_is_random_16_byte_nonce() {
        let key = generate_websocket_key().unwrap();
        let nonce = base64::engine::general_purpose::STANDARD.decode(&key).unwrap();
        assert_eq!(nonce.len(), 16);
        assert_ne!(key, generate_websocket_key().unwrap());
    }

    #[test]
    fn test_build_packet_stream_path_encodes_device_and_chip_metadata() {
        let mut chip_info = ChipInfo::new("Pixel 9", ChipKind::BLUETOOTH);
        if let Some(info) = &mut chip_info.device_info {
            info.kind = "CUTTLEFISH".to_string();
            info.sdk_version = "36".to_string();
            info.avd_path = "/tmp/cvd 1".to_string();
        }
        if let Some(chip) = &mut chip_info.chip {
            chip.id = "BLUETOOTH".to_string();
            chip.address = "02:00:00:00:00:01".to_string();
            chip.loopback = true;
        }
        assert_eq!(
            build_packet_stream_path(&chip_info, "dualfd-Pixel 9"),
            "/v1/websocket/bluetooth?device_guid=dualfd-Pixel+9&name=Pixel+9\
             &device_kind=CUTTLEFISH&sdk_version=36&avd_path=%2Ftmp%2Fcvd+1\
             &chip_id=BLUETOOTH&manufacturer=Netsim&product_name=Virtual+Chip\
             &address=02%3A00%3A00%3A00%3A00%3A01&loopback=1"
        );
    }

    #[tokio::test]
    async fn test_connect_loopback_echo_and_clean_close() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let (_runner, device_client) = device_actor::new();
        tokio::spawn(crate::server::run(listener, device_client));

        let (mut stream, mut sink) = connect(&addr, "/v1/websocket/bt?loopback=1").await.unwrap();
        for payload in
            [Bytes::from_static(&[0x01, 0x03, 0x0c, 0x00]), Bytes::from(vec![0xAB; 70_000])]
        {
            sink.send(payload.clone()).await.unwrap();
            assert_eq!(stream.next().await.unwrap().unwrap(), payload);
        }

        drop(sink);
        assert!(stream.next().await.is_none());
    }

    #[tokio::test]
    async fn test_connect_rejected_on_unknown_route() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let (_runner, device_client) = device_actor::new();
        tokio::spawn(crate::server::run(listener, device_client));

        let result = connect(&addr, "/v1/websocket/invalid").await;
        assert!(matches!(result, Err(ClientError::Rejected(404))));
    }

    #[tokio::test]
    async fn test_connect_registered_chip_round_trip_and_cleanup() {
        let (add_tx, mut add_rx) = mpsc::channel(4);
        let (del_tx, mut del_rx) = mpsc::channel(4);
        let next_id = Arc::new(AtomicU32::new(42));

        let mut mock = MockActorClient::<DeviceActor>::new();
        mock.expect_clone_box().returning(move || {
            let mut inner = MockActorClient::<DeviceActor>::new();
            let add_tx = add_tx.clone();
            let del_tx = del_tx.clone();
            let next_id = Arc::clone(&next_id);
            inner.expect_perform_action().returning(move |_, action| match action {
                DeviceAction::AddChipByGuid { params } => {
                    let device_id = DeviceId(next_id.fetch_add(1, Ordering::SeqCst));
                    let chip_id = ChipId(7);
                    let _ = add_tx.try_send((params, device_id, chip_id));
                    Ok(DeviceActionResult::AddChipByGuidSuccess { device_id, chip_id })
                }
                DeviceAction::NotifyChipRemoved(device_id, chip_id) => {
                    let _ = del_tx.try_send((device_id, chip_id));
                    Ok(DeviceActionResult::Success)
                }
                _ => panic!("unexpected action"),
            });
            Box::new(inner)
        });

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        tokio::spawn(crate::server::run(listener, DeviceClient::new(Box::new(mock))));

        let mut chip_info = ChipInfo::new("cvd-1", ChipKind::WIFI);
        if let Some(info) = &mut chip_info.device_info {
            info.kind = "CUTTLEFISH".to_string();
        }
        let path = build_packet_stream_path(&chip_info, "dualfd-cvd-1");
        let (mut client_stream, mut client_sink) = connect(&addr, &path).await.unwrap();

        let (mut add_req, device_id, chip_id) = add_rx.recv().await.unwrap();
        assert_eq!(add_req.device_guid, "dualfd-cvd-1");
        assert_eq!(add_req.device_config.name, "cvd-1");
        assert_eq!(add_req.chip.kind, ChipKind::WIFI);

        let mut backend_stream = add_req.packet_stream.take().unwrap();
        let mut backend_sink = add_req.packet_sink.take().unwrap();

        let outbound = Bytes::from_static(&[0x10, 0x20, 0x30]);
        client_sink.send(outbound.clone()).await.unwrap();
        assert_eq!(backend_stream.next().await.unwrap(), outbound);

        let inbound = Bytes::from_static(&[0x40, 0x50]);
        backend_sink.send(inbound.clone()).await.unwrap();
        assert_eq!(client_stream.next().await.unwrap().unwrap(), inbound);

        drop(client_sink);
        drop(client_stream);
        assert_eq!(del_rx.recv().await, Some((device_id, chip_id)));
    }
}
