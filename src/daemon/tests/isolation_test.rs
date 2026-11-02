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

use crate::world::World;

// Scenario: Global Reset Isolation
//   Given a running Netsim Daemon
//   And a built-in Default AP exists
//   And the built-in Default AP is mutated
//   And an internal CoffeeShop AP is created
//   And an internal mock device is created
//   When the global reset RPC is called
//   Then the internal CoffeeShop AP should be deleted
//   And the internal mock device should be deleted
//   And the built-in Default AP should be restored to its initial configuration
#[tokio::test]
#[cfg(not(feature = "cuttlefish"))]
async fn test_global_reset_isolation() {
    let mut world = World::new().await;
    world.when_spawn_daemon().await;

    // Verify Default AP exists (Built-in)
    world.then_access_point_exists_by_ssid("AndroidWifi").await;

    // Mutate the built-in Default AP (id: 0)
    world.when_update_access_point(0, Some("MutatedWifi"), Some(11)).await;
    world.then_access_point_exists(0, "MutatedWifi").await;

    // Create an internal CoffeeShop AP
    let coffeeshop_ap_id = world.when_create_access_point("CoffeeShop", 36, "a").await;

    // Create an internal mock device (No GUID)
    let mock_device_id = world.when_create_device("mock-device", "beacon").await;

    // Verify all exist before reset
    world.then_access_point_in_list(coffeeshop_ap_id).await;
    world.then_device_list_contains(mock_device_id, "mock-device").await;

    // When I call the reset RPC
    world.when_reset_is_called().await;

    // Then the internal CoffeeShop AP should be deleted
    world.then_access_point_not_in_list(coffeeshop_ap_id).await;

    // And the internal mock device should be deleted
    world.then_device_list_does_not_contain(mock_device_id).await;

    // And the built-in Default AP should be restored to its initial
    // configuration
    world.then_access_point_exists(0, "AndroidWifi").await;
    world.then_access_point_exists_by_ssid("AndroidWifi").await;
}
