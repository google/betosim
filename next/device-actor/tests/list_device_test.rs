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

// Feature: List Devices
//
//   As a user
//   I want to list all created devices
//   So that I can see the current state of the simulation

use netsim_model::ChipKind;

use crate::world::World;

// Scenario: List devices when no devices exist
//   Given a running Device Actor
//   When I list devices
//   Then the list is empty
#[tokio::test]
async fn test_list_devices_empty() {
    // Given a running Device Actor
    let world = World::new().await;

    // When I list devices
    let devices = world.client.list().await.unwrap();

    // Then the list is empty
    assert!(devices.is_empty());
}

// Scenario: List devices with created devices
//   Given a running Device Actor
//   When I create multiple devices
//   And I list devices
//   Then the list contains all created devices
#[tokio::test]
async fn test_list_devices_populated() {
    // Given a running Device Actor
    let world = World::new().await;

    // When I create multiple devices
    let device_names = vec!["dev-1", "dev-2", "dev-3"];
    let mut created_ids = Vec::new();

    for name in &device_names {
        created_ids.push(world.when_create_device(name).await);
    }

    // And I list devices
    let devices = world.client.list().await.unwrap();

    // Then the list contains all created devices
    assert_eq!(devices.len(), 3);

    for id in created_ids {
        assert!(devices.iter().any(|d| d.id == id.0));
    }
}

// Scenario: Create a device with a single Bluetooth chip
//   Given a running Device Actor
//   When I create a device with a Bluetooth chip
//   And I list devices
//   Then the device has exactly one chip
#[tokio::test]
async fn test_create_single_bluetooth_chip() {
    // Given a running Device Actor
    let world = World::new().await;

    // When I create a device with a Bluetooth chip
    // Using when_add_chip with a unique GUID to create a new device
    let _id = world.when_add_chip("test-guid-1", "bt-chip").await;

    // And I list devices
    let devices = world.client.list().await.unwrap();

    // Then the device has exactly one chip
    assert_eq!(devices.len(), 1);
    let device = &devices[0];
    assert_eq!(device.chips.len(), 1);

    // Check that the chip kind is Bluetooth
    let chip = &device.chips[0];
    assert_eq!(chip.kind, ChipKind::BLUETOOTH);

    // Verify that the variant contains both BLE and Classic radios
    assert_eq!(
        chip.variant,
        Some(netsim_model::ChipVariant::Bluetooth(Box::new(netsim_model::Bluetooth {
            low_energy: Default::default(),
            classic: Default::default(),
            ..Default::default()
        })))
    );
}
