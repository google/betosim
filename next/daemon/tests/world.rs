#![allow(clippy::field_reassign_with_default)]
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

use std::{
    path::PathBuf,
    sync::{Arc, OnceLock},
    time::Duration,
};

use bytes::Bytes;
use daemon::{NetsimDaemon, StartUpMode};
use futures::{SinkExt, StreamExt};
use grpcio::ChannelBuilder;
use netsim_proto::{
    access_point::{
        AccessPoint, CreateAccessPointRequest, DeleteAccessPointRequest, DisconnectRequest,
        ExecuteAccessPointRequest, GetAccessPointRequest, UpdateAccessPointRequest,
    },
    access_point_grpc::AccessPointServiceClient,
    common::ChipKind,
    frontend::{CreateDeviceRequest, DeleteChipRequest, DeleteDeviceRequest},
    frontend_grpc::FrontendServiceClient,
    hci_packet::hcipacket::PacketType,
    model::{ChipCreate, DeviceCreate},
    nfc_service_grpc::NfcServiceClient,
    packet_streamer::PacketRequest,
    packet_streamer_grpc::PacketStreamerClient,
    protobuf::{EnumOrUnknown, MessageField},
    startup::{Chip as StartupChip, ChipInfo, DeviceInfo},
};
use tokio::time::timeout;
use websocket_server::ManualWebSocketClient;

// Share a single gRPC Environment across all server instances within the same
// process (namely during parallel test execution) to prevent Abseil lock
// contention.
static SHARED_ENV: OnceLock<Arc<grpcio::Environment>> = OnceLock::new();

/// The BDD World for Daemon tests.
pub struct World {
    pub daemon: Option<NetsimDaemon>,
    pub daemon_task: Option<tokio::task::JoinHandle<()>>,
    pub frontend_client: FrontendServiceClient,
    pub access_point_client: AccessPointServiceClient,
    pub nfc_client: NfcServiceClient,
    pub packet_client: PacketStreamerClient,
    pub capture_client: capture_actor::CaptureClient,
    pub packet_sender:
        Option<grpcio::ClientDuplexSender<netsim_proto::packet_streamer::PacketRequest>>,
    pub packet_receiver:
        Option<grpcio::ClientDuplexReceiver<netsim_proto::packet_streamer::PacketResponse>>,

    pub grpc_port: u16,
    pub http_port: u16,
    ws_port: Option<u16>,
    ws_client: Option<ManualWebSocketClient>,
    _temp_dir: PathBuf,
}

impl Drop for World {
    fn drop(&mut self) {
        if let Some(task) = self.daemon_task.take() {
            task.abort();
        }
        let _ = std::fs::remove_dir_all(&self._temp_dir);
    }
}

impl World {
    /// Given a running Netsim Daemon
    pub async fn new() -> Self {
        let mut args = daemon::Args::default();
        args.logtostderr = true; // Disable log redirection
        args.no_shutdown = true; // Prevent tests from dying when deleting devices
        Self::new_with_args(args).await
    }

    /// Starts a Netsim Daemon configured with an active WebSocket listener on port 0.
    pub async fn new_with_websocket() -> Self {
        let mut args = daemon::Args::default();
        args.logtostderr = true;
        args.no_shutdown = true;
        args.ws_port = Some(0);
        Self::new_with_args(args).await
    }

    pub async fn new_with_args(args: daemon::Args) -> Self {
        let temp_dir = std::env::temp_dir().join(format!("netsim_test_{}", rand::random::<u32>()));
        std::fs::create_dir_all(&temp_dir).expect("Failed to create temp dir");
        Self::new_with_args_in_dir(temp_dir, args).await
    }

