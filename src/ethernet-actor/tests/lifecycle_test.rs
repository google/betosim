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

use netsim_model::ChipKind;

use super::world::World;

// Scenario: Creating an Ethernet chip stores the chip under the requested ID
#[tokio::test]
async fn test_create_ethernet_chip_stores_chip_under_requested_id() {
    // Given an initialized EthernetActor world
    let mut world = World::new().await;
    // When an Ethernet chip is created with ID 11
    world.when_create_chip(11, ChipKind::ETHERNET).await;
    // Then the stored chip matches the default Ethernet properties and the chip count is 1
    world.then_chip_matches(11, ChipKind::ETHERNET, "eth0", true, None).await;
    world.then_chip_count_is(1).await;
}

// Scenario: Creating a cellular data chip is accepted and stored
#[tokio::test]
async fn test_create_cellular_data_chip_is_accepted() {
    // Given an initialized EthernetActor world
    let mut world = World::new().await;
    // When a cellular data chip is created with ID 12
    world.when_create_chip(12, ChipKind::CELLULAR_DATA).await;
    // Then the stored chip matches the default cellular data properties and the chip count is 1
    world.then_chip_matches(12, ChipKind::CELLULAR_DATA, "eth0", true, None).await;
    world.then_chip_count_is(1).await;
}

// Scenario: Creating unsupported chip kinds is rejected with InvalidChipKind
#[tokio::test]
async fn test_create_rejects_unsupported_chip_kinds() {
    // Given an initialized EthernetActor world
    let mut world = World::new().await;
    // When creating chips with unsupported wireless chip kinds
    for kind in [ChipKind::BLUETOOTH, ChipKind::WIFI, ChipKind::UWB] {
        world.when_create_chip(13, kind).await;
        // Then each create operation fails with Invalid ChipKind
        world.then_last_error_contains("Invalid ChipKind");
    }
    world.then_chip_count_is(0).await;
    world.then_chip_not_found(13).await;
}

// Scenario: Operations on an unknown chip ID are rejected without side effects
#[tokio::test]
async fn test_unknown_chip_operations_rejected_without_side_effects() {
    // Given an initialized EthernetActor world with no chips
    let mut world = World::new().await;

    // When reading, updating, resetting, and deleting an unknown chip ID 99
    world.when_read_chip(99).await;
    world.then_last_error_is_chip_not_found(99);

    world.when_update_chip(99, Some("eth1"), Some(false), Some(false)).await;
    world.then_last_error_contains("Chip 99 not found");

    world.when_reset_chip(99).await;
    world.then_last_error_contains("Chip 99 not found");

    world.when_delete_chip(99).await;
    world.then_last_operation_succeeded();

    // Then no chips exist and no device removal notification is emitted
    world.then_chip_count_is(0).await;
    world.then_no_device_notification().await;
}

// Scenario: Updating a chip persists its name, enabled flag, and radio state
#[tokio::test]
async fn test_update_persists_name_enabled_and_radio_state() {
    let mut world = World::new().await;
    // Given an active Ethernet chip with ID 14
    world.given_chip(14, ChipKind::ETHERNET).await;
    // When the chip is updated with a new name, disabled flag, and disabled radio state
    world.when_update_chip(14, Some("eth1"), Some(false), Some(false)).await;
    // Then reading the chip reflects the updated name, enabled state, and radio state
    world.then_chip_matches(14, ChipKind::ETHERNET, "eth1", false, Some(false)).await;
}

// Scenario: Resetting an active chip returns the reset chip entity
#[tokio::test]
async fn test_reset_returns_active_chip() {
    let mut world = World::new().await;
    // Given an active Ethernet chip with ID 15
    world.given_chip(15, ChipKind::ETHERNET).await;
    // When the chip is reset
    world.when_reset_chip(15).await;
    // Then the returned chip has enabled set to true
    world.then_last_reset_chip_matches(15, ChipKind::ETHERNET, "eth0", true);
}

// Scenario: Deleting an active chip removes it and notifies the device actor
#[tokio::test]
async fn test_delete_removes_chip_and_notifies_device() {
    let mut world = World::new().await;
    // Given an active Ethernet chip with ID 16
    world.given_chip(16, ChipKind::ETHERNET).await;
    // When the chip is deleted
    world.when_delete_chip(16).await;
    // Then the chip is removed from the actor and the device actor is notified
    world.then_chip_count_is(0).await;
    world.then_chip_not_found(16).await;
    world.then_device_notified_chip_removed(16).await;
}

// Scenario: Listing chips returns all active Ethernet and cellular data chips and reflects deletions
#[tokio::test]
async fn test_list_returns_active_ethernet_and_cellular_data_chips_and_reflects_deletions() {
    let mut world = World::new().await;
    // Given an active Ethernet chip and an active cellular data chip
    world.given_chip(18, ChipKind::ETHERNET).await;
    world.given_chip(19, ChipKind::CELLULAR_DATA).await;

    // When listing all chips via EthernetClient::list
    world.when_list_chips().await;
    // Then both active chip IDs are returned
    world.then_listed_chip_ids_match(&[18, 19]);

    // When one chip is deleted and chips are listed again
    world.when_delete_chip(18).await;
    world.when_list_chips().await;
    // Then only the remaining active chip ID is returned
    world.then_listed_chip_ids_match(&[19]);
}

// Scenario: Querying radio statistics, global statistics, and shutting down the actor succeed
#[tokio::test]
async fn test_statistics_reporting_and_shutdown() {
    let mut world = World::new().await;
    // Given an active Ethernet chip with ID 17
    world.given_chip(17, ChipKind::ETHERNET).await;

    // When reading radio statistics, global statistics, and shutting down the actor
    world.when_read_statistics().await;
    world.then_statistics_are_empty();

    world.when_get_global_stats().await;
    world.then_global_stats_reported();

    world.when_shutdown().await;
    // Then all operations complete without error
    world.then_last_operation_succeeded();
}
