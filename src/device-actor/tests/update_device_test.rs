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

// Feature: Update Device
//
//   As a user
//   I want to update device properties
//   So that I can simulate dynamic changes in the environment

use std::collections::HashMap;

use device_api::DeviceUpdate;
use netsim_model::{
    Chip, ChipClient, ChipId, ChipKind, ChipUpdate, ChipVariantUpdate, MockChipClient, WifiUpdate,
};

use crate::world::World;

// Scenario: Update device properties propagates to chips
//   Given a running Device Actor with expectation for Update call
//   When I create a device and update its properties
//   Then the device properties are updated
//   And the Chip Client received the expected Update call
#[tokio::test]
async fn test_update_device_propagates_to_chips() {
    // Given a running Device Actor with expectation for Update call
    let mut mock_chip_client = MockChipClient::new();
    mock_chip_client.expect_create().times(1).returning(|_, _| Ok(()));
    mock_chip_client
        .expect_update()
        .withf(|_, patch| patch.pose.position.is_some() && patch.pose.orientation.is_some())
        .times(1)
        .returning(|id, _| Ok(Chip { id: id.0, kind: ChipKind::BLUETOOTH, ..Default::default() }));

    let mut chip_clients = HashMap::new();
    chip_clients.insert(ChipKind::BLUETOOTH, Box::new(mock_chip_client) as Box<dyn ChipClient>);

    // Use default link mock
    let mock_link_client = World::create_default_link_client();

    let world = World::with_clients(chip_clients, mock_link_client).await;

    // When I create a device and update its properties
    let device_name = "test-dev";
    let device_id = world.when_create_device(device_name).await;

    let mut update = DeviceUpdate::default();
    update.id = device_id.0;
    update.pose.position = Some(device_api::Position { x: 10.0, y: 10.0, z: 0.0 });
    update.pose.orientation = Some(device_api::Orientation { yaw: 1.0, pitch: 0.0, roll: 0.0 });
    update.name = Some("updated-name".to_string());

    world.when_update_device(device_id, update).await;

    // Then the device properties are updated
    let device = world.client.get(device_id).await.unwrap().unwrap();
    assert_eq!(device.pose.position.x, 10.0);
    assert_eq!(device.pose.orientation.yaw, 1.0);
    assert_eq!(device.name, "updated-name");

    // And the Chip Client received the expected Update call (verified by
    // checkpoint)
}

// Scenario: Notify chip removed
//   Given a running Device Actor
//   When I create a device and notify that a chip was removed
//   Then the device no longer contains the chip
#[tokio::test]
async fn test_notify_chip_removed() {
    // Given a running Device Actor
    let world = World::new().await;

    // When I create a device and notify that a chip was removed
    let device_name = "test-dev";
    let device_id = world.when_create_device(device_name).await;
    let device = world.client.get(device_id).await.unwrap().unwrap();
    let chip_id = ChipId(device.chips[0].id);

    world.when_notify_chip_removed(device_id, chip_id).await;

    // Then the device no longer contains the chip (it is auto-deleted)
    let device = world.client.get(device_id).await.unwrap();
    assert!(device.is_none());
}

// Scenario: Notify chip removed on a multi-chip device preserves remaining chips
//   Given a running Device Actor with a device containing a Bluetooth chip and a Wi-Fi chip
//   When I notify that the Bluetooth chip was removed
//   Then the Bluetooth chip is deleted while the device and Wi-Fi chip remain active
//   And a duplicate removal notification for the same chip is a no-op
#[tokio::test]
async fn test_notify_chip_removed_preserves_remaining_chips() {
    let world = World::new().await;

    let bt_params = World::create_device_add_chip_params(
        "guid-1".to_string(),
        "bt-chip".to_string(),
        "00:00:00:00:00:01".to_string(),
    );
    let (device_id, bt_chip_id) = world.client.add_chip_with_id(bt_params).await.unwrap();

    let mut wifi_params = World::create_device_add_chip_params(
        "guid-1".to_string(),
        "wifi-chip".to_string(),
        String::new(),
    );
    wifi_params.chip.kind = ChipKind::WIFI;
    wifi_params.chip.variant = None;
    let (device_id_2, wifi_chip_id) = world.client.add_chip_with_id(wifi_params).await.unwrap();
    assert_eq!(device_id, device_id_2);

    // Removing the Bluetooth chip preserves the device and Wi-Fi chip, and deletes the BT chip
    // from mock_chips
    world.when_notify_chip_removed(device_id, bt_chip_id).await;
    assert!(!world.mock_chips.lock().unwrap().contains_key(&bt_chip_id));
    assert!(world.mock_chips.lock().unwrap().contains_key(&wifi_chip_id));

    // Duplicate notification for the already-removed chip is a no-op
    world.when_notify_chip_removed(device_id, bt_chip_id).await;
    let device = world.client.get(device_id).await.unwrap().unwrap();
    assert_eq!(device.chips.len(), 1);
    assert_eq!(device.chips[0].id, wifi_chip_id.0);
}

