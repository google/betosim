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

use std::{path::PathBuf, time::Duration};

use daemon::{Args, NetsimDaemon, StartUpMode};
use device_actor::DeviceClient;
use netsim_model::{Chip, ChipKind, ChipVariant, DeviceAddChip, DeviceConfig, DeviceId, Pose};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

/// A lightweight, standalone World implementation for testing the gated legacy
/// Rootcanal server. Reuses NetsimDaemon and DeviceClient in-process without
/// gRPC dependencies.
struct RootcanalWorld {
    daemon: Option<NetsimDaemon>,
    daemon_task: Option<tokio::task::JoinHandle<()>>,
    temp_dir: PathBuf,
    test_port: u16,
    device_client: DeviceClient,
}

impl Drop for RootcanalWorld {
    fn drop(&mut self) {
        if let Some(task) = self.daemon_task.take() {
            task.abort();
        }
        let _ = std::fs::remove_dir_all(&self.temp_dir);
    }
}

impl RootcanalWorld {
    /// Creates a new RootcanalWorld with a daemon configured on dynamic OS
    /// ports.
    async fn new() -> Self {
        let args = Args {
            logtostderr: true,
            no_shutdown: true,
            // Set hci_port to 0 to bind to dynamic, random OS port
            hci_port: Some(0),
            ..Default::default()
        };

        let temp_dir =
            std::env::temp_dir().join(format!("netsim_rootcanal_test_{}", rand::random::<u32>()));
        std::fs::create_dir_all(&temp_dir).expect("Failed to create temp dir");

        let startup_mode = NetsimDaemon::new_with_dirs(temp_dir.clone(), args)
            .await
            .expect("Failed to create daemon");

        let daemon = match startup_mode {
            StartUpMode::Owner(daemon) => daemon,
            _ => panic!("Expected daemon to start as Owner"),
        };

        // Bind our own local TcpListener for the Rootcanal server
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("Failed to bind local Rootcanal listener");
        let test_port = listener.local_addr().expect("Failed to get local address").port();
        let device_client = daemon.device_client();

        // Spawn the Rootcanal server in-process, passing the listener and device_client
        tokio::spawn(rootcanal_server::run(listener, device_client.clone()));

        RootcanalWorld {
            daemon: Some(daemon),
            daemon_task: None,
            temp_dir,
            test_port,
            device_client,
        }
    }

    /// Spawns the daemon background event loop task
    async fn when_spawn_daemon(&mut self) {
        if self.daemon_task.is_some() {
            panic!("Daemon already spawned");
        }
        if let Some(daemon) = self.daemon.take() {
            let task = tokio::spawn(async move {
                let _ = daemon.run_daemon().await;
            });
            self.daemon_task = Some(task);
            // Allow time to yield and let the listener bind
            tokio::time::sleep(Duration::from_millis(50)).await;
        } else {
            panic!("Daemon has already been consumed or not initialized");
        }
    }

    /// Connects a local test client to the legacy Rootcanal TCP port
    fn when_connect_client(&self) -> impl std::future::Future<Output = RootcanalTestClient> + Send {
        let port = self.test_port;
        async move {
            let stream = TcpStream::connect(("127.0.0.1", port))
                .await
                .expect("Failed to connect to Rootcanal legacy control TCP port");
            RootcanalTestClient { stream }
        }
    }

