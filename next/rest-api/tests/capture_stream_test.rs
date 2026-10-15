// Copyright 2026 The Android Open Source Project
// SPDX-License-Identifier: Apache-2.0

//! End-to-end tests for the `/v1/capture` WebSocket over a real socket.

use std::{net::SocketAddr, sync::Arc, time::SystemTime};

use ap_actor::ApClient;
use bytes::Bytes;
use capture_actor::{CaptureActor, CaptureClient};
use capture_api::{CaptureCreate, CaptureSender, Direction};
use device_actor::DeviceClient;
use netsim_model::{ChipId, ChipKind};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    time::{Duration, timeout},
};

use crate::server_test::MockLinkClient;

const IO_TIMEOUT: Duration = Duration::from_secs(5);
/// Example key and accept value from RFC 6455, section 1.3.
const CLIENT_KEY: &str = "dGhlIHNhbXBsZSBub25jZQ==";
const EXPECTED_ACCEPT: &str = "s3pPLMBiTxaQ9kYGzzhZRbK+xOo=";
const PCAP_HEADER_LEN: usize = 24;
const PCAP_RECORD_HEADER_LEN: usize = 16;
const DLT_BLUETOOTH_H4: u32 = 187;

struct World {
    addr: SocketAddr,
    capture_client: CaptureClient,
    _tasks: Vec<tokio::task::JoinHandle<()>>,
}

impl World {
    /// Starts a capture actor and the REST server with one disabled Bluetooth
    /// capture on chip 1.
    async fn new() -> Self {
        let (runner, capture_client) = capture_actor::new();
        let capture_task = tokio::spawn(runner.run(CaptureActor::new(false, None)));
        CaptureSender::create_capture(
            &capture_client,
            ChipId(1),
            CaptureCreate {
                chip_kind: ChipKind::BLUETOOTH,
                device_name: "ws-test".to_string(),
                enabled_flag: Default::default(),
            },
        )
        .await
        .unwrap();

        let (device_tx, _) = tokio::sync::mpsc::channel(1);
        let device_client =
            DeviceClient::new(Box::new(actor_framework::ResourceClient::new(device_tx)));
        let (ap_tx, _) = tokio::sync::mpsc::channel(1);
        let ap_client = ApClient::new(actor_framework::ResourceClient::new(ap_tx));

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server_task = tokio::spawn(netsim_rest_api::server::run(
            listener,
            device_client,
            Arc::new(MockLinkClient),
            ap_client,
            capture_client.clone(),
            "test".to_string(),
        ));
        World { addr, capture_client, _tasks: vec![capture_task, server_task] }
    }

    /// Sends a request for `target` and returns the socket and response head.
    async fn request(&self, target: &str, upgrade: bool) -> (TcpStream, String) {
        let mut stream = TcpStream::connect(self.addr).await.unwrap();
        let upgrade_headers = if upgrade {
            format!(
                "Upgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Version: 13\r\n\
                 Sec-WebSocket-Key: {CLIENT_KEY}\r\n"
            )
        } else {
            String::new()
        };
        let request = format!("GET {target} HTTP/1.1\r\nHost: localhost\r\n{upgrade_headers}\r\n");
        stream.write_all(request.as_bytes()).await.unwrap();

        let mut head = Vec::new();
        while !head.ends_with(b"\r\n\r\n") {
            head.push(timeout(IO_TIMEOUT, stream.read_u8()).await.unwrap().unwrap());
        }
        (stream, String::from_utf8(head).unwrap())
    }

    async fn send_packet(&self, packet: &'static [u8]) {
        let tx = self.capture_client.packet_sender(ChipId(1)).await.unwrap();
        tx.send((SystemTime::now(), Direction::Sent, Bytes::from_static(packet))).unwrap();
    }
}

fn status_line(head: &str) -> &str {
    head.lines().next().unwrap_or_default()
}

/// Reads one server frame and returns (opcode, payload).
async fn read_frame(stream: &mut TcpStream) -> (u8, Vec<u8>) {
    timeout(IO_TIMEOUT, async {
        let b0 = stream.read_u8().await.unwrap();
        let b1 = stream.read_u8().await.unwrap();
        assert_eq!(b0 & 0x80, 0x80, "expected a final frame");
        assert_eq!(b1 & 0x80, 0, "server frames must not be masked");
        let len = match b1 & 0x7f {
            126 => u64::from(stream.read_u16().await.unwrap()),
            127 => stream.read_u64().await.unwrap(),
            len => u64::from(len),
        };
        let mut payload = vec![0; len as usize];
        stream.read_exact(&mut payload).await.unwrap();
        (b0 & 0x0f, payload)
    })
    .await
    .unwrap()
}

