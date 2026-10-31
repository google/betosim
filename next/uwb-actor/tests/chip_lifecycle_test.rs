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

use netsim_model::ChipError;

use crate::world::World;

// Feature: UWB Chip Lifecycle
//
//   As a client
//   I want to manage the lifecycle of UWB chips
//   So that I can simulate chips appearing and disappearing

// Scenario: Retrieve non-existent chip fails
//
//   Given the UWB actor is running
//   When request to get a chip that does not exist
//   Then the retrieval fails with ChipNotFound error
#[tokio::test]
async fn test_get_chip_not_found() {
    // Given
    let world = World::new().await;
    let chip_id = 99;

    // When
    let result = world.when_get_chip(chip_id).await;

    // Then
    match result.as_ref().map_err(|err| err.as_chip_error()) {
        Err(Some(ChipError::ChipNotFound(id))) => {
            assert_eq!(id.0, chip_id);
        }
        _ => panic!("Expected ChipNotFound error, got {:?}", result),
    }
}

// Scenario: Delete chip successfully
//
//   Given a chip exists
//   When request to delete the chip
//   Then the chip is deleted and cannot be retrieved
#[tokio::test]
async fn test_delete_chip() {
    // Given
    let mut world = World::new().await;
    let chip_id = 3;
    world.given_a_chip(chip_id).await;

    // When
    world.when_delete_chip(chip_id).await.unwrap();

    // Then
    world.then_chip_does_not_exist(chip_id).await;
}

// Scenario: Chip is deleted when its packet stream is closed
//
//   Given a chip exists
//   When its packet stream is closed
//   Then the chip is automatically deleted
#[tokio::test]
async fn test_chip_deleted_on_stream_close() {
    // Given
    let mut world = World::new().await;
    let chip_id = 4;
    world.given_a_chip(chip_id).await;
    world.then_chip_exists(chip_id).await;

    // When
    world.and_packet_stream_is_closed(chip_id);

    // Then
    world.then_chip_does_not_exist(chip_id).await;
}

#[tokio::test]
async fn test_chip_deleted_on_sink_close() {
    let mut world = World::new().await;
    let chip_id = 5;
    world.given_a_chip(chip_id).await;
    world.then_chip_exists(chip_id).await;

    world.and_packet_sink_is_closed(chip_id);
    world.when_chip_is_reset(chip_id).await;

    world.then_chip_does_not_exist(chip_id).await;
}
