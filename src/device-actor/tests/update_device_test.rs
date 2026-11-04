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

use crate::world::{
    DEFAULT_MANUFACTURER, MATCHED_MANUFACTURER, UPDATED_MANUFACTURER, UPDATED_PRODUCT_NAME, World,
};

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
        .returning(|id, _| {
            Ok(Chip {
                id: id.0,
                kind: ChipKind::BLUETOOTH,
                name: "beacon".to_string(),
                ..Default::default()
            })
        });

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
        .returning(|id, _| {
            Ok(Chip {
                id: id.0,
                kind: ChipKind::BLUETOOTH,
                name: "beacon".to_string(),
                ..Default::default()
            })
        });

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
        .returning(|id, _| {
            Ok(Chip {
                id: id.0,
                kind: ChipKind::BLUETOOTH,
                name: "beacon".to_string(),
                ..Default::default()
            })
        });

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
        .returning(|id, _| {
            Ok(Chip {
                id: id.0,
                kind: ChipKind::BLUETOOTH,
                name: "beacon".to_string(),
                ..Default::default()
            })
        });

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
        .returning(|id, _| {
            Ok(Chip {
                id: id.0,
                kind: ChipKind::BLUETOOTH,
                name: "beacon".to_string(),
                ..Default::default()
            })
        });

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
        .returning(|id, _| {
            Ok(Chip {
                id: id.0,
                kind: ChipKind::BLUETOOTH,
                name: "beacon".to_string(),
                ..Default::default()
            })
        });

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
    world.when_update_device_chip(device_id, chip_update).await;

    // Then the Wi-Fi chip receives the update and the Bluetooth chip is untouched
    world.then_chips_received_updates(&[wifi_chip_id]);
}

// Scenario: Update device rejects ambiguous shared name without variant
//   Given a running Device Actor with a device containing a Bluetooth chip
//     and a Wi-Fi chip both named "cvd-1"
//   When I update the device specifying chip name "cvd-1" without variant
//   Then the operation fails with an AmbiguousMatch error
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
    world.when_try_update_device_chip(device_id, chip_update).await;

    // Then the operation fails with an AmbiguousMatch error
    world.then_error_is_ambiguous_match("cvd-1");
}

// Scenario: Update device rejects mismatched ID and variant kind
//   Given a running Device Actor with a device containing a Bluetooth chip
//   When I update the device specifying the chip ID with a Wi-Fi variant
//   Then the operation fails with a MismatchedChipKind error
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
    world.when_try_update_device_chip(device_id, chip_update).await;

    // Then the operation fails with a MismatchedChipKind error
    world.then_error_is_mismatched_chip_kind(ChipKind::WIFI, ChipKind::BLUETOOTH);
}

// Scenario: Update device rejects mismatched name and variant kind
//   Given a running Device Actor with a device containing a Bluetooth chip
//   When I update the device specifying the chip name with a Wi-Fi variant
//   Then the operation fails with a MismatchedChipKind error
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
    world.when_try_update_device_chip(device_id, chip_update).await;

    // Then the operation fails with a MismatchedChipKind error
    world.then_error_is_mismatched_chip_kind(ChipKind::WIFI, ChipKind::BLUETOOTH);
}

// Scenario: Update device rejects ambiguous shared name and variant when multiple chips match both
//   Given a running Device Actor with a device containing two Wi-Fi chips sharing substring "wlan-shared"
//   When I update the device specifying chip name "wlan-shared" with a Wi-Fi variant update
//   Then the operation fails with an AmbiguousMatch error
#[tokio::test]
async fn test_update_device_rejects_ambiguous_name_and_variant_for_duplicate_chips() {
    // Given a running Device Actor with a device containing two Wi-Fi chips sharing substring
    // "wlan-shared"
    let world = World::new().await;
    let device_id = world
        .given_device_with_chip(
            "test-dev",
            "wlan-shared-0",
            device_api::ChipCreateVariant::Wifi(Default::default()),
        )
        .await;
    world
        .given_chip_added_to_device(
            device_id,
            "wlan-shared-1",
            device_api::ChipCreateVariant::Wifi(Default::default()),
        )
        .await;

    // When I update the device specifying chip name "wlan-shared" with a Wi-Fi variant update
    let chip_update = ChipUpdate {
        name: Some("wlan-shared".to_string()),
        variant: Some(ChipVariantUpdate::Wifi(WifiUpdate::default())),
        ..Default::default()
    };
    world.when_try_update_device_chip(device_id, chip_update).await;

    // Then the operation fails with an AmbiguousMatch error
    world.then_error_is_ambiguous_match("wlan-shared");
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
    world.when_try_update_device_chip(device_id, chip_update).await;

    // Then the operation fails with an InvalidInput error indicating at least one selector is
    // required
    world.then_update_rejected_as_invalid_input(
        "ChipUpdate requires at least one selector (id, name, or variant)",
    );
}

