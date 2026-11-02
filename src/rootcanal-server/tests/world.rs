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

struct RootcanalTestClient {
    stream: TcpStream,
}

impl RootcanalTestClient {
    async fn connect(port: u16) -> Self {
        let stream = TcpStream::connect(("127.0.0.1", port))
            .await
            .expect("Failed to connect to Rootcanal legacy control TCP port");
        Self { stream }
    }

    fn encode_command(name: &str, args: &[&str]) -> Vec<u8> {
        let mut payload = Vec::new();
        payload.push(name.len() as u8);
        payload.extend_from_slice(name.as_bytes());
        payload.push(args.len() as u8);
        for arg in args {
            payload.push(arg.len() as u8);
            payload.extend_from_slice(arg.as_bytes());
        }
        payload
    }

    async fn read_response(&mut self) -> String {
        let mut size_buf = [0u8; 4];
        self.stream.read_exact(&mut size_buf).await.unwrap();
        let size = u32::from_le_bytes(size_buf) as usize;

        let mut content_buf = vec![0u8; size];
        self.stream.read_exact(&mut content_buf).await.unwrap();
        String::from_utf8(content_buf).unwrap()
    }

    async fn write_raw(&mut self, bytes: &[u8]) {
        self.stream.write_all(bytes).await.unwrap();
        self.stream.flush().await.unwrap();
    }

    async fn send_command(&mut self, name: &str, args: &[&str]) -> String {
        let payload = Self::encode_command(name, args);
        self.write_raw(&payload).await;
        self.read_response().await
    }

    async fn expect_welcome(&mut self) {
        let welcome = self.read_response().await;
        assert_eq!(welcome, "RootCanal\n");
    }
}

pub(crate) struct World {
    daemon: Option<NetsimDaemon>,
    daemon_task: Option<tokio::task::JoinHandle<()>>,
    temp_dir: PathBuf,
    test_port: u16,
    device_client: DeviceClient,
    client: Option<RootcanalTestClient>,
    concurrent_clients: Vec<RootcanalTestClient>,
    last_response: String,
    pipelined_responses: Vec<String>,
    concurrent_responses: Vec<String>,
}

impl Drop for World {
    fn drop(&mut self) {
        if let Some(task) = self.daemon_task.take() {
            task.abort();
        }
        let _ = std::fs::remove_dir_all(&self.temp_dir);
    }
}

impl World {
    pub(crate) async fn new() -> Self {
        let args =
            Args { logtostderr: true, no_shutdown: true, hci_port: Some(0), ..Default::default() };

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

        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("Failed to bind local Rootcanal listener");
        let test_port = listener.local_addr().expect("Failed to get local address").port();
        let device_client = daemon.device_client();

        tokio::spawn(rootcanal_server::run(listener, device_client.clone()));

        World {
            daemon: Some(daemon),
            daemon_task: None,
            temp_dir,
            test_port,
            device_client,
            client: None,
            concurrent_clients: Vec::new(),
            last_response: String::new(),
            pipelined_responses: Vec::new(),
            concurrent_responses: Vec::new(),
        }
    }

    async fn bluetooth_chips(&mut self, device_id: u32) -> Vec<Chip> {
        let device =
            self.device_client.get(DeviceId(device_id)).await.unwrap().expect("Device missing");
        let bt_chips: Vec<_> =
            device.chips.into_iter().filter(|c| c.kind == ChipKind::BLUETOOTH).collect();
        assert!(!bt_chips.is_empty(), "No bluetooth chips found on device {}", device_id);
        bt_chips
    }

    pub(crate) async fn given_running_daemon(&mut self) {
        let daemon = self.daemon.take().expect("Daemon already spawned");
        let task = tokio::spawn(async move {
            let _ = daemon.run_daemon().await;
        });
        self.daemon_task = Some(task);
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    pub(crate) async fn given_connected_client(&mut self) {
        self.when_client_connects().await;
        self.then_welcome_received().await;
    }

    pub(crate) async fn given_concurrent_clients(&mut self, count: usize) {
        self.concurrent_clients.clear();
        for _ in 0..count {
            let mut client = RootcanalTestClient::connect(self.test_port).await;
            client.expect_welcome().await;
            self.concurrent_clients.push(client);
        }
    }

    pub(crate) async fn given_device_with_bluetooth(
        &mut self,
        device_guid: &str,
        chip_address: &str,
    ) -> u32 {
        let mut device_config =
            DeviceConfig::new(device_guid.to_string(), true, Pose::default(), false);
        device_config.device_info = Some(netsim_model::DeviceInfo {
            name: "RootcanalTestDevice".to_string(),
            ..Default::default()
        });

        let add_chip_params = DeviceAddChip {
            device_guid: device_guid.to_string(),
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
                enabled: true,
                ..Default::default()
            },
        };

        let device_id =
            self.device_client.add_chip(add_chip_params).await.expect("Failed to add test chip");
        self.then_bluetooth_le_enabled_is(device_id.0, true).await;
        self.then_bluetooth_classic_enabled_is(device_id.0, true).await;
        device_id.0
    }