    /// Given a running Netsim Daemon started in an existing discovery
    /// directory, which lets scenarios pre-seed `netsim.ini` and its locks.
    pub async fn new_with_args_in_dir(temp_dir: PathBuf, args: daemon::Args) -> Self {
        static LOG_INIT: std::sync::Once = std::sync::Once::new();
        LOG_INIT.call_once(common::util::netsim_logger::init_for_test);

        let startup_mode =
            NetsimDaemon::new_with_dirs(temp_dir.clone(), Self::with_test_ports(args))
                .await
                .expect("Failed to create daemon");

        let daemon = match startup_mode {
            StartUpMode::Owner(daemon) => daemon,
            _ => panic!("Expected to start as Owner"),
        };

        let grpc_port = daemon.grpc_port().expect("NetsimDaemon has no gRPC port");
        let http_port = daemon.http_port().expect("NetsimDaemon has no HTTP port");
        let ws_port = daemon.ws_port();

        // Allow some time for bindings
        tokio::time::sleep(Duration::from_millis(100)).await;

        let capture_client = daemon.capture_client().clone();

        let env = SHARED_ENV.get_or_init(|| Arc::new(grpcio::Environment::new(1))).clone();
        let ch = ChannelBuilder::new(env).connect(&format!("localhost:{}", grpc_port));
        let frontend_client = FrontendServiceClient::new(ch.clone());
        let access_point_client = AccessPointServiceClient::new(ch.clone());
        let nfc_client = NfcServiceClient::new(ch.clone());
        let packet_client = PacketStreamerClient::new(ch.clone());

        World {
            daemon: Some(daemon),
            daemon_task: None,
            frontend_client,
            access_point_client,
            nfc_client,
            packet_client,
            capture_client,
            packet_sender: None,
            packet_receiver: None,
            grpc_port,
            http_port,
            ws_port,
            ws_client: None,
            _temp_dir: temp_dir,
        }
    }

    pub fn get_temp_dir(&self) -> &std::path::Path {
        &self._temp_dir
    }

    /// Lets the OS assign random available ports unless a scenario set them.
    fn with_test_ports(mut args: daemon::Args) -> daemon::Args {
        if args.hci_port.is_none() {
            args.hci_port = Some(0);
        }
        #[cfg(feature = "cuttlefish")]
        if args.test_port.is_none() {
            args.test_port = Some(0);
        }
        args
    }

    /// When another netsimd starts in this World's discovery directory
    pub async fn when_start_daemon_in_same_dir(&mut self, args: daemon::Args) -> StartUpMode {
        NetsimDaemon::new_with_dirs(self._temp_dir.clone(), Self::with_test_ports(args))
            .await
            .expect("Failed to start another daemon")
    }

    /// Then netsim.ini advertises `grpc_port` and netsim.ini.lock is still held
    pub fn then_discovery_file_advertises_port(&self, grpc_port: u16) {
        let ini_file = daemon::IniFile::new_for_dir(self._temp_dir.clone(), 1).unwrap();
        match ini_file.try_acquire() {
            Ok(daemon::IniFileAccess::Reader(config)) => assert_eq!(config.grpc_port, grpc_port),
            other => panic!("Expected Reader({grpc_port}), got {other:?}"),
        }
    }

    /// Then netsim.ini is gone and netsim.ini.lock can be acquired again
    pub fn then_discovery_files_released(&self) {
        assert!(!self._temp_dir.join("netsim.ini").exists(), "netsim.ini was not removed");
        let ini_file = daemon::IniFile::new_for_dir(self._temp_dir.clone(), 1).unwrap();
        assert!(
            matches!(ini_file.try_acquire(), Ok(daemon::IniFileAccess::Writer(_))),
            "netsim.ini.lock was not released"
        );
    }

    /// When I request the version
    pub async fn when_get_version(&mut self) -> String {
        let resp = self
            .frontend_client
            .get_version_async(&netsim_proto::empty::Empty::new())
            .expect("GetVersion failed")
            .await
            .expect("RPC failed");
        resp.version
    }

    /// When I call reset
    pub async fn when_reset_is_called(&mut self) {
        self.frontend_client
            .reset_async(&netsim_proto::empty::Empty::new())
            .expect("Reset failed")
            .await
            .expect("RPC failed");
    }

    /// When I create a device with name and chip
    pub async fn when_create_device(&mut self, name: &str, chip_name: &str) -> u32 {
        let mut create_req = CreateDeviceRequest::new();
        let mut device_create = DeviceCreate::new();
        device_create.name = name.to_string();

        let mut chip_create = ChipCreate::new();
        chip_create.name = chip_name.to_string();
        chip_create.kind = EnumOrUnknown::new(ChipKind::BLUETOOTH);
        chip_create.manufacturer = "TestMfg".to_string();
        chip_create.product_name = "TestProduct".to_string();
        chip_create.address = "11:22:33:44:55:66".to_string();
        let mut ble_beacon = netsim_proto::model::chip_create::BleBeaconCreate::new();
        ble_beacon.address = "11:22:33:44:55:66".to_string();
        chip_create.set_ble_beacon(ble_beacon);

        device_create.chips.push(chip_create);
        create_req.device = MessageField::some(device_create);

        let resp = self
            .frontend_client
            .create_device_async(&create_req)
            .expect("CreateDevice failed")
            .await
            .expect("RPC failed");
        resp.device.id
    }