const BROADCAST_POS_X: f32 = 50.0;
const BROADCAST_POS_Y: f32 = 60.0;
const BROADCAST_POS_Z: f32 = 70.0;
const BROADCAST_YAW: f32 = 1.5;

// Scenario: Update chip metadata succeeds and updates device view
#[tokio::test]
async fn test_update_chip_metadata() {
    // Given a running Device Actor with chip client configured to accept metadata updates
    let world = World::given_with_bluetooth_metadata_mock().await;
    let device_id = world.given_device_created("test-dev").await;

    // When I update the chip manufacturer and product name
    world
        .when_patch_chip_metadata(device_id, "beacon", UPDATED_MANUFACTURER, UPDATED_PRODUCT_NAME)
        .await;

    // Then the device view reflects the updated chip metadata
    world.then_chip_count_is(device_id, 1).await;
    world
        .then_chip_metadata_is(device_id, "beacon", UPDATED_MANUFACTURER, UPDATED_PRODUCT_NAME)
        .await;
}

// Scenario: Exact chip name match wins over ambiguous prefix/suffix match
#[tokio::test]
async fn test_patch_chip_exact_match_wins_over_ambiguous() {
    // Given a device with chips "chip" and "chip-extra"
    let world = World::new().await;
    let device_id = world
        .given_device_with_chip(
            "test-dev",
            "chip",
            device_api::ChipCreateVariant::Beacon(Default::default()),
        )
        .await;
    world
        .given_chip_added_to_device(
            device_id,
            "chip-extra",
            device_api::ChipCreateVariant::Beacon(Default::default()),
        )
        .await;

    // When I update chip "chip" with a new manufacturer
    world.when_patch_chip_manufacturer(device_id, "chip", MATCHED_MANUFACTURER).await;

    // Then only the exact matching chip "chip" receives the updated manufacturer
    world.then_chip_manufacturer_is(device_id, "chip", MATCHED_MANUFACTURER).await;
    world.then_chip_manufacturer_is(device_id, "chip-extra", DEFAULT_MANUFACTURER).await;
}

// Scenario: Substring match updates unique matching chip
#[tokio::test]
async fn test_patch_chip_substring_match() {
    // Given a device with chip "my-awesome-bluetooth-chip"
    let world = World::new().await;
    let device_id = world
        .given_device_with_chip(
            "test-dev",
            "my-awesome-bluetooth-chip",
            device_api::ChipCreateVariant::Beacon(Default::default()),
        )
        .await;

    // When I update chip using substring "awesome" with a new manufacturer
    world.when_patch_chip_manufacturer(device_id, "awesome", MATCHED_MANUFACTURER).await;

    // Then the matching chip receives the updated manufacturer
    world
        .then_chip_manufacturer_is(device_id, "my-awesome-bluetooth-chip", MATCHED_MANUFACTURER)
        .await;
}

