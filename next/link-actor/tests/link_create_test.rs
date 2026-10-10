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
