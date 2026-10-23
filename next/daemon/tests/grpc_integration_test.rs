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

use netsim_proto::{
    common::ChipKind,
    configuration::{Controller, ControllerFeatures, ControllerPreset},
    frontend::PatchDeviceRequest,
    hci_packet::hcipacket::PacketType,
    model::ChipCreate,
    protobuf::{EnumOrUnknown, MessageField},
};

use crate::world::World;

// Feature: gRPC Frontend Lifecycle
//
//   As a developer
//   I want to manage devices and streams via gRPC
//   So that I can control the netsim environment

// Scenario: Verify Daemon Version and Device Lifecycle
//   Given a running Netsim Daemon
//   Then I can retrieve the daemon version
//   When I create a new device
//   Then the device appears in the device list
//   When I delete the device
//   Then the device disappears from the device list
#[tokio::test]
async fn test_grpc_frontend_lifecycle() {
    // Given a running Netsim Daemon
    let mut world = World::new().await;

    // Spawn the daemon in a separate task
    world.when_spawn_daemon().await;

    // Verify Version
    let version = world.when_get_version().await;
    assert!(version.starts_with("1."), "Version must be a 1.x release");

    // When I create a new device
    let device_name = "grpc-test-device";
    let device_id = world.when_create_device(device_name, "beacon-chip").await;
    assert!(device_id > 0);

    // Then the device list contains the new device
    world.then_device_list_contains(device_id, device_name).await;

    // When I delete the device
    world.when_delete_device(device_id).await;

    // Then the device list does not contain the device
    world.then_device_list_does_not_contain(device_id).await;

    // Cleanup: In a real scenario we might want to shut down gracefully,
    // but here we just let the test finish which aborts the daemon task.
    // However, to be clean we could abort or just let it drop.
    // The TempDir will be cleaned up on Drop of World.
}

// Scenario: Packet Streamer Lifecycle
//   Given a running Netsim Daemon
//   When I establish a packet stream
//   And I send initial chip info
//   And I send a packet
//   Then the stream remains open and processes packets
#[tokio::test]
async fn test_packet_streamer_lifecycle() {
    // Given a running Netsim Daemon
    let mut world = World::new().await;

    world.when_spawn_daemon().await;

    // Spawn client task to interact with packet streamer
    // We need to clone world bits or just do it inline here since we own world.
    // However, `when_spawn_daemon` moved the daemon instance to background.
    // The client is created via gRPC port which we have in world.

    // 1. Open stream
    world.when_open_packet_stream().await;

    // 2. Send InitialInfo
    world.when_send_packet_initial_info("streamer-test-chip").await;

    // 3. Send a Packet (HCI)
    world.when_send_hci_packet(PacketType::COMMAND, vec![0x01, 0x02, 0x03]).await;

    // 4. Close stream
    world.when_close_packet_stream().await;

    // 5. Verify we don't get an error immediately
    world.then_packet_stream_receives().await;
}

// Scenario: Loopback packet stream
//   Given a running Netsim Daemon
//   When I open a packet stream with a loopback Bluetooth chip
//   And I send HCI packets
//   Then each packet comes back unchanged, in order
//   And no device is created
#[tokio::test]
async fn test_packet_streamer_loopback() {
    let mut world = World::new().await;
    world.when_spawn_daemon().await;

    world.when_open_packet_stream().await;
    world.when_send_loopback_initial_info("loopback-device").await;

    let packets = [
        (PacketType::COMMAND, vec![0x03, 0x0c, 0x00]),
        (PacketType::ACL, vec![0xAB; 60_000]),
        (PacketType::EVENT, vec![0x0e, 0x04]),
    ];
    for (packet_type, data) in &packets {
        world.when_send_hci_packet(*packet_type, data.clone()).await;
    }
    for (packet_type, data) in &packets {
        world.then_packet_stream_receives_hci(*packet_type, data).await;
    }

    let devices = world.when_list_devices().await;
    assert!(
        !devices.iter().any(|d| d.name == "loopback-device"),
        "loopback must not create a device"
    );
    world.when_close_packet_stream().await;
}