    /// Directly provisions a virtual device with a Bluetooth chip via
    /// DeviceClient (bypassing gRPC)
    fn when_create_device_with_bluetooth(
        &self,
        device_guid: &str,
        chip_address: &str,
    ) -> impl std::future::Future<Output = u32> + Send {
        let client = self.device_client.clone();
        let device_guid = device_guid.to_string();
        let chip_address = chip_address.to_string();
        async move {
            let mut device_config =
                DeviceConfig::new(device_guid.clone(), true, Pose::default(), false);
            device_config.device_info = Some(netsim_model::DeviceInfo {
                name: "RootcanalTestDevice".to_string(),
                ..Default::default()
            });

            let add_chip_params = DeviceAddChip {
                device_guid: device_guid.clone(),
                packet_stream: None,
                packet_sink: None,
                device_config,
                chip: Chip {
                    name: format!("chip_{}", device_guid),
                    manufacturer: "Netsim".to_string(),
                    product_name: "RootcanalBeacon".to_string(),
                    kind: ChipKind::BLUETOOTH,
                    variant: Some(ChipVariant::Bluetooth(Box::new(netsim_model::Bluetooth {
                        address: chip_address.to_string(),
                        mode: netsim_model::BluetoothMode::Device(Default::default()),
                        bt_properties: Default::default(),
                        ..Default::default()
                    }))),
                    enabled: true, // Initial state
                    ..Default::default()
                },
            };

            let device_id =
                client.add_chip(add_chip_params).await.expect("Failed to add test chip");
            device_id.0
        }
    }

    /// Asserts whether the Bluetooth chip(s) on a virtual device are
    /// enabled/disabled
    fn then_bluetooth_le_enabled_is(
        &self,
        device_id: u32,
        expected: bool,
    ) -> impl std::future::Future<Output = ()> + Send {
        let client = self.device_client.clone();
        async move {
            let device = client.get(DeviceId(device_id)).await.unwrap().expect("Device missing");

            let bt_chips: Vec<_> =
                device.chips.iter().filter(|c| c.kind == ChipKind::BLUETOOTH).collect();

            assert!(!bt_chips.is_empty(), "No bluetooth chips found on device {}", device_id);
            for chip in bt_chips {
                assert_eq!(
                    chip.is_le_enabled(),
                    expected,
                    "Bluetooth chip {} LE enabled state mismatch: expected {}, got {}",
                    chip.id,
                    expected,
                    chip.is_le_enabled()
                );
            }
        }
    }

    fn then_bluetooth_classic_enabled_is(
        &self,
        device_id: u32,
        expected: bool,
    ) -> impl std::future::Future<Output = ()> + Send {
        let client = self.device_client.clone();
        async move {
            let device = client.get(DeviceId(device_id)).await.unwrap().expect("Device missing");

            let bt_chips: Vec<_> =
                device.chips.iter().filter(|c| c.kind == ChipKind::BLUETOOTH).collect();

            assert!(!bt_chips.is_empty(), "No bluetooth chips found on device {}", device_id);
            for chip in bt_chips {
                assert_eq!(
                    chip.is_classic_enabled(),
                    expected,
                    "Bluetooth chip {} Classic enabled state mismatch: expected {}, got {}",
                    chip.id,
                    expected,
                    chip.is_classic_enabled()
                );
            }
        }
    }

    /// Asserts whether the Bluetooth chip's preset matches the expected value
    async fn then_bluetooth_preset_is(&mut self, device_id: u32, expected: Option<&str>) {
        let device =
            self.device_client.get(DeviceId(device_id)).await.unwrap().expect("Device missing");

        let bt_chips: Vec<_> =
            device.chips.iter().filter(|c| c.kind == ChipKind::BLUETOOTH).collect();

        assert!(!bt_chips.is_empty(), "No bluetooth chips found on device {}", device_id);
        for chip in bt_chips {
            if let Some(ChipVariant::Bluetooth(bluetooth)) = &chip.variant {
                assert_eq!(
                    bluetooth.preset.as_deref(),
                    expected,
                    "Bluetooth chip {} preset mismatch: expected {:?}, got {:?}",
                    chip.id,
                    expected,
                    bluetooth.preset.as_deref()
                );
            }
        }
    }

    /// Asserts that a device is fully deleted from the simulation
    fn then_device_is_deleted(&self, id: u32) -> impl std::future::Future<Output = ()> + Send {
        let client = self.device_client.clone();
        async move {
            let device = client.get(DeviceId(id)).await.unwrap();
            assert!(device.is_none(), "Expected device {} to be fully deleted", id);
        }
    }

