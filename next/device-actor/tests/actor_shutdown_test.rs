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

// Feature: Server Shutdown
//
//   As a system administrator or developer
//   I want the device actor server to shut down when idle or when requested
//   So that resources are released when not in use

use std::time::Duration;

use crate::world::World;

// Scenario: Server shuts down after idle timeout
//   Given a running Device Actor
//   When the server is idle for a duration
//   Then the server shuts down automatically
// Scenario: Server shuts down after startup timeout if no devices connect
//   Given a running Device Actor with a startup timeout
//   When no devices are created for a duration
//   Then the server shuts down automatically
#[tokio::test]
async fn test_server_shutdown_on_startup_timeout() {
    let chip_clients = World::create_default_chip_clients();
    let link_client = crate::world::World::create_default_link_client();

    // Startup timeout = 50ms
    let world = World::with_clients_and_timeout(
        chip_clients,
        link_client,
        Some(Duration::from_millis(50)), // startup
        None,                            // idle
        None,
    )
    .await;

    world.then_actor_should_shutdown().await;
}

// Scenario: Server does NOT shut down if device exists
//   Given a running Device Actor with timeouts
//   When I create a device
//   Then the server stays alive even after startup timeout
#[tokio::test]
async fn test_server_stays_alive_with_device() {
    let chip_clients = World::create_default_chip_clients();
    let link_client = crate::world::World::create_default_link_client();

    let world = World::with_clients_and_timeout(
        chip_clients,
        link_client,
        Some(Duration::from_millis(200)), // startup
        Some(Duration::from_millis(50)),  // idle
        None,
    )
    .await;

    // Create a device with a Bluetooth chip (which counts as Active)
    let _id = world.when_add_chip("guid-1", "chip-1").await;

    // Wait > timeouts
    tokio::time::sleep(Duration::from_millis(200)).await;

    assert!(!world.is_actor_finished(), "Server should stay alive while device exists");
}

// Scenario: Server shuts down after idle timeout when last device is removed
//   Given a running Device Actor
//   When I create a device and then delete it
//   Then the server shuts down after the idle timeout
#[tokio::test]
async fn test_server_shuts_down_after_idle_timeout_when_last_device_removed() {
    let chip_clients = World::create_default_chip_clients();
    let link_client = crate::world::World::create_default_link_client();

    let world = World::with_clients_and_timeout(
        chip_clients,
        link_client,
        None,                            // startup
        Some(Duration::from_millis(50)), // idle
        None,
    )
    .await;

    let device_id = world.when_add_chip("guid-1", "chip-1").await;

    // Delete device -> count goes to 0 -> triggers idle timer
    world.when_delete_device(device_id).await;

    // Wait < idle timeout (e.g. 10ms)
    tokio::time::sleep(Duration::from_millis(10)).await;
    assert!(!world.is_actor_finished(), "Server should NOT shut down immediately");

    world.then_actor_should_shutdown().await;
}