    pub async fn when_delete_device(&mut self, device_id: u32) {
        let mut req = DeleteDeviceRequest::new();
        req.id = device_id;
        self.frontend_client
            .delete_device_async(&req)
            .expect("DeleteDevice failed")
            .await
            .expect("RPC failed");
    }

    /// When I list access points
    pub async fn when_list_access_points(
        &mut self,
    ) -> Vec<netsim_proto::access_point::AccessPoint> {
        let resp = self
            .access_point_client
            .list_async(&netsim_proto::access_point::ListAccessPointsRequest::new())
            .expect("ListAccessPoints failed")
            .await
            .expect("RPC failed");
        resp.access_points
    }

    /// When I list devices
    pub async fn when_list_devices(&mut self) -> Vec<netsim_proto::model::Device> {
        let resp = self
            .frontend_client
            .list_device_async(&netsim_proto::empty::Empty::new())
            .expect("ListDevice failed")
            .await
            .expect("RPC failed");
        resp.devices
    }

    /// When I delete a chip (device)
    pub async fn when_delete_chip(&mut self, chip_id: u32) {
        let mut req = DeleteChipRequest::new();
        req.id = chip_id;
        self.frontend_client
            .delete_chip_async(&req)
            .expect("DeleteChip failed")
            .await
            .expect("RPC failed");
    }

    /// When I create a device with specific chips
    pub async fn when_create_device_with_chips(
        &mut self,
        name: &str,
        chips: Vec<ChipCreate>,
    ) -> u32 {
        let mut create_req = CreateDeviceRequest::new();
        let mut device_create = DeviceCreate::new();
        device_create.name = name.to_string();
        device_create.chips = chips;
        create_req.device = MessageField::some(device_create);

        let resp = self
            .frontend_client
            .create_device_async(&create_req)
            .expect("CreateDevice failed")
            .await
            .expect("RPC failed");
        resp.device.id
    }

    /// When I patch a device
    pub async fn when_patch_device(
        &mut self,
        patch_req: &netsim_proto::frontend::PatchDeviceRequest,
    ) {
        self.frontend_client
            .patch_device_async(patch_req)
            .expect("PatchDevice failed")
            .await
            .expect("RPC failed");
    }

    /// Start the daemon in the background, handing it the discovery lock so
    /// that, as in `daemon::run`, `netsim.ini.lock` is released when
    /// `run_daemon` returns.
    pub async fn when_spawn_daemon(&mut self) {
        if self.daemon_task.is_some() {
            panic!("Daemon already spawned");
        }
        if let Some(daemon) = self.daemon.take() {
            let task = tokio::spawn(async move {
                let _ = daemon.run_daemon().await;
            });
            self.daemon_task = Some(task);
        } else {
            panic!("Daemon not initialized or already consumed");
        }
    }

    pub async fn then_daemon_shutdown(&mut self, timeout_ms: u64) {
        if let Some(task) = self.daemon_task.take() {
            let result =
                tokio::time::timeout(std::time::Duration::from_millis(timeout_ms), task).await;
            match result {
                Ok(Ok(_)) => {}
                Ok(Err(e)) => panic!("Daemon task failed: {}", e),
                Err(e) => panic!("Daemon failed to shut down within timeout: {e}"),
            }
        } else {
            panic!("No daemon spawned to shut down");
        }
    }

    pub fn is_daemon_finished(&self) -> bool {
        if let Some(task) = &self.daemon_task {
            task.is_finished()
        } else {
            true // If no task, treat as finished (or not running)
        }
    }

    /// Helper to create a Bluetooth chip configuration
    pub fn make_bluetooth_chip(name: &str, address: &str) -> ChipCreate {
        let mut chip = ChipCreate::new();
        chip.name = name.to_string();
        chip.kind = EnumOrUnknown::new(ChipKind::BLUETOOTH);
        chip.address = address.to_string();
        chip
    }