    /// Asserts whether the entire Bluetooth chip is enabled/disabled (not just
    /// individual sub-radios)
    fn then_bluetooth_chip_enabled_is(
        &self,
        device_id: u32,
        expected: bool,
    ) -> impl std::future::Future<Output = ()> + Send {
        let client = self.device_client.clone();
        async move {
            let device = client.get(DeviceId(device_id)).await.unwrap().expect("Device missing");
            let chip = device.chips.iter().find(|c| c.kind == ChipKind::BLUETOOTH).unwrap();
            assert_eq!(
                chip.enabled, expected,
                "Bluetooth chip enabled state mismatch: expected {}, got {}",
                expected, chip.enabled
            );
        }
    }
}

/// A simple TCP client that implements the packed binary Rootcanal test channel
/// protocol.
struct RootcanalTestClient {
    stream: TcpStream,
}

impl RootcanalTestClient {
    /// Reads a response from the server (4-byte LE size + UTF-8 string)
    async fn read_response(&mut self) -> String {
        let mut size_buf = [0u8; 4];
        self.stream.read_exact(&mut size_buf).await.unwrap();
        let size = u32::from_le_bytes(size_buf) as usize;

        let mut content_buf = vec![0u8; size];
        self.stream.read_exact(&mut content_buf).await.unwrap();
        String::from_utf8(content_buf).unwrap()
    }

    /// Sends a legacy Rootcanal control command using packed binary
    /// serialization
    async fn send_command(&mut self, name: &str, args: &[&str]) -> String {
        let mut payload = Vec::new();

        payload.push(name.len() as u8);
        payload.extend_from_slice(name.as_bytes());

        payload.push(args.len() as u8);

        for arg in args {
            payload.push(arg.len() as u8);
            payload.extend_from_slice(arg.as_bytes());
        }

        self.stream.write_all(&payload).await.unwrap();
        self.stream.flush().await.unwrap();

        if name != "CLOSE_TEST_CHANNEL" { self.read_response().await } else { "".to_string() }
    }

    /// Asserts that the server welcome message was received correctly
    async fn then_welcome_received(&mut self) {
        let welcome = self.read_response().await;
        assert_eq!(welcome, "RootCanal\n");
    }

    /// Sends 'add' command to provision a new device and returns its ID
    async fn when_add_device(&mut self, device_type: &str, address: Option<&str>) -> u32 {
        let args = match address {
            Some(addr) => vec![device_type, addr],
            None => vec![device_type],
        };
        let resp = self.send_command("add", &args).await;
        assert!(resp.contains(":hci_device_"), "Unexpected add response: {}", resp);
        let id_str = resp.split(':').next().unwrap();
        id_str.parse::<u32>().expect("Failed to parse device ID")
    }

    /// Sends 'del' command to remove a device by ID
    async fn when_delete_device(&mut self, id: u32) {
        let resp = self.send_command("del", &[&id.to_string()]).await;
        assert_eq!(resp, format!("TestCommandHandler 'del' called with device at index {id}"));
    }

    /// Sends 'reset' command to clear all devices
    async fn when_reset(&mut self) {
        let resp = self.send_command("reset", &[]).await;
        assert_eq!(resp, "OK");
    }

    /// Toggles a device's PHY state (add/del device to/from phy)
    async fn when_toggle_phy(&mut self, device_id: u32, phy_index: &str, enable: bool) {
        let cmd = if enable { "add_device_to_phy" } else { "del_device_from_phy" };
        let resp = self.send_command(cmd, &[&device_id.to_string(), phy_index]).await;
        assert_eq!(
            resp,
            format!(
                "TestCommandHandler '{}' called with device {} and phy {}",
                cmd, device_id, phy_index
            )
        );
    }
}

// ======================================================================
// TESTS
// ======================================================================

/// Scenario 1: TCP Connection Handshake
///   Given a running Netsim Daemon
///   When a TCP client connects to the Rootcanal control port
///   Then it immediately receives the welcome handshake string "RootCanal\n"
#[tokio::test]
async fn test_rootcanal_handshake() {
    let mut world = RootcanalWorld::new().await;
    world.when_spawn_daemon().await;

    let mut client = world.when_connect_client().await;
    client.then_welcome_received().await;
}

