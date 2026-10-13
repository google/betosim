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

use netsim_model::ChipId;

use crate::world::World;

// Feature: Link Creation
//
//   As a client
//   I want to create links between chips
//   So that the link manages communication between chips

// Scenario: Successfully create a valid link
//
//   Given the link actor is running with initialized chips
//   When request to create a link between two existing chips
//   Then the link is created successfully
//   And the link appears in the list
#[tokio::test]
async fn test_create_link_succeeds() {
    // Given
    let world = World::new().await;
    world.given_default_chips().await;
    let link_id = world.when_create_link(ChipId(1), ChipId(2), -50).await.unwrap();

    // Then
    let links = world.client.list().await.unwrap();
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].id, link_id);
    assert_eq!(links[0].sender, ChipId(1));
    assert_eq!(links[0].receiver, ChipId(2));
}

// Scenario: Fail to create link with mismatched chip kinds
//
//   Given the link actor is running with BLE and WIFI chips
//   When request to create a link between a BLE chip and a WIFI chip
//   Then the creation fails
#[tokio::test]
async fn test_create_link_fails_mismatch() {
    // Given
    let world = World::new().await;
    world.given_default_chips().await;
    // Chip 1 is BLE, Chip 3 is WIFI (from common setup)
    let result = world.when_create_link(ChipId(1), ChipId(3), -50).await;

    // Then
    assert!(result.is_err());
}

// Scenario: Fail to create link with non-existent receiver chip
//
//   Given the link actor is running
//   When request to create a link involving a non-existent receiver chip ID
//   Then the creation fails
#[tokio::test]
async fn test_create_link_fails_missing() {
    // Given
    let world = World::new().await;
    world.given_default_chips().await;
    // Chip 99 does not exist
    let result = world.when_create_link(ChipId(1), ChipId(99), -50).await;

    // Then
    assert!(result.is_err());
}

// Scenario: Fail to create link with non-existent sender chip
//
//   Given the link actor is running
//   When request to create a link involving a non-existent sender chip ID
//   Then the creation fails
#[tokio::test]
async fn test_create_link_fails_missing_sender() {
    // Given
    let world = World::new().await;
    world.given_default_chips().await;
    // Chip 99 does not exist as sender
    let result = world.when_create_link(ChipId(99), ChipId(2), -50).await;

    // Then
    assert!(result.is_err());
}

// Scenario: Fail to create duplicate link
//
//   Given a link already exists between two chips
//   When request to create another link between the same chips
//   Then the creation fails
#[tokio::test]
async fn test_duplicate_create_fails() {
    // Given
    let world = World::new().await;
    world.given_default_chips().await;
    world.when_create_link(ChipId(1), ChipId(2), -50).await.unwrap();

    // When
    let result = world.when_create_link(ChipId(1), ChipId(2), -50).await;

    // Then
    assert!(result.is_err());
}

// Scenario: Fail to create loopback link
//
//   Given a chip
//   When request to create a link from the chip to itself
//   Then the creation fails
#[tokio::test]
async fn test_create_link_fails_self() {
    // Given
    let world = World::new().await;
    world.given_default_chips().await;
    let result = world.when_create_link(ChipId(1), ChipId(1), -50).await;

    // Then
    assert!(result.is_err());
}

// Scenario: Validation failure does not advance next_id
//
//   Given the link actor is running with default chips
//   When an invalid link creation request fails
//   Then subsequent valid link creations still allocate sequential IDs
#[tokio::test]
async fn test_create_link_validation_failure_does_not_increment_next_id() {
    // Given
    let world = World::new().await;
    world.given_default_chips().await;

    // When an invalid request fails (self-link)
    let err_result = world.when_create_link(ChipId(1), ChipId(1), -50).await;
    assert!(err_result.is_err());

    // Then a valid link creation starts from the initial ID (1)
    let first_id = world.when_create_link(ChipId(1), ChipId(2), -50).await.unwrap();
    assert_eq!(first_id, link_api::LinkId(1));

    // When another invalid request fails (missing chip)
    let missing_result = world.when_create_link(ChipId(1), ChipId(99), -50).await;
    assert!(missing_result.is_err());

    // Then subsequent valid link receives the next sequential ID (2)
    let second_id = world.when_create_link(ChipId(2), ChipId(1), -60).await.unwrap();
    assert_eq!(second_id, link_api::LinkId(2));
}

// Scenario: Auto-allocation skips existing custom link IDs
//
//   Given the link actor is running with default chips
//   When a link is created with an explicit custom ID ahead in sequence
//   Then subsequent auto-allocated links skip the custom ID without colliding
#[tokio::test]
async fn test_allocate_link_id_skips_existing_custom_ids() {
    // Given
    let world = World::new().await;
    world.given_default_chips().await;

    // Create link 1 (auto-allocated as ID 1)
    let id1 = world.when_create_link(ChipId(1), ChipId(2), -50).await.unwrap();
    assert_eq!(id1, link_api::LinkId(1));

    // Delete link 1 so chip pair (1, 2) is free
    world.client.delete(id1).await.unwrap();

    // Create a link with custom ID 2
    let params_custom = link_api::LinkCreate { sender: ChipId(1), receiver: ChipId(2), rssi: -55 };
    let id_custom = world.client.create_with_id(link_api::LinkId(2), params_custom).await.unwrap();
    assert_eq!(id_custom, link_api::LinkId(2));

    // When auto-allocating another link (pair 2 -> 1)
    // next_id is currently 2, which already exists, so it must skip to 3
    let id3 = world.when_create_link(ChipId(2), ChipId(1), -60).await.unwrap();
    assert_eq!(id3, link_api::LinkId(3));

    // Verify both links coexist without collision
    let list = world.client.list().await.unwrap();
    assert_eq!(list.len(), 2);
    assert!(list.iter().any(|l| l.id == link_api::LinkId(2)));
    assert!(list.iter().any(|l| l.id == link_api::LinkId(3)));
}