    pub(crate) async fn given_added_device(
        &mut self,
        device_type: &str,
        address: Option<&str>,
    ) -> u32 {
        let id = self.when_add_device(device_type, address).await;
        self.then_add_device_succeeded(id);
        self.then_bluetooth_preset_is(id, None).await;
        id
    }

    pub(crate) async fn when_client_connects(&mut self) {
        self.client = Some(RootcanalTestClient::connect(self.test_port).await);
    }

    pub(crate) async fn when_send_command(&mut self, name: &str, args: &[&str]) {
        self.last_response =
            self.client.as_mut().expect("Client not connected").send_command(name, args).await;
    }

    pub(crate) async fn when_add_device(
        &mut self,
        device_type: &str,
        address: Option<&str>,
    ) -> u32 {
        let args = match address {
            Some(addr) => vec![device_type, addr],
            None => vec![device_type],
        };
        let resp =
            self.client.as_mut().expect("Client not connected").send_command("add", &args).await;
        let id = resp
            .split(':')
            .next()
            .and_then(|s| s.parse::<u32>().ok())
            .expect("Failed to parse device ID from add response");
        self.last_response = resp;
        id
    }

    pub(crate) async fn when_delete_device(&mut self, id: u32) {
        self.last_response = self
            .client
            .as_mut()
            .expect("Client not connected")
            .send_command("del", &[&id.to_string()])
            .await;
    }

    pub(crate) async fn when_reset(&mut self) {
        self.last_response =
            self.client.as_mut().expect("Client not connected").send_command("reset", &[]).await;
    }

    pub(crate) async fn when_toggle_phy(&mut self, device_id: u32, phy_index: &str, enable: bool) {
        let cmd = if enable { "add_device_to_phy" } else { "del_device_from_phy" };
        self.last_response = self
            .client
            .as_mut()
            .expect("Client not connected")
            .send_command(cmd, &[&device_id.to_string(), phy_index])
            .await;
    }

    pub(crate) async fn when_set_device_configuration(&mut self, device_id: u32, preset: &str) {
        self.last_response = self
            .client
            .as_mut()
            .expect("Client not connected")
            .send_command("set_device_configuration", &[&device_id.to_string(), preset])
            .await;
    }

    pub(crate) async fn when_send_close_test_channel(&mut self) {
        let client = self.client.as_mut().expect("Client not connected");
        let payload = RootcanalTestClient::encode_command("CLOSE_TEST_CHANNEL", &[]);
        client.write_raw(&payload).await;
    }