    /// Helper to create a UWB chip configuration
    pub fn make_uwb_chip(name: &str) -> ChipCreate {
        let mut chip = ChipCreate::new();
        chip.name = name.to_string();
        chip.kind = EnumOrUnknown::new(ChipKind::UWB);
        chip
    }

    /// When I patch the radio state of a chip
    pub async fn when_patch_state(
        &mut self,
        device_id: u32,
        chip_id: Option<u32>,
        kind: ChipKind,
        state: bool,
    ) {
        let mut patch_req = netsim_proto::frontend::PatchDeviceRequest::new();
        patch_req.id = Some(device_id);

        let mut patch_fields =
            netsim_proto::frontend::patch_device_request::PatchDeviceFields::new();

        let mut chip_patch = netsim_proto::model::Chip::new();
        if let Some(id) = chip_id {
            chip_patch.id = id;
        }
        chip_patch.kind = EnumOrUnknown::new(kind);

        if kind == ChipKind::BLUETOOTH {
            let mut bt_data = netsim_proto::model::chip::Bluetooth::new();
            let mut classic = netsim_proto::model::chip::Radio::new();
            classic.state = Some(state);
            bt_data.classic = MessageField::some(classic);
            chip_patch.set_bt(bt_data);
        } else if kind == ChipKind::UWB {
            let mut radio = netsim_proto::model::chip::Radio::new();
            radio.state = Some(state);
            chip_patch.set_uwb(radio);
        }

        patch_fields.chips.push(chip_patch);
        patch_req.device = MessageField::some(patch_fields);

        self.when_patch_device(&patch_req).await;
    }

    /// Then I verify the radio state of a chip
    pub async fn then_radio_state_is(&mut self, device_id: u32, kind: ChipKind, expected: bool) {
        let devices = self.when_list_devices().await;
        let device = devices.iter().find(|d| d.id == device_id).expect("Device missing");

        // Find the chips of the given kind
        let chips: Vec<_> =
            device.chips.iter().filter(|c| c.kind == EnumOrUnknown::new(kind)).collect();

        assert!(!chips.is_empty(), "No chips of kind {:?} found", kind);

        for chip in chips {
            let state = match kind {
                ChipKind::BLUETOOTH => {
                    if let Some(netsim_proto::model::chip::Chip::Bt(bt)) = &chip.chip {
                        bt.classic.as_ref().and_then(|r| r.state).unwrap_or(true)
                    } else {
                        panic!("Chip kind mismatch");
                    }
                }
                ChipKind::UWB => {
                    if let Some(netsim_proto::model::chip::Chip::Uwb(radio)) = &chip.chip {
                        radio.state.unwrap_or(true)
                    } else {
                        panic!("Chip kind mismatch");
                    }
                }
                _ => true, // Default to true for other kinds for now
            };
            assert_eq!(state, expected, "Radio state for chip {} should be {}", chip.id, expected);
        }
    }

    /// When I patch the capture state of a chip
    pub async fn when_patch_capture(&mut self, chip_id: u32, enabled: bool) {
        self.capture_client
            .patch_capture(netsim_model::ChipId::from(chip_id), enabled)
            .await
            .expect("Failed to patch capture");
    }

    /// Then I verify the capture state of a chip
    pub async fn then_capture_is(&mut self, chip_id: u32, expected: bool) {
        let captures = self.capture_client.list_captures().await.expect("Failed to list captures");
        let capture = captures
            .iter()
            .find(|c| c.chip_id == netsim_model::ChipId::from(chip_id))
            .expect("Capture info missing");

        assert_eq!(
            capture.enabled, expected,
            "Capture expected for chip {} should be {}",
            chip_id, expected
        );
    }

    /// When I create a new Access Point
    pub async fn when_create_access_point(
        &mut self,
        ssid: &str,
        channel: u32,
        hw_mode: &str,
    ) -> u32 {
        let mut ap_config = AccessPoint::new();
        ap_config.ssid = ssid.to_string();
        ap_config.channel = channel;
        ap_config.hw_mode = hw_mode.to_string();

        let mut create_req = CreateAccessPointRequest::new();
        create_req.access_point = MessageField::some(ap_config);

        let created_ap = self
            .access_point_client
            .create_async(&create_req)
            .expect("Create AP failed")
            .await
            .expect("RPC failed");
        created_ap.id
    }

