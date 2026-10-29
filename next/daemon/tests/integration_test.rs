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

fn bind_wedged_loopback_listeners() -> (u16, Vec<std::net::TcpListener>) {
    for _ in 0..32 {
        let v4 = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = v4.local_addr().unwrap().port();
        match std::net::TcpListener::bind((std::net::Ipv6Addr::LOCALHOST, port)) {
            Ok(v6) => return (port, vec![v4, v6]),
            Err(err) if err.kind() == std::io::ErrorKind::AddrInUse => continue,
            Err(err) if err.kind() == std::io::ErrorKind::AddrNotAvailable => {
                return (port, vec![v4]);
            }
            Err(err) => panic!("failed to bind IPv6 loopback listener on [::1]:{port}: {err}"),
        }
    }
    panic!("failed to bind matching IPv4 and IPv6 loopback listeners");
}

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

    let (dead_port, _wedged_listeners) = bind_wedged_loopback_listeners();

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

// Scenario: Start daemon when shared discovery directory advertises a port from
// another container's isolated network namespace
//   Given netsim.ini in a shared /tmp or TMPDIR advertises a gRPC port that is
//     not listening in the current network namespace
//   And the other container's netsimd still holds netsim.ini.lock
//   When I start netsimd in the same discovery directory
//   Then the 100 ms TCP liveness probe rejects the unreachable port
//   And netsimd evicts the cross-namespace lock and starts as Owner on a reachable port
#[tokio::test]
async fn test_startup_evicts_cross_namespace_unreachable_port_and_starts_daemon() {
    let temp_dir =
        std::env::temp_dir().join(format!("netsim_cross_ns_lock_{}", rand::random::<u32>()));
    std::fs::create_dir_all(&temp_dir).expect("Failed to create temp dir");

    let ephemeral_listener = std::net::TcpListener::bind("localhost:0").unwrap();
    let unreachable_port = ephemeral_listener.local_addr().unwrap().port();
    drop(ephemeral_listener);

    let foreign_ini = daemon::IniFile::new_for_dir(temp_dir.clone(), 1).unwrap();
    let Ok(daemon::IniFileAccess::Writer(foreign_writer)) = foreign_ini.try_acquire() else {
        panic!("Foreign container owner should be Writer");
    };
    let mut foreign_data = HashMap::new();
    foreign_data.insert("pid".to_string(), std::process::id().to_string());
    foreign_data.insert("grpc.port".to_string(), unreachable_port.to_string());
    let foreign_guard = foreign_writer.write(&foreign_data).unwrap();

    let mut args = daemon::Args::default();
    args.logtostderr = true;
    args.no_shutdown = true;

    let mut world = World::new_with_args_in_dir(temp_dir, args).await;
    world.when_spawn_daemon().await;

    assert!(world.grpc_port != 0);
    assert!(!foreign_guard.still_owns_lock());
    world.then_discovery_file_advertises_port(world.grpc_port);
    assert!(common::util::ini_file::is_server_reachable(&format!("localhost:{}", world.grpc_port)));

    drop(foreign_guard);
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

    let (dead_port, wedged_listeners) = bind_wedged_loopback_listeners();

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
    // then until both 400 ms HTTP/2 liveness probe attempts finish (and at
    // least 900 ms have elapsed) plus a 150 ms grace window so the first
    // eviction attempt deterministically finds it busy, then release it so the
    // retry succeeds.
    let init_lock_path = temp_dir.join("netsim.ini.init.lock");
    let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel::<()>(0);
    let init_lock_holder = tokio::task::spawn_blocking(move || {
        for listener in &wedged_listeners {
            listener.set_nonblocking(true).unwrap();
        }
        let init_lock = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&init_lock_path)
            .unwrap();
        ready_tx.send(()).unwrap();

        let mut deadline = std::time::Instant::now() + Duration::from_secs(5);
        let mut locked_at: Option<std::time::Instant> = None;
        let mut accepted_streams: Vec<(std::net::TcpStream, Option<std::time::Instant>, bool)> =
            Vec::new();
        let mut buf = [0u8; 256];

        let completed_http2_probes = loop {
            for listener in &wedged_listeners {
                loop {
                    match listener.accept() {
                        Ok((stream, _)) => {
                            if locked_at.is_none() {
                                init_lock.lock().unwrap();
                                let now = std::time::Instant::now();
                                locked_at = Some(now);
                                deadline = now + Duration::from_secs(5);
                            }
                            stream.set_nonblocking(true).unwrap();
                            accepted_streams.push((stream, None, false));
                        }
                        Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => break,
                        Err(err) => panic!("accept on the dead port failed: {err}"),
                    }
                }
            }

            for (stream, preface_at, closed) in &mut accepted_streams {
                if *closed {
                    continue;
                }
                loop {
                    match std::io::Read::read(stream, &mut buf) {
                        Ok(n) if n > 0 => {
                            if preface_at.is_none() {
                                *preface_at = Some(std::time::Instant::now());
                            }
                        }
                        Ok(_) => {
                            *closed = true;
                            break;
                        }
                        Err(err)
                            if err.kind() == std::io::ErrorKind::WouldBlock
                                || err.kind() == std::io::ErrorKind::Interrupted =>
                        {
                            break;
                        }
                        Err(_) => {
                            *closed = true;
                            break;
                        }
                    }
                }
            }

            // gRPC C-core keeps in-flight HTTP/2 handshake sockets open past a
            // short `wait_for_connected` deadline, so a 400 ms probe attempt is
            // complete once its HTTP/2 client preface has been observed for at
            // least 450 ms (or the peer closed the stream).
            let completed_http2_probes = accepted_streams
                .iter()
                .filter(|(_, preface_at, closed)| {
                    preface_at.is_some_and(|t| *closed || t.elapsed() >= Duration::from_millis(450))
                })
                .count();
            if let Some(locked_instant) = locked_at {
                if completed_http2_probes >= 2
                    && locked_instant.elapsed() >= Duration::from_millis(900)
                {
                    break completed_http2_probes;
                }
            }

            if std::time::Instant::now() >= deadline {
                break completed_http2_probes;
            }
            std::thread::sleep(Duration::from_millis(10));
        };

        let probe_connected = locked_at.is_some();
        if probe_connected {
            std::thread::sleep(Duration::from_millis(150));
            // Unlock explicitly so the daemon's retry never observes a lazily
            // released LockFileEx lock on Windows.
            init_lock.unlock().unwrap();
            drop(init_lock);
        }
        // Keep the accepted probe connections and the dead port open (still
        // silent) until the test ends.
        (probe_connected, completed_http2_probes, accepted_streams, wedged_listeners)
    });
    ready_rx.recv().unwrap();

    let mut args = daemon::Args::default();
    args.logtostderr = true;
    args.no_shutdown = true;

    // `new_with_args_in_dir` panics unless the daemon starts as Owner.
    let mut world = World::new_with_args_in_dir(temp_dir, args).await;
    world.when_spawn_daemon().await;

    let (probe_connected, completed_http2_probes, _accepted_streams, _wedged_listeners) =
        init_lock_holder.await.unwrap();
    assert!(probe_connected, "the dead port never saw the liveness probe connect");
    assert!(
        completed_http2_probes >= 2,
        "expected 2 completed HTTP/2 liveness probes before releasing init lock, got {completed_http2_probes}"
    );
    assert_ne!(world.grpc_port, dead_port);
    assert!(!stale_guard.still_owns_lock());
    world.then_discovery_file_advertises_port(world.grpc_port);
    drop(stale_guard);
}

