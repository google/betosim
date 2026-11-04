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

use pica::packets::uci;

use crate::world::World;

// Feature: UWB Chip Reset via Actor Client
//
//   As a client
//   I want to reset a UWB chip through the UwbClient
//   And receive the expected UCI responses from Pica
//   So that I can verify the end-to-end reset flow

// Scenario: Reset UWB chip and receive response/notification
//
//   Given the UWB actor is running with Pica
//   And a UWB chip is created
//   When the chip is reset
//   Then a CORE_DEVICE_RESET_RSP(OK) is received
//   And a CORE_DEVICE_STATUS_NTF(READY) is received
#[tokio::test]
async fn test_chip_reset() {
    // Given
    let mut world = World::new().await;
    let chip_id = 1;
    world.given_a_chip(chip_id).await;
    world.then_status_notification_is_received(chip_id, uci::DeviceState::DeviceStateReady).await;

    // When
    world.when_chip_is_reset(chip_id).await;

    // Then
    world.then_reset_response_is_received(chip_id, uci::Status::Ok).await;
    world.then_status_notification_is_received(chip_id, uci::DeviceState::DeviceStateReady).await;
}

const POST_RESET_UCI_RX_PACKETS: u64 = 2;

// Scenario: Resetting a UWB chip clears its packet counters
#[tokio::test]
async fn test_chip_reset_clears_packet_counters() {
    // Given a UWB chip that has exchanged traffic with non-zero packet counters
    let mut world = World::new().await;
    let chip_id = world.given_initialized_chip("uwb-reset-stats").await;
    world.given_chip_with_nonzero_packet_counters(chip_id).await;

    // When the chip is reset
    world.when_chip_is_reset(chip_id).await;

    // Then the packet counters are reset to only the post-reset UCI handshake packets
    world.then_packet_counters_are(chip_id, 0, POST_RESET_UCI_RX_PACKETS).await;
}