// Scenario: Patch Device Resolution
//   Given a running Netsim Daemon
//   When I create a device named "Resolution-Device"
//   And I patch the device position by name
//   Then the device position is updated
//   When I patch the device position by ID
//   Then the device position is updated again
#[tokio::test]
async fn test_patch_device_resolution() {
    let mut world = World::new().await;
    world.when_spawn_daemon().await;

    // 1. Create Device "Resolution-Device"
    let mut beacon = ChipCreate::new();
    beacon.name = "beacon".to_string();
    beacon.kind = EnumOrUnknown::new(ChipKind::BLUETOOTH);
    beacon.address = "11:22:33:44:55:66".to_string();
    let mut ble_beacon = netsim_proto::model::chip_create::BleBeaconCreate::new();
    ble_beacon.address = "11:22:33:44:55:66".to_string();
    beacon.set_ble_beacon(ble_beacon);

    let device_id = world.when_create_device_with_chips("Resolution-Device", vec![beacon]).await;
    assert!(device_id > 0);

    // 2. Patch Device by Name (ID = 0/None)
    let mut patch_req = PatchDeviceRequest::new();
    let mut patch_fields = netsim_proto::frontend::patch_device_request::PatchDeviceFields::new();
    patch_fields.name = Some("Resolution-Device".to_string());
    patch_fields.position = MessageField::some(netsim_proto::model::Position {
        x: 10.0,
        y: 10.0,
        z: 0.0,
        ..Default::default()
    });
    patch_req.device = MessageField::some(patch_fields);

    world.when_patch_device(&patch_req).await;

    // Verify position update (Name patch)
    world.then_device_position_by_name_is("Resolution-Device", 10.0, 10.0).await;

    // 3. Patch Device by Explicit ID
    let mut patch_req_id = PatchDeviceRequest::new();
    patch_req_id.id = Some(device_id);
    let mut patch_fields_id =
        netsim_proto::frontend::patch_device_request::PatchDeviceFields::new();
    patch_fields_id.position = MessageField::some(netsim_proto::model::Position {
        x: 20.0,
        y: 20.0,
        z: 0.0,
        ..Default::default()
    });
    patch_req_id.device = MessageField::some(patch_fields_id);

    world.when_patch_device(&patch_req_id).await;

    // Verify position update (ID patch)
    world.then_device_position_is(device_id, 20.0, 20.0).await;
}

// Scenario: Chip Update
//   Given a running Netsim Daemon
//   When I create a device with a Bluetooth Beacon chip
//   And I patch the device position (which triggers a chip update)
//   Then the device position is updated
#[tokio::test]
async fn test_chip_update() {
    let mut world = World::new().await;
    world.when_spawn_daemon().await;

    // 1. Create Device with 1 Bluetooth Beacon chip
    let mut beacon = ChipCreate::new();
    beacon.name = "beacon0".to_string();
    beacon.kind = EnumOrUnknown::new(ChipKind::BLUETOOTH);
    let mut ble_beacon = netsim_proto::model::chip_create::BleBeaconCreate::new();
    ble_beacon.address = "11:22:33:44:55:66".to_string();
    beacon.set_ble_beacon(ble_beacon);

    let device_id = world.when_create_device_with_chips("Chip-Update-Device", vec![beacon]).await;
    assert!(device_id > 0);

    // 2. Patch Device Position (should trigger updates on the chip)
    let mut patch_req = PatchDeviceRequest::new();
    patch_req.id = Some(device_id);
    let mut patch_fields = netsim_proto::frontend::patch_device_request::PatchDeviceFields::new();
    patch_fields.position = MessageField::some(netsim_proto::model::Position {
        x: 50.0,
        y: 50.0,
        z: 0.0,
        ..Default::default()
    });
    let mut chip_patch = netsim_proto::model::Chip::new();
    chip_patch.name = "beacon0".to_string();
    let mut beacon_patch = netsim_proto::model::chip::BleBeacon::new();
    let mut settings = netsim_proto::model::chip::ble_beacon::AdvertiseSettings::new();
    settings.set_dbm(-12);
    settings.timeout = 500;
    beacon_patch.settings = MessageField::some(settings);
    let mut adv_data = netsim_proto::model::chip::ble_beacon::AdvertiseData::new();
    adv_data.include_tx_power_level = true;
    adv_data.manufacturer_data = vec![0xAA, 0xBB];
    beacon_patch.adv_data = MessageField::some(adv_data);
    chip_patch.set_ble_beacon(beacon_patch);
    patch_fields.chips.push(chip_patch);
    patch_req.device = MessageField::some(patch_fields);

    world.when_patch_device(&patch_req).await;

    // Verify position and BLE beacon update reflected in device list
    world.then_device_position_is(device_id, 50.0, 50.0).await;
    let devices = world.when_list_devices().await;
    let device = devices.iter().find(|d| d.id == device_id).expect("Device missing");
    let Some(netsim_proto::model::chip::Chip::BleBeacon(b)) = &device.chips[0].chip else {
        panic!("Expected BleBeacon chip in ListDevice response");
    };
    assert_eq!(b.settings.dbm(), -12);
    assert_eq!(b.settings.timeout, 500);
    assert!(b.adv_data.include_tx_power_level);
    assert_eq!(b.adv_data.manufacturer_data, vec![0xAA, 0xBB]);
}

