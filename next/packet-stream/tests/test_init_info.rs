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
// tests/test_init_info.rs - Integration tests for init_info protocol handling
//=============================================================================

use bytes::Bytes;
use futures::SinkExt;
use packet_stream::{Chip, ChipInfo, ChipKind, DeviceInfo, Streams, TransportType};
use tokio::{
    io::AsyncWriteExt,
    net::TcpStream,
    time::{Duration, timeout},
};

/// Helper to create test chip info
fn create_test_chip_info() -> ChipInfo {
    ChipInfo {
        name: "test-chip".to_string(),
        chip: Some(Chip {
            kind: ChipKind::WIFI,
            id: "test-wifi-chip".to_string(),
            name: "Test WiFi Chip v1.0".to_string(),
            manufacturer: "Test WiFi Corp".to_string(),
            product_name: "Test WiFi Chip v1.0".to_string(),
            address: "".to_string(),
            sim_type: None,
            sim_profile: None,
            loopback: false,
        }),
        device_info: Some(DeviceInfo {
            name: "test-init-info-device".to_string(),
            id: "test-init-info-device".to_string(),
            avd_path: "".to_string(),
            ..Default::default()
        }),
    }
}

#[tokio::test]
async fn test_normal_init_info_protocol() {
    let mut streams = Streams::new();
    streams.start_listener("tcp", TransportType::tcp("localhost", 0)).await.unwrap();
    let listener_addr = streams.listener_address("tcp").cloned().unwrap();
    let port = listener_addr.to_string().split(':').last().unwrap().parse::<u16>().unwrap();

    let chip_info = create_test_chip_info();
    let (_, mut client_sink) = timeout(
        Duration::from_secs(5),
        streams.connect(TransportType::tcp("localhost", port), chip_info.clone()),
    )
    .await
    .unwrap()
    .unwrap();

    let (_name, (_server_stream, _server_sink, chip_info, _guid)) =
        streams.accept_any().await.unwrap();

    assert_eq!(chip_info.device_info.as_ref().unwrap().name, "test-init-info-device");
    assert_eq!(chip_info.chip.as_ref().unwrap().manufacturer, "Test WiFi Corp");

    let test_data = b"Post-init-info packet";
    client_sink.send(Bytes::from_static(test_data)).await.unwrap();
}

#[tokio::test]
async fn test_malformed_init_info_handling() {
    let mut streams = Streams::new();
    streams.start_listener("tcp", TransportType::tcp("localhost", 0)).await.unwrap();
    let listener_addr = streams.listener_address("tcp").cloned().unwrap();
    let port = listener_addr.to_string().split(':').last().unwrap().parse::<u16>().unwrap();

    let mut raw_stream =
        timeout(Duration::from_secs(5), TcpStream::connect(format!("localhost:{}", port)))
            .await
            .unwrap()
            .unwrap();

    let malformed_data = b"INVALID JSON DATA";
    let len = malformed_data.len() as u32;
    raw_stream.write_all(&len.to_ne_bytes()).await.unwrap();
    raw_stream.write_all(malformed_data).await.unwrap();

    let accept_result = timeout(Duration::from_secs(3), streams.accept_any()).await;
    assert!(accept_result.is_err() || accept_result.unwrap().is_err());
}

#[tokio::test]
async fn test_missing_device_info() {
    let mut streams = Streams::new();
    streams.start_listener("tcp", TransportType::tcp("localhost", 0)).await.unwrap();
    let listener_addr = streams.listener_address("tcp").cloned().unwrap();
    let port = listener_addr.to_string().split(':').last().unwrap().parse::<u16>().unwrap();

    let mut incomplete_chip_info = create_test_chip_info();
    incomplete_chip_info.device_info = None;

    let connect_result = timeout(
        Duration::from_secs(3),
        streams.connect(TransportType::tcp("localhost", port), incomplete_chip_info),
    )
    .await;
    assert!(connect_result.is_ok());
}

