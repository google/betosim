// Copyright 2022 The Android Open Source Project
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
use crate::args::{
    Beacon, BeaconCreate, BeaconPatch, Capture, Command, Link, LinkDelete, LinkPatch, OnOffState,
    RadioType, UpDownStatus,
};
use crate::grpc_client::{self, GrpcRequest, GrpcResponse};
use log::error;
use netsim_common::util::time_display::TimeDisplay;
use netsim_proto::common::ChipKind;
use netsim_proto::frontend;
use netsim_proto::frontend::patch_capture_request::PatchCapture as PatchCaptureProto;
use netsim_proto::frontend::patch_device_request::PatchDeviceFields as PatchDeviceFieldsProto;
use netsim_proto::frontend_grpc::FrontendServiceClient;
use netsim_proto::model::chip::{
    BleBeacon as Chip_Ble_Beacon, Bluetooth as Chip_Bluetooth, Chip as Chip_Type,
    Radio as Chip_Radio,
};
use netsim_proto::model::{
    self, Chip, ChipCreate as ChipCreateProto, DeviceCreate as DeviceCreateProto,
    PhyKind as PhyKindProto, Position, chip_create,
};
use protobuf::MessageField;

fn radio_type_to_proto_phy_kind(radio_type: RadioType) -> PhyKindProto {
    match radio_type {
        RadioType::Ble => PhyKindProto::BLUETOOTH_LOW_ENERGY,
        RadioType::Classic => PhyKindProto::BLUETOOTH_CLASSIC,
        RadioType::Wifi => PhyKindProto::WIFI,
        RadioType::Uwb => PhyKindProto::UWB,
    }
}

impl Command {
    /// Return the generated request protobuf message
    /// The parsed command parameters are used to construct the request protobuf
    pub fn get_request(&self) -> GrpcRequest {
        match self {
            Command::Version => GrpcRequest::GetVersion,
            Command::Radio(cmd) => {
                let mut chip = Chip { ..Default::default() };
                let chip_state = match cmd.status {
                    UpDownStatus::Up => true,
                    UpDownStatus::Down => false,
                };
                if cmd.radio_type == RadioType::Wifi {
                    let mut wifi_chip = Chip_Radio::new();
                    wifi_chip.state = chip_state.into();
                    chip.set_wifi(wifi_chip);
                    chip.kind = ChipKind::WIFI.into();
                } else if cmd.radio_type == RadioType::Uwb {
                    let mut uwb_chip = Chip_Radio::new();
                    uwb_chip.state = chip_state.into();
                    chip.set_uwb(uwb_chip);
                    chip.kind = ChipKind::UWB.into();
                } else {
                    let mut bt_chip = Chip_Bluetooth::new();
                    let mut bt_chip_radio = Chip_Radio::new();
                    bt_chip_radio.state = chip_state.into();
                    if cmd.radio_type == RadioType::Ble {
                        bt_chip.low_energy = Some(bt_chip_radio).into();
                    } else {
                        bt_chip.classic = Some(bt_chip_radio).into();
                    }
                    chip.kind = ChipKind::BLUETOOTH.into();
                    chip.set_bt(bt_chip);
                }
                let mut result = frontend::PatchDeviceRequest::new();
                let mut device = PatchDeviceFieldsProto::new();
                device.name = Some(cmd.name.clone());
                device.chips.push(chip);
                result.device = Some(device).into();
                GrpcRequest::PatchDevice(result)
            }
            Command::Move(cmd) => {
                let mut result = frontend::PatchDeviceRequest::new();
                let mut device = PatchDeviceFieldsProto::new();
                let position = Position {
                    x: cmd.x,
                    y: cmd.y,
                    z: cmd.z.unwrap_or_default(),
                    ..Default::default()
                };
                device.name = Some(cmd.name.clone());
                device.position = Some(position).into();
                result.device = Some(device).into();
                GrpcRequest::PatchDevice(result)
            }
            Command::Devices(_) => GrpcRequest::ListDevice,
            Command::Reset => GrpcRequest::Reset,
            Command::Gui => {
                unimplemented!("get_request is not implemented for Gui Command.");
            }
            Command::Capture(cmd) => match cmd {
                Capture::List(_) => GrpcRequest::ListCapture,
                Capture::Get(_) => {
                    unimplemented!(
                        "get_request not implemented for Capture Get command. Use get_requests instead."
                    )
                }
                Capture::Patch(_) => {
                    unimplemented!(
                        "get_request not implemented for Capture Patch command. Use get_requests instead."
                    )
                }
            },
            Command::Artifact => {
                unimplemented!("get_request is not implemented for Artifact Command.");
            }
            Command::Beacon(action) => match action {
                Beacon::Create(kind) => match kind {
                    BeaconCreate::Ble(args) => {
                        let device = MessageField::some(DeviceCreateProto {
                            name: args.device_name.clone().unwrap_or_default(),
                            chips: vec![ChipCreateProto {
                                name: args.chip_name.clone().unwrap_or_default(),
                                kind: ChipKind::BLUETOOTH_BEACON.into(),
                                chip: Some(chip_create::Chip::BleBeacon(
                                    chip_create::BleBeaconCreate {
                                        address: args.address.clone().unwrap_or_default(),
                                        settings: MessageField::some((&args.settings).into()),
                                        adv_data: MessageField::some((&args.advertise_data).into()),
                                        scan_response: MessageField::some(
                                            (&args.scan_response_data).into(),
                                        ),
                                        ..Default::default()
                                    },
                                )),
                                ..Default::default()
                            }],
                            ..Default::default()
                        });

                        let result = frontend::CreateDeviceRequest { device, ..Default::default() };
                        GrpcRequest::CreateDevice(result)
                    }
                },
                Beacon::Patch(kind) => match kind {
                    BeaconPatch::Ble(args) => {
                        let device = MessageField::some(PatchDeviceFieldsProto {
                            name: Some(args.device_name.clone()),
                            chips: vec![Chip {
                                name: args.chip_name.clone(),
                                kind: ChipKind::BLUETOOTH_BEACON.into(),
                                chip: Some(Chip_Type::BleBeacon(Chip_Ble_Beacon {
                                    bt: MessageField::some(Chip_Bluetooth::new()),
                                    address: args.address.clone().unwrap_or_default(),
                                    settings: MessageField::some((&args.settings).into()),
                                    adv_data: MessageField::some((&args.advertise_data).into()),
                                    scan_response: MessageField::some(
                                        (&args.scan_response_data).into(),
                                    ),
                                    ..Default::default()
                                })),
                                ..Default::default()
                            }],
                            ..Default::default()
                        });

                        let result = frontend::PatchDeviceRequest { device, ..Default::default() };
                        GrpcRequest::PatchDevice(result)
                    }
                },
                Beacon::Remove(_) => {
                    // Placeholder - actual DeleteChipRequest will be constructed later
                    GrpcRequest::DeleteChip(frontend::DeleteChipRequest { ..Default::default() })
                }
            },
            Command::Bumble => {
                unimplemented!("get_request is not implemented for Bumble Command.");
            }
            Command::Link(link_cmd) => match link_cmd {
                Link::List => GrpcRequest::ListLink,
                Link::Patch(patch_struct) => match &patch_struct.command {
                    LinkPatch::Rssi(args) => {
                        let link = model::Link {
                            sender_id: args.sender_id.unwrap_or(0),
                            receiver_id: args.receiver_id.unwrap_or(0),
                            link_kind: radio_type_to_proto_phy_kind(args.radio_type).into(),
                            rssi: args.value as i32,
                            ..Default::default()
                        };
                        let request = frontend::PatchLinkRequest {
                            link: MessageField::some(link),
                            ..Default::default()
                        };
                        GrpcRequest::PatchLink(request)
                    }
                },
                Link::Delete(delete_struct) => match &delete_struct.command {
                    LinkDelete::Rssi(args) => {
                        let link = model::Link {
                            sender_id: args.sender_id.unwrap_or(0),
                            receiver_id: args.receiver_id.unwrap_or(0),
                            link_kind: radio_type_to_proto_phy_kind(args.radio_type).into(),
                            ..Default::default()
                        };
                        let request = frontend::DeleteLinkRequest {
                            link: MessageField::some(link),
                            ..Default::default()
                        };
                        GrpcRequest::DeleteLink(request)
                    }
                },
            },
        }
    }

