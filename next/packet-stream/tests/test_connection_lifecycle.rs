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

use std::time::Duration;

use bytes::Bytes;

use crate::world::{World, create_chip_info};

// Scenario: Client gracefully closes sink and server detects EOF
// Given a running TCP listener
// And a connected client
// When the connection is accepted
// And the client sends an initial packet
// Then the server receives the packet
// When the client gracefully closes its sink
// Then the server stream closes
#[tokio::test]
async fn test_graceful_client_disconnect() {
    let mut world = World::new();
    world.given_tcp_listener("tcp").await;

    let chip_info = create_chip_info("test-chip");
    world.when_connect_client("client", "tcp", chip_info).await;
    world.when_accept_connection().await;

    let test_data = b"Connection established";
    world.when_send_packet_from_client("client", Bytes::from_static(test_data)).await;
    world.then_server_receives_packet("test-chip", test_data).await;

    world.when_close_client_sink("client").await;
    world.then_server_stream_closes("test-chip").await;
}

// Scenario: Client drops abruptly and server detects EOF
// Given a running TCP listener
// And a connected client
// When the connection is accepted
// And the client sends an initial packet
// Then the server receives the packet
// When the client is dropped abruptly
// Then the server stream closes
#[tokio::test]
async fn test_abrupt_client_disconnect() {
    let mut world = World::new();
    world.given_tcp_listener("tcp").await;

    let chip_info = create_chip_info("test-chip");
    world.when_connect_client("client", "tcp", chip_info).await;
    world.when_accept_connection().await;

    let test_data = b"Before disconnect";
    world.when_send_packet_from_client("client", Bytes::from_static(test_data)).await;
    world.then_server_receives_packet("test-chip", test_data).await;

    world.when_drop_client("client");
    tokio::time::sleep(Duration::from_millis(100)).await;

    world.then_server_stream_closes("test-chip").await;
}

// Scenario: Raw socket disconnects before handshake and server ignores it
// Given a running TCP listener
// When a raw TCP connection connects and drops immediately
// Then no connection is accepted
#[tokio::test]
async fn test_raw_socket_disconnect() {
    let mut world = World::new();
    world.given_tcp_listener("tcp").await;

    world.when_connect_raw_tcp("raw", "tcp").await;
    world.when_drop_raw_tcp("raw");

    world.then_no_connection_accepted().await;
}

// Scenario: One client disconnects while another remains connected and active
// Given a running TCP listener
// And two connected clients
// When both connections are accepted
// And both clients send messages
// Then the server receives both messages
// When the first client drops
// Then the second client can still send messages
// And the first server stream closes
#[tokio::test]
async fn test_multiple_client_disconnect() {
    let mut world = World::new();
    world.given_tcp_listener("tcp").await;

    let chip_info1 = create_chip_info("client1");
    let chip_info2 = create_chip_info("client2");

    world.when_connect_client("client1", "tcp", chip_info1).await;
    world.when_connect_client("client2", "tcp", chip_info2).await;

    world.when_accept_connection().await;
    world.when_accept_connection().await;

    world.when_send_packet_from_client("client1", Bytes::from_static(b"Client 1 message")).await;
    world.when_send_packet_from_client("client2", Bytes::from_static(b"Client 2 message")).await;

    world.then_server_receives_packet("client1", b"Client 1 message").await;
    world.then_server_receives_packet("client2", b"Client 2 message").await;

    world.when_drop_client("client1");

    world
        .when_send_packet_from_client("client2", Bytes::from_static(b"Client 2 still works"))
        .await;
    world.then_server_receives_packet("client2", b"Client 2 still works").await;

    world.then_server_stream_closes("client1").await;
}

// Scenario: Shutting down all listeners stops accepting new connections
// Given a running TCP listener
// And a connected client
// When the connection is accepted
// And all listeners are shut down
// Then the listener is reported as stopped
// And subsequent connection attempts fail
#[tokio::test]
async fn test_listener_shutdown() {
    let mut world = World::new();
    world.given_tcp_listener("tcp").await;

    let chip_info = create_chip_info("test-chip");
    world.when_connect_client("client", "tcp", chip_info).await;
    world.when_accept_connection().await;
    world.then_connection_accepted("test-chip");

    world.when_shutdown_all().await;
    world.then_listener_is_running("tcp", false);

    world.when_try_connect_client("tcp", create_chip_info("test-chip")).await;
    world.then_connect_failed();
}

// Scenario: Repeated connect, accept, and drop cycles clean up resources properly
// Given a running TCP listener
// When multiple clients repeatedly connect, accept, and drop
// Then subsequent connections can still be established and accepted
#[tokio::test]
async fn test_connection_resource_cleanup() {
    let mut world = World::new();
    world.given_tcp_listener("tcp").await;

    for _ in 0..5 {
        let chip_info = create_chip_info("test-chip");
        world.when_connect_client("client", "tcp", chip_info).await;
        world.when_accept_connection().await;
        world.then_connection_accepted("test-chip");

        world.when_drop_client("client");
        world.when_drop_server("test-chip");

        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    let chip_info = create_chip_info("test-chip");
    world.when_connect_client("client", "tcp", chip_info).await;
    world.when_accept_connection().await;
    world.then_connection_accepted("test-chip");
}
