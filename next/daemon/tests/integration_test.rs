// Copyright 2023-2025 The Android Open Source Project
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

use std::{collections::HashMap, time::Duration};

use bytes::Bytes;
use futures::{SinkExt, StreamExt};
use netsim_model::{ChipInfo, ChipKind};
use packet_stream::{Streams, TransportType};
use tokio::time::timeout;

use crate::world::World;

// HACK: Raw HCI packets until rootcanal packet crate is easily usable
const HCI_RESET_COMMAND: [u8; 3] = [0x03, 0x0c, 0x00]; // OpCode, Length

// Feature: Device Integration via UDS
//
//   As a virtual device
//   I want to connect to netsim via Unix Domain Socket
//   So that I can send and receive radio packets

// Scenario: Bluetooth HCI Reset via UDS
//   Given a running Netsim Daemon listening on UDS
//   When a client connects and identifies as a Bluetooth chip
//   And the client sends an HCI Reset command
//   Then the client receives an HCI Command Complete event
#[tokio::test]
#[ignore]
async fn test_bluetooth_hci_reset() {
    // Given a running Netsim Daemon
    let mut world = World::new().await;

    // Start daemon
    world.when_spawn_daemon().await;

    // Spawn the client task
    let client_task = tokio::spawn(async move {
        // Allow some time for netsimd to fully start
        tokio::time::sleep(Duration::from_millis(100)).await;

        let transport = TransportType::tcp("localhost", world.grpc_port);
        let chip_info = ChipInfo::new("bt_test", ChipKind::BLUETOOTH);

        let streams = Streams::new();
        let (mut stream, mut sink) = streams
            .connect(transport, chip_info)
            .await
            .expect("Failed to connect to netsim socket");

        // Send HCI Reset Command
        sink.send(Bytes::from_static(&HCI_RESET_COMMAND)).await.expect("Failed to send HCI Reset");

        // Receive and check response
        match timeout(Duration::from_secs(2), stream.next()).await {
            Ok(Some(Ok(packet))) => {
                assert!(packet.len() >= 6, "Response too short");
                let expected_event: [u8; 6] = [0x0e, 0x04, 0x01, 0x03, 0x0c, 0x00];
                assert_eq!(&packet[0..6], expected_event, "HCI Reset event did not match");
            }
            Ok(Some(Err(e))) => panic!("Failed to receive response: {}", e),
            Ok(None) => panic!("Stream closed unexpectedly"),
            Err(_) => panic!("Timeout waiting for HCI Reset response"),
        }
    });

    // Wait for client
    match timeout(Duration::from_secs(5), async move {
        let _ = client_task.await;
    })
    .await
    {
        Ok(_) => {}
        Err(_) => panic!("Test timed out"),
    }
}

// Scenario: Configure Default AP via CLI Args
//   Given I start netsimd with --wifi-ssid, --wifi-password, etc.
//   Then the default AP should be created with those settings
#[tokio::test]
async fn test_ap_config_args() {
    let mut args = daemon::Args::default();
    args.logtostderr = true;
    args.wifi.wifi_ssid = Some("CustomAP".to_string());
    args.wifi.wifi_password = Some("Secret123".to_string());
    args.wifi.wifi_channel = Some(6);
    args.wifi.wifi_beacon_interval = Some(200);
    args.wifi.wifi_mode = Some(daemon::ClapWifiMode::N);

    let mut world = World::new_with_args(args).await;

    world.then_access_point_matches_by_ssid("CustomAP", 6, "n").await;
}

// Scenario: Start daemon with --pcap
//   Given I start netsimd with --pcap
//   Then default capture state for new devices is enabled
#[tokio::test]
async fn test_pcap_args_enabled() {
    let mut args = daemon::Args::default();
    args.logtostderr = true;
    args.no_shutdown = true;
    args.pcap = true;

    let mut world = World::new_with_args(args).await;

    // Spawn daemon task to process background tasks
    world.when_spawn_daemon().await;

    let device_id = world.when_create_device("TestDevice", "TestChip").await;
    let devices = world.when_list_devices().await;
    let device = devices.iter().find(|d| d.id == device_id).expect("Device missing");
    let chip = device.chips.first().expect("Chip missing");

    world.then_capture_is(chip.id, true).await;
}

// Scenario: Start daemon without --pcap
//   Given I start netsimd without --pcap
//   Then default capture state for new devices is disabled
#[tokio::test]
async fn test_pcap_args_disabled() {
    let mut args = daemon::Args::default();
    args.logtostderr = true;
    args.no_shutdown = true;
    args.pcap = false;

    let mut world = World::new_with_args(args).await;

    world.when_spawn_daemon().await;

    let device_id = world.when_create_device("TestDevice", "TestChip").await;
    let devices = world.when_list_devices().await;
    let device = devices.iter().find(|d| d.id == device_id).expect("Device missing");
    let chip = device.chips.first().expect("Chip missing");

    world.then_capture_is(chip.id, false).await;
}

