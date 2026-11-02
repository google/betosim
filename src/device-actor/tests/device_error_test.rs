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

// Feature: Device Error Handling
//
//   As a user
//   I want the system to handle invalid operations gracefully
//   So that my application doesn't crash on bad input

use device_api::{DeviceId, DeviceUpdate};

use crate::world::World;

// Scenario: Update a non-existent device
//   Given a running Device Actor
//   When I attempt to update a device that does not exist
//   Then the operation fails with a DeviceNotFound error
#[tokio::test]
async fn test_update_non_existent_device_fails() {
    // Given a running Device Actor
    let world = World::new().await;

    // When I attempt to update a device that does not exist
    let non_existent_id = DeviceId(9999);
    let update = DeviceUpdate::default();

    let result = world.client.update(non_existent_id, update).await;

    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("Device not found"));
}

// Scenario: Delete a non-existent device
//   Given a running Device Actor
//   When I attempt to delete a device that does not exist
//   Then the operation fails with a DeviceNotFound error
#[tokio::test]
async fn test_delete_non_existent_device_fails() {
    // Given a running Device Actor
    let world = World::new().await;

    // When I attempt to delete a device that does not exist
    let non_existent_id = DeviceId(9999);
    let result = world.client.delete(non_existent_id).await;

    // Then the operation fails with a DeviceNotFound error
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("Device not found"));
}

// Scenario: Add chip with unsupported kind fails
//   Given a running Device Actor and an existing device
//   When I attempt to add a chip with an unsupported kind
//   Then the operation fails with a ChipKindNotSupported error
#[tokio::test]
async fn test_add_chip_unsupported_kind_fails() {
    let world = World::new().await;
    let device_guid = "guid-unsupported";
    let _ = world.when_add_chip(device_guid, "beacon").await;

    let mut params = World::create_device_add_chip_params(
        device_guid.to_string(),
        "unsupported-chip".to_string(),
        "00:00:00:00:00:00".to_string(),
    );
    params.chip.kind = netsim_model::ChipKind::ETHERNET;
    params.chip.variant = Some(netsim_model::ChipVariant::Ethernet(netsim_model::Ethernet {
        radio: Default::default(),
    }));
    let result = world.client.add_chip(params).await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("No chip client for ETHERNET"));
}

// Scenario: Notify chip removed on non-existent device
//   Given a running Device Actor
//   When notify_chip_removed is called for a non-existent device
//   Then the operation fails with a DeviceNotFound error
#[tokio::test]
async fn test_notify_chip_removed_device_not_found() {
    let world = World::new().await;
    let result = world.client.notify_chip_removed(DeviceId(9999), netsim_model::ChipId(1)).await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("Device not found"));
}

// Scenario: Notify chip removed for non-existent chip on existing device
//   Given a running Device Actor and a created device
//   When notify_chip_removed is called for a non-existent chip
//   Then the operation completes without error
#[tokio::test]
async fn test_notify_chip_removed_chip_not_found() {
    let world = World::new().await;
    let device_id = world.when_create_device("test-dev").await;
    let result = world.client.notify_chip_removed(device_id, netsim_model::ChipId(9999)).await;
    assert!(result.is_ok());
}