// Scenario: Update device propagates by variant when no ID is provided
//   Given a running Device Actor with expectation for Update call
//   When I create a device and update it using a variant (e.g. Bluetooth)
// without Chip ID   Then the correct chip (Bluetooth) receives the update
#[tokio::test]
async fn test_update_device_propagates_by_variant() {
    // Given a running Device Actor with expectation for Update call
    let mut mock_chip_client = MockChipClient::new();
    mock_chip_client.expect_create().times(1).returning(|_, _| Ok(()));
    mock_chip_client
        .expect_update()
        // Verify that update is called despite NO ID being provided in the patch
        .withf(|_, patch| patch.variant.is_some())
        .times(1)
        .returning(|id, _| Ok(Chip { id: id.0, kind: ChipKind::BLUETOOTH, ..Default::default() }));

    let mut chip_clients = HashMap::new();
    chip_clients.insert(ChipKind::BLUETOOTH, Box::new(mock_chip_client) as Box<dyn ChipClient>);

    // Use default link mock
    let mock_link_client = World::create_default_link_client();

    let world = World::with_clients(chip_clients, mock_link_client).await;

    // When I create a device and update it using a variant without Chip ID
    let device_name = "test-dev";
    let device_id = world.when_create_device(device_name).await;

    // And I update it with a generic Bluetooth variant update (NO ID)
    let chip_update = World::create_bluetooth_chip_update(None, None);
    world.when_update_device_chip(device_id, chip_update).await;
}

// Scenario: Update Chip Radio State via Device Update
//   Given a running Device Actor with expectation for Chip Update
//   When I create a device and update its Bluetooth Low Energy state to
// disabled   Then the Chip Client receives the updates with the correct states
//   And I update the device to enable the Bluetooth Low Energy state
//   Then the Chip Client receives the updates with the correct states
#[tokio::test]
async fn test_update_chip_ble_radio_state() {
    // Given a running Device Actor with expectation for Update call
    let mut mock_chip_client = MockChipClient::new();
    mock_chip_client.expect_create().times(1).returning(|_, _| Ok(()));

    let mut seq = mockall::Sequence::new();

    mock_chip_client
        .expect_update()
        // Verify 1: Update to FALSE (Disabled)
        .withf(|_, patch| {
            if let Some(ChipVariantUpdate::Bluetooth(bt_update)) = &patch.variant {
                return bt_update.low_energy.state == Some(false);
            }
            false
        })
        .times(1)
        .in_sequence(&mut seq)
        .returning(|id, _| Ok(Chip { id: id.0, kind: ChipKind::BLUETOOTH, ..Default::default() }));

    mock_chip_client
        .expect_update()
        // Verify 2: Update to TRUE (Enabled)
        .withf(|_, patch| {
            if let Some(ChipVariantUpdate::Bluetooth(bt_update)) = &patch.variant {
                return bt_update.low_energy.state == Some(true);
            }
            false
        })
        .times(1)
        .in_sequence(&mut seq)
        .returning(|id, _| Ok(Chip { id: id.0, kind: ChipKind::BLUETOOTH, ..Default::default() }));

    let mut chip_clients = HashMap::new();
    chip_clients.insert(ChipKind::BLUETOOTH, Box::new(mock_chip_client) as Box<dyn ChipClient>);

    // Use default link mock
    let mock_link_client = World::create_default_link_client();

    let world = World::with_clients(chip_clients, mock_link_client).await;

    // When I create a device and update its Bluetooth Low Energy state to disabled
    let device_name = "test-dev";
    let device_id = world.when_create_device(device_name).await;

    let chip_update = World::create_bluetooth_chip_update(Some(false), None);
    world.when_update_device_chip(device_id, chip_update).await;

    // Then the Chip Client receives the update with the correct state (verified by
    // checkpoint)

    // When I update the device again to ENABLE the Bluetooth Low Energy state
    let chip_update_enable = World::create_bluetooth_chip_update(Some(true), None);
    world.when_update_device_chip(device_id, chip_update_enable).await;
}

