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

use crate::world::TestWorld;

#[tokio::test]
async fn test_default_query_parameters() {
    let mut world = TestWorld::new().await;

    // Connect with no query parameters
    let client_idx = world.given_a_websocket_client_connected(None, None).await;

    world.then_backend_receives_default_add_chip_request(client_idx);
}

#[tokio::test]
async fn test_percent_encoded_query_parameters() {
    let mut world = TestWorld::new().await;

    // Percent-encoded name and Bluetooth MAC address
    let client_a = world
        .given_a_websocket_client_connected(
            Some("Pixel%209&x=a+b"),
            Some("00%3A1a%3A11%3Ae2%3Ae0%3A01"),
        )
        .await;
    world.then_backend_receives_add_chip_request(client_a, "Pixel 9", "00:1a:11:e2:e0:01");

    // '+' decoded as space in form-urlencoded query strings
    let client_b = world.given_a_websocket_client_connected(Some("Pixel+9"), None).await;
    world.then_backend_receives_add_chip_request(client_b, "Pixel 9", "");

    // Plain unencoded values (such as Bumble's) remain unchanged
    let client_c = world
        .given_a_websocket_client_connected(Some("bumble&loopback=0"), Some("00:11:22:33:44:55"))
        .await;
    world.then_backend_receives_add_chip_request(client_c, "bumble", "00:11:22:33:44:55");
}

#[tokio::test]
async fn test_invalid_resource_path() {
    let mut world = TestWorld::new().await;

    world.when_client_attempts_to_connect_to_invalid_path().await;
    world.then_the_connection_fails_with_status(404);

    for invalid_path in [
        "/v1/websocket",
        "/v1/websocket/",
        "/v1/websocket/unknown",
        "/v1/websocket/unspecified",
        "/v1/websocket/bt/extra",
        "/v1/websocketbt",
    ] {
        world.when_client_attempts_to_connect_to_path(invalid_path).await;
        world.then_the_connection_fails_with_status(404);
    }
}

#[tokio::test]
async fn test_multiple_concurrent_clients() {
    let mut world = TestWorld::new().await;

    let client_a = world.given_a_websocket_client_connected(Some("client-a"), None).await;
    let client_b = world.given_a_websocket_client_connected(Some("client-b"), None).await;

    let packet_a = [0xAA];
    let packet_b = [0xBB];

    world.when_client_sends_packet(client_a, &packet_a).await;
    world.when_client_sends_packet(client_b, &packet_b).await;

    world.then_backend_receives_packet(client_a, &packet_a).await;
    world.then_backend_receives_packet(client_b, &packet_b).await;

    world.when_client_closes_connection(client_a).await;
    world.then_backend_deletes_chip(client_a).await;

    // Client B should still be alive
    world.when_backend_sends_packet(client_b, &packet_b).await;
    world.then_client_receives_packet(client_b, &packet_b).await;
}

#[tokio::test]
async fn test_backend_initiated_termination() {
    let mut world = TestWorld::new().await;

    let client_idx = world.given_a_websocket_client_connected(None, None).await;

    // Terminate from backend
    world.when_backend_closes_connection(client_idx).await;

    // Wait for cleanup
    world.then_backend_deletes_chip(client_idx).await;

    // Verify client receives EOF/Close
    world.then_client_receives_eof(client_idx).await;
}

#[tokio::test]
async fn test_large_data_payloads() {
    let mut world = TestWorld::new().await;
    let client_idx = world.given_a_websocket_client_connected(None, None).await;

    let large_packet = [0xCC; 2000];

    world.when_client_sends_packet(client_idx, &large_packet).await;
    world.then_backend_receives_packet(client_idx, &large_packet).await;

    world.when_backend_sends_packet(client_idx, &large_packet).await;
    world.then_client_receives_packet(client_idx, &large_packet).await;
}