// Scenario: Patching with an ambiguous chip name returns AmbiguousMatch error
#[tokio::test]
async fn test_patch_chip_ambiguous_error() {
    // Given a device with two chips "beacon-alpha" and "beacon-beta"
    let world = World::new().await;
    let device_id = world
        .given_device_with_chip(
            "test-dev",
            "beacon-alpha",
            device_api::ChipCreateVariant::Beacon(Default::default()),
        )
        .await;
    world
        .given_chip_added_to_device(
            device_id,
            "beacon-beta",
            device_api::ChipCreateVariant::Beacon(Default::default()),
        )
        .await;

    // When I update with common substring "beacon"
    world.when_try_patch_chip_by_name(device_id, "beacon").await;

    // Then the update fails with AmbiguousMatch
    world.then_error_is_ambiguous_match("beacon");
}

// Scenario: Exact match on chip name across different chip kinds is ambiguous without variant
#[tokio::test]
async fn test_patch_chip_ambiguous_exact_match_across_kinds() {
    // Given a device with Bluetooth and UWB chips sharing the name "shared-name"
    let world = World::new().await;
    let device_id = world
        .given_device_with_chip(
            "test-dev",
            "shared-name",
            device_api::ChipCreateVariant::Beacon(Default::default()),
        )
        .await;
    world
        .given_chip_added_to_device(
            device_id,
            "shared-name",
            device_api::ChipCreateVariant::Uwb(Default::default()),
        )
        .await;

    // When I update chip "shared-name" without specifying a variant filter
    world.when_try_patch_chip_by_name(device_id, "shared-name").await;

    // Then the update fails with AmbiguousMatch
    world.then_error_is_ambiguous_match("shared-name");
}

// Scenario: Disambiguate identical chip names across kinds by specifying variant
#[tokio::test]
async fn test_patch_chip_disambiguate_exact_match_across_kinds_with_variant() {
    // Given a device with Bluetooth and UWB chips both named "shared-name"
    let world = World::new().await;
    let device_id = world
        .given_device_with_chip(
            "test-dev",
            "shared-name",
            device_api::ChipCreateVariant::Beacon(Default::default()),
        )
        .await;
    world
        .given_chip_added_to_device(
            device_id,
            "shared-name",
            device_api::ChipCreateVariant::Uwb(Default::default()),
        )
        .await;

    // When I update chip "shared-name" specifying UWB variant and new manufacturer
    world
        .when_patch_chip_variant_manufacturer(
            device_id,
            "shared-name",
            ChipVariantUpdate::Uwb(Default::default()),
            MATCHED_MANUFACTURER,
        )
        .await;

    // Then the UWB chip receives the updated manufacturer while Bluetooth chip remains unchanged
    world
        .then_chip_of_kind_manufacturer_is(
            device_id,
            netsim_model::ChipKind::UWB,
            MATCHED_MANUFACTURER,
        )
        .await;
    world
        .then_chip_of_kind_manufacturer_is(
            device_id,
            netsim_model::ChipKind::BLUETOOTH,
            DEFAULT_MANUFACTURER,
        )
        .await;
}

// Scenario: Empty chip name update fails when multiple chips of that kind exist
#[tokio::test]
async fn test_patch_chip_ambiguous_empty_name_error() {
    // Given a device with two Bluetooth chips "bt-1" and "bt-2"
    let world = World::new().await;
    let device_id = world
        .given_device_with_chip(
            "test-dev",
            "bt-1",
            device_api::ChipCreateVariant::Beacon(Default::default()),
        )
        .await;
    world
        .given_chip_added_to_device(
            device_id,
            "bt-2",
            device_api::ChipCreateVariant::Beacon(Default::default()),
        )
        .await;

    // When I update without a chip name specifying Bluetooth state
    world.when_try_patch_bluetooth_chip(device_id, Some(true), None).await;

    // Then the update fails with AmbiguousEmptyMatch for Bluetooth
    world.then_error_is_ambiguous_empty_match(ChipKind::BLUETOOTH);
}

// Scenario: Patching a non-existent chip name returns ChipNotFound error
#[tokio::test]
async fn test_patch_chip_not_found_error() {
    // Given a device "test-dev"
    let world = World::new().await;
    let device_id = world.given_device_created("test-dev").await;

    // When I update a chip with name "ghost-chip"
    world.when_try_patch_chip_by_name(device_id, "ghost-chip").await;

    // Then the update fails with ChipNotFound
    world.then_error_is_chip_not_found("ghost-chip", "test-dev");
}

