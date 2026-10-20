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

use netsim_model::{ChipClient, Position};

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

// Scenario: Update chip bt_properties applies to Rootcanal
#[tokio::test]
async fn test_update_chip_bt_properties_applies_to_rootcanal() {
    let mut world = World::new();

    // Given a bluetooth chip in default mode
    world.given_device("A").await;

    // Verify default buffer size
    world.when_packet_sent("A", bytes::Bytes::from(vec![0x01, 0x03, 0x0c, 0x00])).await;
    world.then_packet_received("A", &[0x04, 0x0e, 0x04, 0x01, 0x03, 0x0c, 0x00]).await;
    world.when_packet_sent("A", bytes::Bytes::from(vec![0x01, 0x02, 0x20, 0x00])).await;
    world
        .then_packet_received("A", &[0x04, 0x0e, 0x07, 0x01, 0x02, 0x20, 0x00, 0x1b, 0x00, 0x14])
        .await;

    // When updating chip with Laird BL654 preset via bt_properties
    let chip_id = *world.chips.get("A").expect("chip A");
    let update = netsim_model::ChipUpdate {
        variant: Some(netsim_model::ChipVariantUpdate::Bluetooth(netsim_model::BluetoothUpdate {
            bt_properties: Some(netsim_model::BtControllerProperties {
                preset: Some("laird_bl654".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        })),
        ..Default::default()
    };
    world.client.update(chip_id, update).await.expect("update chip");

    // Then buffer size reflects Laird BL654 preset (512 bytes / 4 packets)
    world.when_packet_sent("A", bytes::Bytes::from(vec![0x01, 0x03, 0x0c, 0x00])).await;
    world.then_packet_received("A", &[0x04, 0x0e, 0x04, 0x01, 0x03, 0x0c, 0x00]).await;
    world.when_packet_sent("A", bytes::Bytes::from(vec![0x01, 0x02, 0x20, 0x00])).await;
    world
        .then_packet_received("A", &[0x04, 0x0e, 0x07, 0x01, 0x02, 0x20, 0x00, 0x00, 0x02, 0x04])
        .await;
}

// Scenario: Reset chip restores initial default bt_properties in Rootcanal
#[tokio::test]
async fn test_reset_chip_restores_initial_bt_properties_in_rootcanal() {
    let mut world = World::new();

    // Given a bluetooth chip in default mode
    world.given_device("A").await;
    let chip_id = *world.chips.get("A").expect("chip A");

    // When updating chip with Laird BL654 preset via bt_properties
    let update = netsim_model::ChipUpdate {
        variant: Some(netsim_model::ChipVariantUpdate::Bluetooth(netsim_model::BluetoothUpdate {
            bt_properties: Some(netsim_model::BtControllerProperties {
                preset: Some("laird_bl654".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        })),
        ..Default::default()
    };
    world.client.update(chip_id, update).await.expect("update chip");

    // Verify buffer size reflects Laird BL654 preset (512 bytes / 4 packets)
    world.when_packet_sent("A", bytes::Bytes::from(vec![0x01, 0x03, 0x0c, 0x00])).await;
    world.then_packet_received("A", &[0x04, 0x0e, 0x04, 0x01, 0x03, 0x0c, 0x00]).await;
    world.when_packet_sent("A", bytes::Bytes::from(vec![0x01, 0x02, 0x20, 0x00])).await;
    world
        .then_packet_received("A", &[0x04, 0x0e, 0x07, 0x01, 0x02, 0x20, 0x00, 0x00, 0x02, 0x04])
        .await;

    // When resetting chip via BluetoothAction::Reset
    world.client.reset(chip_id).await.expect("reset chip");

    // Then RootCanal restores initial default controller config (27 bytes / 20 packets)
    world.when_packet_sent("A", bytes::Bytes::from(vec![0x01, 0x03, 0x0c, 0x00])).await;
    world.then_packet_received("A", &[0x04, 0x0e, 0x04, 0x01, 0x03, 0x0c, 0x00]).await;
    world.when_packet_sent("A", bytes::Bytes::from(vec![0x01, 0x02, 0x20, 0x00])).await;
    world
        .then_packet_received("A", &[0x04, 0x0e, 0x07, 0x01, 0x02, 0x20, 0x00, 0x1b, 0x00, 0x14])
        .await;
}
