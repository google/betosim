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

use futures::SinkExt;
use netsim_proto::{packet_streamer::PacketRequest, startup::ChipInfo};

use crate::world::World;

// Scenario: Daemon shuts down when the last client disconnects
//   Given a running Netsim Daemon
//   When a client connects, adds a chip, and then disconnects
//   Then the daemon counts 0 active chips and shuts down
#[tokio::test]
async fn test_auto_shutdown_on_chip_removal() {
    // 1. Start Daemon with 1s timeout
    let mut args = daemon::Args::default();
    args.logtostderr = true;
    args.idle_shutdown_timeout = Some(1000);
    let mut world = World::new_with_args(args).await;
    world.when_spawn_daemon().await;

    // 2. Connect Client (mimic emulator)
    let (mut client_sender, _client_receiver) =
        world.packet_client.stream_packets().expect("Failed to create stream");

    // 3. Add Chip
    let mut initial_req = PacketRequest::new();
    let mut chip_info = ChipInfo::new();

    // Create a valid Chip model (startup::Chip)
    let mut chip = netsim_proto::startup::Chip::new();
    chip.kind =
        netsim_proto::protobuf::EnumOrUnknown::new(netsim_proto::common::ChipKind::BLUETOOTH);
    chip.address = "33:33:33:33:33:33".to_string();
    chip.manufacturer = "TestMfg".to_string();
    chip.product_name = "TestProduct".to_string();

    chip_info.chip = netsim_proto::protobuf::MessageField::some(chip);

    let mut device_info = netsim_proto::startup::DeviceInfo::new();
    device_info.name = "shutdown-test-device".to_string();
    device_info.kind = "EMULATOR".to_string();
    chip_info.device_info = netsim_proto::protobuf::MessageField::some(device_info);

    initial_req.set_initial_info(chip_info);

    client_sender
        .send((initial_req, grpcio::WriteFlags::default()))
        .await
        .expect("Failed to send InitialInfo");

    // Allow some time for the daemon to process the new chip
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    // 3.5 Verify Device List
    world.then_device_list_contains_by_name("shutdown-test-device").await;

    // 4. Disconnect (Drop sender)
    drop(client_sender);

    // 5. Verify EARLY Check (should NOT be shut down yet)
    // Timeout is 3s. We wait 0.5s. Daemon should still be running.
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    if world.is_daemon_finished() {
        panic!("Daemon unexpectedly shut down early!");
    }

    world.then_daemon_shutdown(3000).await;
}

// Scenario: Daemon does NOT shut down before the timeout
//   Given a running Netsim Daemon with 2s timeout
//   When a client disconnects
//   Then the daemon is still running after 1s
//   And the daemon shuts down after 2.5s
#[tokio::test]
async fn test_daemon_stays_alive_before_timeout() {
    // 1. Start Daemon with 2s idle timeout
    let mut args = daemon::Args::default();
    args.idle_shutdown_timeout = Some(2000);
    args.logtostderr = true;
    let mut world = World::new_with_args(args).await;
    world.when_spawn_daemon().await;

    // 2. Connect Client
    let (mut client_sender, _client_receiver) =
        world.packet_client.stream_packets().expect("Failed to create stream");

    // 3. Add Chip and Verify
    let mut initial_req = PacketRequest::new();
    let mut chip_info = ChipInfo::new();
    let mut chip = netsim_proto::startup::Chip::new();
    chip.kind =
        netsim_proto::protobuf::EnumOrUnknown::new(netsim_proto::common::ChipKind::BLUETOOTH);
    chip.address = "33:33:33:33:33:33".to_string();
    chip_info.chip = netsim_proto::protobuf::MessageField::some(chip);
    initial_req.set_initial_info(chip_info);
    client_sender
        .send((initial_req, grpcio::WriteFlags::default()))
        .await
        .expect("Failed to send InitialInfo");

    // Wait for chip to be added
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    // 4. Disconnect
    drop(client_sender);

    // 5. Wait 1s (daemon should still be alive)
    tokio::time::sleep(std::time::Duration::from_millis(1000)).await;
    if world.is_daemon_finished() {
        panic!("Daemon shut down prematurely!");
    }

    world.then_daemon_shutdown(2000).await;
}

