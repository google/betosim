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

//! Full-stack Daemon BDD integration tests for BLE beacon create, patch, and
//! remove over the `/v1/devices` HTTP REST API (`b/566532200`).

use netsim_model::{
    AdvertiseData, AdvertiseMode, AdvertiseSettings, AdvertiseTxPower, BleBeacon, BluetoothMode,
    ChipCreateVariant, ChipKind, ChipVariant, Device, DeviceChipCreate, DeviceConfig, DeviceCreate,
    Interval, Pose, Service, TxPower,
};
use netsim_rest_api::NetsimRestClient;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};

use crate::world::World;

/// Sends a raw HTTP/1.1 request to the daemon's HTTP port and returns the
/// parsed HTTP status code (`u16`) and response body bytes.
async fn send_raw_http_request(port: u16, method: &str, path: &str, body: &[u8]) -> (u16, Vec<u8>) {
    let mut stream =
        TcpStream::connect(("127.0.0.1", port)).await.expect("connect to daemon HTTP port");
    let request_head = if body.is_empty() {
        format!("{method} {path} HTTP/1.1\r\nHost: localhost:{port}\r\nConnection: close\r\n\r\n")
    } else {
        format!(
            "{method} {path} HTTP/1.1\r\nHost: localhost:{port}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        )
    };
    stream.write_all(request_head.as_bytes()).await.expect("write HTTP request head");
    if !body.is_empty() {
        stream.write_all(body).await.expect("write HTTP request body");
    }

    let mut raw_resp = Vec::new();
    stream.read_to_end(&mut raw_resp).await.expect("read HTTP response");
    let resp_str = String::from_utf8_lossy(&raw_resp);
    let (head, _) = resp_str.split_once("\r\n\r\n").expect("HTTP header/body separator");
    let status = head
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse::<u16>().ok())
        .expect("parse HTTP status code");
    let sep_pos = raw_resp
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .expect("find raw HTTP header/body separator");
    let resp_body = raw_resp[sep_pos + 4..].to_vec();
    (status, resp_body)
}

fn make_beacon_create_request(device_name: &str, chip_name: &str, address: &str) -> DeviceCreate {
    let ble_beacon = BleBeacon {
        address: address.to_string(),
        settings: Some(AdvertiseSettings {
            interval: Some(Interval::AdvertiseMode(AdvertiseMode::LowLatency)),
            tx_power: Some(TxPower::TxPowerLevel(AdvertiseTxPower::High)),
            scannable: true,
            timeout: 5000,
        }),
        adv_data: Some(AdvertiseData {
            include_device_name: true,
            include_tx_power_level: true,
            manufacturer_data: vec![0x01, 0x02],
            services: vec![Service { uuid: "180D".to_string(), data: vec![0xAB] }],
        }),
        scan_response: Some(AdvertiseData {
            include_device_name: false,
            include_tx_power_level: false,
            manufacturer_data: vec![0x03, 0x04],
            services: vec![],
        }),
    };

    DeviceCreate {
        device_config: DeviceConfig::new(device_name, true, Pose::default(), false),
        chip: DeviceChipCreate::new(
            chip_name,
            "Google",
            "Netsim BLE Beacon",
            ChipCreateVariant::Beacon(ble_beacon),
        ),
    }
}

// Feature: BLE Beacon REST Lifecycle on NetsimDaemon
//
//   As a developer or CLI user
//   I want to create, patch, and delete BLE beacon devices over /v1/devices
//   So that BLE beacons are managed consistently across REST and gRPC