    /// When I get an Access Point by ID
    pub async fn when_get_access_point(&mut self, id: u32) -> AccessPoint {
        let mut get_req = GetAccessPointRequest::new();
        get_req.id = id;
        self.access_point_client
            .get_async(&get_req)
            .expect("Get AP failed")
            .await
            .expect("RPC failed")
    }

    /// When I update an Access Point
    pub async fn when_update_access_point(
        &mut self,
        id: u32,
        ssid: Option<&str>,
        channel: Option<u32>,
    ) {
        let mut update_req = UpdateAccessPointRequest::new();
        update_req.id = id;
        if let Some(s) = ssid {
            update_req.ssid = Some(s.to_string());
        }
        if let Some(c) = channel {
            update_req.channel = Some(c);
        }
        self.access_point_client
            .update_async(&update_req)
            .expect("Update AP failed")
            .await
            .expect("RPC failed");
    }

    /// When I delete an Access Point
    pub async fn when_delete_access_point(&mut self, id: u32) {
        let mut delete_req = DeleteAccessPointRequest::new();
        delete_req.id = id;
        self.access_point_client
            .delete_async(&delete_req)
            .expect("Delete AP failed")
            .await
            .expect("RPC failed");
    }

    /// When I execute disconnect on an Access Point
    pub async fn when_execute_disconnect(&mut self, id: u32, mac_address: &str) {
        let mut disconnect_req = DisconnectRequest::new();
        disconnect_req.mac_address = mac_address.to_string();

        let mut execute_req = ExecuteAccessPointRequest::new();
        execute_req.id = id;
        execute_req.set_disconnect(disconnect_req);

        self.access_point_client
            .execute_async(&execute_req)
            .expect("Execute failed")
            .await
            .expect("RPC failed");
    }

    /// Then I verify the Access Point exists and matches the SSID
    pub async fn then_access_point_exists(&mut self, id: u32, expected_ssid: &str) {
        let ap = self.when_get_access_point(id).await;
        assert_eq!(ap.ssid, expected_ssid);
    }

    /// Then I verify the Access Point matches the expected configuration
    pub async fn then_access_point_matches(
        &mut self,
        id: u32,
        ssid: &str,
        channel: u32,
        hw_mode: &str,
    ) {
        let ap = self.when_get_access_point(id).await;
        assert_eq!(ap.ssid, ssid);
        assert_eq!(ap.channel, channel);
        assert_eq!(ap.hw_mode, hw_mode);
    }

    /// Then I verify the Access Point is not found
    pub async fn then_access_point_not_found(&mut self, id: u32) {
        let mut get_req = GetAccessPointRequest::new();
        get_req.id = id;
        let get_result = self.access_point_client.get_async(&get_req).expect("Get AP failed").await;
        assert!(get_result.is_err(), "Get should fail for deleted AP");
    }

    /// Then I verify the Access Point is in the list
    pub async fn then_access_point_in_list(&mut self, id: u32) {
        let aps = self.when_list_access_points().await;
        assert!(aps.iter().any(|ap| ap.id == id));
    }

    /// Then I verify the Access Point is NOT in the list
    pub async fn then_access_point_not_in_list(&mut self, id: u32) {
        let aps = self.when_list_access_points().await;
        assert!(!aps.iter().any(|ap| ap.id == id));
    }

    /// Then I verify the device list contains a device
    pub async fn then_device_list_contains(&mut self, device_id: u32, device_name: &str) {
        let devices = self.when_list_devices().await;
        assert!(devices.iter().any(|d| d.id == device_id && d.name == device_name));
    }

    /// Then I verify the device list does not contain a device
    pub async fn then_device_list_does_not_contain(&mut self, device_id: u32) {
        let devices = self.when_list_devices().await;
        assert!(!devices.iter().any(|d| d.id == device_id));
    }
    /// Then I verify the device position
    pub async fn then_device_position_is(
        &mut self,
        device_id: u32,
        expected_x: f32,
        expected_y: f32,
    ) {
        let devices = self.when_list_devices().await;
        let device = devices.iter().find(|d| d.id == device_id).expect("Device missing");
        let pos = device.position.as_ref().unwrap();
        assert!((pos.x - expected_x).abs() < 0.001, "Expected X {}, got {}", expected_x, pos.x);
        assert!((pos.y - expected_y).abs() < 0.001, "Expected Y {}, got {}", expected_y, pos.y);
    }

