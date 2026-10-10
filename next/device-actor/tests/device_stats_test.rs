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

use crate::world::World;

// Scenario: Device Stats are persisted
//   Given I have a device configuration with specific details
//   When I create the device from the pending configuration
//   And I shut down the actor
//   Then the persisted stats should match the device details
#[tokio::test]
async fn test_device_stats_persisted() {
    let mut world = World::new().await;

    let name = "Test Device";
    let device_kind = "EMULATOR";
    let build_id = "34.1.15.0";
    let build_tags = "33";
    let manufacturer = "TE1A.220922.034";
    let model = "sdk_gphone_x86_64-userdebug";
    let arch = "x86_64";

    world.given_device_config(name, device_kind, build_id, build_tags, manufacturer, model, arch);
    world.when_create_device_from_config().await;
    world.when_shutdown_actor().await;

    world
        .then_stats_should_contain_device_details(
            device_kind,
            build_id,
            build_tags,
            manufacturer,
            model,
            arch,
        )
        .await;
}