/// Scenario 2: Listing Virtual Devices and Phys
///   Given a running Netsim Daemon
///   And a virtual device with a Bluetooth chip is created
///   When the TCP client connects and sends the 'list' command
///   Then the response contains the device listed under Devices: and Phys:
/// LOW_ENERGY and BR_EDR categories
#[tokio::test]
async fn test_rootcanal_list_devices() {
    let mut world = RootcanalWorld::new().await;
    let device_id = world.when_create_device_with_bluetooth("guid_1", "11:22:33:44:55:66").await;

    world.when_spawn_daemon().await;

    let mut client = world.when_connect_client().await;
    client.then_welcome_received().await;

    let list_output = client.send_command("list", &[]).await;

    let expected_device_row = format!("{}:hci_device_{}", device_id, device_id);
    assert!(list_output.contains("Devices:"), "Response missing Devices: category");
    assert!(
        list_output.contains(&expected_device_row),
        "Response missing expected device entry: {}",
        expected_device_row
    );
    assert!(list_output.contains("Phys:"), "Response missing Phys: category");

    let le_line = list_output
        .lines()
        .find(|line| line.trim().starts_with("0:LOW_ENERGY:"))
        .expect("Missing LOW_ENERGY physical category");
    let le_devices = le_line.split(':').nth(2).unwrap().trim();
    let le_device_ids: Vec<&str> = le_devices.split(',').map(|s| s.trim()).collect();
    assert!(
        le_device_ids.contains(&device_id.to_string().as_str()),
        "Device {} not found in LOW_ENERGY devices: {:?}",
        device_id,
        le_device_ids
    );

    let classic_line = list_output
        .lines()
        .find(|line| line.trim().starts_with("1:BR_EDR:"))
        .expect("Missing BR_EDR physical category");
    let classic_devices = classic_line.split(':').nth(2).unwrap().trim();
    let classic_device_ids: Vec<&str> = classic_devices.split(',').map(|s| s.trim()).collect();
    assert!(
        classic_device_ids.contains(&device_id.to_string().as_str()),
        "Device {} not found in BR_EDR devices: {:?}",
        device_id,
        classic_device_ids
    );
}

/// Scenario 3: Disconnecting and Reconnecting Device via Phys
///   Given a running Netsim Daemon
///   And a virtual device with a Bluetooth chip is created
///   When the TCP client sends 'del_device_from_phy' for the device
///   Then the Bluetooth chip on the device becomes disabled
///   When the TCP client sends 'add_device_to_phy' for the device
///   Then the Bluetooth chip on the device becomes enabled again
#[tokio::test]
async fn test_rootcanal_phy_toggle() {
    let mut world = RootcanalWorld::new().await;
    let device_id = world.when_create_device_with_bluetooth("guid_2", "11:22:33:44:55:77").await;

    world.when_spawn_daemon().await;

    let mut client = world.when_connect_client().await;
    client.then_welcome_received().await;

    world.then_bluetooth_le_enabled_is(device_id, true).await;
    world.then_bluetooth_classic_enabled_is(device_id, true).await;

    // 1. Disconnect from Low Energy Phy
    client.when_toggle_phy(device_id, "0", false).await;
    world.then_bluetooth_le_enabled_is(device_id, false).await;
    world.then_bluetooth_classic_enabled_is(device_id, true).await;

    // 2. Disconnect from Classic Phy
    client.when_toggle_phy(device_id, "1", false).await;
    world.then_bluetooth_le_enabled_is(device_id, false).await;
    world.then_bluetooth_classic_enabled_is(device_id, false).await;

    // 3. Connect back to Low Energy Phy
    client.when_toggle_phy(device_id, "0", true).await;
    world.then_bluetooth_le_enabled_is(device_id, true).await;
    world.then_bluetooth_classic_enabled_is(device_id, false).await;
}

