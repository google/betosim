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

use netsim_model::{ApCreate, ApUpdate};
use netsim_rest_api::{DisconnectApRequest, NetsimRestClient};

use crate::world::World;

// Feature: REST Frontend
//
//   As a developer
//   I want to manage devices over the /v1 HTTP API
//   So that I can control the netsim environment without a gRPC client

// Scenario: gRPC and REST Share Actor State
//   Given a running Netsim Daemon
//   Then both front ends report the same daemon version
//   When I create a device over gRPC
//   Then the device is visible over REST
//   When I delete the device over gRPC
//   Then the device is no longer visible over REST
#[tokio::test]
async fn test_grpc_and_rest_share_actor_state() {
    // Given a running Netsim Daemon
    let mut world = World::new().await;
    world.when_spawn_daemon().await;
    let rest_client = NetsimRestClient::new("localhost", world.http_port);

    // Then both front ends report the same daemon version
    let grpc_version = world.when_get_version().await;
    assert!(!grpc_version.is_empty(), "gRPC version must not be empty");
    let rest_version = rest_client
        .get_version()
        .send()
        .await
        .expect("Failed to get version via REST client")
        .version;
    assert_eq!(rest_version, grpc_version, "REST version must match gRPC version");

    // When I create a device over gRPC
    let device_name = "rest-test-device";
    let device_id = world.when_create_device(device_name, "beacon-chip").await;
    assert!(device_id > 0);

    // Then the device is visible over REST, so both front ends are backed by
    // the same actors rather than by separate state.
    let devices_resp =
        rest_client.get_devices().send().await.expect("Failed to get devices via REST client");
    assert!(
        devices_resp.devices.iter().any(|d| d.name == device_name),
        "REST get_devices() must contain the gRPC created device: {devices_resp:?}"
    );

    // When I delete the device over gRPC
    world.when_delete_device(device_id).await;

    // Then the device is no longer visible over REST
    let devices_resp =
        rest_client.get_devices().send().await.expect("Failed to get devices via REST client");
    assert!(
        !devices_resp.devices.iter().any(|d| d.name == device_name),
        "REST get_devices() must not contain the deleted device: {devices_resp:?}"
    );
}

// Scenario: Stream BLE Scan and Sniff Packets over REST WebSocket
//   Given a running Netsim Daemon with an advertising BLE beacon
//   When I connect to GET /v1/ble/scan over WebSocket
//   Then I receive an HCI LE Advertising Report from the beacon
//   When I close the WebSocket stream
//   And I connect to GET /v1/ble/sniff over WebSocket
//   Then I receive a raw LE Legacy Advertising PDU
//   When I close the WebSocket stream
//   Then the transient scanner and sniffer devices are removed
#[tokio::test]
async fn test_rest_ble_scan_and_sniff_streams() {
    let mut world = World::new().await;
    world.when_spawn_daemon().await;
    let beacon_id = world.when_create_device("rest-ble-beacon", "beacon-chip").await;

    world.when_connect_http_websocket("/v1/ble/scan?x=0&y=0&z=0&active=false").await;
    world.then_websocket_receives_ble_scan_report([0x11, 0x22, 0x33, 0x44, 0x55, 0x66]).await;
    world.when_close_websocket().await;

    world.when_connect_http_websocket("/v1/ble/sniff?x=0&y=0&z=0").await;
    world.then_websocket_receives_ble_sniff_pdu().await;
    world.when_close_websocket().await;

    world.then_transient_ble_devices_removed(beacon_id).await;
}

// Scenario: The REST Client Reaches the HTTP Listener and Not Some Other Port
//   Given a running Netsim Daemon
//   When I send a REST request to the gRPC port
//   Then the request fails
//
// This is the negative control for the scenario above. Without it, a daemon
// that answered REST on every port would still pass, and we would not know
// which listener served the request.
#[tokio::test]
async fn test_rest_is_not_served_on_grpc_port() {
    // Given a running Netsim Daemon
    let mut world = World::new().await;
    world.when_spawn_daemon().await;

    // When I send a REST request to the gRPC port, Then it fails
    let rest_on_grpc_port = NetsimRestClient::new("localhost", world.grpc_port);
    assert!(
        rest_on_grpc_port.get_version().send().await.is_err(),
        "gRPC port must not serve the REST API"
    );

    // And the same request against the HTTP port succeeds, so the failure
    // above is the port and not a broken client or a dead daemon.
    let rest_client = NetsimRestClient::new("localhost", world.http_port);
    assert!(rest_client.get_version().send().await.is_ok(), "HTTP port must serve the REST API");
}