// Scenario: WebSocket with Laird BL654 preset applies buffer size (b/573454032, b/560823435)
//   Given a running Netsim Daemon with WebSocket enabled and a connected BT WebSocket client
//   When an initial Text JSON chip-config frame sets preset "laird_bl654" and HCI LE Read Buffer Size is queried
//   Then the controller reports LAIRD_BL654 buffer size (512 bytes / 0x0200, 4 packets / 0x04)
#[tokio::test]
async fn test_websocket_bt_properties_laird_bl654_preset_buffer_size() {
    let mut world = World::new_with_websocket().await;
    world.when_spawn_daemon().await;
    world.when_connect_websocket("/v1/websocket/bt?name=ws-laird").await;
    world.when_send_websocket_text(r#"{"preset":"laird_bl654"}"#).await;

    world.when_send_websocket_binary(&[0x01, 0x03, 0x0c, 0x00]).await;
    world.then_websocket_receives_binary(&[0x04, 0x0e, 0x04, 0x01, 0x03, 0x0c, 0x00]).await;

    // LAIRD_BL654: 512 bytes (0x0200) and 4 packets (0x04)
    world.when_send_websocket_binary(&[0x01, 0x02, 0x20, 0x00]).await;
    world
        .then_websocket_receives_binary(&[
            0x04, 0x0e, 0x07, 0x01, 0x02, 0x20, 0x00, 0x00, 0x02, 0x04,
        ])
        .await;
}

// Scenario: WebSocket with default binary-only streaming config buffer size
//   Given a running Netsim Daemon with WebSocket enabled and a connected BT WebSocket client
//   When binary HCI commands are streamed directly without an initial Text config frame
//   Then the controller reports DEFAULT buffer size (27 bytes / 0x001b, 20 packets / 0x14)
#[tokio::test]
async fn test_websocket_bt_properties_binary_only_default_buffer_size() {
    let mut world = World::new_with_websocket().await;
    world.when_spawn_daemon().await;
    world.when_connect_websocket("/v1/websocket/bt?name=ws-default").await;

    world.when_send_websocket_binary(&[0x01, 0x03, 0x0c, 0x00]).await;
    world.then_websocket_receives_binary(&[0x04, 0x0e, 0x04, 0x01, 0x03, 0x0c, 0x00]).await;

    // DEFAULT: 27 bytes (0x001b) and 20 packets (0x14)
    world.when_send_websocket_binary(&[0x01, 0x02, 0x20, 0x00]).await;
    world
        .then_websocket_receives_binary(&[
            0x04, 0x0e, 0x07, 0x01, 0x02, 0x20, 0x00, 0x1b, 0x00, 0x14,
        ])
        .await;
}

// Scenario: WebSocket disabling CIS feature clears feature bits
//   Given a running Netsim Daemon with WebSocket enabled and a connected BT WebSocket client
//   When an initial Text JSON chip-config frame disables le_connected_isochronous_stream
//   Then HCI LE Read Local Supported Features reports the CIS feature bits cleared
#[tokio::test]
async fn test_websocket_bt_properties_disable_cis_feature() {
    let mut world = World::new_with_websocket().await;
    world.when_spawn_daemon().await;
    world.when_connect_websocket("/v1/websocket/bt?name=ws-cis").await;
    world
        .when_send_websocket_text(r#"{"features":{"le_connected_isochronous_stream":false}}"#)
        .await;

    world.when_send_websocket_binary(&[0x01, 0x03, 0x0c, 0x00]).await;
    world.then_websocket_receives_binary(&[0x04, 0x0e, 0x04, 0x01, 0x03, 0x0c, 0x00]).await;

    world.when_send_websocket_binary(&[0x01, 0x03, 0x20, 0x00]).await;
    world.then_websocket_receives_le_features_with_cis_cleared().await;
}

// Scenario: HCI and Rootcanal control servers fall back to ephemeral ports and
// record them in netsim.ini when default ports are already in use
//   Given the default hci_port (6402 + instance - 1) and test_port (7500 + instance - 1) are occupied
//   When netsimd starts without explicit --hci-port or --test-port overrides
//   Then netsimd falls back to OS-assigned ephemeral ports and writes hci.port (and test.port) to netsim.ini
#[tokio::test]
async fn test_daemon_hci_and_test_port_ephemeral_fallback_recorded_in_ini() {
    let instance = 200 + (rand::random::<u16>() % 200);
    let default_hci_port = common::util::os_utils::get_hci_port(0, instance - 1) as u16;
    let _occupied_hci = common::util::net::bind_tcp_loopback(default_hci_port).unwrap();
    #[cfg(feature = "cuttlefish")]
    let default_test_port = 7500 + instance - 1;
    #[cfg(feature = "cuttlefish")]
    let _occupied_test = common::util::net::bind_tcp_loopback(default_test_port).unwrap();

    let mut args = daemon::Args::default();
    args.logtostderr = true;
    args.no_shutdown = true;
    args.instance = Some(instance);

    let world = World::new_with_args(args).await;

    world.then_ini_records_ephemeral_hci_and_test_ports(
        instance,
        default_hci_port,
        #[cfg(feature = "cuttlefish")]
        default_test_port,
    );
}
