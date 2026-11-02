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
use packet_stream::TransportType;

use crate::world::{World, create_chip_info};

// Scenario: TCP transport echoes packets bidirectionally
// Given a running TCP listener
// And a connected client
// When the connection is accepted
// And the client sends a packet
// Then the server receives the packet
// When the server echoes the packet back
// Then the client receives the echoed packet
#[tokio::test]
async fn test_tcp_transport_echo() {
    let mut world = World::new();
    world.given_tcp_listener("tcp").await;

    let chip_info = create_chip_info("test-chip");
    world.when_connect_client("client", "tcp", chip_info).await;
    world.when_accept_connection().await;

    let test_data = b"Hello PacketStream!";
    world.when_send_packet_from_client("client", Bytes::from_static(test_data)).await;
    world.then_server_receives_packet("test-chip", test_data).await;

    world.when_send_packet_from_server("test-chip", Bytes::from_static(test_data)).await;
    world.then_client_receives_packet("client", test_data).await;
}

// Scenario: UDS transport sends packets from client to server
// Given a running Unix Domain Socket listener
// And a connected client
// When the connection is accepted
// And the client sends a packet
// Then the server receives the packet
#[cfg(unix)]
#[tokio::test]
async fn test_uds_transport_echo() {
    let mut world = World::new();
    world.given_uds_listener("uds").await;

    let chip_info = create_chip_info("test-chip");
    world.when_connect_client("client", "uds", chip_info).await;
    world.when_accept_connection().await;

    let test_data = b"Zero-copy test!";
    world.when_send_packet_from_client("client", Bytes::from_static(test_data)).await;
    world.then_server_receives_packet("test-chip", test_data).await;
}

// Scenario: Connection handshake transmits device and chip metadata
// Given a running TCP listener
// When a client connects with custom device and chip info
// And the connection is accepted
// Then the server received matching device info and manufacturer
#[tokio::test]
async fn test_chip_info_protocol() {
    let mut world = World::new();
    world.given_tcp_listener("tcp").await;

    let mut chip_info = create_chip_info("test-chip");
    chip_info.device_info.as_mut().expect("device_info").name = "specific-test-device".to_string();
    chip_info.chip.as_mut().expect("chip").manufacturer = "Specific Corp".to_string();

    world.when_connect_client("client", "tcp", chip_info).await;
    world.when_accept_connection().await;

    world.then_init_info_matches("test-chip", "specific-test-device", "Specific Corp");
}

// Scenario: Streams manages multiple concurrent transport listeners
// Given running TCP and UDS listeners
// When clients connect to both listeners
// Then both connections are successfully accepted
#[cfg(unix)]
#[tokio::test]
async fn test_multiple_transports() {
    let mut world = World::new();
    world.given_tcp_listener("tcp").await;
    world.given_uds_listener("uds").await;

    let tcp_chip = create_chip_info("tcp-chip");
    let uds_chip = create_chip_info("uds-chip");

    world.when_connect_client("tcp-client", "tcp", tcp_chip).await;
    world.when_connect_client("uds-client", "uds", uds_chip).await;

    world.when_accept_connection().await;
    world.when_accept_connection().await;

    world.then_connection_accepted("tcp-chip");
    world.then_connection_accepted("uds-chip");
}

// Scenario: Connecting to an unreachable port yields an error or timeout
// Given an unreachable TCP transport target
// When a client attempts to connect
// Then the connection fails or times out
#[tokio::test]
async fn test_error_handling() {
    let mut world = World::new();
    world
        .when_try_connect_to_transport(
            TransportType::tcp("localhost", 65534),
            create_chip_info("test-chip"),
        )
        .await;
    world.then_connect_failed();
}

// Scenario: Multiple clients connect concurrently to the same listener
// Given a running TCP listener
// When two clients connect concurrently
// Then both connections are accepted by the server
#[tokio::test]
async fn test_concurrent_connections() {
    let mut world = World::new();
    world.given_tcp_listener("tcp").await;

    let chip_info1 = create_chip_info("client1");
    let chip_info2 = create_chip_info("client2");

    world.when_connect_client("client1", "tcp", chip_info1).await;
    world.when_connect_client("client2", "tcp", chip_info2).await;

    world.when_accept_connection().await;
    world.when_accept_connection().await;

    world.then_connection_accepted("client1");
    world.then_connection_accepted("client2");
}
