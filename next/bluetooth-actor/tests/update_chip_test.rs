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

use netsim_model::Position;

use crate::world::World;

// Feature: Update Chip Properties
//
//   As a client
//   I want to update bluetooth chip properties
//   So that I can simulate device movement and other changes

// Scenario: Update chip position
//
//   Given a bluetooth chip
//   When the position is updated
//   Then the chip reflects the new position
#[tokio::test]
async fn test_update_position() {
    let mut world = World::new();

    // Given a bluetooth chip
    world.given_device("A").await;

    // When the position is updated
    let new_pos = Position { x: 10.0, y: 20.0, z: 30.0 };
    world.when_update_chip_position("A", new_pos.clone()).await;

    // Then the chip reflects the new position
    world.then_chip_position_is("A", new_pos).await;
}