// Scenario: Update Chip Classic Radio State via Device Update
//   Given a running Device Actor with expectation for Chip Update
//   When I create a device and update its Bluetooth Classic state to disabled
//   Then the Chip Client receives the update with the correct state
//   And I update the device to enable the Bluetooth Classic state
//   Then the Chip Client receives the update with the correct state
#[tokio::test]
async fn test_update_chip_classic_radio_state() {
    // Given a running Device Actor with expectation for Chip Update
    let mut mock_chip_client = MockChipClient::new();
    mock_chip_client.expect_create().times(1).returning(|_, _| Ok(()));

    let mut seq = mockall::Sequence::new();

    mock_chip_client
        .expect_update()
        // Verify 1: Update to FALSE (Disabled)
        .withf(|_, patch| {
            if let Some(ChipVariantUpdate::Bluetooth(bt_update)) = &patch.variant {
                return bt_update.classic.state == Some(false);
            }
            false
        })
        .times(1)
        .in_sequence(&mut seq)
        .returning(|id, _| Ok(Chip { id: id.0, kind: ChipKind::BLUETOOTH, ..Default::default() }));

    mock_chip_client
        .expect_update()
        // Verify 2: Update to TRUE (Enabled)
        .withf(|_, patch| {
            if let Some(ChipVariantUpdate::Bluetooth(bt_update)) = &patch.variant {
                return bt_update.classic.state == Some(true);
            }
            false
        })
        .times(1)
        .in_sequence(&mut seq)
        .returning(|id, _| Ok(Chip { id: id.0, kind: ChipKind::BLUETOOTH, ..Default::default() }));

    let mut chip_clients = HashMap::new();
    chip_clients.insert(ChipKind::BLUETOOTH, Box::new(mock_chip_client) as Box<dyn ChipClient>);

    // Use default link mock
    let mock_link_client = World::create_default_link_client();

    let world = World::with_clients(chip_clients, mock_link_client).await;

    // When I create a device and update its Bluetooth Classic state to disabled
    let device_name = "test-dev";
    let device_id = world.when_create_device(device_name).await;

    let chip_update = World::create_bluetooth_chip_update(None, Some(false));
    world.when_update_device_chip(device_id, chip_update).await;

    // Then the Chip Client receives the update with the correct state (verified by
    // checkpoint)

    // When I update the device again to ENABLE the Bluetooth Classic state
    let chip_update_enable = World::create_bluetooth_chip_update(None, Some(true));
    world.when_update_device_chip(device_id, chip_update_enable).await;
}

// Scenario: Update device disambiguates shared chip name by variant
//   Given a running Device Actor with a device containing a Bluetooth chip
//     and a Wi-Fi chip both named "cvd-1"
//   When I update the device specifying chip name "cvd-1" with a Wi-Fi variant
//     update
//   Then the Wi-Fi chip receives the update and the Bluetooth chip is untouched
#[tokio::test]
async fn test_update_device_disambiguates_shared_name_by_variant() {
    // Given a running Device Actor with a device containing a Bluetooth chip and a Wi-Fi chip both
    // named "cvd-1"
    let world = World::new().await;
    let (device_id, chip_ids) = world
        .given_device_with_chips_sharing_name(
            "guid-1",
            "cvd-1",
            &[ChipKind::BLUETOOTH, ChipKind::WIFI],
        )
        .await;
    let wifi_chip_id = chip_ids[1];

    // When I update the device specifying chip name "cvd-1" with a Wi-Fi variant update
    let chip_update = ChipUpdate {
        name: Some("cvd-1".to_string()),
        variant: Some(ChipVariantUpdate::Wifi(WifiUpdate::default())),
        ..Default::default()
    };
    let result = world.when_try_update_device_chip(device_id, chip_update).await;

    // Then the Wi-Fi chip receives the update and the Bluetooth chip is untouched
    assert!(result.is_ok());
    world.then_chips_received_updates(&[wifi_chip_id]);
}

// Scenario: Update device rejects ambiguous shared name without variant
//   Given a running Device Actor with a device containing a Bluetooth chip
//     and a Wi-Fi chip both named "cvd-1"
//   When I update the device specifying chip name "cvd-1" without variant
//   Then the operation fails with an InvalidInput error indicating ambiguous
//     chip name
#[tokio::test]
async fn test_update_device_rejects_ambiguous_shared_name_without_variant() {
    // Given a running Device Actor with a device containing a Bluetooth chip and a Wi-Fi chip both
    // named "cvd-1"
    let world = World::new().await;
    let (device_id, _) = world
        .given_device_with_chips_sharing_name(
            "guid-1",
            "cvd-1",
            &[ChipKind::BLUETOOTH, ChipKind::WIFI],
        )
        .await;

    // When I update the device specifying chip name "cvd-1" without variant
    let chip_update =
        ChipUpdate { name: Some("cvd-1".to_string()), enabled: Some(true), ..Default::default() };
    let result = world.when_try_update_device_chip(device_id, chip_update).await;

    // Then the operation fails with an InvalidInput error indicating ambiguous chip name
    world.then_update_rejected_as_invalid_input(
        result,
        "Ambiguous chip name 'cvd-1' matches multiple chips on device",
    );
}

