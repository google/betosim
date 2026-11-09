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

mod ftm_test;
mod gateway_test;
mod hwsim_helper;
mod lifecycle_test;
mod multiclient_nullfunc_test;
mod packet_routing_test;
mod pmsr_test;
mod slirp_dual_mapping_test;
mod stats_test;
mod veth_test;
mod world;

use world::World;

// Scenario: Full chip lifecycle, P2P messaging, disable drop, and deletion notification
// Given a Wifi World with two provisioned chips
// When Chip 1 transmits a unicast frame to Chip 2, then Chip 2 is disabled, and Chip 1 is deleted
// Then Chip 2 receives the first frame, drops the frame while disabled, and DeviceActor is
//   notified of Chip 1 removal
#[tokio::test]
async fn test_full_lifecycle_and_messaging() {
    // Given
    let mut world = World::new().await;
    let chip1_id = world.given_a_chip(1).await;
    let _chip2_id = world.given_a_chip(2).await;

    // When / Then: Verify communication (Chip 1 -> Chip 2)
    world.when_chip_transmits_unicast(0, 1, "Hello Integration").await;
    world.then_chip_receives_payload(1, "Hello Integration").await;

    // When: Disable Chip 2 (via DeviceClient)
    world.given_chip_is_disabled(1).await;

    // Then: Verify dropped packet
    world.when_chip_transmits_unicast(0, 1, "Should be dropped").await;
    world.then_chip_receives_nothing(1).await;

    // When: Delete Chip 1 (Simulate DeviceActor deletion)
    use netsim_model::{ChipClient, ChipId};
    world.wifi_client.delete(ChipId(chip1_id)).await.expect("Failed to delete chip 1");

    // Then: Verify Device Notification
    use device_api::DeviceAction;
    let action = world.device_action_rx.recv().await.expect("Expected DeviceAction");
    if let DeviceAction::NotifyChipRemoved(_device_id, removed_chip_id) = action {
        assert_eq!(removed_chip_id.0, chip1_id, "Expected notification for chip 1 removal");
    } else {
        panic!("Expected NotifyChipRemoved action, got {:?}", action);
    }
}
