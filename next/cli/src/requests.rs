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
use common::util::time_display::TimeDisplay;
use netsim_proto::{
    common::ChipKind,
    frontend,
    model::{
        self, ChipCreate as ChipCreateProto, DeviceCreate as DeviceCreateProto, Orientation,
        Position,
    },
};
use protobuf::MessageField;

use crate::{
    args::{
        Beacon, BeaconCreate, ChipKind as ArgsChipKind, Command, Link, RadioType, UpDownStatus,
    },
    error::{Error, Result},
    grpc_client::{GrpcMethodExecutor, GrpcRequest, GrpcResponse},
};

fn chip_kind_to_proto(chip_kind: ArgsChipKind) -> ChipKind {
    match chip_kind {
        ArgsChipKind::Bluetooth => ChipKind::BLUETOOTH,
        ArgsChipKind::Wifi => ChipKind::WIFI,
        ArgsChipKind::Uwb => ChipKind::UWB,
        ArgsChipKind::Nfc => ChipKind::NFC,
        ArgsChipKind::Cellular => ChipKind::CELLULAR,
        ArgsChipKind::CellularData => ChipKind::CELLULAR_DATA,
        ArgsChipKind::Ethernet => ChipKind::ETHERNET,
    }
}

pub(crate) fn radio_to_device_update(cmd: &crate::args::Radio) -> netsim_model::DeviceUpdate {
    let chip_state = match cmd.status {
        UpDownStatus::Up => true,
        UpDownStatus::Down => false,
    };
    let radio = netsim_model::RadioUpdate { state: Some(chip_state) };
    let variant = match cmd.radio_type {
        RadioType::Wifi => {
            netsim_model::ChipVariantUpdate::Wifi(netsim_model::WifiUpdate { radio })
        }
        RadioType::Uwb => netsim_model::ChipVariantUpdate::Uwb(netsim_model::UwbUpdate { radio }),
        RadioType::Nfc => netsim_model::ChipVariantUpdate::Nfc(netsim_model::NfcUpdate { radio }),
        RadioType::Ethernet => {
            netsim_model::ChipVariantUpdate::Ethernet(netsim_model::EthernetUpdate { radio })
        }
        RadioType::Cellular => {
            let state = if chip_state {
                netsim_model::MODEM_STATE_IDLE
            } else {
                netsim_model::MODEM_STATE_DOWN
            };
            netsim_model::ChipVariantUpdate::Cell(netsim_model::CellUpdate {
                radio,
                state: Some(state.to_string()),
                ..Default::default()
            })
        }
        RadioType::CellularData => {
            netsim_model::ChipVariantUpdate::CellularData(netsim_model::CellularDataUpdate {
                radio,
            })
        }
        RadioType::Ble => {
            netsim_model::ChipVariantUpdate::Bluetooth(netsim_model::BluetoothUpdate {
                low_energy: radio,
                ..Default::default()
            })
        }
        RadioType::Classic => {
            netsim_model::ChipVariantUpdate::Bluetooth(netsim_model::BluetoothUpdate {
                classic: radio,
                ..Default::default()
            })
        }
    };
    netsim_model::DeviceUpdate {
        chips: Some(vec![netsim_model::ChipUpdate {
            variant: Some(variant),
            ..Default::default()
        }]),
        ..Default::default()
    }
}

pub(crate) fn move_to_device_update(cmd: &crate::args::Move) -> netsim_model::DeviceUpdate {
    netsim_model::DeviceUpdate {
        pose: netsim_model::PoseUpdate {
            position: Some(netsim_model::Position {
                x: cmd.x,
                y: cmd.y,
                z: cmd.z.unwrap_or_default(),
            }),
            orientation: None,
        },
        ..Default::default()
    }
}

impl From<&crate::args::BeaconCreateBle> for netsim_model::DeviceCreate {
    fn from(args: &crate::args::BeaconCreateBle) -> Self {
        let mut ble_beacon = netsim_model::BleBeacon {
            address: args.address.clone().unwrap_or_default(),
            settings: Some((&args.settings).into()),
            adv_data: Some((&args.advertise_data).into()),
            scan_response: Some((&args.scan_response_data).into()),
        };
        ble_beacon.apply_create_defaults();
        netsim_model::DeviceCreate {
            device_config: netsim_model::DeviceConfig {
                name: args.device_name.clone().unwrap_or_default(),
                ..Default::default()
            },
            chip: netsim_model::DeviceChipCreate {
                name: args.chip_name.clone().unwrap_or_default(),
                manufacturer: String::new(),
                product_name: String::new(),
                chip: netsim_model::ChipCreateVariant::Beacon(ble_beacon),
            },
        }
    }
}

impl crate::args::BeaconPatchBle {
    pub(crate) fn to_device_update(
        &self,
        device_id: u32,
        chip_id: u32,
    ) -> netsim_model::DeviceUpdate {
        let settings: netsim_model::AdvertiseSettingsUpdate = (&self.settings).into();
        let adv_data: netsim_model::AdvertiseDataUpdate = (&self.advertise_data).into();
        let scan_response: netsim_model::AdvertiseDataUpdate = (&self.scan_response_data).into();
        let ble_beacon = netsim_model::BleBeaconUpdate {
            address: self.address.clone().unwrap_or_default(),
            settings: (!settings.is_empty()).then_some(settings),
            adv_data: (!adv_data.is_empty()).then_some(adv_data),
            scan_response: (!scan_response.is_empty()).then_some(scan_response),
        };
        netsim_model::DeviceUpdate {
            id: device_id,
            name: Some(self.device_name.clone()),
            chips: Some(vec![netsim_model::ChipUpdate {
                id: Some(netsim_model::ChipId(chip_id)),
                name: Some(self.chip_name.clone()),
                variant: Some(netsim_model::ChipVariantUpdate::Bluetooth(
                    netsim_model::BluetoothUpdate {
                        ble_beacon: Some(ble_beacon),
                        ..Default::default()
                    },
                )),
                ..Default::default()
            }]),
            ..Default::default()
        }
    }
}

pub(crate) fn capture_filename(capture: &netsim_rest_api::CaptureInfo) -> String {
    let time_display = TimeDisplay::new(capture.seconds, capture.nanos as u32);
    format!(
        "netsim-{}-{}-{}-{}.pcap",
        capture.chip_id.0,
        capture.device_name.replace(' ', "_"),
        capture.chip_kind,
        time_display.utc_display(),
    )
}