// Scenario: Full lifecycle of a BLE beacon device over REST on a running Netsim Daemon
//   Given a running Netsim Daemon
//   When I create a BLE beacon device via POST /v1/devices
//   Then the server responds with 201 Created and returns a Device with BluetoothMode::Beacon
//   And both GET /v1/devices and gRPC ListDevice include the created beacon
//   When I update the beacon via PATCH /v1/devices/{id}
//   Then the server responds with 200 OK and GET /v1/devices reflects the updated BleBeacon settings
//   When I delete the beacon via DELETE /v1/devices/{id}
//   Then the server responds with 204 No Content and an empty body
//   And both GET /v1/devices and gRPC ListDevice omit the deleted beacon
//   And subsequent PATCH /v1/devices/{id} and DELETE /v1/devices/{id} return 404 Not Found
#[tokio::test]
async fn test_daemon_beacon_rest_full_lifecycle() {
    // Given a running Netsim Daemon
    let mut world = World::new().await;
    world.when_spawn_daemon().await;
    let rest_client = NetsimRestClient::new("localhost", world.http_port);

    // When I create a BLE beacon device via POST /v1/devices
    let create_req =
        make_beacon_create_request("beacon-integ-1", "beacon-chip-1", "02:11:22:33:44:55");
    let create_body = serde_json::to_vec(&create_req).expect("serialize DeviceCreate");
    let (create_status, create_resp_bytes) =
        send_raw_http_request(world.http_port, "POST", "/v1/devices", &create_body).await;

    // Then the server responds with 201 Created and returns a Device with BluetoothMode::Beacon
    assert_eq!(
        create_status,
        201,
        "POST /v1/devices must return 201 Created, body: {}",
        String::from_utf8_lossy(&create_resp_bytes)
    );
    let created: Device =
        serde_json::from_slice(&create_resp_bytes).expect("deserialize created Device");
    assert!(created.id > 0);
    assert_eq!(created.name, "beacon-integ-1");
    assert_eq!(created.chips.len(), 1);
    assert_eq!(created.chips[0].kind, ChipKind::BLUETOOTH);
    assert_eq!(created.chips[0].name, "beacon-chip-1");
    match &created.chips[0].variant {
        Some(ChipVariant::Bluetooth(bt)) => match &bt.mode {
            BluetoothMode::Beacon(params) => {
                assert_eq!(params.ble_beacon.address, "02:11:22:33:44:55");
            }
            other => panic!("Expected BluetoothMode::Beacon, got {other:?}"),
        },
        other => panic!("Expected ChipVariant::Bluetooth, got {other:?}"),
    }

    // And both GET /v1/devices and gRPC ListDevice include the created beacon
    let rest_devices =
        rest_client.get_devices().send().await.expect("GET /v1/devices after create");
    assert!(
        rest_devices.devices.iter().any(|d| d.id == created.id && d.name == "beacon-integ-1"),
        "REST GET /v1/devices must include beacon-integ-1: {rest_devices:?}"
    );
    world.then_device_list_contains(created.id, "beacon-integ-1").await;

    // When I update the beacon via PATCH /v1/devices/{id}
    let updated_ble_beacon = BleBeacon {
        address: "02:11:22:33:44:55".to_string(),
        settings: Some(AdvertiseSettings {
            interval: Some(Interval::AdvertiseMode(AdvertiseMode::LowPower)),
            tx_power: Some(TxPower::TxPowerLevel(AdvertiseTxPower::Low)),
            scannable: false,
            timeout: 10000,
        }),
        adv_data: Some(AdvertiseData {
            include_device_name: false,
            include_tx_power_level: false,
            manufacturer_data: vec![0xDE, 0xAD],
            services: vec![],
        }),
        scan_response: Some(AdvertiseData::default()),
    };
    let patch_payload = serde_json::json!({
        "id": created.id,
        "pose": {},
        "chips": [{
            "id": created.chips[0].id,
            "name": "beacon-chip-1",
            "pose": {},
            "variant": {
                "Bluetooth": {
                    "classic": {},
                    "low_energy": {},
                    "ble_beacon": updated_ble_beacon
                }
            }
        }]
    });
    let patched: Device = rest_client
        .patch::<Device>(&format!("/v1/devices/{}", created.id))
        .json(&patch_payload)
        .send()
        .await
        .expect("PATCH /v1/devices/{id}");

    // Then the server responds with 200 OK and GET /v1/devices reflects the updated BleBeacon settings
    assert_eq!(patched.id, created.id);
    let listed_after_patch =
        rest_client.get_devices().send().await.expect("GET /v1/devices after patch");
    let patched_in_list = listed_after_patch
        .devices
        .iter()
        .find(|d| d.id == created.id)
        .expect("patched beacon present in GET /v1/devices");
    match &patched_in_list.chips[0].variant {
        Some(ChipVariant::Bluetooth(bt)) => match &bt.mode {
            BluetoothMode::Beacon(params) => {
                let settings =
                    params.ble_beacon.settings.as_ref().expect("updated AdvertiseSettings");
                assert_eq!(settings.tx_power, Some(TxPower::TxPowerLevel(AdvertiseTxPower::Low)));
                assert!(!settings.scannable);
                assert_eq!(settings.timeout, 10000);
            }
            other => panic!("Expected BluetoothMode::Beacon, got {other:?}"),
        },
        other => panic!("Expected ChipVariant::Bluetooth, got {other:?}"),
    }

    // When I delete the beacon via DELETE /v1/devices/{id}
    let (delete_status, delete_body) = send_raw_http_request(
        world.http_port,
        "DELETE",
        &format!("/v1/devices/{}", created.id),
        &[],
    )
    .await;

    // Then the server responds with 204 No Content and an empty body
    assert_eq!(delete_status, 204, "DELETE /v1/devices/{{id}} must return 204 No Content");
    assert!(delete_body.is_empty(), "204 No Content response body must be empty");

    // And both GET /v1/devices and gRPC ListDevice omit the deleted beacon
    let listed_after_delete =
        rest_client.get_devices().send().await.expect("GET /v1/devices after delete");
    assert!(
        !listed_after_delete.devices.iter().any(|d| d.id == created.id),
        "REST GET /v1/devices must omit deleted beacon: {listed_after_delete:?}"
    );
    world.then_device_list_does_not_contain(created.id).await;

    // And subsequent PATCH /v1/devices/{id} and DELETE /v1/devices/{id} return 404 Not Found
    let patch_after_delete_err = rest_client
        .patch::<Device>(&format!("/v1/devices/{}", created.id))
        .json(&patch_payload)
        .send()
        .await
        .expect_err("PATCH on deleted beacon must fail with 404");
    assert_eq!(patch_after_delete_err.status().map(|s| s.as_u16()), Some(404));

    let delete_after_delete_err = rest_client
        .delete(&format!("/v1/devices/{}", created.id))
        .send()
        .await
        .expect_err("second DELETE on deleted beacon must fail with 404");
    assert_eq!(delete_after_delete_err.status().map(|s| s.as_u16()), Some(404));
}