    /// Create and return the request protobuf(s) for the command.
    /// In the case of a command with pattern argument(s) there may be multiple gRPC requests.
    /// The parsed command parameters are used to construct the request protobuf.
    /// The client is used to send gRPC call(s) to retrieve information needed for request protobufs.
    pub fn get_requests(&mut self, client: &FrontendServiceClient) -> Vec<GrpcRequest> {
        match self {
            Command::Capture(Capture::Patch(cmd)) => {
                let mut reqs = Vec::new();
                let filtered_captures = Self::get_filtered_captures(client, &cmd.patterns);
                // Create a request for each capture
                for capture in &filtered_captures {
                    let mut result = frontend::PatchCaptureRequest::new();
                    result.id = capture.id;
                    let capture_state = match cmd.state {
                        OnOffState::On => true,
                        OnOffState::Off => false,
                    };
                    let mut patch_capture = PatchCaptureProto::new();
                    patch_capture.state = capture_state.into();
                    result.patch = Some(patch_capture).into();
                    reqs.push(GrpcRequest::PatchCapture(result))
                }
                reqs
            }
            Command::Capture(Capture::Get(cmd)) => {
                let mut reqs = Vec::new();
                let filtered_captures = Self::get_filtered_captures(client, &cmd.patterns);
                // Create a request for each capture
                for capture in &filtered_captures {
                    let mut result = frontend::GetCaptureRequest::new();
                    result.id = capture.id;
                    reqs.push(GrpcRequest::GetCapture(result));
                    let time_display = TimeDisplay::new(
                        capture.timestamp.get_or_default().seconds,
                        capture.timestamp.get_or_default().nanos as u32,
                    );
                    let file_extension = "pcap";
                    cmd.filenames.push(format!(
                        "netsim-{:?}-{}-{}-{}.{}",
                        capture.id,
                        capture.device_name.to_owned().replace(' ', "_"),
                        Self::chip_kind_to_string(capture.chip_kind.enum_value_or_default()),
                        time_display.utc_display(),
                        file_extension
                    ));
                }
                reqs
            }
            _ => {
                unimplemented!(
                    "get_requests not implemented for this command. Use get_request instead."
                )
            }
        }
    }