impl Command {
    /// Return the generated request protobuf message
    pub(crate) fn get_request(&self) -> Result<GrpcRequest> {
        Ok(match self {
            Command::Beacon(Beacon::Create(BeaconCreate::Uwb(args))) => {
                let position = Position {
                    x: args.x.unwrap_or_default(),
                    y: args.y.unwrap_or_default(),
                    z: args.z.unwrap_or_default(),
                    ..Default::default()
                };
                let orientation = Orientation {
                    yaw: args.yaw.unwrap_or_default(),
                    pitch: args.pitch.unwrap_or_default(),
                    roll: args.roll.unwrap_or_default(),
                    ..Default::default()
                };
                let device = MessageField::some(DeviceCreateProto {
                    name: args.device_name.clone().unwrap_or_default(),
                    position: MessageField::some(position),
                    orientation: MessageField::some(orientation),
                    chips: vec![ChipCreateProto {
                        name: args.chip_name.clone().unwrap_or_default(),
                        kind: ChipKind::UWB.into(),
                        address: args.address.clone().unwrap_or_default(),
                        ..Default::default()
                    }],
                    ..Default::default()
                });

                let result = frontend::CreateDeviceRequest { device, ..Default::default() };
                GrpcRequest::CreateDevice(result)
            }
            // These commands are intercepted early in main.rs and have no direct gRPC pipeline.
            _ => {
                return Err("get_request is not implemented for this command.".into());
            }
        })
    }

    /// Create and return the request protobuf(s) for the command.
    pub(crate) fn get_requests<T: GrpcMethodExecutor>(
        &self,
        client: &T,
    ) -> Result<Vec<GrpcRequest>> {
        match self {
            Command::Link(link_cmd) => Self::handle_link_command(client, link_cmd),
            _ => {
                Err("get_requests not implemented for this command. Use get_request instead."
                    .into())
            }
        }
    }

    fn handle_link_command<T: GrpcMethodExecutor>(
        client: &T,
        link_cmd: &Link,
    ) -> Result<Vec<GrpcRequest>> {
        match link_cmd {
            Link::List => {
                Err("Link::List uses the REST API and is handled directly in main.rs.".into())
            }
            Link::Create(cmd) => {
                let chip_kind = chip_kind_to_proto(cmd.chip_kind);
                let sender_ids = Self::resolve_chip_ids(
                    client,
                    cmd.sender,
                    cmd.sender_name.as_deref(),
                    chip_kind,
                )?;
                let receiver_ids = Self::resolve_chip_ids(
                    client,
                    cmd.receiver,
                    cmd.receiver_name.as_deref(),
                    chip_kind,
                )?;

                let reqs = sender_ids
                    .iter()
                    .flat_map(|sender_id| {
                        receiver_ids.iter().map(move |receiver_id| (*sender_id, *receiver_id))
                    })
                    .filter(|(sender_id, receiver_id)| sender_id != receiver_id)
                    .map(|(sender_id, receiver_id)| {
                        let link = model::Link {
                            sender_id,
                            receiver_id,
                            kind: chip_kind.into(),
                            rssi: cmd.rssi.unwrap_or(-20),
                            ..Default::default()
                        };
                        GrpcRequest::CreateLink(frontend::CreateLinkRequest {
                            link: MessageField::some(link),
                            ..Default::default()
                        })
                    })
                    .collect();
                Ok(reqs)
            }
            Link::Patch(args) => {
                if let Some(rssi) = args.rssi {
                    let chip_kind = chip_kind_to_proto(args.chip_kind);
                    let sender_ids = Self::resolve_chip_ids(
                        client,
                        args.sender,
                        args.sender_name.as_deref(),
                        chip_kind,
                    )?;
                    let receiver_ids = Self::resolve_chip_ids(
                        client,
                        args.receiver,
                        args.receiver_name.as_deref(),
                        chip_kind,
                    )?;

                    let links = Self::get_links(client, &sender_ids, &receiver_ids, chip_kind)?;

                    let reqs: Vec<GrpcRequest> = links
                        .into_iter()
                        .map(|link| {
                            let mut link_proto = model::Link::new();
                            link_proto.id = link.id;
                            link_proto.rssi = rssi as i32;
                            link_proto.kind = chip_kind.into();
                            // Populate sender/receiver for display purposes
                            link_proto.sender_id = link.sender_id;
                            link_proto.receiver_id = link.receiver_id;

                            GrpcRequest::PatchLink(frontend::PatchLinkRequest {
                                link: MessageField::some(link_proto),
                                id: link.id,
                                ..Default::default()
                            })
                        })
                        .collect();

                    if reqs.is_empty() {
                        return Err(Error::Message(
                            "No links found matching criteria.".to_string(),
                        ));
                    }
                    Ok(reqs)
                } else {
                    Ok(Vec::new())
                }
            }
            Link::Delete(args) => {
                let chip_kind = chip_kind_to_proto(args.chip_kind);
                let sender_ids = Self::resolve_chip_ids(
                    client,
                    args.sender,
                    args.sender_name.as_deref(),
                    chip_kind,
                )?;
                let receiver_ids = Self::resolve_chip_ids(
                    client,
                    args.receiver,
                    args.receiver_name.as_deref(),
                    chip_kind,
                )?;

                let links = Self::get_links(client, &sender_ids, &receiver_ids, chip_kind)?;

                let reqs: Vec<GrpcRequest> = links
                    .into_iter()
                    .map(|link| {
                        GrpcRequest::DeleteLink(frontend::DeleteLinkRequest {
                            id: link.id,
                            link: MessageField::some(link),
                            ..Default::default()
                        })
                    })
                    .collect();

                if reqs.is_empty() {
                    return Err(Error::Message("No links found matching criteria.".to_string()));
                }
                Ok(reqs)
            }
        }
    }

    /// Resolves chip IDs based on optional ID, name, or returns ALL chips of
    /// kind if both are missing (Wildcard).
    fn resolve_chip_ids<T: GrpcMethodExecutor>(
        client: &T,
        chip_id: Option<u32>,
        device_name: Option<&str>,
        chip_kind: ChipKind,
    ) -> Result<Vec<u32>> {
        if let Some(id) = chip_id
            && id != 0
        {
            return Ok(vec![id]);
        }

        // Fetch devices to resolve name or get all chips
        let mut resolved_ids = Vec::new();
        if let GrpcResponse::ListDevice(response) = client.send_grpc(&GrpcRequest::ListDevice)? {
            for device in response.devices {
                // Filter by device name if provided
                if let Some(dev_name) = device_name
                    && device.name != dev_name
                {
                    continue;
                }

                resolved_ids.extend(
                    device
                        .chips
                        .into_iter()
                        .filter(|chip| chip.kind == chip_kind.into())
                        .map(|chip| chip.id),
                );
            }
        }

        Ok(resolved_ids)
    }