// Scenario: Verify HTTP 400, 404, and 409 error codes for beacon REST operations on NetsimDaemon
//   Given a running Netsim Daemon with existing BLE beacon devices
//   When I send invalid POST, PATCH, and DELETE requests to /v1/devices
//   Then malformed JSON, invalid MAC addresses, and non-u32 path IDs return 400 Bad Request
//   And duplicate beacon device name, duplicate BLE address, or conflicting PATCH BLE address return 409 Conflict
//   And missing device IDs, missing chip names, or missing chip IDs return 404 Not Found
#[tokio::test]
async fn test_daemon_beacon_rest_error_codes() {
    // Given a running Netsim Daemon
    let mut world = World::new().await;
    world.when_spawn_daemon().await;
    let rest_client = NetsimRestClient::new("localhost", world.http_port);

    // POST /v1/devices with malformed JSON -> 400 Bad Request
    let (bad_json_post_status, _) =
        send_raw_http_request(world.http_port, "POST", "/v1/devices", b"{invalid-json").await;
    assert_eq!(bad_json_post_status, 400);

    // POST /v1/devices with invalid MAC address -> 400 Bad Request
    let bad_mac_req = make_beacon_create_request("beacon-bad-mac", "chip-bad-mac", "not-a-mac");
    let bad_mac_err = rest_client
        .post::<Device>("/v1/devices")
        .json(&bad_mac_req)
        .send()
        .await
        .expect_err("POST with invalid MAC must fail");
    assert_eq!(bad_mac_err.status().map(|s| s.as_u16()), Some(400));

    // Create two valid beacons to test 409 conflicts and PATCH error cases
    let valid_req =
        make_beacon_create_request("beacon-err-1", "beacon-err-chip-1", "02:22:33:44:55:66");
    let created: Device = rest_client
        .post::<Device>("/v1/devices")
        .json(&valid_req)
        .send()
        .await
        .expect("create first valid beacon");
    let second_req =
        make_beacon_create_request("beacon-err-2", "beacon-err-chip-2", "02:22:33:44:55:68");
    let second: Device = rest_client
        .post::<Device>("/v1/devices")
        .json(&second_req)
        .send()
        .await
        .expect("create second valid beacon");

    // POST /v1/devices with duplicate device name (distinct chip name and BLE address) -> 409 Conflict
    let dup_dev_name_req =
        make_beacon_create_request("beacon-err-1", "distinct-chip-1", "02:22:33:44:55:67");
    let dup_dev_err = rest_client
        .post::<Device>("/v1/devices")
        .json(&dup_dev_name_req)
        .send()
        .await
        .expect_err("duplicate device name POST must fail with 409");
    assert_eq!(dup_dev_err.status().map(|s| s.as_u16()), Some(409));

    // POST /v1/devices with duplicate BLE address (distinct device name and chip name) -> 409 Conflict
    let dup_ble_addr_req =
        make_beacon_create_request("distinct-beacon-dev", "distinct-chip-2", "02:22:33:44:55:66");
    let dup_addr_err = rest_client
        .post::<Device>("/v1/devices")
        .json(&dup_ble_addr_req)
        .send()
        .await
        .expect_err("duplicate BLE address POST must fail with 409");
    assert_eq!(dup_addr_err.status().map(|s| s.as_u16()), Some(409));

    // PATCH /v1/devices/abc (non-u32 id) -> 400 Bad Request
    let sample_patch = serde_json::json!({
        "id": 99999,
        "pose": {},
        "chips": [{
            "name": "beacon-err-chip-1",
            "pose": {},
            "variant": {
                "Bluetooth": {
                    "classic": {},
                    "low_energy": {},
                    "ble_beacon": BleBeacon::default()
                }
            }
        }]
    });
    let bad_id_patch_err = rest_client
        .patch::<Device>("/v1/devices/abc")
        .json(&sample_patch)
        .send()
        .await
        .expect_err("PATCH with non-u32 id must fail with 400");
    assert_eq!(bad_id_patch_err.status().map(|s| s.as_u16()), Some(400));

    // PATCH /v1/devices/{id} with malformed JSON -> 400 Bad Request
    let (bad_json_patch_status, _) = send_raw_http_request(
        world.http_port,
        "PATCH",
        &format!("/v1/devices/{}", created.id),
        b"{bad-json",
    )
    .await;
    assert_eq!(bad_json_patch_status, 400);

    // PATCH /v1/devices/{id} with invalid BLE address -> 400 Bad Request
    let invalid_addr_patch = serde_json::json!({
        "id": created.id,
        "pose": {},
        "chips": [{
            "name": "beacon-err-chip-1",
            "pose": {},
            "variant": {
                "Bluetooth": {
                    "classic": {},
                    "low_energy": {},
                    "ble_beacon": {
                        "address": "not-a-valid-mac"
                    }
                }
            }
        }]
    });
    let invalid_addr_patch_err = rest_client
        .patch::<Device>(&format!("/v1/devices/{}", created.id))
        .json(&invalid_addr_patch)
        .send()
        .await
        .expect_err("PATCH with invalid BLE address must fail with 400");
    assert_eq!(invalid_addr_patch_err.status().map(|s| s.as_u16()), Some(400));

    // PATCH /v1/devices/99999 (missing device id) -> 404 Not Found
    let missing_dev_patch_err = rest_client
        .patch::<Device>("/v1/devices/99999")
        .json(&sample_patch)
        .send()
        .await
        .expect_err("PATCH on missing device id must fail with 404");
    assert_eq!(missing_dev_patch_err.status().map(|s| s.as_u16()), Some(404));

    // PATCH /v1/devices/{id} with missing chip name -> 404 Not Found
    let missing_chip_patch = serde_json::json!({
        "id": created.id,
        "pose": {},
        "chips": [{
            "name": "non-existent-chip",
            "pose": {},
            "variant": {
                "Bluetooth": {
                    "classic": {},
                    "low_energy": {},
                    "ble_beacon": BleBeacon::default()
                }
            }
        }]
    });
    let missing_chip_err = rest_client
        .patch::<Device>(&format!("/v1/devices/{}", created.id))
        .json(&missing_chip_patch)
        .send()
        .await
        .expect_err("PATCH on missing chip name must fail with 404");
    assert_eq!(missing_chip_err.status().map(|s| s.as_u16()), Some(404));

    // PATCH /v1/devices/{id} with missing explicit chip id -> 404 Not Found
    let missing_chip_id_patch = serde_json::json!({
        "id": created.id,
        "pose": {},
        "chips": [{
            "id": 99999,
            "pose": {},
            "variant": {
                "Bluetooth": {
                    "classic": {},
                    "low_energy": {},
                    "ble_beacon": BleBeacon::default()
                }
            }
        }]
    });
    let missing_chip_id_err = rest_client
        .patch::<Device>(&format!("/v1/devices/{}", created.id))
        .json(&missing_chip_id_patch)
        .send()
        .await
        .expect_err("PATCH on missing explicit chip id must fail with 404");
    assert_eq!(missing_chip_id_err.status().map(|s| s.as_u16()), Some(404));

    // PATCH /v1/devices/{id} updating ble_beacon.address to an address already in use -> 409 Conflict
    let conflict_addr_patch = serde_json::json!({
        "id": second.id,
        "pose": {},
        "chips": [{
            "name": "beacon-err-chip-2",
            "pose": {},
            "variant": {
                "Bluetooth": {
                    "classic": {},
                    "low_energy": {},
                    "ble_beacon": {
                        "address": "02:22:33:44:55:66"
                    }
                }
            }
        }]
    });
    let conflict_addr_patch_err = rest_client
        .patch::<Device>(&format!("/v1/devices/{}", second.id))
        .json(&conflict_addr_patch)
        .send()
        .await
        .expect_err("PATCH with conflicting BLE address must fail with 409");
    assert_eq!(conflict_addr_patch_err.status().map(|s| s.as_u16()), Some(409));

    // DELETE /v1/devices/abc (non-u32 id) -> 400 Bad Request
    let bad_id_delete_err = rest_client
        .delete("/v1/devices/abc")
        .send()
        .await
        .expect_err("DELETE with non-u32 id must fail with 400");
    assert_eq!(bad_id_delete_err.status().map(|s| s.as_u16()), Some(400));

    // DELETE /v1/devices/99999 (missing device id) -> 404 Not Found
    let missing_dev_delete_err = rest_client
        .delete("/v1/devices/99999")
        .send()
        .await
        .expect_err("DELETE on missing device id must fail with 404");
    assert_eq!(missing_dev_delete_err.status().map(|s| s.as_u16()), Some(404));
}
