// Copyright 2023-2025 The Android Open Source Project
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

use bytes::Bytes;
use netsim_model::ChipClient;
use tokio::time::Duration;

use crate::world::World;

// Feature: Bluetooth Chip Lifecycle
//
//   As a client
//   I want to manage bluetooth chips
//   So that I can simulate bluetooth devices behavior

// Scenario: Perform HCI Reset
//
//   Given a bluetooth chip in device mode
//   When an HCI reset command is sent
//   Then the chip responds with Command Complete
#[tokio::test]
async fn test_hci_reset_command() {
    let mut world = World::new();

    // 1. Create a virtual device chip.
    world.given_device("A").await;

    // 2. Send an HCI Reset command.
    let hci_reset_cmd = Bytes::from(vec![0x01, 0x03, 0x0c, 0x00]);
    world.when_packet_sent("A", hci_reset_cmd).await;

    // 3. Wait for the HCI Command Complete event.
    // Expected: Command Complete for Reset, status OK.
    let expected_response = vec![0x04, 0x0e, 0x04, 0x01, 0x03, 0x0c, 0x00];
    world.then_packet_received("A", &expected_response).await;
}

// Scenario: Chip removal on packet stream error
//
//   Given a bluetooth chip
//   When the packet stream is closed unexpectedly
//   Then the chip is removed from the actor
#[tokio::test]
async fn test_chip_dies_on_packet_stream_error() {
    let mut world = World::new();

    // 1. Create a virtual device chip.
    world.given_device("A").await;

    // A small delay to ensure the chip is registered before we check the count.
    tokio::time::sleep(Duration::from_millis(10)).await;
    let chip_count: usize = world.client.read_count_for_testing().await.expect("chip count");
    assert_eq!(chip_count, 1);

    // 2. Trigger a packet stream error by closing the channel.
    world.when_stream_dropped("A");

    // 4. Verify the chip has been removed.
    // A small delay is needed to ensure the actor has time to process the death
    // notice.
    tokio::time::sleep(Duration::from_millis(10)).await;
    let chip_count: usize = world.client.read_count_for_testing().await.expect("chip count");
    assert_eq!(chip_count, 0);
}

// Scenario: Chip removal on explicit delete
//
//   Given a bluetooth chip
//   When a delete command is sent
//   Then the chip is removed from the actor
#[tokio::test]
async fn test_delete_chip_shuts_down_task() {
    let mut world = World::new();

    // 1. Create a virtual device chip.
    world.given_device("A").await;

    let chip_count: usize = world.client.read_count_for_testing().await.expect("chip count");
    assert_eq!(chip_count, 1);

    // 2. Send a DeleteChip command.
    world.when_delete_chip("A").await.expect("delete chip");

    // 4. Verify the chip has been removed.
    let chip_count: usize = world.client.read_count_for_testing().await.expect("chip count");
    assert_eq!(chip_count, 0);
}

// Scenario: Chip removal on packet sink error
//
//   Given a bluetooth chip
//   When the packet sink is closed unexpectedly
//   Then the chip is removed from the actor
#[tokio::test]
async fn test_chip_dies_on_packet_sink_error() {
    let mut world = World::new();

    // 1. Create a virtual device chip.
    world.given_device("A").await;

    // A small delay to ensure the chip is registered before we check the count.
    tokio::time::sleep(Duration::from_millis(10)).await;
    let chip_count: usize = world.client.read_count_for_testing().await.expect("chip count");
    assert_eq!(chip_count, 1);

    // 2. Trigger a packet sink error by closing the receiver.
    world.when_sink_dropped("A");

    // 3. Send a packet to trigger the sink write (which will fail).
    // We send an HCI Reset command.
    let hci_reset_cmd = Bytes::from(vec![0x01, 0x03, 0x0c, 0x00]);
    world.when_packet_sent("A", hci_reset_cmd).await;

    // 4. Verify the chip has been removed.
    // A small delay is needed to ensure the actor has time to process the death
    // notice.
    tokio::time::sleep(Duration::from_millis(500)).await;
    let chip_count: usize = world.client.read_count_for_testing().await.expect("chip count");
    assert_eq!(chip_count, 0);
}