// Scenario: Radio State Propagation
//   Given a running Netsim Daemon
//   When I create a device with a Bluetooth chip
//   Then the initial radio state is TRUE (default)
//   When I patch the device to turn the radio OFF
//   Then the radio state becomes FALSE
#[tokio::test]
async fn test_radio_state_propagation() {
    // Given a running Netsim Daemon
    let mut world = World::new().await;
    world.when_spawn_daemon().await;

    // When I create a device with a Bluetooth chip
    let bt_chip = World::make_bluetooth_chip("bt0", "00:11:22:33:44:55");
    let device_id = world.when_create_device_with_chips("Radio-Test-Device", vec![bt_chip]).await;

    // Resolve Chip ID
    let devices = world.when_list_devices().await;
    let device = devices.iter().find(|d| d.id == device_id).expect("Device missing");
    let chip_id = device.chips[0].id;

    // Then the initial radio state is TRUE (default)
    world.then_radio_state_is(device_id, ChipKind::BLUETOOTH, true).await;

    // When I patch the device to turn the radio OFF
    world.when_patch_state(device_id, Some(chip_id), ChipKind::BLUETOOTH, false).await;

    // Then the radio state becomes FALSE
    world.then_radio_state_is(device_id, ChipKind::BLUETOOTH, false).await;
}

#[tokio::test]
async fn test_chip_update_resolution_by_variant() {
    let mut world = World::new().await;
    world.when_spawn_daemon().await;

    // 1. Create Device with multiple chips
    let bt_chip = World::make_bluetooth_chip("bt-res", "00:11:22:33:44:55");
    let uwb_chip = World::make_uwb_chip("uwb-res");

    let device_id =
        world.when_create_device_with_chips("Resolution-By-Variant", vec![bt_chip, uwb_chip]).await;

    // 2. Patch Bluetooth Radio State *WITHOUT* Chip ID
    world.when_patch_state(device_id, None, ChipKind::BLUETOOTH, false).await;

    // 3. Verify
    world.then_radio_state_is(device_id, ChipKind::BLUETOOTH, false).await;
}

