// Copyright 2025 The Android Open Source Project
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
//=============================================================================
// tests/test_connection_lifecycle.rs - Integration tests for connection
// lifecycle
//=============================================================================

use bytes::Bytes;
use futures::{SinkExt, StreamExt};
use packet_stream::{Chip, ChipInfo, ChipKind, DeviceInfo, Streams, TransportType};
use tokio::{
    net::TcpStream,
    time::{Duration, timeout},
};

/// Helper to create test chip info
fn create_test_chip_info() -> ChipInfo {
    ChipInfo {
        name: "test-chip".to_string(),
        chip: Some(Chip {
            kind: ChipKind::WIFI,
            id: "test-lifecycle-chip".to_string(),
            name: "Test Lifecycle Chip v1.0".to_string(),
            manufacturer: "Test Lifecycle Corp".to_string(),
            product_name: "Test Lifecycle Chip v1.0".to_string(),
            address: "".to_string(),
            sim_type: None,
            sim_profile: None,
            loopback: false,
            bt_properties: None,
        }),
        device_info: Some(DeviceInfo {
            name: "test-lifecycle-device".to_string(),
            id: "test-lifecycle-device".to_string(),
            avd_path: "".to_string(),
            ..Default::default()
        }),
    }
}

#[tokio::test]
async fn test_graceful_client_disconnect() {
    let mut streams = Streams::new();
    streams.start_listener("tcp", TransportType::tcp("localhost", 0)).await.unwrap();
    let listener_addr = streams.listener_address("tcp").cloned().unwrap();
    let port = listener_addr.to_string().split(':').last().unwrap().parse::<u16>().unwrap();

    let chip_info = create_test_chip_info();
    let (_client_stream, mut client_sink) = timeout(
        Duration::from_secs(5),
        streams.connect(TransportType::tcp("localhost", port), chip_info),
    )
    .await
    .unwrap()
    .unwrap();

    let (_name, (mut server_stream, _, _, _guid)) =
        timeout(Duration::from_secs(5), streams.accept_any()).await.unwrap().unwrap();

    let test_data = b"Connection established";
    client_sink.send(Bytes::from_static(test_data)).await.unwrap();

    let received =
        timeout(Duration::from_secs(5), server_stream.next()).await.unwrap().unwrap().unwrap();
    assert_eq!(received, &test_data[..]);

    client_sink.close().await.unwrap();

    let server_recv_result = timeout(Duration::from_secs(3), server_stream.next()).await;
    assert!(server_recv_result.is_err() || server_recv_result.unwrap().is_none());
}

#[tokio::test]
async fn test_abrupt_client_disconnect() {
    let mut streams = Streams::new();
    streams.start_listener("tcp", TransportType::tcp("localhost", 0)).await.unwrap();
    let listener_addr = streams.listener_address("tcp").cloned().unwrap();
    let port = listener_addr.to_string().split(':').last().unwrap().parse::<u16>().unwrap();

    let chip_info = create_test_chip_info();
    let (client_stream, mut client_sink) = timeout(
        Duration::from_secs(5),
        streams.connect(TransportType::tcp("localhost", port), chip_info),
    )
    .await
    .unwrap()
    .unwrap();

    let (_name, (mut server_stream, _, _, _guid)) =
        timeout(Duration::from_secs(5), streams.accept_any()).await.unwrap().unwrap();

    let test_data = b"Before disconnect";
    client_sink.send(Bytes::from_static(test_data)).await.unwrap();

    let received =
        timeout(Duration::from_secs(5), server_stream.next()).await.unwrap().unwrap().unwrap();
    assert_eq!(received, &test_data[..]);

    drop(client_sink);
    drop(client_stream);

    tokio::time::sleep(Duration::from_millis(100)).await;

    let server_recv_result = timeout(Duration::from_secs(3), server_stream.next()).await;
    assert!(server_recv_result.is_err() || server_recv_result.unwrap().is_none());
}

#[tokio::test]
async fn test_raw_socket_disconnect() {
    let mut streams = Streams::new();
    streams.start_listener("tcp", TransportType::tcp("localhost", 0)).await.unwrap();
    let listener_addr = streams.listener_address("tcp").cloned().unwrap();
    let port = listener_addr.to_string().split(':').last().unwrap().parse::<u16>().unwrap();

    let raw_stream =
        timeout(Duration::from_secs(5), TcpStream::connect(format!("localhost:{}", port)))
            .await
            .unwrap()
            .unwrap();
    drop(raw_stream);

    let accept_result = timeout(Duration::from_secs(3), streams.accept_any()).await;
    assert!(accept_result.is_err() || accept_result.unwrap().is_err());
}