    /// Get Links matching the set of sender and receiver IDs.
    fn get_links<T: GrpcMethodExecutor>(
        client: &T,
        sender_ids: &[u32],
        receiver_ids: &[u32],
        chip_kind: ChipKind,
    ) -> Result<Vec<model::Link>> {
        let mut matched_links = Vec::new();
        if let GrpcResponse::ListLink(response) = client.send_grpc(&GrpcRequest::ListLink)? {
            matched_links.extend(response.links.into_iter().filter(|link| {
                link.kind == chip_kind.into()
                    && sender_ids.contains(&link.sender_id)
                    && receiver_ids.contains(&link.receiver_id)
            }));
        }
        Ok(matched_links)
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::VecDeque,
        sync::{Arc, Mutex},
    };

    use clap::Parser;
    use netsim_proto::{
        common::ChipKind,
        frontend::{
            CreateDeviceRequest, CreateLinkRequest, ListDeviceResponse, ListLinkResponse,
            PatchLinkRequest,
        },
        model::{
            Chip as ChipProto, ChipCreate as ChipCreateProto, Device as DeviceProto,
            DeviceCreate as DeviceCreateProto, Link as LinkProto, Orientation, Position,
        },
    };
    use protobuf::MessageField;

    use super::*;
    use crate::{
        args::{
            AdvertiseMode, BeaconBleAdvertiseData, BeaconBleScanResponseData, BeaconBleSettings,
            BeaconCreateBle, BeaconCreateUwb, BeaconPatch, BeaconPatchBle, Capture, Command,
            Devices, GetCapture, Interval, Link, LinkCreate, LinkPatch, ListCapture, Move,
            NetsimArgs, OnOffState, ParsableBytes, PatchCapture, Radio, RadioType, StreamCapture,
            TxPower, TxPowerLevel,
        },
        error::Error,
    };

    struct MockClient {
        responses: Arc<Mutex<VecDeque<Result<GrpcResponse>>>>,
    }

    impl MockClient {
        fn new(responses: Vec<Result<GrpcResponse>>) -> Self {
            Self { responses: Arc::new(Mutex::new(responses.into())) }
        }
    }

    impl GrpcMethodExecutor for MockClient {
        fn send_grpc(&self, _grpc_request: &GrpcRequest) -> Result<GrpcResponse> {
            self.responses.lock().unwrap().pop_front().unwrap_or_else(|| {
                Err(Error::Grpc(grpcio::Error::RpcFailure(grpcio::RpcStatus::new(
                    grpcio::RpcStatusCode::UNKNOWN,
                ))))
            })
        }
    }

    // Helper to test parsing text command into expected Command and GrpcRequest
    fn test_command(command: &str, expected_command: Command, expected_grpc_request: GrpcRequest) {
        let command = NetsimArgs::parse_from(command.split_whitespace()).command;
        assert_eq!(command, expected_command);
        // Note: We use a MockClient that returns errors for now because get_request()
        // doesn't seem to use the client for these older tests? Wait, the older
        // tests used `get_request()` which calls `get_request()` (singular).
        // `get_request` does NOT take a client.
        // So this helper is fine for `get_request` commands.
        // But for `Link` commands we need `test_link_command`.
        let request = command.get_request().unwrap();
        assert_eq!(request, expected_grpc_request);
    }

    fn test_link_command(
        command: &str,
        expected_command: Command,
        mock_responses: Vec<Result<GrpcResponse>>,
        expected_requests: Vec<GrpcRequest>,
    ) {
        let mut command_struct = NetsimArgs::parse_from(command.split_whitespace()).command;
        assert_eq!(command_struct, expected_command);
        let client = MockClient::new(mock_responses);
        let requests = command_struct.get_requests(&client).unwrap();
        assert_eq!(requests, expected_requests);
    }

    fn test_radio_command(
        command: &str,
        expected_command: Command,
        expected_update: netsim_model::DeviceUpdate,
    ) {
        let parsed = NetsimArgs::parse_from(command.split_whitespace()).command;
        assert_eq!(parsed, expected_command);
        assert!(parsed.get_request().is_err());
        let Command::Radio(ref radio) = parsed else {
            panic!("Expected Command::Radio, got {parsed:?}");
        };
        assert_eq!(radio_to_device_update(radio), expected_update);
    }

    fn test_move_command(
        command: &str,
        expected_command: Command,
        expected_update: netsim_model::DeviceUpdate,
    ) {
        let parsed = NetsimArgs::parse_from(command.split_whitespace()).command;
        assert_eq!(parsed, expected_command);
        assert!(parsed.get_request().is_err());
        let Command::Move(ref mv) = parsed else {
            panic!("Expected Command::Move, got {parsed:?}");
        };
        assert_eq!(move_to_device_update(mv), expected_update);
    }

    fn get_expected_radio(radio_type: &str, state: &str) -> netsim_model::DeviceUpdate {
        let chip_state = state == "up";
        let radio = netsim_model::RadioUpdate { state: Some(chip_state) };
        let variant = match radio_type {
            "wifi" => netsim_model::ChipVariantUpdate::Wifi(netsim_model::WifiUpdate { radio }),
            "uwb" => netsim_model::ChipVariantUpdate::Uwb(netsim_model::UwbUpdate { radio }),
            "nfc" => netsim_model::ChipVariantUpdate::Nfc(netsim_model::NfcUpdate { radio }),
            "ethernet" => {
                netsim_model::ChipVariantUpdate::Ethernet(netsim_model::EthernetUpdate { radio })
            }
            "cellular" => {
                let state = if chip_state {
                    netsim_model::MODEM_STATE_IDLE
                } else {
                    netsim_model::MODEM_STATE_DOWN
                };
                netsim_model::ChipVariantUpdate::Cell(netsim_model::CellUpdate {
                    radio,
                    state: Some(state.to_string()),
                    ..Default::default()
                })
            }
            "cellular-data" => {
                netsim_model::ChipVariantUpdate::CellularData(netsim_model::CellularDataUpdate {
                    radio,
                })
            }
            "ble" => netsim_model::ChipVariantUpdate::Bluetooth(netsim_model::BluetoothUpdate {
                low_energy: radio,
                ..Default::default()
            }),
            _ => netsim_model::ChipVariantUpdate::Bluetooth(netsim_model::BluetoothUpdate {
                classic: radio,
                ..Default::default()
            }),
        };
        netsim_model::DeviceUpdate {
            chips: Some(vec![netsim_model::ChipUpdate {
                variant: Some(variant),
                ..Default::default()
            }]),
            ..Default::default()
        }
    }

    #[test]
    fn test_radio_ble() {
        test_radio_command(
            "netsim-cli radio ble down 1000",
            Command::Radio(Radio {
                radio_type: RadioType::Ble,
                status: UpDownStatus::Down,
                name: "1000".to_string(),
            }),
            get_expected_radio("ble", "down"),
        );
        test_radio_command(
            "netsim-cli radio ble up 1000",
            Command::Radio(Radio {
                radio_type: RadioType::Ble,
                status: UpDownStatus::Up,
                name: "1000".to_string(),
            }),
            get_expected_radio("ble", "up"),
        );
    }

