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

// Feature: Link Deletion
//
//   As a client
//   I want to delete links
//   So that I can disconnect chips

// Scenario: Successfully delete an existing link
//
//   Given a link exists between two chips
//   When request to delete the link
//   Then the link is removed from the list
#[tokio::test]
async fn test_delete_link_succeeds() {
    // Given
    let world = World::new().await;
    world.given_default_chips().await;
    let link_id = world.when_create_link(ChipId(1), ChipId(2), -50).await.unwrap();
    assert_eq!(world.client.list().await.unwrap().len(), 1);

    // When
    world.client.delete(link_id).await.unwrap();

    // Then
    let links = world.client.list().await.unwrap();
    assert!(links.is_empty());
}

// Scenario: Fail to delete non-existent link
//
//   Given the link actor is running
//   When request to delete a non-existent link ID
//   Then the deletion fails
#[tokio::test]
async fn test_delete_missing_link_fails() {
    // Given
    let world = World::new().await;
    let missing_id = link_api::LinkId(999);

    // When
    let result = world.client.delete(missing_id).await;

    // Then
    assert!(result.is_err());
}
