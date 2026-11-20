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
//=============================================================================
// tests/test_streams_lifecycle.rs - BDD lifecycle tests for Streams API
//=============================================================================

use packet_stream::TransportType;

use crate::world::{World, create_chip_info};

const FD_LISTENER_NAME: &str = "fd-listener";
const SAMPLE_IN_FD: i32 = 3;
const SAMPLE_OUT_FD: i32 = 4;

// Scenario: Starting an FD listener returns InvalidConfig and leaves no listener running
// Given a fresh Streams world
// When attempting to start a listener with TransportType::fd(SAMPLE_IN_FD, Some(SAMPLE_OUT_FD))
// Then InvalidConfig is returned with the FD listener error message
// And the listener is not reported as running
#[tokio::test]
async fn test_start_listener_rejects_fd_transport() {
    let expected_err =
        format!("Stream type FD {SAMPLE_IN_FD}:{SAMPLE_OUT_FD} does not support listeners");

    // Given
    let mut world = World::new();

    // When
    world.when_try_start_fd_listener(FD_LISTENER_NAME, SAMPLE_IN_FD, Some(SAMPLE_OUT_FD)).await;

    // Then
    world.then_last_error_is_invalid_config(&expected_err);
    world.then_listener_is_running(FD_LISTENER_NAME, false);
}

// Scenario: Direct connect on an FD transport returns InvalidConfig
// Given a fresh Streams world
// When attempting to connect directly to TransportType::fd(3, None)
// Then the connect attempt fails with InvalidConfig
#[tokio::test]
async fn test_connect_rejects_fd_transport() {
    const EXPECTED_ERR: &str = "FD transport cannot be created directly, it must be inherited";

    // Given
    let mut world = World::new();

    // When
    world
        .when_try_connect_to_transport(
            TransportType::fd(SAMPLE_IN_FD, None),
            create_chip_info("fd-chip"),
        )
        .await;

    // Then
    world.then_connect_failed();
    world.then_last_error_is_invalid_config(EXPECTED_ERR);
}

// Scenario: Adding, stopping, and shutting down custom TransportListeners updates active names
//   and runs remaining shutdown hooks
// Given two custom listeners "custom-a" and "custom-b" added via add_listener
// Then listener_names reports both listeners
// When stopping an unknown listener name
// Then both custom listeners remain registered
// When stopping "custom-a"
// Then "custom-a" is no longer reported as running
// And listener_names reports only "custom-b"
// When shutting down all remaining listeners
// Then "custom-b" runs its shutdown hook
#[tokio::test]
async fn test_add_and_stop_listener_lifecycle() {
    const CUSTOM_A: &str = "custom-a";
    const CUSTOM_B: &str = "custom-b";
    const UNKNOWN_LISTENER: &str = "unknown-listener";

    // Given
    let mut world = World::new();
    world.given_custom_listener(CUSTOM_A);
    world.given_custom_listener(CUSTOM_B);
    world.then_listener_names_are(&[CUSTOM_A, CUSTOM_B]);

    // When
    world.when_stop_listener(UNKNOWN_LISTENER);

    // Then
    world.then_listener_names_are(&[CUSTOM_A, CUSTOM_B]);

    // When
    world.when_stop_listener(CUSTOM_A);

    // Then
    world.then_listener_is_running(CUSTOM_A, false);
    world.then_listener_names_are(&[CUSTOM_B]);

    // When
    world.when_shutdown_all().await;

    // Then
    world.then_custom_listener_shutdown(CUSTOM_B, true);
}

// Scenario: Starting a listener from ListenerConfig registers it and accepts client connections
// Given a TCP listener started from ListenerConfig
// Then listener_names reports the configured listener as running
// When a client connects and the server accepts the connection
// Then the server records the accepted chip connection
#[tokio::test]
async fn test_start_from_config_registers_and_accepts_connection() {
    const CONFIG_LISTENER: &str = "cfg-tcp";
    const CHIP_NAME: &str = "cfg-chip";

    // Given
    let mut world = World::new();
    world.given_listeners_from_tcp_config(CONFIG_LISTENER).await;

    // Then
    world.then_listener_names_are(&[CONFIG_LISTENER]);
    world.then_listener_is_running(CONFIG_LISTENER, true);

    // When
    world.when_connect_client("cfg-client", CONFIG_LISTENER, create_chip_info(CHIP_NAME)).await;
    world.when_accept_connection().await;

    // Then
    world.then_connection_accepted(CHIP_NAME);
}
