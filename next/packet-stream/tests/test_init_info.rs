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

use crate::world::{World, create_chip_info};

// Scenario: Client exchanges init_info with device info and sends data
// Given a running TCP listener
// When a client connects with custom device and chip metadata
// And the connection is accepted
// Then the received metadata matches
// When the client sends post-init-info packet data
// Then the server receives the packet data
#[tokio::test]
async fn test_normal_init_info_protocol() {
    let mut world = World::new();
    world.given_tcp_listener("tcp").await;

    let mut chip_info = create_chip_info("test-chip");
    chip_info.device_info.as_mut().expect("device_info").name = "test-init-info-device".to_string();
    chip_info.chip.as_mut().expect("chip").manufacturer = "Test WiFi Corp".to_string();

    world.when_connect_client("client", "tcp", chip_info).await;
    world.when_accept_connection().await;

    world.then_init_info_matches("test-chip", "test-init-info-device", "Test WiFi Corp");

    let test_data = b"Post-init-info packet";
    world.when_send_packet_from_client("client", Bytes::from_static(test_data)).await;
    world.then_server_receives_packet("test-chip", test_data).await;
}

// Scenario: Malformed init_info JSON payload is rejected
// Given a running TCP listener
// When a raw client sends invalid JSON in the init_info header
// Then no connection is accepted
#[tokio::test]
async fn test_malformed_init_info_handling() {
    let mut world = World::new();
    world.given_tcp_listener("tcp").await;

    world.when_connect_raw_tcp("raw", "tcp").await;

    let malformed_data = b"INVALID JSON DATA";
    let len = malformed_data.len() as u32;
    let mut payload = Vec::new();
    payload.extend_from_slice(&len.to_be_bytes());
    payload.extend_from_slice(malformed_data);

    world.when_send_raw_tcp_bytes("raw", &payload).await;
    world.then_no_connection_accepted().await;
}

// Scenario: InitInfo with missing device_info connects successfully
// Given a running TCP listener
// When a client connects without device_info
// Then the connection handshake succeeds
#[tokio::test]
async fn test_missing_device_info() {
    let mut world = World::new();
    world.given_tcp_listener("tcp").await;

    let mut incomplete_chip_info = create_chip_info("test-chip");
    incomplete_chip_info.device_info = None;

    world.when_try_connect_client("tcp", incomplete_chip_info).await;
    world.then_connect_succeeded();
}

// Scenario: InitInfo with missing chip info connects successfully
// Given a running TCP listener
// When a client connects without chip info
// Then the connection handshake succeeds
#[tokio::test]
async fn test_missing_chip_info() {
    let mut world = World::new();
    world.given_tcp_listener("tcp").await;

    let mut incomplete_chip_info = create_chip_info("test-chip");
    incomplete_chip_info.chip = None;

    world.when_try_connect_client("tcp", incomplete_chip_info).await;
    world.then_connect_succeeded();
}

// Scenario: Client connects but sends no init_info data
// Given a running TCP listener
// When a raw client connects without sending any data
// Then no connection is accepted
#[tokio::test]
async fn test_init_info_timeout_handling() {
    let mut world = World::new();
    world.given_tcp_listener("tcp").await;

    world.when_connect_raw_tcp("raw", "tcp").await;
    world.then_no_connection_accepted().await;
}

// Scenario: Client sends incomplete init_info payload
// Given a running TCP listener
// When a raw client sends a length prefix but fewer bytes than specified
// Then no connection is accepted
#[tokio::test]
async fn test_partial_init_info_data() {
    let mut world = World::new();
    world.given_tcp_listener("tcp").await;

    world.when_connect_raw_tcp("raw", "tcp").await;

    let full_data = b"{\"chip\":{\"kind\":\"WIFI\"}}";
    let full_len = full_data.len() as u32;
    let partial_data = &full_data[..5];

    let mut payload = Vec::new();
    payload.extend_from_slice(&full_len.to_be_bytes());
    payload.extend_from_slice(partial_data);

    world.when_send_raw_tcp_bytes("raw", &payload).await;
    world.then_no_connection_accepted().await;
}

// Scenario: Client claims oversized init_info payload length
// Given a running TCP listener
// When a raw client claims a 100MB length prefix
// Then no connection is accepted
#[tokio::test]
async fn test_oversized_init_info() {
    let mut world = World::new();
    world.given_tcp_listener("tcp").await;

    world.when_connect_raw_tcp("raw", "tcp").await;

    let oversized_len = 100_000_000u32;
    let mut payload = Vec::new();
    payload.extend_from_slice(&oversized_len.to_be_bytes());
    payload.extend_from_slice(b"small data");

    world.when_send_raw_tcp_bytes("raw", &payload).await;
    world.then_no_connection_accepted().await;
}

// Scenario: Client sends zero-length init_info header
// Given a running TCP listener
// When a raw client sends a 0 length prefix
// Then no connection is accepted
#[tokio::test]
async fn test_empty_init_info() {
    let mut world = World::new();
    world.given_tcp_listener("tcp").await;

    world.when_connect_raw_tcp("raw", "tcp").await;

    let zero_len = 0u32;
    world.when_send_raw_tcp_bytes("raw", &zero_len.to_be_bytes()).await;
    world.then_no_connection_accepted().await;
}