/// Scenario 4: Device Lifecycle (add, del, reset)
///   Given a running Netsim Daemon
///   When the TCP client sends 'add' to create a device
///   Then the device is successfully added to the simulation
///   When the TCP client sends 'del' to remove the device
///   Then the device is successfully removed
///   When the TCP client sends 'reset'
///   Then all devices are cleared from the simulation
#[tokio::test]
async fn test_rootcanal_device_lifecycle() {
    let mut world = RootcanalWorld::new().await;
    world.when_spawn_daemon().await;

    let mut client = world.when_connect_client().await;
    client.then_welcome_received().await;

    // 1. Test 'add'
    let device_id = client.when_add_device("device", Some("11:22:33:44:55:88")).await;

    // Verify device exists and is enabled
    world.then_bluetooth_le_enabled_is(device_id, true).await;
    world.then_bluetooth_classic_enabled_is(device_id, true).await;

    // 2. Test 'del'
    client.when_delete_device(device_id).await;

    // Verify device was deleted
    world.then_device_is_deleted(device_id).await;

    // 3. Test 'reset'
    client.when_reset().await;
}

/// Scenario 5: Command Argument Validation
///   Given a running Netsim Daemon
///   When the TCP client sends 'list' with unexpected arguments
///   Then the server returns an arity error message
///   When the TCP client sends 'add' with missing arguments
///   Then the server returns an arity error message
///   When the TCP client sends 'del' with a non-integer ID
///   Then the server returns an invalid ID error message
///   When the TCP client sends 'del' with a non-existent device ID
///   Then the server returns a device not found error message
#[tokio::test]
async fn test_rootcanal_argument_validation() {
    let mut world = RootcanalWorld::new().await;
    world.when_spawn_daemon().await;

    let mut client = world.when_connect_client().await;
    client.then_welcome_received().await;

    // 1. Arity check for 'list' (expects 0, send 1)
    let resp = client.send_command("list", &["extra_arg"]).await;
    assert_eq!(resp, "TestCommandHandler 'list' takes no arguments");

    // 2. Arity check for 'add' (expects at least 1, send 0)
    let resp = client.send_command("add", &[]).await;
    assert_eq!(resp, "TestCommandHandler 'add' takes an argument");

    // 3. Arity check for 'del' (expects 1, send 0)
    let resp = client.send_command("del", &[]).await;
    assert_eq!(resp, "TestCommandHandler 'del' takes an argument");

    // 4. Type check for 'del' (expects integer, send string)
    let resp = client.send_command("del", &["abc"]).await;
    assert_eq!(resp, "Invalid device_id");

    // 5. Value check for 'del' (device not found)
    let resp = client.send_command("del", &["999"]).await;
    assert_eq!(
        resp,
        "Delete failed: Framework error: Service error: Device not found (explicit): 999"
    );
}

/// Scenario 6: Unknown Commands
///   Given a running Netsim Daemon
///   When the TCP client sends an unknown command
///   Then the server returns "OK" (fallback shim)
#[tokio::test]
async fn test_rootcanal_unknown_commands() {
    let mut world = RootcanalWorld::new().await;
    world.when_spawn_daemon().await;

    let mut client = world.when_connect_client().await;
    client.then_welcome_received().await;

    let resp = client.send_command("completely_unknown_command", &["arg1", "arg2"]).await;
    assert_eq!(resp, "OK");
}

/// Scenario 7: Labeled Session Close
///   Given a running Netsim Daemon
///   When the TCP client sends 'CLOSE_TEST_CHANNEL'
///   Then the server immediately closes the connection without writing any
/// response
#[tokio::test]
async fn test_rootcanal_session_close() {
    let mut world = RootcanalWorld::new().await;
    world.when_spawn_daemon().await;

    let mut client = world.when_connect_client().await;
    client.then_welcome_received().await;

    // Send CLOSE_TEST_CHANNEL (helper returns empty string for CLOSE_TEST_CHANNEL,
    // but we want to verify socket closure)
    let _ = client.send_command("CLOSE_TEST_CHANNEL", &[]).await;

    // Attempt to read from the stream. It should return EOF (0 bytes) immediately.
    let mut buf = [0u8; 1];
    let read_result = client.stream.read(&mut buf).await;
    assert!(read_result.is_ok());
    assert_eq!(read_result.unwrap(), 0, "Expected EOF (connection closed by server)");
}

