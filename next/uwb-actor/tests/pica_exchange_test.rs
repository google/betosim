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

use pdl_runtime::Packet;
use pica::packets::uci;

use crate::world::World;

// Feature: UWB UCI Packet Exchange with Pica
//
//   As a client
//   I want to send UCI packets to a UWB chip
//   And receive responses from the Pica simulator
//   So that I can verify the end-to-end integration

// Scenario: Send CORE_DEVICE_RESET_CMD and receive response/notification
//
//   Given the UWB actor is running with Pica
//   And a UWB chip is created
//   When I send a CORE_DEVICE_RESET_CMD
//   Then I receive a CORE_DEVICE_RESET_RSP
//   And I receive a CORE_DEVICE_STATUS_NTF(READY)
#[tokio::test]
async fn test_uci_reset_exchange() {
    // Given
    let mut world = World::new().await;
    let chip_id = 100;
    world.given_a_chip(chip_id).await;
    world.then_status_notification_is_received(chip_id, uci::DeviceState::DeviceStateReady).await;

    // When: Send CORE_DEVICE_RESET_CMD (reset_config=UWBS_RESET)
    // Use strongly typed packet from pica crate
    let reset_cmd = uci::CoreDeviceResetCmd { reset_config: uci::ResetConfig::UwbsReset };
    world.when_packet_is_sent(chip_id, &reset_cmd.encode_to_vec().unwrap()).await;

    // Then: Wait for responses
    // We expect at least two packets: RSP and NTF
    let mut packets = Vec::new();
    // Use the timeout built into then_packet_is_received
    for _ in 0..2 {
        let p = world.then_packet_is_received(chip_id).await;
        packets.push(p);
    }

    // TODO(b/483093069): These should be UWB world methods

    assert!(packets.len() >= 2, "Expected at least 2 packets, got {}", packets.len());

    let mut has_reset_rsp = false;
    let mut has_status_ntf = false;

    for p in packets {
        if let Ok((rsp, _)) = uci::CoreDeviceResetRsp::decode(&p) {
            assert_eq!(rsp.status, uci::Status::Ok);
            has_reset_rsp = true;
        } else if let Ok((ntf, _)) = uci::CoreDeviceStatusNtf::decode(&p) {
            assert_eq!(ntf.device_state, uci::DeviceState::DeviceStateReady);
            has_status_ntf = true;
        }
    }

    assert!(has_reset_rsp, "Missing CORE_DEVICE_RESET_RSP or decode failed");
    assert!(has_status_ntf, "Missing CORE_DEVICE_STATUS_NTF or decode failed");
}
