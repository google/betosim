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

use netsim_model::WifiMode;

use crate::world::ApWorld;

// ============================================================================
// Feature: System Resilience and Lifecycle Management
//
// As a system administrator, I want the AP services to gracefully handle
// system events and resource cleanup to prevent leaks or zombie processes.
// ============================================================================

// Scenario: Actor shuts down when the input stream is closed
// Given a registered AP
// When the input stream is closed
// Then the actor task should exit
#[tokio::test]
async fn test_actor_shutdown_on_stream_close() {
    let mut world = ApWorld::new().await;
    world.given_a_registered_ap("LifecycleAP").await;

    // When
    world.when_input_stream_closed().await;

    // Then
    world.then_actor_shuts_down().await;
}

// Scenario: Actor shuts down when the sink errors (closed)
// Given a registered AP
// When the output sink is closed
// Then the actor task should exit (upon next write attempt)
#[tokio::test]
async fn test_actor_shutdown_on_sink_error() {
    let mut world = ApWorld::new().await;
    world.given_a_registered_ap("SinkErrorAP").await;

    // When
    world.when_sink_closed().await;

    // Then
    world.then_actor_shuts_down().await;
}

// Scenario: Create Duplicate BSSID
// Given a running Actor
// When two APs are created with the same BSSID
// Then both creations succeed (System allows overwriting/sharing)
#[tokio::test]
async fn test_create_duplicate_bssid() {
    let mut world = ApWorld::new().await;

    // Create AP1
    let config1 = ap_actor::ApConfig {
        ssid: "Dup1".to_string(),
        bssid: "02:AA:00:00:00:01".try_into().unwrap(),
        channel: 1,
        hw_mode: WifiMode::G,
        wpa_passphrase: None,
        beacon_interval: 100,
        country_code: None,
        dtim_period: 2,
        hidden_ssid: false,
        sae: false,
        wmm_enabled: true,
        enterprise_enabled: false,
        mac_acl_mode: 0,
        mac_acl_list: vec![],
        ftm_responder_enabled: true,
        position: netsim_model::Position::default(),
    };

    // Create AP2 (Same BSSID)
    let config2 = ap_actor::ApConfig {
        ssid: "Dup2".to_string(),
        bssid: "02:AA:00:00:00:01".try_into().unwrap(),
        channel: 6,
        hw_mode: WifiMode::G,
        wpa_passphrase: None,
        beacon_interval: 100,
        country_code: None,
        dtim_period: 2,
        hidden_ssid: false,
        sae: false,
        wmm_enabled: true,
        enterprise_enabled: false,
        mac_acl_mode: 0,
        mac_acl_list: vec![],
        ftm_responder_enabled: true,
        position: netsim_model::Position::default(),
    };

    // Direct client usage as ApWorld helpers might mask IDs or return types
    let id1 = 1001;
    let id2 = 1002;
    let res1 = world.client.create_ap(Some(id1), config1).await;
    let res2 = world.client.create_ap(Some(id2), config2).await;

    assert!(res1.is_ok(), "First AP creation failed");
    assert!(res2.is_ok(), "Second AP creation with duplicate BSSID failed");
}

// Scenario: Delete Non-Existent AP
// Given a running Actor
// When deleting an AP ID that does not exist
// Then the operation succeeds (Idempotent / No Error)
#[tokio::test]
async fn test_delete_non_existent_ap() {
    let world = ApWorld::new().await;

    // Delete random ID
    let result = world.client.destroy_ap(9999).await;

    assert!(result.is_ok(), "Deleting non-existent AP should not return error");
}

// Scenario: AP Lifecycle CRUD (Create, Read, Update, Delete)
// Given a running Actor
// When an AP is Created, Retrieved, Updated, Listed, and Deleted
// Then all operations reflect the expected state changes
#[tokio::test]
async fn test_ap_lifecycle_crud() {
    let mut world = ApWorld::new().await;

    // 1. Create
    world.given_a_registered_ap("CrudAP").await;
    let id = world.ap_id.expect("AP ID missing");

    // 2. Get
    let ap_state = world.client.get_ap(id).await.expect("Get failed").expect("AP not found");
    assert_eq!(ap_state.config.ssid, "CrudAP");

    // 3. Update
    let updated_state = world
        .client
        .update_ap(id, Some("UpdatedAP".to_string()), None)
        .await
        .expect("Update failed");
    assert_eq!(updated_state.config.ssid, "UpdatedAP");

    // Verify Update with Get
    let ap_state_new = world.client.get_ap(id).await.expect("Get failed").expect("AP not found");
    assert_eq!(ap_state_new.config.ssid, "UpdatedAP");

    // 4. List
    world.given_a_registered_ap("SecondAP").await;
    let list = world.client.list_aps().await.expect("List failed");
    assert!(list.len() >= 2);
    assert!(list.iter().any(|(_, ap)| ap.config.ssid == "UpdatedAP"));
    assert!(list.iter().any(|(_, ap)| ap.config.ssid == "SecondAP"));

    // 5. Delete
    world.when_ap_is_deleted().await; // Deletes the stored ap_id (SecondAP)
    let list_after = world.client.list_aps().await.expect("List failed");
    assert!(!list_after.iter().any(|(_, ap)| ap.config.ssid == "SecondAP"));
    assert!(list_after.iter().any(|(_, ap)| ap.config.ssid == "UpdatedAP")); // First
    // one still
    // there
}
