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

// Feature: Add Chip Integration
//
//   As a user
//   I want to add chips to devices via PacketStream
//   So that I can support multiple chips per device and dynamic attachment

use std::collections::HashMap;

use netsim_model::{ChipKind, MockChipClient};

use crate::world::World;

// Scenario: Add chip to new device
//   Given a running Device Actor
//   When I add a chip
//   Then a new device is created
//   And the device contains the chip
#[tokio::test]
async fn test_add_chip_creates_new_device() {
    // Given a running Device Actor
    let world = World::new().await;

    // When I add a chip
    let device_guid = "guid-1";
    let device_id = world.when_add_chip(device_guid, "beacon").await;

    // Then a new device is created (implied by ID return)

    // And the device contains the chip
    let device = world.client.get(device_id).await.unwrap().unwrap();
    assert_eq!(device.chips.len(), 1);
    assert_eq!(device.chips[0].name, "beacon");
}

// Scenario: Add chip to existing device
//   Given a running Device Actor
//   When I add a chip
//   And I add another chip with the same device GUID
//   Then the device contains both chips
#[tokio::test]
async fn test_add_chip_to_existing_device() {
    // Given a running Device Actor with expectation for multiple Create calls
    // on ChipClient
    let mut mock_chip_client = MockChipClient::new();
    mock_chip_client.expect_create().times(2).returning(|_, _| Ok(()));
    mock_chip_client.expect_read_statistics().returning(|| Ok(Box::from(Vec::new())));
    mock_chip_client.expect_read().returning(|id| {
        Ok(netsim_model::Chip {
            id: id.0,
            kind: netsim_model::ChipKind::BLUETOOTH,
            ..Default::default()
        })
    });

    let mut chip_clients = HashMap::new();
    chip_clients.insert(
        ChipKind::BLUETOOTH,
        Box::new(mock_chip_client) as Box<dyn netsim_model::ChipClient>,
    );

    let mut mock_link_client = link_api::MockLinkClient::new();
    mock_link_client.expect_action().returning(|_, _| Ok(()));
    mock_link_client.expect_create().returning(|_| Ok(link_api::LinkId(0)));
    mock_link_client.expect_notify_chip_added().returning(|_, _| Ok(()));

    let world = World::with_clients(chip_clients, mock_link_client).await;

    // When I add a chip
    let device_guid = "guid-shared";
    let device_id_1 = world.when_add_chip(device_guid, "beacon-1").await;

    // And I add another chip with the same device GUID
    let device_id_2 = world.when_add_chip(device_guid, "beacon-2").await;

    // Then the device contains both chips and IDs match
    assert_eq!(device_id_1, device_id_2);

    let device = world.client.get(device_id_1).await.unwrap().unwrap();
    assert_eq!(device.chips.len(), 2);

    // Verify names
    let names: Vec<String> = device.chips.iter().map(|c| c.name.clone()).collect();
    assert!(names.contains(&"beacon-1".to_string()));
    assert!(names.contains(&"beacon-2".to_string()));
}

// Scenario: Concurrent Add Chip Race Condition
//   Given a running Device Actor
//   When I add two chips with the same device GUID concurrently
//   Then only one device is created
//   And duplicate devices are prevented
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_concurrent_add_chip_race_condition() {
    // Given a running Device Actor with expectation for multiple Create calls
    // on ChipClient We expect 2 chips to be created (one for each add_chip
    // call)
    let mut mock_chip_client = MockChipClient::new();
    mock_chip_client.expect_create().times(2).returning(|_, _| Ok(()));
    mock_chip_client.expect_read_statistics().returning(|| Ok(Box::from(Vec::new())));
    mock_chip_client.expect_read().returning(|id| {
        Ok(netsim_model::Chip {
            id: id.0,
            kind: netsim_model::ChipKind::BLUETOOTH,
            ..Default::default()
        })
    });
    // Allow cleanup deletions
    mock_chip_client.expect_delete().returning(|_| Ok(()));

    let mut chip_clients = HashMap::new();
    chip_clients.insert(
        ChipKind::BLUETOOTH,
        Box::new(mock_chip_client) as Box<dyn netsim_model::ChipClient>,
    );

    let mut mock_link_client = link_api::MockLinkClient::new();
    mock_link_client.expect_action().returning(|_, _| Ok(()));
    mock_link_client.expect_create().returning(|_| Ok(link_api::LinkId(0)));
    mock_link_client.expect_notify_chip_added().returning(|_, _| Ok(()));

    let world = World::with_clients(chip_clients, mock_link_client).await;
    let client = world.client.clone();

    // When I add two chips with the same device GUID concurrently
    let device_guid = "concurrent-guid";
    let (id1, id2) = world.when_concurrently_add_chips(device_guid, "beacon-1", "beacon-2").await;

    // Then only one device is created (IDs must match)
    assert_eq!(id1, id2, "Device IDs should match for the same GUID even with concurrent creation");
}