    #[test]
    fn test_radio_ble_aliases() {
        test_radio_command(
            "netsim-cli radio ble Down 1000",
            Command::Radio(Radio {
                radio_type: RadioType::Ble,
                status: UpDownStatus::Down,
                name: "1000".to_string(),
            }),
            get_expected_radio("ble", "down"),
        );
        test_radio_command(
            "netsim-cli radio ble Up 1000",
            Command::Radio(Radio {
                radio_type: RadioType::Ble,
                status: UpDownStatus::Up,
                name: "1000".to_string(),
            }),
            get_expected_radio("ble", "up"),
        );
        test_radio_command(
            "netsim-cli radio ble DOWN 1000",
            Command::Radio(Radio {
                radio_type: RadioType::Ble,
                status: UpDownStatus::Down,
                name: "1000".to_string(),
            }),
            get_expected_radio("ble", "down"),
        );
        test_radio_command(
            "netsim-cli radio ble UP 1000",
            Command::Radio(Radio {
                radio_type: RadioType::Ble,
                status: UpDownStatus::Up,
                name: "1000".to_string(),
            }),
            get_expected_radio("ble", "up"),
        );
    }

    #[test]
    fn test_radio_classic() {
        test_radio_command(
            "netsim-cli radio classic down 100",
            Command::Radio(Radio {
                radio_type: RadioType::Classic,
                status: UpDownStatus::Down,
                name: "100".to_string(),
            }),
            get_expected_radio("classic", "down"),
        );
        test_radio_command(
            "netsim-cli radio classic up 100",
            Command::Radio(Radio {
                radio_type: RadioType::Classic,
                status: UpDownStatus::Up,
                name: "100".to_string(),
            }),
            get_expected_radio("classic", "up"),
        );
    }

    #[test]
    fn test_radio_wifi() {
        test_radio_command(
            "netsim-cli radio wifi down a",
            Command::Radio(Radio {
                radio_type: RadioType::Wifi,
                status: UpDownStatus::Down,
                name: "a".to_string(),
            }),
            get_expected_radio("wifi", "down"),
        );
        test_radio_command(
            "netsim-cli radio wifi up b",
            Command::Radio(Radio {
                radio_type: RadioType::Wifi,
                status: UpDownStatus::Up,
                name: "b".to_string(),
            }),
            get_expected_radio("wifi", "up"),
        );
    }

    #[test]
    fn test_radio_uwb() {
        test_radio_command(
            "netsim-cli radio uwb down a",
            Command::Radio(Radio {
                radio_type: RadioType::Uwb,
                status: UpDownStatus::Down,
                name: "a".to_string(),
            }),
            get_expected_radio("uwb", "down"),
        );
        test_radio_command(
            "netsim-cli radio uwb up b",
            Command::Radio(Radio {
                radio_type: RadioType::Uwb,
                status: UpDownStatus::Up,
                name: "b".to_string(),
            }),
            get_expected_radio("uwb", "up"),
        );
    }

    #[test]
    fn test_radio_nfc() {
        for (status_str, status) in [("down", UpDownStatus::Down), ("up", UpDownStatus::Up)] {
            test_radio_command(
                &format!("netsim-cli radio nfc {status_str} a"),
                Command::Radio(Radio { radio_type: RadioType::Nfc, status, name: "a".to_string() }),
                get_expected_radio("nfc", status_str),
            );
        }
    }

    #[test]
    fn test_radio_ethernet() {
        test_radio_command(
            "netsim-cli radio ethernet down a",
            Command::Radio(Radio {
                radio_type: RadioType::Ethernet,
                status: UpDownStatus::Down,
                name: "a".to_string(),
            }),
            get_expected_radio("ethernet", "down"),
        );
        test_radio_command(
            "netsim-cli radio ethernet up b",
            Command::Radio(Radio {
                radio_type: RadioType::Ethernet,
                status: UpDownStatus::Up,
                name: "b".to_string(),
            }),
            get_expected_radio("ethernet", "up"),
        );
    }

    #[test]
    fn test_radio_cellular() {
        test_radio_command(
            "netsim-cli radio cellular down a",
            Command::Radio(Radio {
                radio_type: RadioType::Cellular,
                status: UpDownStatus::Down,
                name: "a".to_string(),
            }),
            get_expected_radio("cellular", "down"),
        );
        test_radio_command(
            "netsim-cli radio cellular up b",
            Command::Radio(Radio {
                radio_type: RadioType::Cellular,
                status: UpDownStatus::Up,
                name: "b".to_string(),
            }),
            get_expected_radio("cellular", "up"),
        );
    }

    #[test]
    fn test_radio_cellular_data() {
        test_radio_command(
            "netsim-cli radio cellular-data down a",
            Command::Radio(Radio {
                radio_type: RadioType::CellularData,
                status: UpDownStatus::Down,
                name: "a".to_string(),
            }),
            get_expected_radio("cellular-data", "down"),
        );
        test_radio_command(
            "netsim-cli radio cellular-data up b",
            Command::Radio(Radio {
                radio_type: RadioType::CellularData,
                status: UpDownStatus::Up,
                name: "b".to_string(),
            }),
            get_expected_radio("cellular-data", "up"),
        );
    }

    fn get_expected_move(x: f32, y: f32, z: Option<f32>) -> netsim_model::DeviceUpdate {
        netsim_model::DeviceUpdate {
            pose: netsim_model::PoseUpdate {
                position: Some(netsim_model::Position { x, y, z: z.unwrap_or_default() }),
                orientation: None,
            },
            ..Default::default()
        }
    }

    #[test]
    fn test_move_int() {
        test_move_command(
            "netsim-cli move 1 1 2 3",
            Command::Move(Move { name: "1".to_string(), x: 1.0, y: 2.0, z: Some(3.0) }),
            get_expected_move(1.0, 2.0, Some(3.0)),
        )
    }

    #[test]
    fn test_move_float() {
        test_move_command(
            "netsim-cli move 1000 1.2 3.4 5.6",
            Command::Move(Move { name: "1000".to_string(), x: 1.2, y: 3.4, z: Some(5.6) }),
            get_expected_move(1.2, 3.4, Some(5.6)),
        )
    }

    #[test]
    fn test_move_mixed() {
        test_move_command(
            "netsim-cli move 1000 1.1 2 3.4",
            Command::Move(Move { name: "1000".to_string(), x: 1.1, y: 2.0, z: Some(3.4) }),
            get_expected_move(1.1, 2.0, Some(3.4)),
        )
    }

    #[test]
    fn test_move_no_z() {
        test_move_command(
            "netsim-cli move 1000 1.2 3.4",
            Command::Move(Move { name: "1000".to_string(), x: 1.2, y: 3.4, z: None }),
            get_expected_move(1.2, 3.4, None),
        )
    }

