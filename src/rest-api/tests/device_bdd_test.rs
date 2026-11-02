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

//! Tier 2 Actor BDD integration tests for `/v1/devices`, `/v1/devices:reset`,
//! and `/v1/captures` over an ephemeral in-process HTTP listener
//! (`netsim_rest_api::server::run`).

use netsim_model::{DeviceId, Position};
use netsim_rest_api::RpcStatus;

use crate::world::World;

// Feature: Device REST Endpoints and HTTP Body Parsing
//
//   As a netsim CLI or external HTTP client
//   I want GET /v1/devices and POST /v1/devices:reset served over HTTP/1.1
//   So that device state, pose, radio/cellular state, and reset work end-to-end

// Scenario: GET /v1/devices serializes pose.position and distinct Cell radio.state vs modem state
//   Given a running REST server with a Cellular device at (1.5, 2.5, 3.5)
//   When a client requests GET /v1/devices over HTTP
//   Then the server responds with 200 OK and Content-Type: application/json
//   And the raw JSON body contains pose.position (x, y, z) and distinct Cell.radio.state (bool) vs Cell.state (string)
//   And NetsimRestClient::get_devices() deserializes the response into ListDeviceResponse
#[tokio::test]
async fn test_bdd_get_devices_http_body_parsing_pose_and_cell_state() {
    let mut world = World::new().await;
    let expected_pos = Position { x: 1.5, y: 2.5, z: 3.5 };
    world.given_cellular_device(DeviceId(1), "pixel-cell", expected_pos).await;

    world.when_http_request("GET", "/v1/devices", None, &[]).await;

    world.then_response_is_cellular_device_list(1, "pixel-cell", expected_pos).await;
}