#[tokio::test]
async fn test_link_wiring_grpc() {
    let mut world = World::new().await;
    world.when_spawn_daemon().await;

    // 1. Create two devices with chips
    let (_dev1, chip1) = world
        .when_create_detailed_device(
            "device1",
            "chip1",
            ChipKind::BLUETOOTH,
            "11:11:11:11:11:11",
            true,
        )
        .await;
    let (_dev2, chip2) = world
        .when_create_detailed_device(
            "device2",
            "chip2",
            ChipKind::BLUETOOTH,
            "22:22:22:22:22:22",
            true,
        )
        .await;

    // 2. Create Link (CreateLink)
    let link_id = world.when_create_link(chip1, chip2, -50, ChipKind::BLUETOOTH).await;
    assert!(link_id > 0);

    // 3. Verify Link (ListLink)
    world.then_link_matches(link_id, chip1, chip2, -50, ChipKind::BLUETOOTH).await;

    // 4. Update Link (PatchLink)
    world.when_patch_link(link_id, -70, ChipKind::BLUETOOTH).await;

    // 5. Verify Update
    world.then_link_rssi_is(link_id, -70).await;

    // 6. Delete Link
    world.when_delete_link(link_id).await;

    // 7. Verify Deletion
    world.then_link_count_is(0).await;
}

// Scenario: Frontend API Stats Tracking
//   Given a running Netsim Daemon
//   When I perform frontend API actions
//   Then the session stats are correctly populated with counts
#[tokio::test]
async fn test_frontend_stats_tracking() {
    let mut args = daemon::Args::default();
    args.no_shutdown = false;
    // Set a short idle shutdown timeout (500ms) to rapidly trigger daemon teardown
    // after the last active client connection closes.
    args.idle_shutdown_timeout = Some(500);
    args.logtostderr = true;
    let mut world = World::new_with_args(args).await;
    world.when_spawn_daemon().await;

    // Perform API calls
    let _ = world.when_get_version().await;
    let device_id = world.when_create_device("stats_device", "chip1").await;
    let _devices = world.when_list_devices().await;

    // Patch device
    let mut patch_req = netsim_proto::frontend::PatchDeviceRequest::new();
    patch_req.id = Some(device_id);
    let mut patch_fields = netsim_proto::frontend::patch_device_request::PatchDeviceFields::new();
    patch_fields.name = Some("new_name".to_string());
    patch_req.device = netsim_proto::protobuf::MessageField::some(patch_fields);
    world.when_patch_device(&patch_req).await;

    world.when_delete_device(device_id).await;

    // Wait for the daemon to shut down via idle timeout, which flushes stats.
    // A 5000ms timeout provides a safe upper bound to ensure all asynchronous
    // teardown and IO operations (like writing the stats JSON) complete reliably.
    world.then_daemon_shutdown(5000).await;

    let stats_path = world.get_temp_dir().join("netsim_session_stats.json");

    // Verify stats file is written after shutdown
    let content = std::fs::read_to_string(&stats_path).expect("Failed to read stats JSON file");

    // Deserialize and assert on the strictly typed fields
    let json: serde_json::Value = serde_json::from_str(&content).expect("Failed to parse JSON");
    let frontend_stats = json.get("frontend_stats").expect("frontend_stats not found in JSON");

    assert!(
        frontend_stats.get("get_version").and_then(|v| v.as_u64()).unwrap_or(0) > 0,
        "get_version not tracked or is 0"
    );
    assert!(
        frontend_stats.get("create_device").and_then(|v| v.as_u64()).unwrap_or(0) > 0,
        "create_device not tracked or is 0"
    );
    assert!(
        frontend_stats.get("list_device").and_then(|v| v.as_u64()).unwrap_or(0) > 0,
        "list_device not tracked or is 0"
    );
    assert!(
        frontend_stats.get("patch_device").and_then(|v| v.as_u64()).unwrap_or(0) > 0,
        "patch_device not tracked or is 0"
    );
    assert!(
        frontend_stats.get("delete_device").and_then(|v| v.as_u64()).unwrap_or(0) > 0,
        "delete_device not tracked or is 0"
    );
}