// Scenario: Toggle capture using generic patch
//   Given I start netsimd
//   When a device is created and its capture is toggled
//   Then its capture state updates correctly
#[tokio::test]
async fn test_capture_patch_enabled_flag() {
    let mut args = daemon::Args::default();
    args.logtostderr = true;
    args.no_shutdown = true;
    args.pcap = false;

    let mut world = World::new_with_args(args).await;

    world.when_spawn_daemon().await;

    let device_id = world.when_create_device("TestDevice", "TestChip").await;
    let devices = world.when_list_devices().await;
    let device = devices.iter().find(|d| d.id == device_id).expect("Device missing");
    let chip = device.chips.first().expect("Chip missing");

    // Initially disabled
    world.then_capture_is(chip.id, false).await;

    // Patch to enable
    world.when_patch_capture(chip.id, true).await;
    world.then_capture_is(chip.id, true).await;

    // Patch to disable
    world.when_patch_capture(chip.id, false).await;
    world.then_capture_is(chip.id, false).await;
}

// Scenario: Start daemon with --test_beacons
//   Given I start netsimd with --test_beacons
//   Then test beacons should be created automatically
#[tokio::test]
async fn test_test_beacons_args() {
    let mut args = daemon::Args::default();
    args.logtostderr = true;
    args.test_beacons = true;

    let mut world = World::new_with_args(args).await;

    world.then_device_list_contains_by_name("gDevice-beacon-1").await;
    world.then_device_list_contains_by_name("gDevice-beacon-2").await;
}

// Scenario: Start daemon with unresolvable HTTP proxy
//   Given I start netsimd with an unresolvable --http-proxy
//   Then netsimd logs the error without crashing or exiting, and gRPC services
// remain responsive
#[tokio::test]
async fn test_daemon_startup_with_invalid_proxy() {
    let mut args = daemon::Args::default();
    args.logtostderr = true;
    args.no_shutdown = true;
    args.http_proxy = Some("http://invalid-unresolvable-proxy.invalid:8080".to_string());

    // Daemon should start up successfully without crashing or exiting
    let mut world = World::new_with_args(args).await;
    world.when_spawn_daemon().await;

    // Verify gRPC frontend service is healthy and responsive
    let version = world.when_get_version().await;
    assert!(!version.is_empty());
}

// Scenario: Start daemon with empty --http-proxy override
//   Given I start netsimd with an empty/whitespace --http-proxy
//   Then sanitize() clears http_proxy to None and daemon starts normally
#[tokio::test]
async fn test_daemon_startup_with_empty_http_proxy() {
    let mut args = daemon::Args::default();
    args.logtostderr = true;
    args.no_shutdown = true;
    args.http_proxy = Some("   ".to_string());

    let mut world = World::new_with_args(args).await;
    world.when_spawn_daemon().await;

    let version = world.when_get_version().await;
    assert!(!version.is_empty());
}

// Scenario: Start daemon with valid HTTP proxy
//   Given I start netsimd with a valid --http-proxy
//   Then netsimd initializes HTTP proxy manager and starts normally
#[tokio::test]
async fn test_daemon_startup_with_valid_proxy() {
    let mut args = daemon::Args::default();
    args.logtostderr = true;
    args.no_shutdown = true;
    args.http_proxy = Some("http://127.0.0.1:8080".to_string());

    let mut world = World::new_with_args(args).await;
    world.when_spawn_daemon().await;

    let version = world.when_get_version().await;
    assert!(!version.is_empty());
}

// Feature: Discovery lock recovery
//
//   As an emulator launching netsimd
//   I want a wedged netsimd's stale netsim.ini.lock to be evicted
//   So that later AVDs can still boot

// Scenario: Start daemon while an unresponsive owner holds netsim.ini.lock
//   Given netsim.ini advertises a gRPC port that accepts TCP but never answers
//   And its owner still holds netsim.ini.lock
//   When I start netsimd in the same discovery directory
//   Then it evicts the stale lock and starts as Owner on a new port
//   And the evicted owner releasing its lock leaves the new netsim.ini intact
#[tokio::test]
async fn test_startup_evicts_stale_lock_of_unresponsive_owner() {
    let temp_dir =
        std::env::temp_dir().join(format!("netsim_stale_lock_{}", rand::random::<u32>()));
    std::fs::create_dir_all(&temp_dir).expect("Failed to create temp dir");

    let wedged_listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let dead_port = wedged_listener.local_addr().unwrap().port();

    let stale_ini = daemon::IniFile::new_for_dir(temp_dir.clone(), 1).unwrap();
    let Ok(daemon::IniFileAccess::Writer(stale_writer)) = stale_ini.try_acquire() else {
        panic!("Wedged owner should be Writer");
    };
    let mut stale_data = HashMap::new();
    stale_data.insert("pid".to_string(), std::process::id().to_string());
    stale_data.insert("grpc.port".to_string(), dead_port.to_string());
    let stale_guard = stale_writer.write(&stale_data).unwrap();

    let mut args = daemon::Args::default();
    args.logtostderr = true;
    args.no_shutdown = true;

    // `new_with_args_in_dir` panics unless the daemon starts as Owner.
    let mut world = World::new_with_args_in_dir(temp_dir, args).await;
    world.when_spawn_daemon().await;

    assert_ne!(world.grpc_port, dead_port);
    assert!(!stale_guard.still_owns_lock());
    world.then_discovery_file_advertises_port(world.grpc_port);

    drop(stale_guard);
    world.then_discovery_file_advertises_port(world.grpc_port);
}