// Scenario: Daemon shuts down on its own due to the startup grace period
//   Given a newly started Netsim daemon with a 2s startup timeout
//   When no connected devices are spawned
//   Then the daemon gracefully self-terminates after slightly more than 2s
#[tokio::test]
async fn test_startup_shutdown_timeout() {
    // 1. Start Daemon with a short 2s startup timeout override
    let mut args = daemon::Args::default();
    args.startup_timeout = Some(2000);
    args.logtostderr = true;
    let mut world = World::new_with_args(args).await;
    world.when_spawn_daemon().await;

    world.then_daemon_shutdown(3000).await;
}

// Scenario: Daemon releases its discovery files when it shuts down
//   Given a running Netsim Daemon that owns netsim.ini.lock
//   When the startup grace period expires with no devices
//   Then netsim.ini is removed and netsim.ini.lock can be acquired again
#[tokio::test]
async fn test_shutdown_releases_discovery_files() {
    let mut args = daemon::Args::default();
    args.startup_timeout = Some(1000);
    args.logtostderr = true;
    let mut world = World::new_with_args(args).await;
    world.when_spawn_daemon().await;

    world.then_daemon_shutdown(5000).await;
    world.then_discovery_files_released();
}

const NO_SHUTDOWN_TEST_STARTUP_TIMEOUT_STR: &str = "1000";
const SUPPRESSED_STARTUP_TIMEOUT_STR: &str = "2500";
const ZERO_IDLE_TIMEOUT_STR: &str = "0";
const OVERRIDDEN_IDLE_TIMEOUT_STR: &str = "2000";
const SHORT_STARTUP_STAY_ALIVE_MS: u64 = 1500;
const SUPPRESSED_STARTUP_STAY_ALIVE_MS: u64 = 3200;
const IDLE_STAY_ALIVE_MS: u64 = 800;
const SHUTDOWN_DEADLINE_MS: u64 = 5000;

// Scenario: --no-shutdown keeps daemon alive past startup and keepalive-close idle timeouts
//   Given a running daemon with --no-shutdown alongside 1000ms startup and 0ms idle flags
//   When no devices connect and a /v1/keepalive lease opens and closes
//   Then the daemon stays running across both windows
#[tokio::test]
async fn test_no_shutdown_flag_stays_up_without_and_after_keepalive() {
    // Given
    let mut world = World::new_with_cli_args(&[
        "--no-shutdown",
        "--startup-timeout",
        NO_SHUTDOWN_TEST_STARTUP_TIMEOUT_STR,
        "--idle-shutdown-timeout",
        ZERO_IDLE_TIMEOUT_STR,
    ])
    .await;
    world.when_spawn_daemon().await;
    // Then
    world.then_daemon_stays_running(SHORT_STARTUP_STAY_ALIVE_MS).await;
    // When
    world.when_open_keepalive().await;
    world.when_close_keepalive();
    // Then
    world.then_daemon_stays_running(IDLE_STAY_ALIVE_MS).await;
}

// Scenario: Default 0s --idle-shutdown-timeout shuts down immediately when last keepalive closes
//   Given a running daemon with default shutdown flags and two /v1/keepalive leases
//   When the first lease closes and then the last lease closes with no connected devices
//   Then the daemon stays up after the first close and shuts down cleanly after the last close
#[tokio::test]
async fn test_default_idle_shutdown_on_last_keepalive_close() {
    // Given
    let mut world = World::new_with_cli_args(&[]).await;
    world.when_spawn_daemon().await;
    world.when_open_keepalive().await;
    world.when_open_keepalive().await;
    // When
    world.when_close_keepalive();
    // Then
    world.then_daemon_stays_running(IDLE_STAY_ALIVE_MS).await;
    // When
    world.when_close_keepalive();
    // Then
    world.then_daemon_shutdown(SHUTDOWN_DEADLINE_MS).await;
    world.then_discovery_files_released();
}