// Scenario: Reset re-enables chip states
//
//   Given a disabled bluetooth chip
//   When a reset action is sent
//   Then the chip Classic and LE states are re-enabled
#[tokio::test]
async fn test_actor_reset_re_enables_chip() {
    let mut world = World::new();

    // 1. Create a virtual device chip.
    world.given_device("A").await;
    let id = *world.chips.get("A").unwrap();

    // 2. Disable the chip
    let update = netsim_model::ChipUpdate {
        variant: Some(netsim_model::ChipVariantUpdate::Bluetooth(netsim_model::BluetoothUpdate {
            low_energy: netsim_model::RadioUpdate { state: Some(false) },
            classic: netsim_model::RadioUpdate { state: Some(false) },
            ..Default::default()
        })),
        ..Default::default()
    };
    world.client.update(id, update).await.unwrap();

    // 3. Perform Reset Action
    world.client.reset(id).await.unwrap();

    // 4. Verify it is re-enabled!
    let chip = world.client.read(id).await.unwrap();
    if let Some(netsim_model::ChipVariant::Bluetooth(bt)) = &chip.variant {
        assert_eq!(bt.low_energy.state, Some(true), "LE should be re-enabled");
        assert_eq!(bt.classic.state, Some(true), "Classic should be re-enabled");
    } else {
        panic!("Not a Bluetooth chip");
    }
}

// Scenario: Create chip with Laird BL654 preset applies buffer size
#[tokio::test]
async fn test_create_chip_with_laird_bl654_preset_applies_buffer_size() {
    let mut world = World::new();

    let props = netsim_model::BtControllerProperties {
        preset: Some("laird_bl654".to_string()),
        ..Default::default()
    };
    world.given_created_device_chip_with_properties("A", props).await;

    world.when_packet_sent("A", Bytes::from(vec![0x01, 0x03, 0x0c, 0x00])).await;
    world.then_packet_received("A", &[0x04, 0x0e, 0x04, 0x01, 0x03, 0x0c, 0x00]).await;

    world.when_packet_sent("A", Bytes::from(vec![0x01, 0x02, 0x20, 0x00])).await;
    world
        .then_packet_received("A", &[0x04, 0x0e, 0x07, 0x01, 0x02, 0x20, 0x00, 0x00, 0x02, 0x04])
        .await;
}

// Scenario: Create chip with default config buffer size
#[tokio::test]
async fn test_create_chip_with_default_config_buffer_size() {
    let mut world = World::new();

    world.given_device("A").await;

    world.when_packet_sent("A", Bytes::from(vec![0x01, 0x03, 0x0c, 0x00])).await;
    world.then_packet_received("A", &[0x04, 0x0e, 0x04, 0x01, 0x03, 0x0c, 0x00]).await;

    world.when_packet_sent("A", Bytes::from(vec![0x01, 0x02, 0x20, 0x00])).await;
    world
        .then_packet_received("A", &[0x04, 0x0e, 0x07, 0x01, 0x02, 0x20, 0x00, 0x1b, 0x00, 0x14])
        .await;
}

// Scenario: Create chip disabling CIS feature clears feature bits
#[tokio::test]
async fn test_create_chip_disabling_cis_feature_clears_feature_bits() {
    let mut world = World::new();

    let props = netsim_model::BtControllerProperties {
        features: Some(netsim_model::BtControllerFeatures {
            le_connected_isochronous_stream: Some(false),
            ..Default::default()
        }),
        ..Default::default()
    };
    world.given_created_device_chip_with_properties("A", props).await;

    world.when_packet_sent("A", Bytes::from(vec![0x01, 0x03, 0x0c, 0x00])).await;
    world.then_packet_received("A", &[0x04, 0x0e, 0x04, 0x01, 0x03, 0x0c, 0x00]).await;

    world.when_packet_sent("A", Bytes::from(vec![0x01, 0x03, 0x20, 0x00])).await;
    let packet = world.receive_packet("A").await;
    assert_eq!(packet.len(), 15);
    assert_eq!(&packet[0..7], &[0x04, 0x0e, 0x0c, 0x01, 0x03, 0x20, 0x00]);
    // Feature byte 3 (index 10 with H4 prefix) has CIS bits 0x30 cleared:
    assert_eq!(packet[10] & 0x30, 0x00, "CIS feature bits should be cleared");
}
