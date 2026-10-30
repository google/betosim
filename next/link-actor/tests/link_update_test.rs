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

use link_api::LinkId;
use netsim_model::ChipId;

use crate::world::World;

// Feature: Link Update
//
//   As a client
//   I want to update link properties (e.g. RSSI)
//   So that simulation state reflects changes

// Scenario: Successfully update link RSSI
//
//   Given a link exists
//   When request to update the link's RSSI
//   Then the link reflects the new RSSI value
#[tokio::test]
async fn test_update_link_rssi() {
    // Given
    let world = World::new().await;
    world.given_default_chips().await;
    let link_id = world.when_create_link(ChipId(1), ChipId(2), -50).await.unwrap();

    // When
    let update_result = world.when_update_link(link_id, -70).await;

    // Then
    assert!(update_result.is_ok());
    let links = world.client.list().await.unwrap();
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].id, link_id);
    assert_eq!(links[0].rssi, -70);
}

// Scenario: Fail to update non-existent link
//
//   Given the link actor is running
//   When request to update a non-existent link
//   Then the update fails
#[tokio::test]
async fn test_update_missing_link_fails() {
    // Given
    let world = World::new().await;
    let missing_id = LinkId(999);

    // When
    let result = world.when_update_link(missing_id, -70).await;

    // Then
    assert!(result.is_err());
}
