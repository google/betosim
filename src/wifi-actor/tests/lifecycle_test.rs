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

use device_api::DeviceAction;
use netsim_model::{ChipClient, ChipId};
use wifi_actor::WifiClient;

use crate::world::World;

// Feature: Chip Creation
// Scenario: Create a new chip
//
//   Given a new world
//   When we create a chip
//   Then the chip exists in the wifi actor
#[tokio::test]
async fn test_create_chip() {
    // Given
    let mut world = World::new().await;

    // When
    let chip_id = world.given_a_chip(1).await;
    assert_eq!(chip_id, 1);

    // Then
    let chip = world.wifi_client.read(ChipId(1)).await;
    assert!(chip.is_ok());
}

// Feature: Chip Deletion
// Scenario: Delete an existing chip
//
//   Given a world with a chip
//   When we delete the chip
//   Then the chip is removed from the wifi actor
#[tokio::test]
async fn test_delete_chip() {
    // Given
    let mut world = World::new().await;
    let chip_id = world.given_a_chip(3).await;

    // When
    world.wifi_client.delete(ChipId(chip_id)).await.expect("Failed to delete chip");

    // Then
    let chip = world.wifi_client.read(ChipId(chip_id)).await;
    assert!(chip.is_err());
}

// Feature: Sink Closure Notification
// Scenario: Notify DeviceActor when a chip's sink closes
//
//   Given a world with a chip
//   When the packet sink is closed (simulated by dropping the chip's channels)
//   Then the device actor receives a NotifyChipRemoved action
//   And the chip is removed from the WifiActor
#[tokio::test]
async fn test_sink_closure_notification() {
    // Given
    let mut world = World::new().await;
    let _chip_id = world.given_a_chip(2).await;

    // When
    world.chips.retain(|c| c.id != 2);

    // Then
    let timeout = tokio::time::sleep(std::time::Duration::from_secs(1));
    tokio::pin!(timeout);

    let mut notification_received = false;
    loop {
        tokio::select! {
             Some(action) = world.device_action_rx.recv() => {
                 match action {
                     DeviceAction::NotifyChipRemoved(_, id) => {
                         if id.0 == 2 {
                             notification_received = true;
                             break;
                         }
                     }
                     _ => {}
                 }
             }
             _ = &mut timeout => {
                 panic!("Timeout waiting for NotifyChipRemoved");
             }
        }
    }

    assert!(notification_received);

    // And
    let chip = world.wifi_client.read(ChipId(2)).await;
    assert!(chip.is_err(), "Chip should be removed from WifiActor");
}

#[test]
fn test_wifi_client_implements_chip_client() {
    fn assert_chip_client<T: netsim_model::ChipClient>() {}
    assert_chip_client::<WifiClient>();
}

// Feature: Chip State Update
// Scenario: Update chip wifi radio state
//
//   Given a world with a chip
//   When a chip is updated to state false
//   Then the chip radio state is updated to false
//   When a chip is updated to state true
//   Then the chip radio state is updated to true
#[tokio::test]
async fn test_update_chip_state() {
    // Given
    let mut world = World::new().await;
    let chip_id = world.given_a_chip(4).await;

    // When - Update to disabled
    let mut update = netsim_model::ChipUpdate {
        id: Some(ChipId(chip_id)),
        name: None,
        manufacturer: None,
        product_name: None,
        pose: Default::default(),
        variant: Some(netsim_model::ChipVariantUpdate::Wifi(netsim_model::WifiUpdate {
            radio: netsim_model::RadioUpdate { state: Some(false) },
        })),
        links: None,
        enabled: Some(false),
    };
    world.wifi_client.update(ChipId(chip_id), update.clone()).await.expect("Failed to update chip");

    // Then
    let chip = world.wifi_client.read(ChipId(chip_id)).await.expect("Failed to read chip");
    assert_eq!(chip.enabled, false);
    if let Some(netsim_model::ChipVariant::Wifi(radio)) = chip.variant {
        assert_eq!(radio.radio.state, Some(false));
    } else {
        panic!("Expected Wifi variant");
    }

    // When - Update to enabled
    update.variant = Some(netsim_model::ChipVariantUpdate::Wifi(netsim_model::WifiUpdate {
        radio: netsim_model::RadioUpdate { state: Some(true) },
    }));
    update.enabled = Some(true);
    world.wifi_client.update(ChipId(chip_id), update).await.expect("Failed to update chip");

    // Then
    let chip = world.wifi_client.read(ChipId(chip_id)).await.expect("Failed to read chip");
    assert_eq!(chip.enabled, true);
    if let Some(netsim_model::ChipVariant::Wifi(radio)) = chip.variant {
        assert_eq!(radio.radio.state, Some(true));
    } else {
        panic!("Expected Wifi variant");
    }
}