    #[test]
    fn test_devices() {
        let cmd = NetsimArgs::parse_from("netsim-cli devices".split_whitespace()).command;
        assert_eq!(cmd, Command::Devices(Devices::default()));
        assert!(cmd.get_request().is_err());
    }

    #[test]
    fn test_devices_json_parsing() {
        let cmd = NetsimArgs::parse_from("netsim-cli devices --json".split_whitespace()).command;
        assert_eq!(cmd, Command::Devices(Devices { continuous: false, json: true }));
        assert!(cmd.get_request().is_err());
    }

    #[test]
    fn test_devices_json_output_and_parsing() {
        let dev = netsim_model::Device {
            id: 1,
            name: "test_device".to_string(),
            chips: vec![netsim_model::Chip {
                id: 100,
                kind: netsim_model::ChipKind::BLUETOOTH,
                ..Default::default()
            }],
            ..Default::default()
        };

        let resp = netsim_rest_api::ListDeviceResponse { devices: vec![dev] };

        let json_str = serde_json::to_string(&resp).unwrap();

        // Verify it contains expected data
        assert!(json_str.contains("test_device"));
        assert!(json_str.contains("100")); // Check for chip ID

        // Parse it back
        let parsed_resp: netsim_rest_api::ListDeviceResponse =
            serde_json::from_str(&json_str).unwrap();

        assert_eq!(parsed_resp.devices.len(), 1);
        assert_eq!(parsed_resp.devices[0].name, "test_device");
        assert_eq!(parsed_resp.devices[0].chips.len(), 1);
        assert_eq!(parsed_resp.devices[0].chips[0].id, 100);
    }

    #[test]
    fn test_reset() {
        let cmd = NetsimArgs::parse_from("netsim-cli reset".split_whitespace()).command;
        assert_eq!(cmd, Command::Reset);
        assert!(cmd.get_request().is_err());
    }

    #[test]
    fn test_capture_list() {
        let cmd = NetsimArgs::parse_from("netsim-cli capture list".split_whitespace()).command;
        assert_eq!(cmd, Command::Capture(Capture::List(ListCapture { ..Default::default() })));
        assert!(cmd.get_request().is_err());
    }

    #[test]
    fn test_capture_list_alias() {
        let cmd = NetsimArgs::parse_from("netsim-cli pcap list".split_whitespace()).command;
        assert_eq!(cmd, Command::Capture(Capture::List(ListCapture { ..Default::default() })));
        assert!(cmd.get_request().is_err());
    }

    #[test]
    fn test_capture_patch_and_get() {
        let patch_cmd =
            NetsimArgs::parse_from("netsim-cli capture patch on wifi".split_whitespace()).command;
        assert_eq!(
            patch_cmd,
            Command::Capture(Capture::Patch(PatchCapture {
                state: OnOffState::On,
                patterns: vec!["wifi".to_string()],
            }))
        );
        assert!(patch_cmd.get_request().is_err());

        let get_cmd =
            NetsimArgs::parse_from("netsim-cli pcap get -o /tmp/pcaps 1000".split_whitespace())
                .command;
        assert_eq!(
            get_cmd,
            Command::Capture(Capture::Get(GetCapture {
                patterns: vec!["1000".to_string()],
                location: Some("/tmp/pcaps".to_string()),
            }))
        );
        assert!(get_cmd.get_request().is_err());

        let info = netsim_rest_api::CaptureInfo {
            chip_id: netsim_model::ChipId(1000),
            chip_kind: netsim_model::ChipKind::WIFI,
            device_name: "Pixel 8".to_string(),
            enabled: true,
            records_written: 2,
            bytes_written: 64,
            seconds: 0,
            nanos: 0,
            filepath: None,
        };
        assert_eq!(capture_filename(&info), "netsim-1000-Pixel_8-WIFI-1970-01-01-00-00-00.pcap");
    }

    #[test]
    fn test_capture_stream_stdout() {
        let cmd = NetsimArgs::parse_from("netsim-cli capture stream 1".split_whitespace()).command;
        assert_eq!(
            cmd,
            Command::Capture(Capture::Stream(StreamCapture { chip_id: 1, output: None }))
        );
        assert!(cmd.get_request().is_err());
    }

    #[test]
    fn test_capture_stream_output_file() {
        let cmd =
            NetsimArgs::parse_from("netsim-cli pcap stream 2 -o capture.pcap".split_whitespace())
                .command;
        assert_eq!(
            cmd,
            Command::Capture(Capture::Stream(StreamCapture {
                chip_id: 2,
                output: Some(std::path::PathBuf::from("capture.pcap")),
            }))
        );

        let cmd_long = NetsimArgs::parse_from(
            "netsim-cli capture stream 3 --output out.pcap".split_whitespace(),
        )
        .command;
        assert_eq!(
            cmd_long,
            Command::Capture(Capture::Stream(StreamCapture {
                chip_id: 3,
                output: Some(std::path::PathBuf::from("out.pcap")),
            }))
        );
    }

    fn get_create_device_req(
        device_name: &str,
        chip_name: &str,
        address: &str,
        mut settings: netsim_model::AdvertiseSettings,
        adv_data: netsim_model::AdvertiseData,
        scan_response: netsim_model::AdvertiseData,
    ) -> netsim_model::DeviceCreate {
        settings.interval.get_or_insert(netsim_model::Interval::AdvertiseMode(
            netsim_model::AdvertiseMode::LowPower,
        ));
        settings.tx_power.get_or_insert(netsim_model::TxPower::TxPowerLevel(
            netsim_model::AdvertiseTxPower::Low,
        ));
        netsim_model::DeviceCreate {
            device_config: netsim_model::DeviceConfig {
                name: String::from(device_name),
                visible: true,
                ..Default::default()
            },
            chip: netsim_model::DeviceChipCreate {
                name: String::from(chip_name),
                manufacturer: String::new(),
                product_name: String::new(),
                chip: netsim_model::ChipCreateVariant::Beacon(netsim_model::BleBeacon {
                    address: String::from(address),
                    settings: Some(settings),
                    adv_data: Some(adv_data),
                    scan_response: Some(scan_response),
                }),
            },
        }
    }

    fn get_patch_device_req(
        device_name: &str,
        chip_name: &str,
        address: &str,
        settings: netsim_model::AdvertiseSettingsUpdate,
        adv_data: netsim_model::AdvertiseDataUpdate,
        scan_response: netsim_model::AdvertiseDataUpdate,
    ) -> netsim_model::DeviceUpdate {
        netsim_model::DeviceUpdate {
            id: 1,
            name: Some(String::from(device_name)),
            chips: Some(vec![netsim_model::ChipUpdate {
                id: Some(netsim_model::ChipId(10)),
                name: Some(String::from(chip_name)),
                variant: Some(netsim_model::ChipVariantUpdate::Bluetooth(
                    netsim_model::BluetoothUpdate {
                        ble_beacon: Some(netsim_model::BleBeaconUpdate {
                            address: String::from(address),
                            settings: (!settings.is_empty()).then_some(settings),
                            adv_data: (!adv_data.is_empty()).then_some(adv_data),
                            scan_response: (!scan_response.is_empty()).then_some(scan_response),
                        }),
                        ..Default::default()
                    },
                )),
                ..Default::default()
            }]),
            ..Default::default()
        }
    }