    pub(crate) async fn when_client_abruptly_disconnects_mid_command(&mut self) {
        let mut client = RootcanalTestClient::connect(self.test_port).await;
        client.expect_welcome().await;
        client.write_raw(&[5u8]).await;
        drop(client);
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    pub(crate) async fn when_send_pipelined_commands(&mut self, commands: &[(&str, &[&str])]) {
        let client = self.client.as_mut().expect("Client not connected");
        let mut pipeline = Vec::new();
        for (name, args) in commands {
            pipeline.extend(RootcanalTestClient::encode_command(name, args));
        }
        client.write_raw(&pipeline).await;

        self.pipelined_responses.clear();
        for _ in 0..commands.len() {
            self.pipelined_responses.push(client.read_response().await);
        }
    }

    pub(crate) async fn when_concurrent_clients_send_command(&mut self, name: &str, args: &[&str]) {
        let clients = std::mem::take(&mut self.concurrent_clients);
        let name = name.to_string();
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();

        let mut tasks = Vec::new();
        for mut client in clients {
            let name = name.clone();
            let args = args.clone();
            let task = tokio::spawn(async move {
                let str_args: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
                let resp = client.send_command(&name, &str_args).await;
                (client, resp)
            });
            tasks.push(task);
        }

        self.concurrent_responses.clear();
        for task in tasks {
            let (client, resp) = task.await.unwrap();
            self.concurrent_clients.push(client);
            self.concurrent_responses.push(resp);
        }
    }

    pub(crate) async fn then_welcome_received(&mut self) {
        self.client.as_mut().expect("Client not connected").expect_welcome().await;
    }

    pub(crate) fn then_response_is(&self, expected: &str) {
        assert_eq!(self.last_response, expected);
    }

    pub(crate) fn then_response_starts_with(&self, prefix: &str) {
        assert!(
            self.last_response.starts_with(prefix),
            "Expected response to start with '{}', got '{}'",
            prefix,
            self.last_response
        );
    }

    pub(crate) fn then_add_device_succeeded(&self, device_id: u32) {
        assert!(
            self.last_response.starts_with(&format!("{device_id}:hci_device_")),
            "Unexpected add response: {}",
            self.last_response
        );
    }

    pub(crate) fn then_delete_device_response_is(&self, id: u32) {
        assert_eq!(
            self.last_response,
            format!("TestCommandHandler 'del' called with device at index {id}")
        );
    }

    pub(crate) fn then_toggle_phy_response_is(
        &self,
        device_id: u32,
        phy_index: &str,
        enable: bool,
    ) {
        let cmd = if enable { "add_device_to_phy" } else { "del_device_from_phy" };
        let expected = format!(
            "TestCommandHandler '{}' called with device {} and phy {}",
            cmd, device_id, phy_index
        );
        assert_eq!(self.last_response, expected);
    }

    pub(crate) fn then_list_contains_device(&self, device_id: u32) {
        let expected_device_row = format!("{}:hci_device_{}", device_id, device_id);
        assert!(self.last_response.contains("Devices:"), "Response missing Devices: category");
        assert!(
            self.last_response.contains(&expected_device_row),
            "Response missing expected device entry: {}",
            expected_device_row
        );
        assert!(self.last_response.contains("Phys:"), "Response missing Phys: category");
    }

    pub(crate) fn then_list_phy_contains_device(&self, phy_prefix: &str, device_id: u32) {
        let phy_line = self
            .last_response
            .lines()
            .find(|line| line.trim().starts_with(phy_prefix))
            .unwrap_or_else(|| panic!("Missing {} physical category", phy_prefix));
        let devices = phy_line.split(':').nth(2).unwrap().trim();
        let device_ids: Vec<&str> = devices.split(',').map(|s| s.trim()).collect();
        assert!(
            device_ids.contains(&device_id.to_string().as_str()),
            "Device {} not found in {} devices: {:?}",
            device_id,
            phy_prefix,
            device_ids
        );
    }

    pub(crate) async fn then_client_stream_is_closed(&mut self) {
        let client = self.client.as_mut().expect("Client not connected");
        let mut buf = [0u8; 1];
        let read_result = client.stream.read(&mut buf).await;
        assert!(read_result.is_ok());
        assert_eq!(read_result.unwrap(), 0, "Expected EOF (connection closed by server)");
    }

    pub(crate) fn then_pipelined_responses_contain(&self, expected_count: usize, substring: &str) {
        assert_eq!(self.pipelined_responses.len(), expected_count);
        for resp in &self.pipelined_responses {
            assert!(
                resp.contains(substring),
                "Pipelined response did not contain '{}': {}",
                substring,
                resp
            );
        }
    }

    pub(crate) fn then_all_concurrent_responses_contain(&self, substring: &str) {
        assert!(!self.concurrent_responses.is_empty(), "No concurrent responses recorded");
        for (i, resp) in self.concurrent_responses.iter().enumerate() {
            assert!(resp.contains(substring), "Client {} got invalid response: {}", i, resp);
        }
    }

    pub(crate) async fn then_bluetooth_le_enabled_is(&mut self, device_id: u32, expected: bool) {
        for chip in self.bluetooth_chips(device_id).await {
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

    pub(crate) async fn then_bluetooth_classic_enabled_is(
        &mut self,
        device_id: u32,
        expected: bool,
    ) {
        for chip in self.bluetooth_chips(device_id).await {
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

    pub(crate) async fn then_bluetooth_chip_enabled_is(&mut self, device_id: u32, expected: bool) {
        for chip in self.bluetooth_chips(device_id).await {
            assert_eq!(
                chip.enabled, expected,
                "Bluetooth chip enabled state mismatch: expected {}, got {}",
                expected, chip.enabled
            );
        }
    }

    pub(crate) async fn then_bluetooth_preset_is(
        &mut self,
        device_id: u32,
        expected: Option<&str>,
    ) {
        for chip in self.bluetooth_chips(device_id).await {
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

    pub(crate) async fn then_device_is_deleted(&mut self, id: u32) {
        let device = self.device_client.get(DeviceId(id)).await.unwrap();
        assert!(device.is_none(), "Expected device {} to be fully deleted", id);
    }
}