// Scenario: Patching a chip by ID with mismatched kind variant fails
#[tokio::test]
async fn test_patch_chip_id_mismatched_kind_error() {
    // Given a device with a Bluetooth chip
    let world = World::new().await;
    let device_id = world.given_device_created("test-dev").await;
    let bt_chip_id = world.given_first_chip_id(device_id).await;

    // When I update the Bluetooth chip by ID but specify Wifi variant
    world
        .when_try_patch_chip_variant_by_id(
            device_id,
            bt_chip_id,
            ChipVariantUpdate::Wifi(Default::default()),
        )
        .await;

    // Then the update fails with MismatchedChipKind
    world.then_error_is_mismatched_chip_kind(ChipKind::WIFI, ChipKind::BLUETOOTH);
}

// Scenario: Updating device pose broadcasts position and orientation to all chips
#[tokio::test]
async fn test_patch_device_pose_broadcast_to_all_chips() {
    // Given a device with multiple chips
    let world = World::new().await;
    let device_id = world
        .given_device_with_chip(
            "multi-radio",
            "bt-chip",
            device_api::ChipCreateVariant::Beacon(Default::default()),
        )
        .await;
    world
        .given_chip_added_to_device(
            device_id,
            "wifi-chip",
            device_api::ChipCreateVariant::Wifi(Default::default()),
        )
        .await;

    // When I update the device-level pose
    world
        .when_patch_device_pose_and_chip(
            device_id,
            BROADCAST_POS_X,
            BROADCAST_POS_Y,
            BROADCAST_POS_Z,
            BROADCAST_YAW,
            "bt-chip",
            "updated-bt",
        )
        .await;

    // Then all chips on the device receive the broadcast position and orientation
    world
        .then_device_and_chips_pose_is(
            device_id,
            BROADCAST_POS_X,
            BROADCAST_POS_Y,
            BROADCAST_POS_Z,
            BROADCAST_YAW,
        )
        .await;
}

// Scenario: Patching device pose ignores stale echoed chip offset pose
#[tokio::test]
async fn test_patch_device_ignores_stale_chip_offset_pose() {
    // Given a device with a chip
    let world = World::given_with_passthrough_bluetooth_and_wifi_mocks().await;
    let device_id = world
        .given_device_with_chip(
            "pose-dev",
            "bt-chip",
            device_api::ChipCreateVariant::Beacon(Default::default()),
        )
        .await;

    // When patching device pose while echoing a stale chip offset pose
    world
        .when_patch_device_pose_with_stale_chip_offset(
            device_id,
            "bt-chip",
            BROADCAST_POS_X,
            BROADCAST_POS_Y,
            BROADCAST_POS_Z,
            BROADCAST_YAW,
        )
        .await;

    // Then the device and chip receive the new device pose, ignoring stale offset, and metadata is updated
    world
        .then_device_and_chips_pose_is(
            device_id,
            BROADCAST_POS_X,
            BROADCAST_POS_Y,
            BROADCAST_POS_Z,
            BROADCAST_YAW,
        )
        .await;
    world.then_chip_manufacturer_is(device_id, "bt-chip", MATCHED_MANUFACTURER).await;
}

// Scenario: Device last modified timestamp is not bumped when chip update fails
#[tokio::test]
async fn test_patch_device_timestamp_not_bumped_on_failure() {
    // Given a device with a chip client configured to fail on update
    let world = World::given_with_failing_bluetooth_update_mock().await;
    let device_id = world.given_device_created("test-dev").await;
    let initial_ts = world.given_recorded_last_modified().await;

    // When an update fails
    world.when_try_patch_bluetooth_chip(device_id, Some(true), None).await;

    // Then the update fails with actor communication error
    world.then_error_is_actor_communication();

    // And the last modified timestamp is unchanged
    world.then_last_modified_is(initial_ts).await;
}