#[tokio::test]
async fn test_missing_chip_info() {
    let mut streams = Streams::new();
    streams.start_listener("tcp", TransportType::tcp("localhost", 0)).await.unwrap();
    let listener_addr = streams.listener_address("tcp").cloned().unwrap();
    let port = listener_addr.to_string().split(':').last().unwrap().parse::<u16>().unwrap();

    let mut incomplete_chip_info = create_test_chip_info();
    incomplete_chip_info.chip = None;

    let connect_result = timeout(
        Duration::from_secs(3),
        streams.connect(TransportType::tcp("localhost", port), incomplete_chip_info),
    )
    .await;
    assert!(connect_result.is_ok());
}

#[tokio::test]
async fn test_init_info_timeout_handling() {
    let mut streams = Streams::new();
    streams.start_listener("tcp", TransportType::tcp("localhost", 0)).await.unwrap();
    let listener_addr = streams.listener_address("tcp").cloned().unwrap();
    let port = listener_addr.to_string().split(':').last().unwrap().parse::<u16>().unwrap();

    let _raw_stream =
        timeout(Duration::from_secs(5), TcpStream::connect(format!("localhost:{}", port)))
            .await
            .unwrap()
            .unwrap();

    let accept_result = timeout(Duration::from_secs(3), streams.accept_any()).await;
    assert!(accept_result.is_err() || accept_result.unwrap().is_err());
}

#[tokio::test]
async fn test_partial_init_info_data() {
    let mut streams = Streams::new();
    streams.start_listener("tcp", TransportType::tcp("localhost", 0)).await.unwrap();
    let listener_addr = streams.listener_address("tcp").cloned().unwrap();
    let port = listener_addr.to_string().split(':').last().unwrap().parse::<u16>().unwrap();

    let mut raw_stream =
        timeout(Duration::from_secs(5), TcpStream::connect(format!("localhost:{}", port)))
            .await
            .unwrap()
            .unwrap();

    let full_data = b"{\"chip\":{\"kind\":\"WIFI\"}}";
    let full_len = full_data.len() as u32;
    let partial_data = &full_data[..5];

    raw_stream.write_all(&full_len.to_ne_bytes()).await.unwrap();
    raw_stream.write_all(partial_data).await.unwrap();

    let accept_result = timeout(Duration::from_secs(3), streams.accept_any()).await;
    assert!(accept_result.is_err() || accept_result.unwrap().is_err());
}

#[tokio::test]
async fn test_oversized_init_info() {
    let mut streams = Streams::new();
    streams.start_listener("tcp", TransportType::tcp("localhost", 0)).await.unwrap();
    let listener_addr = streams.listener_address("tcp").cloned().unwrap();
    let port = listener_addr.to_string().split(':').last().unwrap().parse::<u16>().unwrap();

    let mut raw_stream =
        timeout(Duration::from_secs(5), TcpStream::connect(format!("localhost:{}", port)))
            .await
            .unwrap()
            .unwrap();

    let oversized_len = 100_000_000u32;
    raw_stream.write_all(&oversized_len.to_ne_bytes()).await.unwrap();

    let small_data = b"small data";
    raw_stream.write_all(small_data).await.unwrap();

    let accept_result = timeout(Duration::from_secs(3), streams.accept_any()).await;
    assert!(accept_result.is_err() || accept_result.unwrap().is_err());
}

#[tokio::test]
async fn test_empty_init_info() {
    let mut streams = Streams::new();
    streams.start_listener("tcp", TransportType::tcp("localhost", 0)).await.unwrap();
    let listener_addr = streams.listener_address("tcp").cloned().unwrap();
    let port = listener_addr.to_string().split(':').last().unwrap().parse::<u16>().unwrap();

    let mut raw_stream =
        timeout(Duration::from_secs(5), TcpStream::connect(format!("localhost:{}", port)))
            .await
            .unwrap()
            .unwrap();

    let zero_len = 0u32;
    raw_stream.write_all(&zero_len.to_ne_bytes()).await.unwrap();

    let accept_result = timeout(Duration::from_secs(3), streams.accept_any()).await;
    assert!(accept_result.is_err() || accept_result.unwrap().is_err());
}
