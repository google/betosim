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
// tests/integration_tests.rs - Integration tests for PacketStream core
// functionality
//=============================================================================

use bytes::Bytes;
use futures::{SinkExt, StreamExt};
use packet_stream::{Chip, ChipInfo, ChipKind, DeviceInfo, Streams, TransportType};
use tokio::time::{Duration, timeout};

/// Helper to create test chip info for integration tests
fn create_test_chip_info() -> ChipInfo {
    ChipInfo {
        name: "test-chip".to_string(),
        chip: Some(Chip {
            kind: ChipKind::WIFI,
            id: "test-chip".to_string(),
            name: "Test WiFi Chip".to_string(),
            manufacturer: "Test Corp".to_string(),
            product_name: "Test WiFi Chip".to_string(),
            address: "".to_string(),
            sim_type: None,
            sim_profile: None,
            loopback: false,
            bt_properties: None,
        }),
        device_info: Some(DeviceInfo {
            name: "test-device".to_string(),
            id: "test-device".to_string(),
            avd_path: "".to_string(),
            ..Default::default()
        }),
    }
}

#[tokio::test]
async fn test_tcp_transport_echo() {
    let mut streams = Streams::new();
    streams.start_listener("tcp", TransportType::tcp("localhost", 0)).await.unwrap();
    let listener_addr = streams.listener_address("tcp").cloned().unwrap();
    let port = listener_addr.to_string().split(':').last().unwrap().parse::<u16>().unwrap();

    let chip_info = create_test_chip_info();
    let (mut client_stream, mut client_sink) = timeout(
        Duration::from_secs(5),
        streams.connect(TransportType::tcp("localhost", port), chip_info),
    )
    .await
    .unwrap()
    .unwrap();

    let (_name, (mut server_stream, mut server_sink, _chip_info, _guid)) =
        streams.accept_any().await.unwrap();

    let test_data = b"Hello PacketStream!";
    client_sink.send(Bytes::from_static(test_data)).await.unwrap();

    let received =
        timeout(Duration::from_secs(5), server_stream.next()).await.unwrap().unwrap().unwrap();
    assert_eq!(received, &test_data[..]);

    server_sink.send(received.clone()).await.unwrap();

    let echoed =
        timeout(Duration::from_secs(5), client_stream.next()).await.unwrap().unwrap().unwrap();
    assert_eq!(echoed, &test_data[..]);
}

#[tokio::test]
async fn test_uds_transport_echo() {
    let mut streams = Streams::new();
    let socket_path = std::env::temp_dir().join("test_packet_stream.sock");
    streams.start_listener("uds", TransportType::uds(socket_path.to_string_lossy())).await.unwrap();

    let chip_info = create_test_chip_info();
    let (_client_stream, mut client_sink) = timeout(
        Duration::from_secs(5),
        streams.connect(TransportType::uds(socket_path.to_string_lossy()), chip_info),
    )
    .await
    .unwrap()
    .unwrap();

    let (_name, (mut server_stream, _server_sink, _chip_info, _guid)) =
        streams.accept_any().await.unwrap();

    let test_data = Bytes::from(&b"Zero-copy test!"[..]);
    client_sink.send(test_data.clone()).await.unwrap();

    let received =
        timeout(Duration::from_secs(5), server_stream.next()).await.unwrap().unwrap().unwrap();
    assert_eq!(received, &test_data[..]);
}

#[tokio::test]
async fn test_chip_info_protocol() {
    let mut streams = Streams::new();
    streams.start_listener("tcp", TransportType::tcp("localhost", 0)).await.unwrap();
    let listener_addr = streams.listener_address("tcp").cloned().unwrap();
    let port = listener_addr.to_string().split(':').last().unwrap().parse::<u16>().unwrap();

    let mut client_chip_info = create_test_chip_info();
    client_chip_info.device_info.as_mut().unwrap().name = "specific-test-device".to_string();
    client_chip_info.chip.as_mut().unwrap().manufacturer = "Specific Corp".to_string();

    let _ = timeout(
        Duration::from_secs(5),
        streams.connect(TransportType::tcp("localhost", port), client_chip_info.clone()),
    )
    .await
    .unwrap()
    .unwrap();

    let (_name, (_, _, received_chip_info, _guid)) =
        timeout(Duration::from_secs(5), streams.accept_any()).await.unwrap().unwrap();

    assert_eq!(received_chip_info.device_info.as_ref().unwrap().name, "specific-test-device");
    assert_eq!(received_chip_info.chip.as_ref().unwrap().manufacturer, "Specific Corp");
}

#[tokio::test]
async fn test_multiple_transports() {
    let mut streams = Streams::new();
    streams.start_listener("tcp", TransportType::tcp("localhost", 0)).await.unwrap();
    let socket_path = std::env::temp_dir().join("test_multi_transport.sock");
    streams.start_listener("uds", TransportType::uds(socket_path.to_string_lossy())).await.unwrap();

    let chip_info = create_test_chip_info();
    let listener_addr = streams.listener_address("tcp").cloned().unwrap();
    let port = listener_addr.to_string().split(':').last().unwrap().parse::<u16>().unwrap();

    let _ = timeout(
        Duration::from_secs(5),
        streams.connect(TransportType::tcp("localhost", port), chip_info.clone()),
    )
    .await
    .unwrap()
    .unwrap();

    let _ = timeout(
        Duration::from_secs(5),
        streams.connect(TransportType::uds(socket_path.to_string_lossy()), chip_info.clone()),
    )
    .await
    .unwrap()
    .unwrap();

    let (_name1, _) = timeout(Duration::from_secs(5), streams.accept_any()).await.unwrap().unwrap();
    let (_name2, _) = timeout(Duration::from_secs(5), streams.accept_any()).await.unwrap().unwrap();
}

#[tokio::test]
async fn test_error_handling() {
    let streams = Streams::new();
    let chip_info = create_test_chip_info();
    let result = timeout(
        Duration::from_secs(2),
        streams.connect(TransportType::tcp("localhost", 65534), chip_info),
    )
    .await;
    assert!(result.is_err() || result.unwrap().is_err());
}

#[tokio::test]
async fn test_concurrent_connections() {
    let mut streams = Streams::new();
    streams.start_listener("tcp", TransportType::tcp("localhost", 0)).await.unwrap();
    let listener_addr = streams.listener_address("tcp").cloned().unwrap();
    let port = listener_addr.to_string().split(':').last().unwrap().parse::<u16>().unwrap();

    let chip_info = create_test_chip_info();
    let _ = timeout(
        Duration::from_secs(5),
        streams.connect(TransportType::tcp("localhost", port), chip_info.clone()),
    )
    .await
    .unwrap()
    .unwrap();
    let _ = timeout(
        Duration::from_secs(5),
        streams.connect(TransportType::tcp("localhost", port), chip_info),
    )
    .await
    .unwrap()
    .unwrap();

    let (_name1, _) = timeout(Duration::from_secs(5), streams.accept_any()).await.unwrap().unwrap();
    let (_name2, _) = timeout(Duration::from_secs(5), streams.accept_any()).await.unwrap().unwrap();
}
