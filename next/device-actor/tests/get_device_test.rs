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

// Feature: Get Device
//
//   As a user
//   I want to retrieve a device by its ID
//   So that I can inspect its current state

use device_api::DeviceId;

use crate::world::World;

// Scenario: Get an existing device
//   Given a running Device Actor
//   When I create a device
//   And I request to get the device by its ID
//   Then the device is returned with the correct name and state
#[tokio::test]
async fn test_get_existing_device() {
    // Given a running Device Actor
    let world = World::new().await;

    // When I create a device
    let device_name = "get-test-dev";
    let device_id = world.when_create_device(device_name).await;

    // And I request to get the device by its ID
    let device_opt = world.client.get(device_id).await.unwrap();

    // Then the device is returned with the correct name and state
    assert!(device_opt.is_some());
    let device = device_opt.unwrap();
    assert_eq!(device.name, device_name);
    assert_eq!(device.id, device_id.0);
}

// Scenario: Get a non-existent device
//   Given a running Device Actor
//   When I request to get a device by a non-existent ID
//   Then None is returned (no error, just empty result)
#[tokio::test]
async fn test_get_non_existent_device() {
    // Given a running Device Actor
    let world = World::new().await;

    // When I request to get a device by a non-existent ID
    let non_existent_id = DeviceId(9999);
    let device_opt = world.client.get(non_existent_id).await.unwrap();

    // Then None is returned
    assert!(device_opt.is_none());
}