// Scenario: Position propagation during Add Chip
//   Given a running Device Actor
//   When I add a chip with a device config containing position
//   Then the chip creation is invoked with that position
#[tokio::test]
async fn test_add_chip_propagates_position() {
    let expected_pos = netsim_model::Position { x: 10.0, y: 20.0, z: 30.0 };
    let expected_orient = netsim_model::Orientation { yaw: 1.0, pitch: 2.0, roll: 3.0 };

    let mut mock_chip_client = MockChipClient::new();
    mock_chip_client.expect_create().times(1).returning(move |_, params| {
        assert_eq!(
            params.chip.pose.position, expected_pos,
            "Passed position should match device position"
        );
        assert_eq!(
            params.chip.pose.orientation, expected_orient,
            "Passed orientation should match device orientation"
        );
        Ok(())
    });
    mock_chip_client.expect_read_statistics().returning(|| Ok(Box::from(Vec::new())));
    mock_chip_client.expect_read().returning(|id| {
        Ok(netsim_model::Chip {
            id: id.0,
            kind: netsim_model::ChipKind::BLUETOOTH,
            ..Default::default()
        })
    });

    let mut chip_clients = HashMap::new();
    chip_clients.insert(
        ChipKind::BLUETOOTH,
        Box::new(mock_chip_client) as Box<dyn netsim_model::ChipClient>,
    );

    let mut mock_link_client = link_api::MockLinkClient::new();
    mock_link_client.expect_action().returning(|_, _| Ok(()));
    mock_link_client.expect_create().returning(|_| Ok(link_api::LinkId(0)));
    mock_link_client.expect_notify_chip_added().returning(|_, _| Ok(()));

    let world = World::with_clients(chip_clients, mock_link_client).await;

    world
        .when_add_chip_with_position("guid-with-pos", "beacon", expected_pos, expected_orient)
        .await;
}

// Scenario: Add chip with name matching device name
//   Given a running Device Actor
//   When I add a chip whose name matches the device name
//   Then the chip is added and device contains the chip
#[tokio::test]
async fn test_add_chip_with_matching_device_name() {
    let world = World::new().await;

    // device_config default name is "test-dev", so let chip_name also be
    // "test-dev"
    let device_guid = "guid-matching-name";
    let device_id = world.when_add_chip(device_guid, "test-dev").await;

    let device = world.client.get(device_id).await.unwrap().unwrap();
    assert_eq!(device.chips.len(), 1);
    assert_eq!(device.chips[0].name, "test-dev");
}

// Scenario: Add chip with empty name
//   Given a running Device Actor
//   When I add a chip with an empty name
//   Then the chip is added successfully
#[tokio::test]
async fn test_add_chip_with_empty_name() {
    let world = World::new().await;

    let device_guid = "guid-empty-name";
    let device_id = world.when_add_chip(device_guid, "").await;

    let device = world.client.get(device_id).await.unwrap().unwrap();
    assert_eq!(device.chips.len(), 1);
    assert_eq!(device.chips[0].name, "test-dev");
}