#[tokio::test]
async fn test_multiple_client_disconnect() {
    let mut streams = Streams::new();
    streams.start_listener("tcp", TransportType::tcp("localhost", 0)).await.unwrap();
    let listener_addr = streams.listener_address("tcp").cloned().unwrap();
    let port = listener_addr.to_string().split(':').last().unwrap().parse::<u16>().unwrap();

    let chip_info1 = create_test_chip_info();
    let chip_info2 = create_test_chip_info();

    let (client1_stream, mut client1_sink) = timeout(
        Duration::from_secs(5),
        streams.connect(TransportType::tcp("localhost", port), chip_info1),
    )
    .await
    .unwrap()
    .unwrap();

    let (_client2_stream, mut client2_sink) = timeout(
        Duration::from_secs(5),
        streams.connect(TransportType::tcp("localhost", port), chip_info2),
    )
    .await
    .unwrap()
    .unwrap();

    let (_name1, (mut server1_stream, _, _, _guid1)) =
        timeout(Duration::from_secs(5), streams.accept_any()).await.unwrap().unwrap();
    let (_name2, (mut server2_stream, _, _, _guid2)) =
        timeout(Duration::from_secs(5), streams.accept_any()).await.unwrap().unwrap();

    client1_sink.send(Bytes::from_static(b"Client 1 message")).await.unwrap();
    client2_sink.send(Bytes::from_static(b"Client 2 message")).await.unwrap();

    let msg1 =
        timeout(Duration::from_secs(5), server1_stream.next()).await.unwrap().unwrap().unwrap();
    let msg2 =
        timeout(Duration::from_secs(5), server2_stream.next()).await.unwrap().unwrap().unwrap();

    assert_eq!(msg1, &b"Client 1 message"[..]);
    assert_eq!(msg2, &b"Client 2 message"[..]);

    drop(client1_sink);
    drop(client1_stream);

    client2_sink.send(Bytes::from_static(b"Client 2 still works")).await.unwrap();

    let msg2_after =
        timeout(Duration::from_secs(5), server2_stream.next()).await.unwrap().unwrap().unwrap();
    assert_eq!(msg2_after, &b"Client 2 still works"[..]);

    let server1_result = timeout(Duration::from_secs(3), server1_stream.next()).await;
    assert!(server1_result.is_err() || server1_result.unwrap().is_none());
}

#[tokio::test]
async fn test_listener_shutdown() {
    let mut streams = Streams::new();
    streams.start_listener("tcp", TransportType::tcp("localhost", 0)).await.unwrap();
    let listener_addr = streams.listener_address("tcp").cloned().unwrap();
    let port = listener_addr.to_string().split(':').last().unwrap().parse::<u16>().unwrap();

    let chip_info = create_test_chip_info();
    let _ = timeout(
        Duration::from_secs(5),
        streams.connect(TransportType::tcp("localhost", port), chip_info),
    )
    .await
    .unwrap()
    .unwrap();

    let (_name, _) = timeout(Duration::from_secs(5), streams.accept_any()).await.unwrap().unwrap();

    streams.shutdown_all().await.unwrap();

    let chip_info2 = create_test_chip_info();
    let connect_result = timeout(
        Duration::from_secs(3),
        streams.connect(TransportType::tcp("localhost", port), chip_info2),
    )
    .await;
    assert!(connect_result.is_err() || connect_result.unwrap().is_err());
}

#[tokio::test]
async fn test_connection_resource_cleanup() {
    let mut streams = Streams::new();
    streams.start_listener("tcp", TransportType::tcp("localhost", 0)).await.unwrap();
    let listener_addr = streams.listener_address("tcp").cloned().unwrap();
    let port = listener_addr.to_string().split(':').last().unwrap().parse::<u16>().unwrap();

    for _ in 0..5 {
        let chip_info = create_test_chip_info();
        let (client_stream, client_sink) = timeout(
            Duration::from_secs(5),
            streams.connect(TransportType::tcp("localhost", port), chip_info),
        )
        .await
        .unwrap()
        .unwrap();

        let (_name, (server_stream, server_sink, _, _guid)) =
            timeout(Duration::from_secs(5), streams.accept_any()).await.unwrap().unwrap();

        drop(client_stream);
        drop(client_sink);
        drop(server_stream);
        drop(server_sink);

        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    let chip_info = create_test_chip_info();
    let _ = timeout(
        Duration::from_secs(5),
        streams.connect(TransportType::tcp("localhost", port), chip_info),
    )
    .await
    .unwrap()
    .unwrap();

    let (_name, _) = timeout(Duration::from_secs(5), streams.accept_any()).await.unwrap().unwrap();
}