// Scenario: gRPC PacketStreamer with Laird BL654 preset applies buffer size (b/573454032)
#[tokio::test]
async fn test_grpc_bt_properties_laird_bl654_preset_buffer_size() {
    let mut world = World::new().await;
    world.when_spawn_daemon().await;
    world.when_open_packet_stream().await;

    let mut bt_props = Controller::new();
    bt_props.set_preset(ControllerPreset::LAIRD_BL654);
    world.given_initial_info_sent_with_bt_properties("laird-chip", bt_props).await;

    // Send HCI_Reset ([0x03, 0x0c, 0x00])
    world.when_send_hci_packet(PacketType::COMMAND, vec![0x03, 0x0c, 0x00]).await;
    world
        .then_packet_stream_receives_hci(PacketType::EVENT, &[0x0e, 0x04, 0x01, 0x03, 0x0c, 0x00])
        .await;

    // Send HCI_LE_Read_Buffer_Size ([0x02, 0x20, 0x00])
    world.when_send_hci_packet(PacketType::COMMAND, vec![0x02, 0x20, 0x00]).await;
    // LAIRD_BL654: 512 bytes (0x0200) and 4 packets (0x04)
    world
        .then_packet_stream_receives_hci(
            PacketType::EVENT,
            &[0x0e, 0x07, 0x01, 0x02, 0x20, 0x00, 0x00, 0x02, 0x04],
        )
        .await;

    world.when_close_packet_stream().await;
}

// Scenario: gRPC PacketStreamer with default config buffer size
#[tokio::test]
async fn test_grpc_bt_properties_default_buffer_size() {
    let mut world = World::new().await;
    world.when_spawn_daemon().await;
    world.when_open_packet_stream().await;

    let bt_props = Controller::new();
    world.given_initial_info_sent_with_bt_properties("default-chip", bt_props).await;

    // Send HCI_Reset ([0x03, 0x0c, 0x00])
    world.when_send_hci_packet(PacketType::COMMAND, vec![0x03, 0x0c, 0x00]).await;
    world
        .then_packet_stream_receives_hci(PacketType::EVENT, &[0x0e, 0x04, 0x01, 0x03, 0x0c, 0x00])
        .await;

    // Send HCI_LE_Read_Buffer_Size ([0x02, 0x20, 0x00])
    world.when_send_hci_packet(PacketType::COMMAND, vec![0x02, 0x20, 0x00]).await;
    // DEFAULT: 27 bytes (0x001b) and 20 packets (0x14)
    world
        .then_packet_stream_receives_hci(
            PacketType::EVENT,
            &[0x0e, 0x07, 0x01, 0x02, 0x20, 0x00, 0x1b, 0x00, 0x14],
        )
        .await;

    world.when_close_packet_stream().await;
}

// Scenario: gRPC PacketStreamer disabling CIS feature clears feature bits
#[tokio::test]
async fn test_grpc_bt_properties_disable_cis_feature() {
    let mut world = World::new().await;
    world.when_spawn_daemon().await;
    world.when_open_packet_stream().await;

    let mut features = ControllerFeatures::new();
    features.le_connected_isochronous_stream = Some(false);
    let mut bt_props = Controller::new();
    bt_props.features = MessageField::some(features);
    world.given_initial_info_sent_with_bt_properties("no-cis-chip", bt_props).await;

    // Send HCI_Reset ([0x03, 0x0c, 0x00])
    world.when_send_hci_packet(PacketType::COMMAND, vec![0x03, 0x0c, 0x00]).await;
    world
        .then_packet_stream_receives_hci(PacketType::EVENT, &[0x0e, 0x04, 0x01, 0x03, 0x0c, 0x00])
        .await;

    // Send HCI_LE_Read_Local_Supported_Features ([0x03, 0x20, 0x00])
    world.when_send_hci_packet(PacketType::COMMAND, vec![0x03, 0x20, 0x00]).await;
    let event = world.then_packet_stream_receives_hci_event().await;
    // Command complete event for 0x2003: [0x0e, 0x0c, 0x01, 0x03, 0x20, 0x00, f0, f1, f2, f3, ...]
    assert_eq!(event.len(), 14);
    assert_eq!(&event[0..6], &[0x0e, 0x0c, 0x01, 0x03, 0x20, 0x00]);
    // Feature byte 3 (packet index 9) has CIS bits (0x30) cleared:
    assert_eq!(event[9] & 0x30, 0x00, "CIS feature bits should be cleared");

    world.when_close_packet_stream().await;
}
