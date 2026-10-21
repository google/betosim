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