    /// Then I verify the device position by name
    pub async fn then_device_position_by_name_is(
        &mut self,
        name: &str,
        expected_x: f32,
        expected_y: f32,
    ) {
        let devices = self.when_list_devices().await;
        let device = devices.iter().find(|d| d.name == name).expect("Device missing");
        let pos = device.position.as_ref().unwrap();
        assert!((pos.x - expected_x).abs() < 0.001, "Expected X {}, got {}", expected_x, pos.x);
        assert!((pos.y - expected_y).abs() < 0.001, "Expected Y {}, got {}", expected_y, pos.y);
    }

    /// Then I verify an Access Point exists by SSID
    pub async fn then_access_point_exists_by_ssid(&mut self, ssid: &str) {
        let aps = self.when_list_access_points().await;
        assert!(aps.iter().any(|a| a.ssid == ssid), "AP with SSID {} not found in list", ssid);
    }

    /// Then I verify an Access Point matches by SSID
    pub async fn then_access_point_matches_by_ssid(
        &mut self,
        ssid: &str,
        channel: u32,
        hw_mode: &str,
    ) {
        let aps = self.when_list_access_points().await;
        let ap = aps.iter().find(|a| a.ssid == ssid).expect("AP missing");
        assert_eq!(ap.channel, channel);
        assert_eq!(ap.hw_mode, hw_mode);
    }

    /// Then I verify the device list contains a device by name
    pub async fn then_device_list_contains_by_name(&mut self, name: &str) {
        let devices = self.when_list_devices().await;
        assert!(devices.iter().any(|d| d.name == name));
    }

    /// When I create a test device with detailed options
    pub async fn when_create_detailed_device(
        &mut self,
        device_name: &str,
        chip_name: &str,
        kind: ChipKind,
        address: &str,
        is_beacon: bool,
    ) -> (u32, u32) {
        let mut chip = ChipCreate::new();
        chip.name = chip_name.to_string();
        chip.kind = EnumOrUnknown::new(kind);
        chip.manufacturer = "Mfg".to_string();
        chip.product_name = "Prod".to_string();
        if is_beacon {
            let mut ble_beacon = netsim_proto::model::chip_create::BleBeaconCreate::new();
            ble_beacon.address = address.to_string();
            chip.set_ble_beacon(ble_beacon);
        }

        let mut device = DeviceCreate::new();
        device.name = device_name.to_string();
        device.chips.push(chip);

        let mut req = CreateDeviceRequest::new();
        req.device = MessageField::some(device);

        let resp = self
            .frontend_client
            .create_device_async(&req)
            .expect("CreateDevice failed")
            .await
            .expect("RPC failed");
        let device_id = resp.device.id;

        // Fetch the detailed device to get the chip ID
        let list_resp = self
            .frontend_client
            .list_device_async(&netsim_proto::empty::Empty::new())
            .expect("ListDevice failed")
            .await
            .expect("RPC failed");

        let device_detail =
            list_resp.devices.iter().find(|d| d.id == device_id).expect("Device not found");
        let chip_id = device_detail.chips[0].id;

        (device_id, chip_id)
    }

    pub async fn when_create_link(
        &mut self,
        sender_id: u32,
        receiver_id: u32,
        rssi: i32,
        kind: ChipKind,
    ) -> u32 {
        let mut link = netsim_proto::model::Link::new();
        link.sender_id = sender_id;
        link.receiver_id = receiver_id;
        link.rssi = rssi;
        link.kind = EnumOrUnknown::new(kind);

        let mut create_req = netsim_proto::frontend::CreateLinkRequest::new();
        create_req.link = MessageField::some(link);
        let resp = self
            .frontend_client
            .create_link_async(&create_req)
            .expect("CreateLink")
            .await
            .expect("RPC Create");
        resp.link.id
    }