// Scenario: The Bound HTTP Port Is Discoverable
//   Given a running Netsim Daemon
//   Then netsim.ini reports the HTTP port the daemon actually bound
//
// Clients discover the HTTP port through netsim.ini, so publishing a port
// other than the bound one would leave them unable to connect.
#[tokio::test]
async fn test_http_port_published_to_ini() {
    // Given a running Netsim Daemon
    let mut world = World::new().await;
    world.when_spawn_daemon().await;

    // Then netsim.ini reports the HTTP port the daemon actually bound
    let ini_path = world.get_temp_dir().join(common::util::ini_file::get_ini_filename(1));
    let content = std::fs::read_to_string(&ini_path)
        .unwrap_or_else(|e| panic!("Failed to read {}: {e}", ini_path.display()));

    let expected = format!("http.port={}", world.http_port);
    assert!(
        content.lines().any(|line| line == expected),
        "netsim.ini must contain {expected}, got:\n{content}"
    );
}

// Scenario: Manage Access Points over REST
//   Given a running Netsim Daemon
//   When I create an Access Point over REST
//   Then the Access Point is listed with its computed BSSID
//   When I patch the Access Point SSID and channel
//   Then the updated configuration is visible over REST
//   When I disconnect a client from the Access Point
//   Then the disconnect operation succeeds
//   When I delete the Access Point
//   Then the Access Point is no longer listed over REST
//   And deleting the same Access Point again succeeds idempotently, while patching it returns 404
#[tokio::test]
async fn test_rest_access_point_lifecycle() {
    let mut world = World::new().await;
    world.when_spawn_daemon().await;
    let rest_client = NetsimRestClient::new("localhost", world.http_port);

    // Initial AP list contains default AP (id: 0) pre-seeded by netsimd
    let initial_aps = rest_client.get_aps().send().await.expect("Failed to get initial aps");
    let initial_count = initial_aps.aps.len();
    assert!(initial_aps.aps.iter().any(|ap| ap.id == 0));

    // Create an AP
    let create_req = ApCreate { ssid: "TestAp".to_string(), ..Default::default() };
    let created = rest_client.create_ap(&create_req).send().await.expect("Failed to create AP");
    assert_eq!(created.config.ssid, "TestAp");
    let ap_id = created.id;
    assert!(ap_id > 0);

    // AP is listed in get_aps() and matches created config
    let list_after_create = rest_client.get_aps().send().await.expect("Failed to get aps");
    assert_eq!(list_after_create.aps.len(), initial_count + 1);
    let listed = list_after_create
        .aps
        .iter()
        .find(|ap| ap.id == ap_id)
        .expect("Created AP missing from list");
    assert_eq!(listed.config.ssid, created.config.ssid);
    assert_eq!(listed.config.bssid, created.config.bssid);

    // Patch AP
    let patch_req =
        ApUpdate { ssid: Some("PatchedAp".to_string()), channel: Some(11), ..Default::default() };
    let patched = rest_client.patch_ap(ap_id, &patch_req).send().await.expect("Failed to patch AP");
    assert_eq!(patched.config.ssid, "PatchedAp");
    assert_eq!(patched.config.channel, 11);
    let list_after_patch = rest_client.get_aps().send().await.expect("Failed to get aps");
    let listed_patched = list_after_patch
        .aps
        .iter()
        .find(|ap| ap.id == ap_id)
        .expect("Patched AP missing from list");
    assert_eq!(listed_patched.config.ssid, "PatchedAp");
    assert_eq!(listed_patched.config.channel, 11);

    // Disconnect a client MAC (infallible even if MAC is not associated)
    let disconnect_req = DisconnectApRequest { mac_address: "02:15:b2:00:01:00".to_string() };
    rest_client
        .disconnect_ap(ap_id, &disconnect_req)
        .send()
        .await
        .expect("Failed to disconnect from AP");

    // Delete the AP
    rest_client.delete_ap(ap_id).send().await.expect("Failed to delete AP");

    // AP is no longer listed
    let list_after_delete = rest_client.get_aps().send().await.expect("Failed to get aps");
    assert_eq!(list_after_delete.aps.len(), initial_count);
    assert!(!list_after_delete.aps.iter().any(|ap| ap.id == ap_id));

    // Deleting again is idempotent and returns 204 No Content (Ok)
    rest_client.delete_ap(ap_id).send().await.expect("Second delete must succeed idempotently");

    // Patching the deleted AP returns 404
    let err = rest_client
        .patch_ap(ap_id, &patch_req)
        .send()
        .await
        .expect_err("Patching deleted AP must 404");
    assert_eq!(err.status().map(|s| s.as_u16()), Some(404));
}