    fn test_beacon_create_ble_command(
        command: &str,
        expected_command: Command,
        expected_create: netsim_model::DeviceCreate,
    ) {
        let parsed = NetsimArgs::parse_from(command.split_whitespace()).command;
        assert_eq!(parsed, expected_command);
        assert!(parsed.get_request().is_err());
        let Command::Beacon(Beacon::Create(BeaconCreate::Ble(ref args))) = parsed else {
            panic!("Expected BeaconCreate::Ble, got {parsed:?}");
        };
        assert_eq!(netsim_model::DeviceCreate::from(args), expected_create);
    }

    fn test_beacon_patch_ble_command(
        command: &str,
        expected_command: Command,
        expected_update: netsim_model::DeviceUpdate,
    ) {
        let parsed = NetsimArgs::parse_from(command.split_whitespace()).command;
        assert_eq!(parsed, expected_command);
        assert!(parsed.get_request().is_err());
        let Command::Beacon(Beacon::Patch(BeaconPatch::Ble(ref args))) = parsed else {
            panic!("Expected BeaconPatch::Ble, got {parsed:?}");
        };
        let device_update = args.to_device_update(1, 10);
        assert_eq!(device_update, expected_update);
        let patch_json = serde_json::to_value(&device_update).unwrap();
        assert_eq!(patch_json, serde_json::to_value(&expected_update).unwrap());
        let beacon_json = &patch_json["chips"][0]["variant"]["Bluetooth"]["ble_beacon"];
        if args.settings.scannable {
            assert_eq!(beacon_json["settings"]["scannable"], serde_json::Value::Bool(true));
        } else {
            assert!(beacon_json["settings"].get("scannable").is_none());
        }
        if args.settings.advertise_mode.is_none()
            && args.settings.tx_power_level.is_none()
            && !args.settings.scannable
            && args.settings.timeout.is_none()
        {
            assert!(beacon_json.get("settings").is_none());
        }
        if args.advertise_data.include_device_name {
            assert_eq!(
                beacon_json["adv_data"]["include_device_name"],
                serde_json::Value::Bool(true)
            );
        } else {
            assert!(beacon_json["adv_data"].get("include_device_name").is_none());
        }
        if !args.advertise_data.include_device_name
            && !args.advertise_data.include_tx_power_level
            && args.advertise_data.manufacturer_data.is_none()
            && args.advertise_data.uuids.is_empty()
        {
            assert!(beacon_json.get("adv_data").is_none());
        }
        if args.scan_response_data.scan_response_include_device_name {
            assert_eq!(
                beacon_json["scan_response"]["include_device_name"],
                serde_json::Value::Bool(true)
            );
        } else {
            assert!(beacon_json["scan_response"].get("include_device_name").is_none());
        }
        if !args.scan_response_data.scan_response_include_device_name
            && !args.scan_response_data.scan_response_include_tx_power_level
            && args.scan_response_data.scan_response_manufacturer_data.is_none()
            && args.scan_response_data.scan_response_uuids.is_empty()
        {
            assert!(beacon_json.get("scan_response").is_none());
        }
    }

    #[test]
    fn test_link_create_with_ids() {
        let cmd = "netsim-cli link create bt --sender 1000 --receiver 1001";
        let expected_cmd = Command::Link(Link::Create(LinkCreate {
            chip_kind: ArgsChipKind::Bluetooth,
            sender: Some(1000),
            receiver: Some(1001),
            sender_name: None,
            receiver_name: None,
            rssi: None,
        }));
        // Mock responses for resolve_chip_ids (1000 and 1001 are non-zero, so it
        // returns immediately, no grpc calls)
        let responses = vec![];
        let expected_reqs = vec![GrpcRequest::CreateLink(CreateLinkRequest {
            link: MessageField::some(LinkProto {
                sender_id: 1000,
                receiver_id: 1001,
                kind: ChipKind::BLUETOOTH.into(),
                rssi: -20,
                ..Default::default()
            }),
            ..Default::default()
        })];

        test_link_command(cmd, expected_cmd, responses, expected_reqs);
    }

    #[test]
    fn test_link_patch_with_ids() {
        let cmd = "netsim-cli link patch bt --rssi -20 --sender 1000 --receiver 1001";
        let expected_cmd = Command::Link(Link::Patch(LinkPatch {
            chip_kind: ArgsChipKind::Bluetooth,
            rssi: Some(-20),
            sender: Some(1000),
            receiver: Some(1001),
            sender_name: None,
            receiver_name: None,
        }));
        // Mock responses:
        // 1. resolve_chip_ids(1000) -> immediate
        // 2. resolve_chip_ids(1001) -> immediate
        // 3. get_link_ids -> ListLink
        let mut link_proto = LinkProto::new();
        link_proto.id = 55;
        link_proto.kind = ChipKind::BLUETOOTH.into();
        link_proto.sender_id = 1000;
        link_proto.receiver_id = 1001;

        let list_link_response = ListLinkResponse { links: vec![link_proto], ..Default::default() };
        let responses = vec![Ok(GrpcResponse::ListLink(list_link_response))];

        let expected_reqs = vec![GrpcRequest::PatchLink(PatchLinkRequest {
            link: MessageField::some(LinkProto {
                id: 55,
                rssi: -20,
                sender_id: 1000,
                receiver_id: 1001,
                kind: ChipKind::BLUETOOTH.into(),
                ..Default::default()
            }),
            id: 55,
            ..Default::default()
        })];

        test_link_command(cmd, expected_cmd, responses, expected_reqs);
    }

