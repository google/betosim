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

use netsim_model::{ChipId, ChipKind};

use crate::world::World;

// Feature: Link Lifecycle
//
//   As a system
//   I want links to be managed consistent with chip lifecycle
//   So that no invalid links exist

// Scenario: Chip removal deletes associated links
//
//   Given a link exists
//   When one of the chips involved is removed
//   Then the link is automatically deleted
#[tokio::test]
async fn test_chip_removal_deletes_links() {
    // Given
    let world = World::new().await;
    world.given_default_chips().await;
    let link_id = world.when_create_link(ChipId(1), ChipId(2), -50).await.unwrap();
    let link_id_rev = world.when_create_link(ChipId(2), ChipId(1), -55).await.unwrap();
    assert_eq!(world.client.list().await.unwrap().len(), 2);

    // When
    world.when_notify_chip_removed(ChipId(1)).await;

    // Then
    let links = world.client.list().await.unwrap();
    assert!(links.is_empty(), "Link should be deleted after chip removal");

    // Check that we cannot update the deleted link
    let update_result = world.when_update_link(link_id, -60).await;
    assert!(update_result.is_err(), "Should not be able to update deleted link");
    let update_result_rev = world.when_update_link(link_id_rev, -60).await;
    assert!(update_result_rev.is_err(), "Should not be able to update deleted link");
}

// Scenario: Cannot create link with removed chip
//
//   Given a chip was removed
//   When request to create a link involving that chip
//   Then the creation fails
#[tokio::test]
async fn test_create_link_fails_after_remove() {
    // Given
    let world = World::new().await;
    world.given_default_chips().await;
    // Ensure chip 1 exists (setup provides it)
    world.when_notify_chip_removed(ChipId(1)).await;

    // When
    let result = world.when_create_link(ChipId(1), ChipId(2), -60).await;

    // Then
    assert!(result.is_err());
}

// Scenario: Late chip addition enables link creation
//
//   Given a new chip is added dynamically
//   When request to create a link involving the new chip
//   Then the link is created successfully
#[tokio::test]
async fn test_chip_added_lifecycle() {
    // Given
    let world = World::new().await;
    world.given_default_chips().await;
    assert!(world.when_create_link(ChipId(99), ChipId(2), -50).await.is_err());

    // When
    world.when_notify_chip_added(ChipId(99), ChipKind::BLUETOOTH).await;

    // Then
    world.when_create_link(ChipId(99), ChipId(2), -50).await.unwrap();
}

// Scenario: Chip removal with no active links
//
//   Given a chip exists with no active links
//   When the chip is removed
//   Then the operation succeeds without errors
#[tokio::test]
async fn test_chip_removal_without_links() {
    let world = World::new().await;
    world.given_default_chips().await;
    assert_eq!(world.client.list().await.unwrap().len(), 0);

    world.when_notify_chip_removed(ChipId(1)).await;

    assert_eq!(world.client.list().await.unwrap().len(), 0);
}

// Scenario: Reset restores initial state and restarts link ID allocation
//
//   Given links exist
//   When a reset action occurs
//   Then all links are cleared and subsequent links allocate starting from INITIAL_LINK_ID
#[tokio::test]
async fn test_reset_restores_initial_state_and_link_ids() {
    // Given
    let world = World::new().await;
    world.given_default_chips().await;
    let link_id = world.when_create_link(ChipId(1), ChipId(2), -50).await.unwrap();
    assert_eq!(link_id, link_api::LinkId(1));
    assert_eq!(world.client.list().await.unwrap().len(), 1);

    // When reset is called
    link_api::LinkClient::reset(&world.client).await.unwrap();

    // Then links are cleared
    let links = world.client.list().await.unwrap();
    assert!(links.is_empty());

    // And new links re-start ID allocation from INITIAL_LINK_ID (1)
    let new_link_id = world.when_create_link(ChipId(1), ChipId(2), -60).await.unwrap();
    assert_eq!(new_link_id, link_api::LinkId(1));
}