/// Scenario 8: Abrupt Client Disconnection
///   Given a running Netsim Daemon
///   When a TCP client connects and sends a partial command header
///   And the TCP client abruptly closes the connection
///   Then the server session task terminates gracefully without panicking
#[tokio::test]
async fn test_rootcanal_abrupt_disconnect() {
    let mut world = RootcanalWorld::new().await;
    world.when_spawn_daemon().await;

    // Connect raw TcpStream
    let mut stream = TcpStream::connect(("127.0.0.1", world.test_port)).await.unwrap();

    // Read welcome
    let mut welcome_buf = vec![0u8; 14]; // "RootCanal\n" is 10 bytes, plus 4 bytes for size prefix
    let _ = stream.read_exact(&mut welcome_buf).await.unwrap();

    // Write a partial command: just a 1-byte length prefix (e.g. 5) but no name
    // payload
    stream.write_all(&[5u8]).await.unwrap();
    stream.flush().await.unwrap();

    // Abruptly close the connection by dropping the stream
    std::mem::drop(stream);

    // Yield to let the server process the disconnect and ensure no panic occurs
    tokio::time::sleep(Duration::from_millis(50)).await;

    // If we reach here without the daemon task crashing, the test passes.
    // We can also verify the daemon is still responsive by connecting a new client.
    let mut client = world.when_connect_client().await;
    client.then_welcome_received().await;
}

/// Scenario 9: Command Pipelining
///   Given a running Netsim Daemon
///   When the TCP client sends multiple commands back-to-back in a single write
///   Then the server processes them sequentially and returns both responses in
/// order
#[tokio::test]
async fn test_rootcanal_pipelining() {
    let mut world = RootcanalWorld::new().await;
    world.when_spawn_daemon().await;

    let mut client = world.when_connect_client().await;
    client.then_welcome_received().await;

    // Construct two 'list' commands in a single buffer
    // Command 1: name="list" (len=4), args=[] (len=0)
    // Command 2: name="list" (len=4), args=[] (len=0)
    let mut pipeline = Vec::new();

    // Cmd 1
    pipeline.push(4u8);
    pipeline.extend_from_slice(b"list");
    pipeline.push(0u8);

    // Cmd 2
    pipeline.push(4u8);
    pipeline.extend_from_slice(b"list");
    pipeline.push(0u8);

    // Write both at once
    client.stream.write_all(&pipeline).await.unwrap();
    client.stream.flush().await.unwrap();

    // Read both responses
    let resp1 = client.read_response().await;
    let resp2 = client.read_response().await;

    assert!(resp1.contains("Devices:"));
    assert!(resp2.contains("Devices:"));
}

/// Scenario 10: Concurrent Clients
///   Given a running Netsim Daemon
///   When multiple TCP clients connect concurrently
///   And all clients send commands simultaneously
///   Then the server handles them in parallel and returns valid responses to
/// all
#[tokio::test]
async fn test_rootcanal_concurrency() {
    let mut world = RootcanalWorld::new().await;
    world.when_spawn_daemon().await;

    let mut clients = Vec::new();
    for _ in 0..3 {
        let mut client = world.when_connect_client().await;
        client.then_welcome_received().await;
        clients.push(client);
    }

    // Spawn tasks to send commands concurrently
    let mut tasks = Vec::new();
    for (i, mut client) in clients.into_iter().enumerate() {
        let task = tokio::spawn(async move {
            let resp = client.send_command("list", &[]).await;
            (i, resp)
        });
        tasks.push(task);
    }

    // Verify all tasks completed successfully and got valid responses
    for task in tasks {
        let (i, resp) = task.await.unwrap();
        assert!(resp.contains("Devices:"), "Client {} got invalid response: {}", i, resp);
    }
}