// Scenario: Start daemon while a responsive owner holds netsim.ini.lock
//   Given a running Netsim Daemon
//   When I start another netsimd in the same discovery directory
//   Then it becomes a Client of the running daemon instead of evicting it
#[tokio::test]
async fn test_startup_joins_responsive_owner_as_client() {
    let mut world = World::new().await;
    world.when_spawn_daemon().await;

    let mut args = daemon::Args::default();
    args.logtostderr = true;
    args.no_shutdown = true;

    match world.when_start_daemon_in_same_dir(args).await {
        daemon::StartUpMode::Client(config) => assert_eq!(config.grpc_port, world.grpc_port),
        _ => panic!("Expected StartUpMode::Client"),
    }
    world.then_discovery_file_advertises_port(world.grpc_port);
}

// Scenario: Another process holds netsim.ini.init.lock when the stale owner
// is first evicted
//   Given netsim.ini advertises a gRPC port that accepts TCP but never answers
//   And its owner still holds netsim.ini.lock
//   And another starter takes netsim.ini.init.lock as soon as the liveness
//     probe connects and keeps it past the probe deadline
//   When I start netsimd in the same discovery directory
//   Then its first eviction attempt is skipped rather than consumed
//   And it evicts the stale lock once netsim.ini.init.lock is released
//   And starts as Owner on a new port
#[tokio::test]
async fn test_startup_retries_eviction_after_busy_init_lock() {
    let temp_dir =
        std::env::temp_dir().join(format!("netsim_busy_init_lock_{}", rand::random::<u32>()));
    std::fs::create_dir_all(&temp_dir).expect("Failed to create temp dir");

    let wedged_listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let dead_port = wedged_listener.local_addr().unwrap().port();

    let stale_ini = daemon::IniFile::new_for_dir(temp_dir.clone(), 1).unwrap();
    let Ok(daemon::IniFileAccess::Writer(stale_writer)) = stale_ini.try_acquire() else {
        panic!("Wedged owner should be Writer");
    };
    let mut stale_data = HashMap::new();
    stale_data.insert("pid".to_string(), std::process::id().to_string());
    stale_data.insert("grpc.port".to_string(), dead_port.to_string());
    let stale_guard = stale_writer.write(&stale_data).unwrap();

    // The probe connecting to the dead port is the first moment that is
    // provably after `try_acquire` released `netsim.ini.init.lock` and at
    // least 400 ms before `evict_stale_lock` runs. Hold the init lock from
    // then until well past the 2 x 400 ms probe deadline so the first
    // eviction attempt finds it busy, then release it so the retry succeeds.
    let init_lock_path = temp_dir.join("netsim.ini.init.lock");
    let init_lock_holder = tokio::task::spawn_blocking(move || {
        wedged_listener.set_nonblocking(true).unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        let probe_stream = loop {
            match wedged_listener.accept() {
                Ok((stream, _)) => break Some(stream),
                Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                    if std::time::Instant::now() >= deadline {
                        break None;
                    }
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(err) => panic!("accept on the dead port failed: {err}"),
            }
        };
        let probe_connected = probe_stream.is_some();
        if probe_connected {
            let init_lock = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .open(&init_lock_path)
                .unwrap();
            init_lock.lock().unwrap();
            std::thread::sleep(Duration::from_secs(2));
            // Unlock explicitly so the daemon's retry never observes a lazily
            // released LockFileEx lock on Windows.
            init_lock.unlock().unwrap();
            drop(init_lock);
        }
        // Keep the accepted probe connection and the dead port open (still
        // silent) until the test ends.
        (probe_connected, probe_stream, wedged_listener)
    });

    let mut args = daemon::Args::default();
    args.logtostderr = true;
    args.no_shutdown = true;

    // `new_with_args_in_dir` panics unless the daemon starts as Owner.
    let mut world = World::new_with_args_in_dir(temp_dir, args).await;
    world.when_spawn_daemon().await;

    let (probe_connected, _probe_stream, _wedged_listener) = init_lock_holder.await.unwrap();
    assert!(probe_connected, "the dead port never saw the liveness probe connect");
    assert_ne!(world.grpc_port, dead_port);
    assert!(!stale_guard.still_owns_lock());
    world.then_discovery_file_advertises_port(world.grpc_port);
    drop(stale_guard);
}
