// Copyright (C) 2025 The Android Open Source Project
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

// Feature: Create Device
//
//   As a user
//   I want to create a device
//   So that I can simulate its behavior

use crate::world::World;

// Scenario: Successfully create a device
//   Given a running Device Actor
//   When I create a new device
//   Then the device creation succeeds
//   And the returned device properties match the configuration
#[tokio::test]
async fn test_create_device_succeeds() {
    // Given a running Device Actor
    let world = World::new().await;

    // When I create a new device
    let device_name = "test-dev-1";
    let device_id = world.when_create_device(device_name).await;

    // Then the device creation succeeds
    // (implied by awaiting match in when_create_device)

    // And the returned device properties match the configuration
    let device = world.client.get(device_id).await.unwrap().unwrap();
    assert_eq!(device.name, device_name);
    assert_eq!(device.chips.len(), 1);
}