    pub async fn when_patch_link(&mut self, id: u32, rssi: i32, kind: ChipKind) {
        let mut link = netsim_proto::model::Link::new();
        link.rssi = rssi;
        link.kind = EnumOrUnknown::new(kind);
        let mut patch_req = netsim_proto::frontend::PatchLinkRequest::new();
        patch_req.id = id;
        patch_req.link = MessageField::some(link);
        self.frontend_client
            .patch_link_async(&patch_req)
            .expect("PatchLink")
            .await
            .expect("RPC Patch");
    }

    pub async fn when_delete_link(&mut self, id: u32) {
        let mut delete_req = netsim_proto::frontend::DeleteLinkRequest::new();
        delete_req.id = id;
        self.frontend_client
            .delete_link_async(&delete_req)
            .expect("DeleteLink")
            .await
            .expect("RPC Delete");
    }

    pub async fn then_link_count_is(&mut self, expected_count: usize) {
        let list_resp = self
            .frontend_client
            .list_link_async(&netsim_proto::empty::Empty::new())
            .expect("ListLink")
            .await
            .expect("RPC List");
        assert_eq!(list_resp.links.len(), expected_count);
    }

    pub async fn then_link_matches(
        &mut self,
        id: u32,
        sender_id: u32,
        receiver_id: u32,
        rssi: i32,
        kind: ChipKind,
    ) {
        let list_resp = self
            .frontend_client
            .list_link_async(&netsim_proto::empty::Empty::new())
            .expect("ListLink")
            .await
            .expect("RPC List");
        let link = list_resp.links.iter().find(|l| l.id == id).expect("Link missing");
        assert_eq!(link.sender_id, sender_id);
        assert_eq!(link.receiver_id, receiver_id);
        assert_eq!(link.rssi, rssi);
        assert_eq!(link.kind.enum_value_or_default(), kind);
    }

    pub async fn then_link_rssi_is(&mut self, id: u32, expected_rssi: i32) {
        let list_resp = self
            .frontend_client
            .list_link_async(&netsim_proto::empty::Empty::new())
            .expect("ListLink")
            .await
            .expect("RPC List");
        let link = list_resp.links.iter().find(|l| l.id == id).expect("Link missing");
        assert_eq!(link.rssi, expected_rssi);
    }

    pub async fn when_open_packet_stream(&mut self) {
        let (sender, receiver) =
            self.packet_client.stream_packets().expect("Failed to create stream");
        self.packet_sender = Some(sender);
        self.packet_receiver = Some(receiver);
    }

    pub async fn when_send_packet_initial_info(&mut self, chip_name: &str) {
        let mut chip_info = netsim_proto::startup::ChipInfo::new();
        chip_info.name = chip_name.to_string();

        let mut chip = netsim_proto::startup::Chip::new();
        chip.kind = netsim_proto::protobuf::EnumOrUnknown::new(ChipKind::BLUETOOTH); // Default to Bluetooth for streamer testing
        chip.address = "11:22:33:44:55:66".to_string();
        chip_info.chip = netsim_proto::protobuf::MessageField::some(chip);

        self.send_initial_info(chip_info).await;
    }

    pub async fn given_initial_info_sent_with_bt_properties(
        &mut self,
        chip_name: &str,
        bt_properties: netsim_proto::configuration::Controller,
    ) {
        let mut chip_info = netsim_proto::startup::ChipInfo::new();
        chip_info.name = chip_name.to_string();

        let mut chip = netsim_proto::startup::Chip::new();
        chip.kind = netsim_proto::protobuf::EnumOrUnknown::new(ChipKind::BLUETOOTH);
        chip.address = "11:22:33:44:55:66".to_string();
        chip.bt_properties = netsim_proto::protobuf::MessageField::some(bt_properties);
        chip_info.chip = netsim_proto::protobuf::MessageField::some(chip);

        self.send_initial_info(chip_info).await;
    }

    pub async fn when_send_hci_packet(
        &mut self,
        packet_type: netsim_proto::hci_packet::hcipacket::PacketType,
        data: Vec<u8>,
    ) {
        let sender = self.packet_sender.as_mut().expect("No packet sender");
        let mut packet_req = netsim_proto::packet_streamer::PacketRequest::new();
        let mut hci_packet = netsim_proto::hci_packet::HCIPacket::new();
        hci_packet.packet_type = packet_type.into();
        hci_packet.packet = data;
        packet_req.set_hci_packet(hci_packet);

        sender
            .send((packet_req, grpcio::WriteFlags::default()))
            .await
            .expect("Failed to send HCI Packet");
    }

