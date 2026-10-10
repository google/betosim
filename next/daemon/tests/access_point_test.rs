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

use crate::world::World;

// Scenario: Access Point Lifecycle
//   Given a running Netsim Daemon
//   When I create a new Access Point
//   Then the AP exists and can be retrieved
//   When I update the AP
//   Then the AP reflects the changes
//   When I list APs
//   Then the AP is in the list
//   When I delete the AP
//   Then the AP is no longer found
#[tokio::test]
async fn test_access_point_lifecycle() {
    // Given a running Netsim Daemon
    let mut world = World::new().await;
    world.when_spawn_daemon().await;

    // 1. Create AP
    let ap_id = world.when_create_access_point("TestAP", 36, "a").await;
    assert!(ap_id > 0);
    world.then_access_point_matches(ap_id, "TestAP", 36, "a").await;

    // 2. Update AP
    world.when_update_access_point(ap_id, Some("UpdatedAP"), Some(40)).await;
    world.then_access_point_matches(ap_id, "UpdatedAP", 40, "a").await;

    // 3. List APs
    world.then_access_point_in_list(ap_id).await;

    // 4. Execute Disconnect (Action)
    world.when_execute_disconnect(ap_id, "00:11:22:33:44:55").await;

    // 5. Delete AP
    world.when_delete_access_point(ap_id).await;

    // Verify Deletion
    world.then_access_point_not_found(ap_id).await;
}