// Scenario: Active /v1/keepalive lease suppresses --startup-timeout override
//   Given a running daemon with a 2500ms --startup-timeout override
//   When a /v1/keepalive lease opens before startup timeout and later closes
//   Then the daemon stays up past 2500ms while held and shuts down once the lease closes
#[tokio::test]
async fn test_keepalive_suppresses_startup_timeout_flag() {
    // Given
    let mut world =
        World::new_with_cli_args(&["--startup-timeout", SUPPRESSED_STARTUP_TIMEOUT_STR]).await;
    world.when_spawn_daemon().await;
    // When
    world.when_open_keepalive().await;
    // Then
    world.then_daemon_stays_running(SUPPRESSED_STARTUP_STAY_ALIVE_MS).await;
    // When
    world.when_close_keepalive();
    // Then
    world.then_daemon_shutdown(SHUTDOWN_DEADLINE_MS).await;
}

// Scenario: --idle-shutdown-timeout flag override delays shutdown after keepalive closes
//   Given a running daemon with --idle-shutdown-timeout 2000
//   When a /v1/keepalive lease opens and closes with zero devices
//   Then the daemon stays up during the 2000ms idle grace period and shuts down afterward
#[tokio::test]
async fn test_idle_shutdown_timeout_flag_delays_keepalive_close_shutdown() {
    // Given
    let mut world =
        World::new_with_cli_args(&["--idle-shutdown-timeout", OVERRIDDEN_IDLE_TIMEOUT_STR]).await;
    world.when_spawn_daemon().await;
    // When
    world.when_open_keepalive().await;
    world.when_close_keepalive();
    // Then
    world.then_daemon_stays_running(IDLE_STAY_ALIVE_MS).await;
    world.then_daemon_shutdown(SHUTDOWN_DEADLINE_MS).await;
}

// Scenario: Connected device keeps daemon running after keepalive closes until device disconnects
//   Given a running daemon with default shutdown flags
//   When /v1/keepalive opens, a non-loopback device connects, /v1/keepalive closes, and later the device disconnects
//   Then the daemon stays up while the device is connected and shuts down once the device disconnects
#[tokio::test]
async fn test_connected_device_prevents_shutdown_on_keepalive_close() {
    // Given
    let mut world = World::new_with_cli_args(&[]).await;
    world.when_spawn_daemon().await;
    // When
    world.when_open_keepalive().await;
    world.when_open_packet_stream().await;
    world.when_send_device_initial_info("keepalive-handoff-device").await;
    world.then_eventually_device_list_contains_by_name("keepalive-handoff-device").await;
    world.when_close_keepalive();
    // Then
    world.then_daemon_stays_running(IDLE_STAY_ALIVE_MS).await;
    // When
    world.when_close_packet_stream().await;
    // Then
    world.then_daemon_shutdown(SHUTDOWN_DEADLINE_MS).await;
}

// Scenario: --startup-timeout flag shuts down daemon when no keepalive or device connects
//   Given a running daemon with a 1000ms --startup-timeout CLI flag
//   When no keepalive lease or device connects
//   Then the daemon shuts down within the shutdown deadline
#[tokio::test]
async fn test_startup_timeout_flag_shuts_down_daemon_without_keepalive() {
    // Given
    let mut world =
        World::new_with_cli_args(&["--startup-timeout", NO_SHUTDOWN_TEST_STARTUP_TIMEOUT_STR])
            .await;
    // When
    world.when_spawn_daemon().await;
    // Then
    world.then_daemon_shutdown(SHUTDOWN_DEADLINE_MS).await;
}