// Scenario: Update device rejects mismatched ID and variant kind
//   Given a running Device Actor with a device containing a Bluetooth chip
//   When I update the device specifying the chip ID with a Wi-Fi variant
//   Then the operation fails with an InvalidInput error indicating kind
//     mismatch
#[tokio::test]
async fn test_update_device_rejects_mismatched_id_and_variant() {
    // Given a running Device Actor with a device containing a Bluetooth chip
    let world = World::new().await;
    let (device_id, chip_ids) =
        world.given_device_with_chips_sharing_name("guid-1", "bt-0", &[ChipKind::BLUETOOTH]).await;
    let bt_chip_id = chip_ids[0];

    // When I update the device specifying the chip ID with a Wi-Fi variant
    let chip_update = ChipUpdate {
        id: Some(bt_chip_id),
        variant: Some(ChipVariantUpdate::Wifi(WifiUpdate::default())),
        ..Default::default()
    };
    let result = world.when_try_update_device_chip(device_id, chip_update).await;

    // Then the operation fails with an InvalidInput error indicating kind mismatch
    world.then_update_rejected_as_invalid_input(
        result,
        "has kind BLUETOOTH, but update has kind WIFI",
    );
}

// Scenario: Update device rejects mismatched name and variant kind
//   Given a running Device Actor with a device containing a Bluetooth chip
//   When I update the device specifying the chip name with a Wi-Fi variant
//   Then the operation fails with an InvalidInput error indicating kind
//     mismatch
#[tokio::test]
async fn test_update_device_rejects_mismatched_name_and_variant() {
    // Given a running Device Actor with a device containing a Bluetooth chip
    let world = World::new().await;
    let (device_id, _) =
        world.given_device_with_chips_sharing_name("guid-1", "bt-0", &[ChipKind::BLUETOOTH]).await;

    // When I update the device specifying the chip name with a Wi-Fi variant
    let chip_update = ChipUpdate {
        name: Some("bt-0".to_string()),
        variant: Some(ChipVariantUpdate::Wifi(WifiUpdate::default())),
        ..Default::default()
    };
    let result = world.when_try_update_device_chip(device_id, chip_update).await;

    // Then the operation fails with an InvalidInput error indicating kind mismatch
    world.then_update_rejected_as_invalid_input(
        result,
        &format!("Chip name 'bt-0' exists on device {} but not with radio kind WIFI", device_id.0),
    );
}

// Scenario: Update device rejects ambiguous shared name and variant when multiple chips match both
//   Given a running Device Actor with a device containing two Wi-Fi chips both named "wlan-shared"
//   When I update the device specifying chip name "wlan-shared" with a Wi-Fi variant update
//   Then the operation fails with an InvalidInput error indicating ambiguous chip name of radio
//     kind
#[tokio::test]
async fn test_update_device_rejects_ambiguous_name_and_variant_for_duplicate_chips() {
    // Given a running Device Actor with a device containing two Wi-Fi chips both
    // named "wlan-shared"
    let world = World::new().await;
    let (device_id, _) = world
        .given_device_with_chips_sharing_name(
            "guid-1",
            "wlan-shared",
            &[ChipKind::WIFI, ChipKind::WIFI],
        )
        .await;

    // When I update the device specifying chip name "wlan-shared" with a Wi-Fi variant update
    let chip_update = ChipUpdate {
        name: Some("wlan-shared".to_string()),
        variant: Some(ChipVariantUpdate::Wifi(WifiUpdate::default())),
        ..Default::default()
    };
    let result = world.when_try_update_device_chip(device_id, chip_update).await;

    // Then the operation fails with an InvalidInput error indicating ambiguous chip name of radio
    // kind
    world.then_update_rejected_as_invalid_input(
        result,
        "Ambiguous chip name 'wlan-shared' matches multiple chips of radio kind WIFI on device",
    );
}

// Scenario: Update device rejects selectorless chip update
//   Given a running Device Actor with a device
//   When I update the device with a ChipUpdate lacking id, name, and variant
//   Then the operation fails with an InvalidInput error indicating at least
//     one selector is required
#[tokio::test]
async fn test_update_device_rejects_selectorless_update() {
    // Given a running Device Actor with a device
    let world = World::new().await;
    let device_name = "test-dev";
    let device_id = world.when_create_device(device_name).await;

    // When I update the device with a ChipUpdate lacking id, name, and variant
    let chip_update = ChipUpdate { id: None, name: None, variant: None, ..Default::default() };
    let result = world.when_try_update_device_chip(device_id, chip_update).await;

    // Then the operation fails with an InvalidInput error indicating at least one selector is
    // required
    world.then_update_rejected_as_invalid_input(
        result,
        "ChipUpdate requires at least one selector (id, name, or variant)",
    );
}