    #[test]
    fn test_link_create_resolve_names() {
        let cmd = "netsim-cli link create wifi --sender-name dev1 --receiver-name dev2";
        let expected_cmd = Command::Link(Link::Create(LinkCreate {
            chip_kind: ArgsChipKind::Wifi,
            sender: None,
            receiver: None,
            sender_name: Some("dev1".to_string()),
            receiver_name: Some("dev2".to_string()),
            rssi: None,
        }));

        // Mock ListDevice response
        // Device 1: id=10, name="dev1", chips=[{id=100, kind=WIFI}]
        // Device 2: id=20, name="dev2", chips=[{id=200, kind=WIFI}]
        let dev1 = DeviceProto {
            id: 10,
            name: "dev1".to_string(),
            chips: vec![ChipProto { id: 100, kind: ChipKind::WIFI.into(), ..Default::default() }],
            ..Default::default()
        };
        let dev2 = DeviceProto {
            id: 20,
            name: "dev2".to_string(),
            chips: vec![ChipProto { id: 200, kind: ChipKind::WIFI.into(), ..Default::default() }],
            ..Default::default()
        };
        let list_device_response =
            ListDeviceResponse { devices: vec![dev1, dev2], ..Default::default() };

        // resolve_chip_ids called twice. Both call ListDevice.
        let responses = vec![
            Ok(GrpcResponse::ListDevice(list_device_response.clone())), // For sender
            Ok(GrpcResponse::ListDevice(list_device_response)),         // For receiver
        ];

        let expected_reqs = vec![GrpcRequest::CreateLink(CreateLinkRequest {
            link: MessageField::some(LinkProto {
                sender_id: 100,
                receiver_id: 200,
                kind: ChipKind::WIFI.into(),
                rssi: -20,
                ..Default::default()
            }),
            ..Default::default()
        })];

        test_link_command(cmd, expected_cmd, responses, expected_reqs);
    }