// Scenario: Reset Devices and Access Points over REST
//   Given a running Netsim Daemon with a created beacon device and a dynamic AP
//   When I call POST /v1/devices:reset over REST
//   Then the dynamic beacon device is removed and dynamic APs are reset to the default AP
#[tokio::test]
async fn test_rest_reset_devices_and_aps() {
    let mut world = World::new().await;
    world.when_spawn_daemon().await;
    let rest_client = NetsimRestClient::new("localhost", world.http_port);

    // Create a beacon device and a dynamic AP
    let device_name = "reset-test-beacon";
    let device_id = world.when_create_device(device_name, "beacon-chip").await;
    assert!(device_id > 0);
    let create_ap_req = ApCreate { ssid: "DynamicAp".to_string(), ..Default::default() };
    let created_ap =
        rest_client.create_ap(&create_ap_req).send().await.expect("Failed to create AP");
    assert!(created_ap.id > 0);

    // Verify both are present before reset
    let devices_before = rest_client.get_devices().send().await.expect("Failed to list devices");
    assert!(devices_before.devices.iter().any(|d| d.name == device_name));
    let aps_before = rest_client.get_aps().send().await.expect("Failed to list APs");
    assert!(aps_before.aps.iter().any(|ap| ap.id == created_ap.id));

    // When I reset over REST
    rest_client.reset().send().await.expect("Failed to reset devices via REST");

    // Then the beacon device and dynamic AP are gone
    let devices_after = rest_client.get_devices().send().await.expect("Failed to list devices");
    assert!(
        !devices_after.devices.iter().any(|d| d.name == device_name),
        "Beacon device must be removed after reset: {devices_after:?}"
    );
    let aps_after = rest_client.get_aps().send().await.expect("Failed to list APs");
    assert!(
        !aps_after.aps.iter().any(|ap| ap.id == created_ap.id),
        "Dynamic AP must be removed after reset: {aps_after:?}"
    );
    assert!(aps_after.aps.iter().any(|ap| ap.id == 0), "Default AP must remain after reset");
}