// Scenario: Adding a chip with duplicate name and kind to a device is rejected
#[tokio::test]
async fn test_create_device_duplicate_chip_rejected() {
    // Given a device with chip "same-chip"
    let world = World::new().await;
    let device_id = world
        .given_device_with_chip(
            "test-dev",
            "same-chip",
            device_api::ChipCreateVariant::Beacon(Default::default()),
        )
        .await;

    // When I add another chip with the same name and kind
    world
        .when_try_add_chip_variant(
            device_id,
            "same-chip",
            device_api::ChipCreateVariant::Beacon(Default::default()),
        )
        .await;

    // Then the operation fails with DuplicateChip
    world.then_error_is_duplicate_chip("same-chip");
}

// Scenario: Creating a device with a name that already exists is rejected
#[tokio::test]
async fn test_create_device_name_already_exists_rejected() {
    // Given an existing device named "existing-device"
    let world = World::new().await;
    world
        .given_device_with_chip(
            "existing-device",
            "chip-1",
            device_api::ChipCreateVariant::Beacon(Default::default()),
        )
        .await;

    // When I attempt to create another device with the name "existing-device"
    world
        .when_try_create_device_with_chip(
            "existing-device",
            "chip-2",
            device_api::ChipCreateVariant::Beacon(Default::default()),
        )
        .await;

    // Then creation fails with AlreadyExists
    world.then_error_is_already_exists("existing-device");
}

// Scenario: Empty chip names use the device name for the first chip and <KIND>-<id> afterward
#[tokio::test]
async fn test_auto_naming_uses_kind_and_id_after_first_chip() {
    // Given a device "test-dev" with an unnamed Bluetooth chip
    let world = World::new().await;
    let device_id = world
        .given_device_with_chip(
            "test-dev",
            "",
            device_api::ChipCreateVariant::Beacon(Default::default()),
        )
        .await;

    // When I add an unnamed WiFi chip and a second unnamed Bluetooth chip
    let wifi_chip_id = world
        .when_add_chip_variant(
            device_id,
            "",
            device_api::ChipCreateVariant::Wifi(Default::default()),
        )
        .await
        .unwrap();
    let second_bt_chip_id = world
        .when_add_chip_variant(
            device_id,
            "",
            device_api::ChipCreateVariant::Beacon(Default::default()),
        )
        .await
        .unwrap();

    // Then the first chip uses the device name and subsequent chips use <KIND>-<id>
    world.then_chip_count_is(device_id, 3).await;
    world.then_first_chip_of_kind_name_is(device_id, ChipKind::BLUETOOTH, "test-dev").await;
    world
        .then_first_chip_of_kind_name_is(
            device_id,
            ChipKind::WIFI,
            &format!("WIFI-{}", wifi_chip_id.0),
        )
        .await;
    world
        .then_chip_name_by_id_is(
            device_id,
            second_bt_chip_id,
            &format!("BLUETOOTH-{}", second_bt_chip_id.0),
        )
        .await;
    world
        .then_chip_name_by_id_is(device_id, wifi_chip_id, &format!("WIFI-{}", wifi_chip_id.0))
        .await;
}

// Scenario: Empty chip names on an unnamed device fall back to <KIND>-<id>
#[tokio::test]
async fn test_auto_naming_with_empty_device_name_uses_kind_and_id() {
    // Given an unnamed device created with an unnamed WiFi chip
    let world = World::new().await;
    let device_id = world
        .given_device_with_chip("", "", device_api::ChipCreateVariant::Wifi(Default::default()))
        .await;
    let first_chip_id = world.given_first_chip_id(device_id).await;

    // When I add a second unnamed WiFi chip to the device
    let second_chip_id = world
        .when_add_chip_variant(
            device_id,
            "",
            device_api::ChipCreateVariant::Wifi(Default::default()),
        )
        .await
        .unwrap();

    // Then both chips are named using <KIND>-<id>
    world.then_chip_count_is(device_id, 2).await;
    world
        .then_chip_name_by_id_is(device_id, first_chip_id, &format!("WIFI-{}", first_chip_id.0))
        .await;
    world
        .then_chip_name_by_id_is(device_id, second_chip_id, &format!("WIFI-{}", second_chip_id.0))
        .await;
}