    #[test]
    fn test_beacon_create_all_params_ble() {
        let device_name = String::from("device");
        let chip_name = String::from("chip");

        let timeout = 1234;
        let manufacturer_data = vec![0x12, 0x34];

        let settings = netsim_model::AdvertiseSettings {
            interval: Some(netsim_model::Interval::AdvertiseMode(
                netsim_model::AdvertiseMode::Balanced,
            )),
            tx_power: Some(netsim_model::TxPower::TxPowerLevel(
                netsim_model::AdvertiseTxPower::UltraLow,
            )),
            scannable: true,
            timeout,
        };

        let adv_data = netsim_model::AdvertiseData {
            include_device_name: true,
            include_tx_power_level: true,
            manufacturer_data: manufacturer_data.clone(),
            ..Default::default()
        };

        let request = get_create_device_req(
            &device_name,
            &chip_name,
            "",
            settings,
            adv_data,
            Default::default(),
        );

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
                manufacturer_data: Some(ParsableBytes(manufacturer_data)),
                uuids: vec![],
            },
            scan_response_data: BeaconBleScanResponseData { ..Default::default() },
        })));

        test_beacon_create_ble_command(
            format!(
                "netsim-cli beacon create ble {device_name} {chip_name} --advertise-mode balanced --tx-power-level ultra-low --scannable --timeout {timeout} --include-device-name --include-tx-power-level --manufacturer-data 0x1234",
            )
            .as_str(),
            command,
            request,
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

        let settings = netsim_model::AdvertiseSettingsUpdate {
            interval: Some(netsim_model::Interval::Milliseconds(interval)),
            tx_power: Some(netsim_model::TxPower::Dbm(tx_power_level)),
            scannable: Some(true),
            timeout: Some(timeout),
        };
        let adv_data = netsim_model::AdvertiseDataUpdate {
            include_device_name: Some(true),
            include_tx_power_level: Some(true),
            manufacturer_data: Some(manufacturer_data.clone()),
            ..Default::default()
        };

        let request = get_patch_device_req(
            &device_name,
            &chip_name,
            "",
            settings,
            adv_data,
            Default::default(),
        );

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
                uuids: vec![],
            },
            scan_response_data: BeaconBleScanResponseData { ..Default::default() },
        })));

        test_beacon_patch_ble_command(
            format!(
                "netsim-cli beacon patch ble {device_name} {chip_name} --advertise-mode {interval} --scannable --timeout {timeout} --tx-power-level {tx_power_level} --manufacturer-data 0xabcdef --include-device-name --include-tx-power-level"
            )
            .as_str(),
            command,
            request,
        )
    }

    #[test]
    fn test_beacon_create_scan_response() {
        let device_name = String::from("device");
        let chip_name = String::from("chip");
        let manufacturer_data = vec![0x21, 0xbe, 0xef];

        let scan_response = netsim_model::AdvertiseData {
            include_device_name: true,
            include_tx_power_level: true,
            manufacturer_data: manufacturer_data.clone(),
            ..Default::default()
        };

        let request = get_create_device_req(
            &device_name,
            &chip_name,
            "",
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
                scan_response_uuids: vec![],
            },
            ..Default::default()
        })));

        test_beacon_create_ble_command(
            format!(
                "netsim-cli beacon create ble {device_name} {chip_name} --scan-response-include-device-name --scan-response-include-tx-power-level --scan-response-manufacturer-data 0x21beef"
            )
            .as_str(),
            command,
            request,
        );
    }

    #[test]
    fn test_beacon_patch_scan_response() {
        let device_name = String::from("device");
        let chip_name = String::from("chip");
        let manufacturer_data = vec![0x59, 0xbe, 0xac, 0x09];

        let scan_response = netsim_model::AdvertiseDataUpdate {
            include_device_name: Some(true),
            include_tx_power_level: Some(true),
            manufacturer_data: Some(manufacturer_data.clone()),
            ..Default::default()
        };

        let request = get_patch_device_req(
            &device_name,
            &chip_name,
            "",
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
                scan_response_uuids: vec![],
            },
        })));

        test_beacon_patch_ble_command(
            format!(
                "netsim-cli beacon patch ble {device_name} {chip_name} --scan-response-include-device-name --scan-response-include-tx-power-level --scan-response-manufacturer-data 59beac09"
            )
            .as_str(),
            command,
            request,
        );
    }

    #[test]
    fn test_beacon_create_ble_tx_power() {
        let device_name = String::from("device");
        let chip_name = String::from("chip");

        let settings = netsim_model::AdvertiseSettings {
            tx_power: Some(netsim_model::TxPower::TxPowerLevel(
                netsim_model::AdvertiseTxPower::High,
            )),
            ..Default::default()
        };
        let adv_data =
            netsim_model::AdvertiseData { include_tx_power_level: true, ..Default::default() };

        let request = get_create_device_req(
            &device_name,
            &chip_name,
            "",
            settings,
            adv_data,
            Default::default(),
        );

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

        test_beacon_create_ble_command(
            format!(
                "netsim-cli beacon create ble {device_name} {chip_name} --tx-power-level high --include-tx-power-level"
            )
            .as_str(),
            command,
            request,
        )
    }

    #[test]
    fn test_beacon_create_default() {
        let request = get_create_device_req(
            Default::default(),
            Default::default(),
            "",
            Default::default(),
            Default::default(),
            Default::default(),
        );

        let command = Command::Beacon(Beacon::Create(BeaconCreate::Ble(BeaconCreateBle {
            ..Default::default()
        })));

        test_beacon_create_ble_command("netsim-cli beacon create ble", command, request)
    }

    #[test]
    fn test_beacon_patch_interval() {
        let device_name = String::from("device");
        let chip_name = String::from("chip");

        let settings = netsim_model::AdvertiseSettingsUpdate {
            interval: Some(netsim_model::Interval::AdvertiseMode(
                netsim_model::AdvertiseMode::LowLatency,
            )),
            ..Default::default()
        };

        let request = get_patch_device_req(
            &device_name,
            &chip_name,
            "",
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

        test_beacon_patch_ble_command(
            format!(
                "netsim-cli beacon patch ble {device_name} {chip_name} --advertise-mode low-latency"
            )
            .as_str(),
            command,
            request,
        )
    }

    #[test]
    fn test_beacon_create_ble_with_address() {
        let address = String::from("12:34:56:78:9a:bc");

        let request = get_create_device_req(
            "",
            "",
            &address,
            Default::default(),
            Default::default(),
            Default::default(),
        );
        let command = Command::Beacon(Beacon::Create(BeaconCreate::Ble(BeaconCreateBle {
            address: Some(address.clone()),
            ..Default::default()
        })));

        test_beacon_create_ble_command(
            format!("netsim-cli beacon create ble --address {address}").as_str(),
            command,
            request,
        )
    }

    #[test]
    fn test_beacon_patch_ble_with_address() {
        let address = String::from("12:34:56:78:9a:bc");
        let device_name = String::from("device");
        let chip_name = String::from("chip");

        let request = get_patch_device_req(
            &device_name,
            &chip_name,
            &address,
            Default::default(),
            Default::default(),
            Default::default(),
        );

        let command = Command::Beacon(Beacon::Patch(BeaconPatch::Ble(BeaconPatchBle {
            device_name: device_name.clone(),
            chip_name: chip_name.clone(),
            address: Some(address.clone()),
            ..Default::default()
        })));

        test_beacon_patch_ble_command(
            format!("netsim-cli beacon patch ble {device_name} {chip_name} --address {address}")
                .as_str(),
            command,
            request,
        )
    }

    #[test]
    fn test_beacon_patch_timeout_zero() {
        let device_name = String::from("device");
        let chip_name = String::from("chip");

        let settings =
            netsim_model::AdvertiseSettingsUpdate { timeout: Some(0), ..Default::default() };

        let request = get_patch_device_req(
            &device_name,
            &chip_name,
            "",
            settings,
            Default::default(),
            Default::default(),
        );
        let command = Command::Beacon(Beacon::Patch(BeaconPatch::Ble(BeaconPatchBle {
            device_name: device_name.clone(),
            chip_name: chip_name.clone(),
            address: None,
            settings: BeaconBleSettings { timeout: Some(0), ..Default::default() },
            ..Default::default()
        })));

        test_beacon_patch_ble_command(
            format!("netsim-cli beacon patch ble {device_name} {chip_name} --timeout 0").as_str(),
            command,
            request,
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
    fn test_beacon_create_uwb_all_params() {
        let device_name = "uwb_dev";
        let chip_name = "uwb_chip";
        let address = "00:01";
        let x = -1.5;
        let y = 2.0;
        let z = -3.0;
        let yaw = -90.0;
        let pitch = 20.0;
        let roll = 30.0;

        let command = Command::Beacon(Beacon::Create(BeaconCreate::Uwb(BeaconCreateUwb {
            device_name: Some(device_name.to_string()),
            chip_name: Some(chip_name.to_string()),
            address: Some(address.to_string()),
            x: Some(x),
            y: Some(y),
            z: Some(z),
            yaw: Some(yaw),
            pitch: Some(pitch),
            roll: Some(roll),
        })));

        let device = MessageField::some(DeviceCreateProto {
            name: device_name.to_string(),
            position: MessageField::some(Position { x, y, z, ..Default::default() }),
            orientation: MessageField::some(Orientation { yaw, pitch, roll, ..Default::default() }),
            chips: vec![ChipCreateProto {
                name: chip_name.to_string(),
                kind: ChipKind::UWB.into(),
                address: address.to_string(),
                ..Default::default()
            }],
            ..Default::default()
        });
        let expected_request =
            GrpcRequest::CreateDevice(CreateDeviceRequest { device, ..Default::default() });

        test_command(
            format!(
                "netsim-cli beacon create uwb {device_name} {chip_name} --address {address} --x {x} --y {y} --z {z} --yaw {yaw} --pitch {pitch} --roll {roll}"
            )
            .as_str(),
            command,
            expected_request,
        );
    }

    #[test]
    fn test_beacon_create_uwb_default() {
        let command =
            Command::Beacon(Beacon::Create(BeaconCreate::Uwb(BeaconCreateUwb::default())));

        let device = MessageField::some(DeviceCreateProto {
            position: MessageField::some(Position::default()),
            orientation: MessageField::some(Orientation::default()),
            chips: vec![ChipCreateProto { kind: ChipKind::UWB.into(), ..Default::default() }],
            ..Default::default()
        });
        let expected_request =
            GrpcRequest::CreateDevice(CreateDeviceRequest { device, ..Default::default() });

        test_command("netsim-cli beacon create uwb", command, expected_request);
    }

    #[test]
    fn test_beacon_create_uwb_with_address() {
        let command = Command::Beacon(Beacon::Create(BeaconCreate::Uwb(BeaconCreateUwb {
            address: Some("00:01".to_string()),
            ..Default::default()
        })));

        let device = MessageField::some(DeviceCreateProto {
            position: MessageField::some(Position::default()),
            orientation: MessageField::some(Orientation::default()),
            chips: vec![ChipCreateProto {
                kind: ChipKind::UWB.into(),
                address: "00:01".to_string(),
                ..Default::default()
            }],
            ..Default::default()
        });
        let expected_request =
            GrpcRequest::CreateDevice(CreateDeviceRequest { device, ..Default::default() });

        test_command("netsim-cli beacon create uwb --address 00:01", command, expected_request);
    }

    #[test]
    fn test_anchor_alias_create_uwb() {
        let command = Command::Beacon(Beacon::Create(BeaconCreate::Uwb(BeaconCreateUwb {
            device_name: Some("my-anchor".to_string()),
            address: Some("00:01".to_string()),
            ..Default::default()
        })));

        let device = MessageField::some(DeviceCreateProto {
            name: "my-anchor".to_string(),
            position: MessageField::some(Position::default()),
            orientation: MessageField::some(Orientation::default()),
            chips: vec![ChipCreateProto {
                kind: ChipKind::UWB.into(),
                address: "00:01".to_string(),
                ..Default::default()
            }],
            ..Default::default()
        });
        let expected_request =
            GrpcRequest::CreateDevice(CreateDeviceRequest { device, ..Default::default() });

        test_command(
            "netsim-cli anchor create uwb my-anchor --address 00:01",
            command,
            expected_request,
        );
    }
}
