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

const RAW_UPGRADE_UNSUPPORTED_VERSION: &str = concat!(
    "GET /v1/websocket/bt HTTP/1.1\r\nHost: localhost\r\n",
    "Upgrade: websocket\r\nConnection: Upgrade\r\n",
    "Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 12\r\n\r\n",
);
const RAW_UPGRADE_MISSING_KEY: &str = concat!(
    "GET /v1/websocket/bt HTTP/1.1\r\nHost: localhost\r\n",
    "Upgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Version: 13\r\n\r\n",
);
const WIFI_PATH: &str = "/v1/websocket/wifi";
const BT_PRESET_CONFIG_TEXT: &str = r#"{"preset":"laird_bl654"}"#;
const INVALID_JSON_TEXT: &str = "{invalid-json";
const EMPTY_JSON_TEXT: &str = "{}";

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
    // Explicit MACs are normalized to uppercase by Bluetooth MAC resolution.
    world.then_backend_receives_add_chip_request(client_a, "Pixel 9", "00:1A:11:E2:E0:01");

    // '+' decoded as space in form-urlencoded query strings
    let client_b = world.given_a_websocket_client_connected(Some("Pixel+9"), None).await;
    // Same device name reconnecting without an address reuses the name-keyed MAC.
    world.then_backend_receives_add_chip_request(client_b, "Pixel 9", "00:1A:11:E2:E0:01");

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

// Scenario: Disconnecting one WebSocket radio on a multi-chip device preserves other radios
//   Given a Bluetooth and a Wi-Fi WebSocket client connected with the same device_guid
//   When the Bluetooth WebSocket client closes its connection
//   Then only the Bluetooth chip is removed from the device
//   And the Wi-Fi WebSocket client continues to send and receive packets
#[tokio::test]
async fn test_multi_chip_same_device_guid_disconnect() {
    let mut world = TestWorld::new().await;

    let bt_client = world
        .given_a_websocket_client_connected_at_path(
            "/v1/websocket/bt?device_guid=shared-guid&name=cvd-1",
        )
        .await;
    let wifi_client = world
        .given_a_websocket_client_connected_at_path(
            "/v1/websocket/wifi?device_guid=shared-guid&name=cvd-1",
        )
        .await;

    assert_eq!(world.device_id(bt_client), world.device_id(wifi_client));
    assert_ne!(world.chip_id(bt_client), world.chip_id(wifi_client));

    // Closing the Bluetooth WebSocket only removes the Bluetooth chip, leaving Wi-Fi active
    world.when_client_closes_connection(bt_client).await;
    world.then_backend_deletes_chip(bt_client).await;
    world.then_backend_deletes_nothing();

    let wifi_packet = [0x80, 0x01];
    world.when_client_sends_packet(wifi_client, &wifi_packet).await;
    world.then_backend_receives_packet(wifi_client, &wifi_packet).await;
    world.when_backend_sends_packet(wifi_client, &wifi_packet).await;
    world.then_client_receives_packet(wifi_client, &wifi_packet).await;

    world.when_client_closes_connection(wifi_client).await;
    world.then_backend_deletes_chip(wifi_client).await;
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

// Scenario: WebSocket handshake rejects unsupported Sec-WebSocket-Version with 426 Upgrade Required
//   Given a running WebSocket server
//   When a raw HTTP upgrade request specifies Sec-WebSocket-Version 12
//   Then the handshake fails with HTTP 426 Upgrade Required
#[tokio::test]
async fn test_handshake_unsupported_websocket_version_returns_426() {
    let mut world = TestWorld::new().await;

    world.when_raw_http_request_is_sent(RAW_UPGRADE_UNSUPPORTED_VERSION).await;
    world.then_the_connection_fails_with_status(http::StatusCode::UPGRADE_REQUIRED.as_u16());
}

// Scenario: WebSocket handshake rejects upgrade requests missing Sec-WebSocket-Key with 400 Bad Request
//   Given a running WebSocket server
//   When a raw HTTP upgrade request omits the Sec-WebSocket-Key header
//   Then the handshake fails with HTTP 400 Bad Request
#[tokio::test]
async fn test_handshake_missing_key_header_returns_400() {
    let mut world = TestWorld::new().await;

    world.when_raw_http_request_is_sent(RAW_UPGRADE_MISSING_KEY).await;
    world.then_the_connection_fails_with_status(http::StatusCode::BAD_REQUEST.as_u16());
}

// Scenario: Pre-binary Text config frame is ignored for non-Bluetooth chips
//   Given a Wi-Fi WebSocket client connected to the server
//   When the client sends a Bluetooth config Text frame before any binary packet
//   Then the text frame is ignored without a device update
#[tokio::test]
async fn test_text_config_ignored_for_non_bluetooth_chip() {
    let mut world = TestWorld::new().await;
    let wifi = world.given_a_websocket_client_connected_at_path(WIFI_PATH).await;

    world.when_client_sends_text(wifi, BT_PRESET_CONFIG_TEXT).await;
    world.then_text_frame_is_ignored_before_next_packet(wifi).await;
}

// Scenario: Invalid JSON pre-binary Text config frame is ignored for Bluetooth chips
//   Given a Bluetooth WebSocket client connected to the server
//   When the client sends an invalid JSON Text frame before any binary packet
//   Then the text frame is ignored without a device update
#[tokio::test]
async fn test_invalid_json_text_config_ignored_for_bluetooth() {
    let mut world = TestWorld::new().await;
    let bt = world.given_a_websocket_client_connected(None, None).await;

    world.when_client_sends_text(bt, INVALID_JSON_TEXT).await;
    world.then_text_frame_is_ignored_before_next_packet(bt).await;
}

// Scenario: Empty JSON pre-binary Text config frame is ignored for Bluetooth chips
//   Given a Bluetooth WebSocket client connected to the server
//   When the client sends an empty JSON Text frame before any binary packet
//   Then the text frame is ignored without a device update
#[tokio::test]
async fn test_empty_json_text_config_ignored_for_bluetooth() {
    let mut world = TestWorld::new().await;
    let bt = world.given_a_websocket_client_connected(None, None).await;

    world.when_client_sends_text(bt, EMPTY_JSON_TEXT).await;
    world.then_text_frame_is_ignored_before_next_packet(bt).await;
}