// Scenario: Patch Device Pose and Radio State over REST
//   Given a running Netsim Daemon with a created device
//   When I call PATCH /v1/devices/{id} to move the device and toggle its BLE radio
//   Then PATCH returns 200 OK with the updated Device and GET /v1/devices reflects the changes
#[tokio::test]
async fn test_rest_patch_device_pose_and_radio() {
    let mut world = World::new().await;
    world.when_spawn_daemon().await;
    let rest_client = NetsimRestClient::new("localhost", world.http_port);

    let device_name = "patch-test-device";
    let device_id = world.when_create_device(device_name, "beacon-chip").await;
    assert!(device_id > 0);

    // 1. Move device over REST
    let move_patch = netsim_model::DeviceUpdate {
        pose: netsim_model::PoseUpdate {
            position: Some(netsim_model::Position { x: 1.5, y: 2.5, z: 3.5 }),
            orientation: None,
        },
        ..Default::default()
    };
    let moved = rest_client
        .patch_device(device_id, &move_patch)
        .send()
        .await
        .expect("Failed to patch device pose via REST");
    assert_eq!(moved.id, device_id);
    assert_eq!(moved.pose.position, netsim_model::Position { x: 1.5, y: 2.5, z: 3.5 });

    // 2. Toggle BLE radio off over REST
    let radio_patch = netsim_model::DeviceUpdate {
        chips: Some(vec![netsim_model::ChipUpdate {
            variant: Some(netsim_model::ChipVariantUpdate::Bluetooth(
                netsim_model::BluetoothUpdate {
                    low_energy: netsim_model::RadioUpdate { state: Some(false) },
                    ..Default::default()
                },
            )),
            ..Default::default()
        }]),
        ..Default::default()
    };
    let toggled = rest_client
        .patch_device(device_id, &radio_patch)
        .send()
        .await
        .expect("Failed to patch device radio via REST");
    assert_eq!(toggled.id, device_id);
    assert!(!toggled.chips[0].is_le_enabled());

    // 3. Read back via GET /v1/devices and verify both pose and radio state persisted
    let listed = rest_client.get_devices().send().await.expect("Failed to list devices");
    let dev = listed
        .devices
        .iter()
        .find(|d| d.id == device_id)
        .expect("Patched device missing from GET /v1/devices");
    assert_eq!(dev.pose.position, netsim_model::Position { x: 1.5, y: 2.5, z: 3.5 });
    assert!(!dev.chips[0].is_le_enabled());

    // 4. Patching a non-existent device returns 404 Not Found
    let err = rest_client
        .patch_device(9999, &move_patch)
        .send()
        .await
        .expect_err("Patching non-existent device must return 404");
    assert_eq!(err.status().map(|s| s.as_u16()), Some(404));
}

// Scenario: List, Patch, and Download Packet Captures over REST
//   Given a running Netsim Daemon with a created device
//   When I call GET /v1/captures over REST
//   Then the device's chip capture is listed
//   When I call PATCH /v1/captures/{id} to enable capture and GET /v1/captures/{id}
//   Then the PCAP file bytes are returned with a valid 24-byte PCAP global header
#[tokio::test]
async fn test_rest_captures_lifecycle() {
    let mut world = World::new().await;
    world.when_spawn_daemon().await;
    let rest_client = NetsimRestClient::new("localhost", world.http_port);

    let device_name = "capture-test-device";
    let device_id = world.when_create_device(device_name, "beacon-chip").await;
    assert!(device_id > 0);

    let captures = rest_client.get_captures().send().await.expect("Failed to list captures");
    let cap = captures
        .captures
        .iter()
        .find(|c| c.device_name == device_name)
        .expect("Created device capture must be in GET /v1/captures");
    let chip_id = cap.chip_id.0;

    let patched = rest_client
        .patch_capture(chip_id, &netsim_rest_api::CaptureUpdate { state: true })
        .send()
        .await
        .expect("Failed to patch capture state to true");
    assert!(patched.enabled);

    let path =
        std::env::temp_dir().join(format!("netsim-daemon-{}-capture.pcap", std::process::id()));
    let client = rest_client.clone();
    let download = path.clone();
    tokio::task::spawn_blocking(move || {
        client.get_capture(chip_id).send_to_file_blocking(&download)
    })
    .await
    .unwrap()
    .expect("Failed to download capture PCAP over REST");
    let pcap_bytes = std::fs::read(&path).unwrap();
    std::fs::remove_file(&path).unwrap();
    assert!(pcap_bytes.len() >= 24, "PCAP must contain 24-byte global header");
    assert_eq!(u32::from_le_bytes(pcap_bytes[0..4].try_into().unwrap()), 0xa1b2c3d4);

    let disabled = rest_client
        .patch_capture(chip_id, &netsim_rest_api::CaptureUpdate { state: false })
        .send()
        .await
        .expect("Failed to patch capture state to false");
    assert!(!disabled.enabled);
}