/// Scenario 11: Verify PhyTarget::All and Chip Toggling via Fallback PHY Index
///   Given a running Netsim Daemon
///   And a virtual device with a Bluetooth chip is created
///   When the TCP client sends 'del_device_from_phy' with an unknown PHY index
/// "2" (maps to PhyTarget::All)   Then the entire Bluetooth chip is disabled
/// (enabled = false)   When the TCP client sends 'add_device_to_phy' with PHY
/// index "2"   Then the entire Bluetooth chip is re-enabled (enabled = true)
#[tokio::test]
async fn test_rootcanal_phy_all_toggle() {
    let mut world = RootcanalWorld::new().await;
    let device_id =
        world.when_create_device_with_bluetooth("guid_phy_all", "11:22:33:44:55:aa").await;
    world.when_spawn_daemon().await;

    let mut client = world.when_connect_client().await;
    client.then_welcome_received().await;

    // 1. Disable the entire chip using PHY target "2" (PhyTarget::All)
    client.when_toggle_phy(device_id, "2", false).await;
    world.then_bluetooth_chip_enabled_is(device_id, false).await;

    // 2. Enable the entire chip using PHY target "2"
    client.when_toggle_phy(device_id, "2", true).await;
    world.then_bluetooth_chip_enabled_is(device_id, true).await;
}

/// Scenario 12: Add Device Without Specifying MAC Address
///   Given a running Netsim Daemon
///   When the TCP client sends 'add' with only the device type argument
///   Then the server dynamically provisions a new device with a random GUID and
/// empty address   And the device is successfully added to the simulation
#[tokio::test]
async fn test_rootcanal_add_device_no_address() {
    let mut world = RootcanalWorld::new().await;
    world.when_spawn_daemon().await;

    let mut client = world.when_connect_client().await;
    client.then_welcome_received().await;

    // Call 'add' with only 1 argument (device_type)
    let device_id = client.when_add_device("device", None).await;

    // Verify device exists and is enabled by default
    world.then_bluetooth_le_enabled_is(device_id, true).await;
    world.then_bluetooth_classic_enabled_is(device_id, true).await;
}

/// Scenario 13: Set Device Configuration
///   Given a running Netsim Daemon
///   And a virtual device with a Bluetooth chip is created
///   When the TCP client sends 'set_device_configuration' with the device ID
/// and preset   Then the server successfully updates the device configuration
/// and returns a success message
#[tokio::test]
async fn test_rootcanal_set_device_configuration() {
    let mut world = RootcanalWorld::new().await;
    world.when_spawn_daemon().await;

    let mut client = world.when_connect_client().await;
    client.then_welcome_received().await;

    let device_id = client.when_add_device("device", Some("11:22:33:44:55:aa")).await;
    world.then_bluetooth_preset_is(device_id, None).await;

    let resp = client
        .send_command("set_device_configuration", &[&device_id.to_string(), "csr_rck_pts_dongle"])
        .await;
    assert_eq!(resp, format!("set_device_configuration {} csr_rck_pts_dongle", device_id));

    world.then_bluetooth_preset_is(device_id, Some("csr_rck_pts_dongle")).await;
}

/// Scenario 14: Set Device Configuration with Invalid Preset
///   Given a running Netsim Daemon
///   And a virtual device with a Bluetooth chip is created
///   When the TCP client sends 'set_device_configuration' with an invalid
/// preset   Then the server returns an error message
///   And the device's configuration preset remains None in the registry
#[tokio::test]
async fn test_rootcanal_set_device_configuration_invalid_preset() {
    let mut world = RootcanalWorld::new().await;
    world.when_spawn_daemon().await;

    let mut client = world.when_connect_client().await;
    client.then_welcome_received().await;

    let device_id = client.when_add_device("device", Some("11:22:33:44:55:aa")).await;
    world.then_bluetooth_preset_is(device_id, None).await;

    let resp = client
        .send_command("set_device_configuration", &[&device_id.to_string(), "invalid_preset_name"])
        .await;

    assert!(resp.starts_with("Failed to update device configuration"));

    world.then_bluetooth_preset_is(device_id, None).await;
}
