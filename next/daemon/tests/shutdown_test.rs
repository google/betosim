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
