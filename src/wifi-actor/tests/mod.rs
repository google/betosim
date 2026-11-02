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
mod packet_routing_test;
mod pmsr_test;
mod slirp_dual_mapping_test;
mod stats_test;
// mod veth_test; // Broken/Outdated
mod world;

use world::World;

// Tests full lifecycle and cross-actor interactions.
#[tokio::test]
async fn test_full_lifecycle_and_messaging() {
    let mut world = World::new().await;

    // 1. Create Chips
    let chip1_id = world.given_a_chip(1).await;
    let chip2_id = world.given_a_chip(2).await;

    // 2. Verify communication (Chip 1 -> Chip 2)
    world.when_chip_transmits_unicast(0, 1, "Hello Integration").await;
    world.then_chip_receives_payload(1, "Hello Integration").await;

    // 3. Disable Chip 2 (via DeviceClient)
    // This triggers wifi_client -> wifi_actor -> medium -> set_enabled(false)
    world.given_chip_is_disabled(1).await;

    // 4. Verify dropped packet
    world.when_chip_transmits_unicast(0, 1, "Should be dropped").await;
    world.then_chip_receives_nothing(1).await;

    // 5. Delete Chip 1 (Simulate DeviceActor deletion)
    // We use the wifi_client directly for deletion as `world` helpers mostly expose
    // higher level given/when but correct integration path is via ChipClient
    // (which `world.wifi_client` provides).
    use netsim_model::{ChipClient, ChipId};
    world.wifi_client.delete(ChipId(chip1_id)).await.expect("Failed to delete chip 1");

    // 6. Verify Device Notification
    // WifiActor should notify DeviceActor about chip removal.
    // Our Mock Device Client forwards actions to `world.device_action_rx`.
    use device_api::DeviceAction;
    let action = world.device_action_rx.recv().await.expect("Expected DeviceAction");
    if let DeviceAction::NotifyChipRemoved(_device_id, removed_chip_id) = action {
        assert_eq!(removed_chip_id.0, chip1_id, "Expected notification for chip 1 removal");
    } else {
        panic!("Expected NotifyChipRemoved action, got {:?}", action);
    }
}