    fn get_filtered_captures(
        client: &FrontendServiceClient,
        patterns: &[String],
    ) -> Vec<model::Capture> {
        // Get list of captures, with explicit type annotation for send_grpc
        let mut result = match grpc_client::send_grpc(client, &GrpcRequest::ListCapture) {
            Ok(GrpcResponse::ListCapture(response)) => response.captures,
            Ok(grpc_response) => {
                error!("Unexpected GrpcResponse: {grpc_response:?}");
                return Vec::new();
            }
            Err(err) => {
                error!("ListCapture Grpc call error: {err}");
                return Vec::new();
            }
        };

        // Filter captures if patterns are provided
        if !patterns.is_empty() {
            Self::filter_captures(&mut result, patterns);
        }

        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::args::{
        AdvertiseMode, BeaconBleAdvertiseData, BeaconBleScanResponseData, BeaconBleSettings,
        BeaconCreateBle, BeaconPatchBle, Command, Devices, Interval, Link, LinkDelete,
        LinkDeleteCommand, LinkPatch, LinkPatchCommand, ListCapture, Move, NetsimArgs,
        ParsableBytes, Radio, RadioType, RssiDelete, RssiPatch, TxPower, TxPowerLevel,
    };

    use clap::Parser;
    use netsim_proto::{
        common::ChipKind,
        frontend::{
            CreateDeviceRequest, PatchDeviceRequest,
            patch_device_request::PatchDeviceFields as PatchDeviceFieldsProto,
        },
        model::{
            self, Chip as ChipProto, ChipCreate as ChipCreateProto,
            DeviceCreate as DeviceCreateProto, PhyKind as PhyKindProto, Position,
            chip::{
                BleBeacon as BleBeaconProto, Bluetooth as Chip_Bluetooth, Chip as ChipKindProto,
                Radio as Chip_Radio,
                ble_beacon::{
                    AdvertiseData as AdvertiseDataProto,
                    AdvertiseSettings as AdvertiseSettingsProto,
                    advertise_settings::{
                        AdvertiseMode as AdvertiseModeProto,
                        AdvertiseTxPower as AdvertiseTxPowerProto, Interval as IntervalProto,
                        Tx_power as TxPowerProto,
                    },
                },
            },
            chip_create::{BleBeaconCreate as BleBeaconCreateProto, Chip as ChipKindCreateProto},
        },
    };
    use protobuf::MessageField;

    // Helper to test parsing text command into expected Command and GrpcRequest
    fn test_command(command: &str, expected_command: Command, expected_grpc_request: GrpcRequest) {
        let command = NetsimArgs::parse_from(command.split_whitespace()).command;
        assert_eq!(command, expected_command);
        let request = command.get_request();
        assert_eq!(request, expected_grpc_request);
    }

    #[test]
    fn test_version_request() {
        test_command("netsim-cli version", Command::Version, GrpcRequest::GetVersion)
    }

    fn get_expected_radio(
        name: &str,
        radio_type: &str,
        state: &str,
    ) -> frontend::PatchDeviceRequest {
        let mut chip = model::Chip { ..Default::default() };
        let chip_state = state == "up";
        if radio_type == "wifi" {
            let mut wifi_chip = Chip_Radio::new();
            wifi_chip.state = chip_state.into();
            chip.set_wifi(wifi_chip);
            chip.kind = ChipKind::WIFI.into();
        } else if radio_type == "uwb" {
            let mut uwb_chip = Chip_Radio::new();
            uwb_chip.state = chip_state.into();
            chip.set_uwb(uwb_chip);
            chip.kind = ChipKind::UWB.into();
        } else {
            let mut bt_chip = Chip_Bluetooth::new();
            let mut bt_chip_radio = Chip_Radio::new();
            bt_chip_radio.state = chip_state.into();
            if radio_type == "ble" {
                bt_chip.low_energy = Some(bt_chip_radio).into();
            } else {
                bt_chip.classic = Some(bt_chip_radio).into();
            }
            chip.kind = ChipKind::BLUETOOTH.into();
            chip.set_bt(bt_chip);
        }
        let mut result = frontend::PatchDeviceRequest::new();
        let mut device = PatchDeviceFieldsProto::new();
        device.name = Some(name.to_string());
        device.chips.push(chip);
        result.device = Some(device).into();
        result
    }

    #[test]
    fn test_radio_ble() {
        test_command(
            "netsim-cli radio ble down 1000",
            Command::Radio(Radio {
                radio_type: RadioType::Ble,
                status: UpDownStatus::Down,
                name: "1000".to_string(),
            }),
            GrpcRequest::PatchDevice(get_expected_radio("1000", "ble", "down")),
        );
        test_command(
            "netsim-cli radio ble up 1000",
            Command::Radio(Radio {
                radio_type: RadioType::Ble,
                status: UpDownStatus::Up,
                name: "1000".to_string(),
            }),
            GrpcRequest::PatchDevice(get_expected_radio("1000", "ble", "up")),
        );
    }

    #[test]
    fn test_radio_ble_aliases() {
        test_command(
            "netsim-cli radio ble Down 1000",
            Command::Radio(Radio {
                radio_type: RadioType::Ble,
                status: UpDownStatus::Down,
                name: "1000".to_string(),
            }),
            GrpcRequest::PatchDevice(get_expected_radio("1000", "ble", "down")),
        );
        test_command(
            "netsim-cli radio ble Up 1000",
            Command::Radio(Radio {
                radio_type: RadioType::Ble,
                status: UpDownStatus::Up,
                name: "1000".to_string(),
            }),
            GrpcRequest::PatchDevice(get_expected_radio("1000", "ble", "up")),
        );
        test_command(
            "netsim-cli radio ble DOWN 1000",
            Command::Radio(Radio {
                radio_type: RadioType::Ble,
                status: UpDownStatus::Down,
                name: "1000".to_string(),
            }),
            GrpcRequest::PatchDevice(get_expected_radio("1000", "ble", "down")),
        );
        test_command(
            "netsim-cli radio ble UP 1000",
            Command::Radio(Radio {
                radio_type: RadioType::Ble,
                status: UpDownStatus::Up,
                name: "1000".to_string(),
            }),
            GrpcRequest::PatchDevice(get_expected_radio("1000", "ble", "up")),
        );
    }

    #[test]
    fn test_radio_classic() {
        test_command(
            "netsim-cli radio classic down 100",
            Command::Radio(Radio {
                radio_type: RadioType::Classic,
                status: UpDownStatus::Down,
                name: "100".to_string(),
            }),
            GrpcRequest::PatchDevice(get_expected_radio("100", "classic", "down")),
        );
        test_command(
            "netsim-cli radio classic up 100",
            Command::Radio(Radio {
                radio_type: RadioType::Classic,
                status: UpDownStatus::Up,
                name: "100".to_string(),
            }),
            GrpcRequest::PatchDevice(get_expected_radio("100", "classic", "up")),
        );
    }

    #[test]
    fn test_radio_wifi() {
        test_command(
            "netsim-cli radio wifi down a",
            Command::Radio(Radio {
                radio_type: RadioType::Wifi,
                status: UpDownStatus::Down,
                name: "a".to_string(),
            }),
            GrpcRequest::PatchDevice(get_expected_radio("a", "wifi", "down")),
        );
        test_command(
            "netsim-cli radio wifi up b",
            Command::Radio(Radio {
                radio_type: RadioType::Wifi,
                status: UpDownStatus::Up,
                name: "b".to_string(),
            }),
            GrpcRequest::PatchDevice(get_expected_radio("b", "wifi", "up")),
        );
    }

    #[test]
    fn test_radio_uwb() {
        test_command(
            "netsim-cli radio uwb down a",
            Command::Radio(Radio {
                radio_type: RadioType::Uwb,
                status: UpDownStatus::Down,
                name: "a".to_string(),
            }),
            GrpcRequest::PatchDevice(get_expected_radio("a", "uwb", "down")),
        );
        test_command(
            "netsim-cli radio uwb up b",
            Command::Radio(Radio {
                radio_type: RadioType::Uwb,
                status: UpDownStatus::Up,
                name: "b".to_string(),
            }),
            GrpcRequest::PatchDevice(get_expected_radio("b", "uwb", "up")),
        );
    }

    fn get_expected_move(
        name: &str,
        x: f32,
        y: f32,
        z: Option<f32>,
    ) -> frontend::PatchDeviceRequest {
        let mut result = frontend::PatchDeviceRequest::new();
        let mut device = PatchDeviceFieldsProto::new();
        let position = Position { x, y, z: z.unwrap_or_default(), ..Default::default() };
        device.name = Some(name.to_string());
        device.position = Some(position).into();
        result.device = Some(device).into();
        result
    }

    #[test]
    fn test_move_int() {
        test_command(
            "netsim-cli move 1 1 2 3",
            Command::Move(Move { name: "1".to_string(), x: 1.0, y: 2.0, z: Some(3.0) }),
            GrpcRequest::PatchDevice(get_expected_move("1", 1.0, 2.0, Some(3.0))),
        )
    }

    #[test]
    fn test_move_float() {
        test_command(
            "netsim-cli move 1000 1.2 3.4 5.6",
            Command::Move(Move { name: "1000".to_string(), x: 1.2, y: 3.4, z: Some(5.6) }),
            GrpcRequest::PatchDevice(get_expected_move("1000", 1.2, 3.4, Some(5.6))),
        )
    }

    #[test]
    fn test_move_mixed() {
        test_command(
            "netsim-cli move 1000 1.1 2 3.4",
            Command::Move(Move { name: "1000".to_string(), x: 1.1, y: 2.0, z: Some(3.4) }),
            GrpcRequest::PatchDevice(get_expected_move("1000", 1.1, 2.0, Some(3.4))),
        )
    }

    #[test]
    fn test_move_no_z() {
        test_command(
            "netsim-cli move 1000 1.2 3.4",
            Command::Move(Move { name: "1000".to_string(), x: 1.2, y: 3.4, z: None }),
            GrpcRequest::PatchDevice(get_expected_move("1000", 1.2, 3.4, None)),
        )
    }

    #[test]
    fn test_devices() {
        test_command(
            "netsim-cli devices",
            Command::Devices(Devices { continuous: false }),
            GrpcRequest::ListDevice,
        )
    }

    #[test]
    fn test_reset() {
        test_command("netsim-cli reset", Command::Reset, GrpcRequest::Reset)
    }

    #[test]
    fn test_capture_list() {
        test_command(
            "netsim-cli capture list",
            Command::Capture(Capture::List(ListCapture { ..Default::default() })),
            GrpcRequest::ListCapture,
        )
    }

    #[test]
    fn test_capture_list_alias() {
        test_command(
            "netsim-cli pcap list",
            Command::Capture(Capture::List(ListCapture { ..Default::default() })),
            GrpcRequest::ListCapture,
        )
    }

    //TODO: Add capture patch and get tests

    fn get_create_device_req(
        device_name: &str,
        chip_name: &str,
        settings: AdvertiseSettingsProto,
        adv_data: AdvertiseDataProto,
        scan_response: AdvertiseDataProto,
    ) -> CreateDeviceRequest {
        let device = MessageField::some(DeviceCreateProto {
            name: String::from(device_name),
            chips: vec![ChipCreateProto {
                name: String::from(chip_name),
                kind: ChipKind::BLUETOOTH_BEACON.into(),
                chip: Some(ChipKindCreateProto::BleBeacon(BleBeaconCreateProto {
                    settings: MessageField::some(settings),
                    adv_data: MessageField::some(adv_data),
                    scan_response: MessageField::some(scan_response),
                    ..Default::default()
                })),
                ..Default::default()
            }],
            ..Default::default()
        });

        CreateDeviceRequest { device, ..Default::default() }
    }

    fn get_patch_device_req(
        device_name: &str,
        chip_name: &str,
        settings: AdvertiseSettingsProto,
        adv_data: AdvertiseDataProto,
        scan_response: AdvertiseDataProto,
    ) -> PatchDeviceRequest {
        let device = MessageField::some(PatchDeviceFieldsProto {
            name: Some(String::from(device_name)),
            chips: vec![ChipProto {
                name: String::from(chip_name),
                kind: ChipKind::BLUETOOTH_BEACON.into(),
                chip: Some(ChipKindProto::BleBeacon(BleBeaconProto {
                    bt: MessageField::some(Chip_Bluetooth::new()),
                    settings: MessageField::some(settings),
                    adv_data: MessageField::some(adv_data),
                    scan_response: MessageField::some(scan_response),
                    ..Default::default()
                })),
                ..Default::default()
            }],
            ..Default::default()
        });

        PatchDeviceRequest { device, ..Default::default() }
    }

    #[test]
    fn test_beacon_create_all_params_ble() {
        let device_name = String::from("device");
        let chip_name = String::from("chip");

        let timeout = 1234;
        let manufacturer_data = vec![0x12, 0x34];

        let settings = AdvertiseSettingsProto {
            interval: Some(IntervalProto::AdvertiseMode(AdvertiseModeProto::BALANCED.into())),
            tx_power: Some(TxPowerProto::TxPowerLevel(AdvertiseTxPowerProto::ULTRA_LOW.into())),
            scannable: true,
            timeout,
            ..Default::default()
        };

        let adv_data = AdvertiseDataProto {
            include_device_name: true,
            include_tx_power_level: true,
            manufacturer_data: manufacturer_data.clone(),
            ..Default::default()
        };

        let request =
            get_create_device_req(&device_name, &chip_name, settings, adv_data, Default::default());

        let command = Command::Beacon(Beacon::Create(BeaconCreate::Ble(BeaconCreateBle {
            device_name: Some(device_name.clone()),
            chip_name: Some(chip_name.clone()),
            address: None,
            settings: BeaconBleSettings {
                advertise_mode: Some(Interval::Mode(AdvertiseMode::Balanced)),
                tx_power_level: Some(TxPower::Level(TxPowerLevel::UltraLow)),
                scannable: true,
                timeout: Some(1234),
            },
            advertise_data: BeaconBleAdvertiseData {
                include_device_name: true,
                include_tx_power_level: true,
                manufacturer_data: Some(ParsableBytes(manufacturer_data.clone())),
            },
            scan_response_data: BeaconBleScanResponseData { ..Default::default() },
        })));

        test_command(
            format!(
                "netsim-cli beacon create ble {device_name} {chip_name} --advertise-mode balanced --tx-power-level ultra-low --scannable --timeout {timeout} --include-device-name --include-tx-power-level --manufacturer-data 0x1234",
            )
            .as_str(),
            command,
            GrpcRequest::CreateDevice(request),
        )
    }

    #[test]
    fn test_beacon_patch_all_params_ble() {
        let device_name = String::from("device");
        let chip_name = String::from("chip");

        let interval = 1234;
        let timeout = 9999;
        let tx_power_level = -3;
        let manufacturer_data = vec![0xab, 0xcd, 0xef];

        let settings = AdvertiseSettingsProto {
            interval: Some(IntervalProto::Milliseconds(interval)),
            tx_power: Some(TxPowerProto::Dbm(tx_power_level)),
            scannable: true,
            timeout,
            ..Default::default()
        };
        let adv_data = AdvertiseDataProto {
            include_device_name: true,
            include_tx_power_level: true,
            manufacturer_data: manufacturer_data.clone(),
            ..Default::default()
        };

        let request =
            get_patch_device_req(&device_name, &chip_name, settings, adv_data, Default::default());

        let command = Command::Beacon(Beacon::Patch(BeaconPatch::Ble(BeaconPatchBle {
            device_name: device_name.clone(),
            chip_name: chip_name.clone(),
            address: None,
            settings: BeaconBleSettings {
                advertise_mode: Some(Interval::Milliseconds(interval)),
                tx_power_level: Some(TxPower::Dbm(tx_power_level as i8)),
                scannable: true,
                timeout: Some(timeout),
            },
            advertise_data: BeaconBleAdvertiseData {
                include_device_name: true,
                include_tx_power_level: true,
                manufacturer_data: Some(ParsableBytes(manufacturer_data)),
            },
            scan_response_data: BeaconBleScanResponseData { ..Default::default() },
        })));

        test_command(
            format!(
                "netsim-cli beacon patch ble {device_name} {chip_name} --advertise-mode {interval} --scannable --timeout {timeout} --tx-power-level {tx_power_level} --manufacturer-data 0xabcdef --include-device-name --include-tx-power-level"
            )
            .as_str(),
            command,
            GrpcRequest::PatchDevice(request),
        )
    }

    #[test]
    fn test_beacon_create_scan_response() {
        let device_name = String::from("device");
        let chip_name = String::from("chip");
        let manufacturer_data = vec![0x21, 0xbe, 0xef];

        let scan_response = AdvertiseDataProto {
            include_device_name: true,
            include_tx_power_level: true,
            manufacturer_data: manufacturer_data.clone(),
            ..Default::default()
        };

        let request = get_create_device_req(
            &device_name,
            &chip_name,
            Default::default(),
            Default::default(),
            scan_response,
        );

        let command = Command::Beacon(Beacon::Create(BeaconCreate::Ble(BeaconCreateBle {
            device_name: Some(device_name.clone()),
            chip_name: Some(chip_name.clone()),
            scan_response_data: BeaconBleScanResponseData {
                scan_response_include_device_name: true,
                scan_response_include_tx_power_level: true,
                scan_response_manufacturer_data: Some(ParsableBytes(manufacturer_data)),
            },
            ..Default::default()
        })));

        test_command(
            format!(
                "netsim-cli beacon create ble {device_name} {chip_name} --scan-response-include-device-name --scan-response-include-tx-power-level --scan-response-manufacturer-data 0x21beef"
            )
            .as_str(),
            command,
            GrpcRequest::CreateDevice(request),
        );
    }

    #[test]
    fn test_beacon_patch_scan_response() {
        let device_name = String::from("device");
        let chip_name = String::from("chip");
        let manufacturer_data = vec![0x59, 0xbe, 0xac, 0x09];

        let scan_response = AdvertiseDataProto {
            include_device_name: true,
            include_tx_power_level: true,
            manufacturer_data: manufacturer_data.clone(),
            ..Default::default()
        };

        let request = get_patch_device_req(
            &device_name,
            &chip_name,
            Default::default(),
            Default::default(),
            scan_response,
        );

        let command = Command::Beacon(Beacon::Patch(BeaconPatch::Ble(BeaconPatchBle {
            device_name: device_name.clone(),
            chip_name: chip_name.clone(),
            address: None,
            settings: BeaconBleSettings { ..Default::default() },
            advertise_data: BeaconBleAdvertiseData { ..Default::default() },
            scan_response_data: BeaconBleScanResponseData {
                scan_response_include_device_name: true,
                scan_response_include_tx_power_level: true,
                scan_response_manufacturer_data: Some(ParsableBytes(manufacturer_data)),
            },
        })));

        test_command(
            format!(
                "netsim-cli beacon patch ble {device_name} {chip_name} --scan-response-include-device-name --scan-response-include-tx-power-level --scan-response-manufacturer-data 59beac09"
            )
            .as_str(),
            command,
            GrpcRequest::PatchDevice(request),
        );
    }

    #[test]
    fn test_beacon_create_ble_tx_power() {
        let device_name = String::from("device");
        let chip_name = String::from("chip");

        let settings = AdvertiseSettingsProto {
            tx_power: Some(TxPowerProto::TxPowerLevel(AdvertiseTxPowerProto::HIGH.into())),
            ..Default::default()
        };
        let adv_data = AdvertiseDataProto { include_tx_power_level: true, ..Default::default() };

        let request =
            get_create_device_req(&device_name, &chip_name, settings, adv_data, Default::default());

        let command = Command::Beacon(Beacon::Create(BeaconCreate::Ble(BeaconCreateBle {
            device_name: Some(device_name.clone()),
            chip_name: Some(chip_name.clone()),
            address: None,
            settings: BeaconBleSettings {
                tx_power_level: Some(TxPower::Level(TxPowerLevel::High)),
                ..Default::default()
            },
            advertise_data: BeaconBleAdvertiseData {
                include_tx_power_level: true,
                ..Default::default()
            },
            scan_response_data: BeaconBleScanResponseData { ..Default::default() },
        })));

        test_command(
            format!(
                "netsim-cli beacon create ble {device_name} {chip_name} --tx-power-level high --include-tx-power-level"
            )
            .as_str(),
            command,
            GrpcRequest::CreateDevice(request),
        )
    }

    #[test]
    fn test_beacon_create_default() {
        let request = get_create_device_req(
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
        );

        let command = Command::Beacon(Beacon::Create(BeaconCreate::Ble(BeaconCreateBle {
            ..Default::default()
        })));

        test_command("netsim-cli beacon create ble", command, GrpcRequest::CreateDevice(request))
    }

    #[test]
    fn test_beacon_patch_interval() {
        let device_name = String::from("device");
        let chip_name = String::from("chip");

        let settings = AdvertiseSettingsProto {
            interval: Some(IntervalProto::AdvertiseMode(AdvertiseModeProto::LOW_LATENCY.into())),
            ..Default::default()
        };

        let request = get_patch_device_req(
            &device_name,
            &chip_name,
            settings,
            Default::default(),
            Default::default(),
        );
        let command = Command::Beacon(Beacon::Patch(BeaconPatch::Ble(BeaconPatchBle {
            device_name: device_name.clone(),
            chip_name: chip_name.clone(),
            address: None,
            settings: BeaconBleSettings {
                advertise_mode: Some(Interval::Mode(AdvertiseMode::LowLatency)),
                ..Default::default()
            },
            ..Default::default()
        })));

        test_command(
            format!(
                "netsim-cli beacon patch ble {device_name} {chip_name} --advertise-mode low-latency"
            )
            .as_str(),
            command,
            GrpcRequest::PatchDevice(request),
        )
    }

    #[test]
    fn test_beacon_create_ble_with_address() {
        let address = String::from("12:34:56:78:9a:bc");

        let device = MessageField::some(DeviceCreateProto {
            chips: vec![ChipCreateProto {
                kind: ChipKind::BLUETOOTH_BEACON.into(),
                chip: Some(ChipKindCreateProto::BleBeacon(BleBeaconCreateProto {
                    address: address.clone(),
                    settings: MessageField::some(AdvertiseSettingsProto::default()),
                    adv_data: MessageField::some(AdvertiseDataProto::default()),
                    scan_response: MessageField::some(AdvertiseDataProto::default()),
                    ..Default::default()
                })),
                ..Default::default()
            }],
            ..Default::default()
        });

        let request = frontend::CreateDeviceRequest { device, ..Default::default() };
        let command = Command::Beacon(Beacon::Create(BeaconCreate::Ble(BeaconCreateBle {
            address: Some(address.clone()),
            ..Default::default()
        })));

        test_command(
            format!("netsim-cli beacon create ble --address {address}").as_str(),
            command,
            GrpcRequest::CreateDevice(request),
        )
    }

    #[test]
    fn test_beacon_patch_ble_with_address() {
        let address = String::from("12:34:56:78:9a:bc");
        let device_name = String::from("device");
        let chip_name = String::from("chip");

        let device = MessageField::some(PatchDeviceFieldsProto {
            name: Some(device_name.clone()),
            chips: vec![ChipProto {
                name: chip_name.clone(),
                kind: ChipKind::BLUETOOTH_BEACON.into(),
                chip: Some(ChipKindProto::BleBeacon(BleBeaconProto {
                    bt: MessageField::some(Chip_Bluetooth::new()),
                    address: address.clone(),
                    settings: MessageField::some(AdvertiseSettingsProto::default()),
                    adv_data: MessageField::some(AdvertiseDataProto::default()),
                    scan_response: MessageField::some(AdvertiseDataProto::default()),
                    ..Default::default()
                })),
                ..Default::default()
            }],
            ..Default::default()
        });

        let request = frontend::PatchDeviceRequest { device, ..Default::default() };

        let command = Command::Beacon(Beacon::Patch(BeaconPatch::Ble(BeaconPatchBle {
            device_name: device_name.clone(),
            chip_name: chip_name.clone(),
            address: Some(address.clone()),
            ..Default::default()
        })));

        test_command(
            format!("netsim-cli beacon patch ble {device_name} {chip_name} --address {address}")
                .as_str(),
            command,
            GrpcRequest::PatchDevice(request),
        )
    }

    #[test]
    fn test_beacon_negative_timeout_fails() {
        let command = String::from("netsim-cli beacon create ble --timeout -1234");
        assert!(NetsimArgs::try_parse_from(command.split_whitespace()).is_err());
    }

    #[test]
    fn test_create_beacon_large_tx_power_fails() {
        let command =
            format!("netsim-cli beacon create ble --tx-power-level {}", (i8::MAX as i32) + 1);
        assert!(NetsimArgs::try_parse_from(command.split_whitespace()).is_err());
    }

    #[test]
    fn test_create_beacon_unknown_mode_fails() {
        let command = String::from("netsim-cli beacon create ble --advertise-mode not-a-mode");
        assert!(NetsimArgs::try_parse_from(command.split_whitespace()).is_err());
    }

    #[test]
    fn test_patch_beacon_negative_timeout_fails() {
        let command = String::from("netsim-cli beacon patch ble --timeout -1234");
        assert!(NetsimArgs::try_parse_from(command.split_whitespace()).is_err());
    }

    #[test]
    fn test_patch_beacon_large_tx_power_fails() {
        let command =
            format!("netsim-cli beacon patch ble --tx-power-level {}", (i8::MAX as i32) + 1);
        assert!(NetsimArgs::try_parse_from(command.split_whitespace()).is_err());
    }

    #[test]
    fn test_patch_beacon_unknown_mode_fails() {
        let command = String::from("netsim-cli beacon patch ble --advertise-mode not-a-mode");
        assert!(NetsimArgs::try_parse_from(command.split_whitespace()).is_err());
    }

    #[test]
    fn test_create_beacon_mfg_data_fails() {
        let command = String::from("netsim-cli beacon create ble --manufacturer-data not-a-number");
        assert!(NetsimArgs::try_parse_from(command.split_whitespace()).is_err());
    }

    #[test]
    fn test_patch_beacon_mfg_data_fails() {
        let command = String::from("netsim-cli beacon patch ble --manufacturer-data not-a-number");
        assert!(NetsimArgs::try_parse_from(command.split_whitespace()).is_err());
    }

    #[test]
    fn test_link_list_request() {
        let command = Command::Link(Link::List);
        let grpc_request = command.get_request();
        assert_eq!(grpc_request, GrpcRequest::ListLink);
    }

    #[test]
    fn test_link_patch_rssi_request_full() {
        let command = Command::Link(Link::Patch(LinkPatchCommand {
            command: LinkPatch::Rssi(RssiPatch {
                radio_type: RadioType::Ble,
                value: -60,
                sender_id: Some(100),
                receiver_id: Some(200),
            }),
        }));
        let grpc_request = command.get_request();
        let expected_link = model::Link {
            sender_id: 100,
            receiver_id: 200,
            link_kind: PhyKindProto::BLUETOOTH_LOW_ENERGY.into(),
            rssi: -60,
            ..Default::default()
        };
        let expected_request = frontend::PatchLinkRequest {
            link: MessageField::some(expected_link),
            ..Default::default()
        };
        assert_eq!(grpc_request, GrpcRequest::PatchLink(expected_request));
    }

    #[test]
    fn test_link_patch_rssi_request_no_ids() {
        let command = Command::Link(Link::Patch(LinkPatchCommand {
            command: LinkPatch::Rssi(RssiPatch {
                radio_type: RadioType::Wifi,
                value: -70,
                sender_id: None,
                receiver_id: None,
            }),
        }));
        let grpc_request = command.get_request();
        let expected_link = model::Link {
            sender_id: 0,   // Default for None
            receiver_id: 0, // Default for None
            link_kind: PhyKindProto::WIFI.into(),
            rssi: -70,
            ..Default::default()
        };
        let expected_request = frontend::PatchLinkRequest {
            link: MessageField::some(expected_link),
            ..Default::default()
        };
        assert_eq!(grpc_request, GrpcRequest::PatchLink(expected_request));
    }

    #[test]
    fn test_link_delete_rssi_request_full() {
        let command = Command::Link(Link::Delete(LinkDeleteCommand {
            command: LinkDelete::Rssi(RssiDelete {
                radio_type: RadioType::Classic,
                sender_id: Some(10),
                receiver_id: Some(20),
            }),
        }));
        let grpc_request = command.get_request();
        let expected_link = model::Link {
            sender_id: 10,
            receiver_id: 20,
            link_kind: PhyKindProto::BLUETOOTH_CLASSIC.into(),
            ..Default::default() // RSSI is not part of delete request key
        };
        let expected_request = frontend::DeleteLinkRequest {
            link: MessageField::some(expected_link),
            ..Default::default()
        };
        assert_eq!(grpc_request, GrpcRequest::DeleteLink(expected_request));
    }

    #[test]
    fn test_link_delete_rssi_request_no_ids() {
        let command = Command::Link(Link::Delete(LinkDeleteCommand {
            command: LinkDelete::Rssi(RssiDelete {
                radio_type: RadioType::Ble,
                sender_id: None,
                receiver_id: None,
            }),
        }));
        let grpc_request = command.get_request();
        let expected_link = model::Link {
            sender_id: 0,   // Default for None
            receiver_id: 0, // Default for None
            link_kind: PhyKindProto::BLUETOOTH_LOW_ENERGY.into(),
            ..Default::default()
        };
        let expected_request = frontend::DeleteLinkRequest {
            link: MessageField::some(expected_link),
            ..Default::default()
        };
        assert_eq!(grpc_request, GrpcRequest::DeleteLink(expected_request));
    }
}