    pub async fn when_close_packet_stream(&mut self) {
        if let Some(mut sender) = self.packet_sender.take() {
            sender.close().await.expect("Failed to close stream");
        }
    }

    pub async fn then_packet_stream_receives(&mut self) {
        let receiver = self.packet_receiver.as_mut().expect("No packet receiver");
        let _ = receiver.next().await;
    }

    /// Sends InitialInfo for a Bluetooth chip with `loopback` set.
    pub async fn when_send_loopback_initial_info(&mut self, device_name: &str) {
        let mut chip = StartupChip::new();
        chip.kind = EnumOrUnknown::new(ChipKind::BLUETOOTH);
        chip.loopback = true;
        let mut device_info = DeviceInfo::new();
        device_info.name = device_name.to_string();
        let mut chip_info = ChipInfo::new();
        chip_info.chip = MessageField::some(chip);
        chip_info.device_info = MessageField::some(device_info);
        self.send_initial_info(chip_info).await;
    }

    async fn send_initial_info(&mut self, chip_info: ChipInfo) {
        let sender = self.packet_sender.as_mut().expect("No packet sender");
        let mut initial_req = PacketRequest::new();
        initial_req.set_initial_info(chip_info);
        sender
            .send((initial_req, grpcio::WriteFlags::default()))
            .await
            .expect("Failed to send InitialInfo");
    }

    pub async fn then_packet_stream_receives_hci(&mut self, packet_type: PacketType, data: &[u8]) {
        let receiver = self.packet_receiver.as_mut().expect("No packet receiver");
        let response = timeout(Duration::from_secs(5), receiver.next())
            .await
            .expect("Timed out waiting for a packet")
            .expect("Stream ended")
            .expect("Stream error");
        let hci = response.hci_packet();
        assert_eq!(hci.packet_type.enum_value_or_default(), packet_type);
        assert_eq!(hci.packet, data);
    }

    pub async fn then_packet_stream_receives_hci_event(&mut self) -> Vec<u8> {
        let receiver = self.packet_receiver.as_mut().expect("No packet receiver");
        let response = timeout(Duration::from_secs(5), receiver.next())
            .await
            .expect("Timed out waiting for a packet")
            .expect("Stream ended")
            .expect("Stream error");
        let hci = response.hci_packet();
        assert_eq!(hci.packet_type.enum_value_or_default(), PacketType::EVENT);
        hci.packet.clone()
    }

    pub async fn when_connect_websocket(&mut self, path: &str) {
        let ws_port = self.ws_port.expect("NetsimDaemon has no WebSocket port");
        let client = ManualWebSocketClient::connect("127.0.0.1", ws_port, path)
            .await
            .expect("Failed to connect WebSocket");
        self.ws_client = Some(client);
    }

    fn ws_client(&mut self) -> &mut ManualWebSocketClient {
        self.ws_client.as_mut().expect("No WebSocket client")
    }

    pub async fn when_send_websocket_text(&mut self, text: &str) {
        self.ws_client().send_text(text).await;
    }

    pub async fn when_send_websocket_binary(&mut self, data: &[u8]) {
        self.ws_client().send_binary(Bytes::copy_from_slice(data)).await;
    }

    async fn read_websocket_binary(&mut self) -> Bytes {
        let client = self.ws_client();
        timeout(Duration::from_secs(5), client.read_binary())
            .await
            .expect("Timed out waiting for WebSocket binary response")
    }

    pub async fn then_websocket_receives_binary(&mut self, expected: &[u8]) {
        let resp = self.read_websocket_binary().await;
        assert_eq!(resp.as_ref(), expected);
    }

    pub async fn then_websocket_receives_le_features_with_cis_cleared(&mut self) {
        let feat_resp = self.read_websocket_binary().await;
        // Command complete event length is 15 (0x04, 0x0e, 0x0c, 0x01, 0x03, 0x20, status, features[8])
        assert_eq!(feat_resp.len(), 15);
        assert_eq!(feat_resp[6], 0x00, "status should be SUCCESS");
        // Byte 3 of features (packet index 10) bits 4-5 correspond to CIS.
        assert_eq!(feat_resp[10] & 0x30, 0x00, "CIS feature bits should be cleared");
    }
}