// Scenario: POST /v1/devices:reset accepts both {} JSON body and empty body and resets actors
//   Given a running REST server with a dynamic Wi-Fi device and a dynamic AP
//   When a client sends POST /v1/devices:reset with {} JSON body
//   Then the server responds with 200 OK `{}` and removes both the dynamic device and dynamic AP
//   When another dynamic Wi-Fi device and dynamic AP are created and reset with an empty body
//   Then the server again responds with 200 OK `{}` and resets both actors
#[tokio::test]
async fn test_bdd_reset_devices_and_aps_empty_and_json_body() {
    let mut world = World::new().await;

    world.given_wifi_device(DeviceId(1), "wifi-phone-1", Position::default()).await;
    let ap1 = world.given_dynamic_ap("TestWifi1").await;
    world.then_device_count_is(1).await;
    world.then_ap_exists(ap1, true).await;

    world.when_http_request("POST", "/v1/devices:reset", Some("application/json"), br#"{}"#).await;

    world.then_response_is_empty_json_200();
    world.then_device_count_is(0).await;
    world.then_ap_exists(ap1, false).await;
    world.then_ap_exists(0, true).await;

    world.given_wifi_device(DeviceId(2), "wifi-phone-2", Position::default()).await;
    let ap2 = world.given_dynamic_ap("TestWifi2").await;
    world.then_device_count_is(1).await;
    world.then_ap_exists(ap2, true).await;

    world.when_http_request("POST", "/v1/devices:reset", None, &[]).await;

    world.then_response_is_empty_json_200();
    world.then_device_count_is(0).await;
    world.then_ap_exists(ap2, false).await;

    world.when_client_resets().await;
}

// Scenario: HTTP request body parsing rejects malformed JSON with 400 and oversized bodies with 413
//   Given a running REST server
//   When a client sends malformed JSON, empty body, or wrong field types to a JSON endpoint
//   Then the server responds with 400 Bad Request and AIP-193 ErrorResponse
//   When a client sends a request body exceeding 1 MiB
//   Then the server responds with 413 Payload Too Large and AIP-193 ErrorResponse
//   When a client requests an unknown route
//   Then the server responds with 404 Not Found and AIP-193 ErrorResponse
#[tokio::test]
async fn test_bdd_http_body_parsing_errors_and_size_limits() {
    let mut world = World::new().await;

    world.when_http_request("POST", "/v1/aps", Some("application/json"), b"{bad json").await;
    world.then_response_is_error(400, RpcStatus::InvalidArgument, "Invalid JSON body:");

    world.when_http_request("POST", "/v1/aps", Some("application/json"), b"").await;
    world.then_response_is_error(400, RpcStatus::InvalidArgument, "Invalid JSON body:");

    world
        .when_http_request(
            "POST",
            "/v1/aps",
            Some("application/json"),
            br#"{"ssid":"Test","channel":"not-a-u8"}"#,
        )
        .await;
    world.then_response_is_error(400, RpcStatus::InvalidArgument, "Invalid JSON body:");

    let oversized_len = (1 << 20) + 1;
    world
        .when_http_request_with_content_length(
            "POST",
            "/v1/devices:reset",
            Some("application/json"),
            oversized_len,
            b"",
        )
        .await;
    world.then_response_is_error(413, RpcStatus::OutOfRange, "413 Payload Too Large");

    world.when_http_request("GET", "/v1/devices/unknown/route", None, &[]).await;
    world.then_response_is_error(
        404,
        RpcStatus::NotFound,
        "Not Found: GET /v1/devices/unknown/route",
    );
}

// Scenario: PATCH /v1/devices/{id} applies partial pose and radio updates over HTTP
//   Given a running REST server with a Bluetooth device at (0.0, 0.0, 0.0)
//   When a client sends PATCH /v1/devices/1 with only pose.position
//   Then the server responds with 200 OK and updates pose.position while preserving BLE/Classic radio state
//   When a client sends PATCH /v1/devices/1 with only Bluetooth.low_energy.state = false
//   Then the server responds with 200 OK and disables BLE while preserving Classic and pose.position
#[tokio::test]
async fn test_bdd_patch_device_partial_pose_and_radio_updates() {
    let mut world = World::new().await;
    let moved_pos = Position { x: 10.5, y: -20.25, z: 3.0 };
    world.given_bluetooth_device(DeviceId(1), "pixel-bt", Position::default()).await;

    let move_body = br#"{"pose":{"position":{"x":10.5,"y":-20.25,"z":3.0}}}"#;
    world.when_http_request("PATCH", "/v1/devices/1", Some("application/json"), move_body).await;
    world.then_response_is_bluetooth_device(1, moved_pos, true, true);

    let radio_body = br#"{"chips":[{"variant":{"Bluetooth":{"low_energy":{"state":false}}}}]}"#;
    world.when_http_request("PATCH", "/v1/devices/1", Some("application/json"), radio_body).await;
    world.then_response_is_bluetooth_device(1, moved_pos, false, true);

    world.then_listed_bluetooth_device_is(moved_pos, false, true).await;
}

// Scenario: PATCH /v1/devices/{id} validates HTTP request body parsing and path parameters
//   Given a running REST server with a Wi-Fi device id = 1
//   When a client sends malformed JSON, empty body, wrong field types, unknown chip variants, or invalid path IDs
//   Then the server responds with 400 Bad Request and ErrorResponse JSON
//   When a client patches a non-existent device ID
//   Then the server responds with 404 Not Found and ErrorResponse JSON
//   When a client sends an oversized body (> 1 MiB)
//   Then the server responds with 413 Payload Too Large and ErrorResponse JSON
#[tokio::test]
async fn test_bdd_patch_device_http_body_parsing_and_path_errors() {
    let mut world = World::new().await;
    world.given_wifi_device(DeviceId(1), "pixel-wifi", Position::default()).await;

    let bad_body_cases: &[&[u8]] = &[
        b"{bad json",
        b"",
        br#"{"visible":"not-a-bool"}"#,
        br#"{"pose":{"position":{"x":"not-a-float","y":0.0,"z":0.0}}}"#,
        br#"{"chips":[{"variant":{"UnknownRadio":{"state":false}}}]}"#,
    ];

    for payload in bad_body_cases {
        world.when_http_request("PATCH", "/v1/devices/1", Some("application/json"), payload).await;
        world.then_response_is_error(400, RpcStatus::InvalidArgument, "Invalid JSON body:");
    }

    world
        .when_http_request("PATCH", "/v1/devices/not-an-id", Some("application/json"), b"{}")
        .await;
    world.then_response_is_error(400, RpcStatus::InvalidArgument, "Invalid path parameter id:");

    world.when_http_request("PATCH", "/v1/devices/999", Some("application/json"), b"{}").await;
    world.then_response_is_error(404, RpcStatus::NotFound, "");

    world
        .when_http_request_with_content_length(
            "PATCH",
            "/v1/devices/1",
            Some("application/json"),
            (1 << 20) + 1,
            b"",
        )
        .await;
    world.then_response_is_error(413, RpcStatus::OutOfRange, "413 Payload Too Large");
}

// Scenario: GET /v1/captures, PATCH /v1/captures/{id}, and GET /v1/captures/{id} over HTTP
//   Given a running REST server with a Wi-Fi device (chip_id = 1000)
//   When a client requests GET /v1/captures
//   Then the server responds with 200 OK and lists capture 1000 with enabled = false
//   When a client sends PATCH /v1/captures/1000 with {"state": true}
//   Then the server responds with 200 OK, enables capture 1000, and creates the PCAP file on disk
//   When a client requests GET /v1/captures/1000
//   Then the server responds with 200 OK, Content-Type: application/vnd.tcpdump.pcap, and valid PCAP global header bytes
#[tokio::test]
async fn test_bdd_captures_list_patch_and_get_pcap_and_errors() {
    let mut world = World::new().await;
    world.given_wifi_device(DeviceId(1), "pixel-wifi", Position::default()).await;

    world.when_http_request("GET", "/v1/captures", None, &[]).await;
    world.then_response_is_capture_list_single(1000, "pixel-wifi", false);

    world.when_http_request("GET", "/v1/captures/1000", None, &[]).await;
    world.then_response_is_error(404, RpcStatus::NotFound, "Capture 1000 has no PCAP yet");

    world
        .when_http_request(
            "PATCH",
            "/v1/captures/1000",
            Some("application/json"),
            br#"{"state":true}"#,
        )
        .await;
    world.then_response_is_patched_capture(1000, true);

    world.when_http_request("GET", "/v1/captures/1000", None, &[]).await;
    world.then_response_is_valid_pcap_and_matches_client(1000).await;

    world.when_http_request("PATCH", "/v1/captures/1000", Some("application/json"), b"{bad").await;
    world.then_response_is_error(400, RpcStatus::InvalidArgument, "Invalid JSON body:");

    world
        .when_http_request(
            "PATCH",
            "/v1/captures/1000",
            Some("application/json"),
            br#"{"state":"not-a-bool"}"#,
        )
        .await;
    world.then_response_is_error(400, RpcStatus::InvalidArgument, "Invalid JSON body:");

    world
        .when_http_request(
            "PATCH",
            "/v1/captures/not-a-u32",
            Some("application/json"),
            br#"{"state":true}"#,
        )
        .await;
    world.then_response_is_error(400, RpcStatus::InvalidArgument, "Invalid path parameter id:");

    world
        .when_http_request(
            "PATCH",
            "/v1/captures/9999",
            Some("application/json"),
            br#"{"state":true}"#,
        )
        .await;
    world.then_response_is_error(404, RpcStatus::NotFound, "");

    world.when_http_request("GET", "/v1/captures/9999", None, &[]).await;
    world.then_response_is_error(404, RpcStatus::NotFound, "");
}