/// Writes one masked client frame with a short payload.
async fn write_client_frame(stream: &mut TcpStream, opcode: u8, payload: &[u8]) {
    assert!(payload.len() < 126);
    let mask = [0x12, 0x34, 0x56, 0x78];
    let mut frame = vec![0x80 | opcode, 0x80 | payload.len() as u8];
    frame.extend_from_slice(&mask);
    frame.extend(payload.iter().enumerate().map(|(i, b)| b ^ mask[i % 4]));
    stream.write_all(&frame).await.unwrap();
}

// Scenario: A client streams a chip's packets as PCAP while file capture is off
#[tokio::test]
async fn test_capture_stream_sends_pcap() {
    let world = World::new().await;

    // When: a client upgrades on /v1/capture
    let (mut stream, head) = world.request("/v1/capture?chip_id=1", true).await;

    // Then: the server switches protocols with the RFC 6455 accept key
    assert_eq!(status_line(&head), "HTTP/1.1 101 Switching Protocols");
    assert!(
        head.to_ascii_lowercase()
            .contains(&format!("sec-websocket-accept: {}", EXPECTED_ACCEPT.to_ascii_lowercase()))
    );

    // And: the first message is the PCAP global header with the H4 link type
    let (opcode, header) = read_frame(&mut stream).await;
    assert_eq!(opcode, 0x2);
    assert_eq!(header.len(), PCAP_HEADER_LEN);
    assert_eq!(&header[0..4], &0xa1b2c3d4u32.to_le_bytes());
    assert_eq!(&header[20..24], &DLT_BLUETOOTH_H4.to_le_bytes());

    // When: the chip sends a packet
    world.send_packet(&[0x01, 0x03, 0x0c, 0x00]).await;

    // Then: the next message is one PCAP record holding that packet
    let (opcode, record) = read_frame(&mut stream).await;
    assert_eq!(opcode, 0x2);
    let incl_len = u32::from_le_bytes(record[8..12].try_into().unwrap()) as usize;
    assert_eq!(record.len(), PCAP_RECORD_HEADER_LEN + incl_len);
    assert!(record.ends_with(&[0x01, 0x03, 0x0c, 0x00]));

    // And: file capture is untouched
    let info = world.capture_client.get_capture(ChipId(1)).await.unwrap().unwrap();
    assert!(!info.enabled);
    assert_eq!(info.records_written, 0);

    // When: the client pings
    write_client_frame(&mut stream, 0x9, b"hi").await;
    // Then: the server answers with a pong carrying the same payload
    assert_eq!(read_frame(&mut stream).await, (0xA, b"hi".to_vec()));

    // When: the client closes
    write_client_frame(&mut stream, 0x8, &1000u16.to_be_bytes()).await;
    // Then: the server echoes a close frame
    assert_eq!(read_frame(&mut stream).await.0, 0x8);
}

// Scenario: Deleting the capture closes the stream
#[tokio::test]
async fn test_capture_stream_closes_on_delete() {
    let world = World::new().await;
    let (mut stream, head) = world.request("/v1/capture?chip_id=1", true).await;
    assert_eq!(status_line(&head), "HTTP/1.1 101 Switching Protocols");
    let _header = read_frame(&mut stream).await;

    world.capture_client.delete_capture(ChipId(1)).await.unwrap();

    assert_eq!(read_frame(&mut stream).await, (0x8, 1000u16.to_be_bytes().to_vec()));
}

// Scenario: Bad requests get an HTTP error instead of an upgrade
#[tokio::test]
async fn test_capture_stream_errors() {
    let world = World::new().await;

    let (_, head) = world.request("/v1/capture?chip_id=99", true).await;
    assert_eq!(status_line(&head), "HTTP/1.1 404 Not Found");

    let (_, head) = world.request("/v1/capture", true).await;
    assert_eq!(status_line(&head), "HTTP/1.1 400 Bad Request");

    let (_, head) = world.request("/v1/capture?chip_id=1", false).await;
    assert_eq!(status_line(&head), "HTTP/1.1 400 Bad Request");
}
