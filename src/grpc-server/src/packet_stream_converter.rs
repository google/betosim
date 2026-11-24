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

use bytes::{BufMut, Bytes, BytesMut};
use netsim_proto::{
    common as proto_common,
    hci_packet::{HCIPacket, hcipacket::PacketType},
    packet_streamer::{self, PacketRequest, PacketResponse},
    startup as proto_startup,
};
use packet_stream::{
    BtControllerProperties, CellularProperties, Chip, ChipInfo, ChipKind, DeviceInfo,
    error::{PacketStreamError, Result},
    non_empty_trimmed,
};
use protobuf::Enum;

fn proto_to_chip_kind(proto: protobuf::EnumOrUnknown<proto_common::ChipKind>) -> ChipKind {
    crate::frontend_converter::from_proto_chip_kind(proto.enum_value_or_default())
        .unwrap_or(ChipKind::UNSPECIFIED)
}

fn proto_to_chip(proto: proto_startup::Chip) -> Chip {
    let bt_properties = proto.bt_properties.as_ref().and_then(BtControllerProperties::from_proto);
    let cell_props = proto.cellular_properties.as_ref();
    let sim_type = cell_props
        .and_then(|p| p.sim_type)
        .or_else(|| (proto.sim_type != 0).then_some(proto.sim_type));
    let sim_profile = non_empty_trimmed(cell_props.and_then(|p| p.sim_profile.as_deref()))
        .or_else(|| non_empty_trimmed(proto.sim_profile.as_deref()));
    let phone_number = non_empty_trimmed(cell_props.and_then(|p| p.phone_number.as_deref()));
    let cellular_properties =
        if sim_type.is_some() || sim_profile.is_some() || phone_number.is_some() {
            Some(CellularProperties { sim_type, sim_profile, phone_number })
        } else {
            None
        };
    Chip {
        kind: proto_to_chip_kind(proto.kind),
        id: proto.id,
        name: String::new(), // Not available in proto Chip
        manufacturer: proto.manufacturer,
        product_name: proto.product_name,
        address: proto.address,
        cellular_properties,
        loopback: proto.loopback,
        bt_properties,
    }
}

fn proto_to_device_info(proto: proto_startup::DeviceInfo) -> DeviceInfo {
    DeviceInfo {
        name: proto.name,
        id: String::new(), // Not available in proto DeviceInfo
        avd_path: proto.avd_path,
        adb_serial: proto.adb_serial,
        kind: proto.kind,
        version: proto.version,
        sdk_version: proto.sdk_version,
        build_id: proto.build_id,
        variant: proto.variant,
        arch: proto.arch,
        timezone: proto.timezone.filter(|s| !s.is_empty()),
    }
}

pub(crate) fn proto_to_chip_info(proto: proto_startup::ChipInfo) -> ChipInfo {
    ChipInfo {
        name: proto.name,
        chip: proto.chip.into_option().map(proto_to_chip),
        device_info: proto.device_info.into_option().map(proto_to_device_info),
    }
}

/// Converts incoming bytes into a `PacketResponse` protobuf message.
pub(crate) fn bytes_to_packet_response(bytes: Bytes, is_bt: bool) -> Result<PacketResponse> {
    let (&type_byte, packet) = bytes
        .split_first()
        .ok_or_else(|| PacketStreamError::InvalidConfig("Empty bytes".to_string()))?;
    let mut res = PacketResponse::new();
    if is_bt {
        let packet_type = PacketType::from_i32(type_byte.into()).ok_or_else(|| {
            PacketStreamError::InvalidConfig(format!("Invalid HCI packet type: {type_byte}"))
        })?;
        let hci_packet = HCIPacket {
            packet_type: packet_type.into(),
            packet: packet.to_vec(),
            ..Default::default()
        };
        res.set_hci_packet(hci_packet);
    } else {
        // Non-Bluetooth radios (Wi-Fi, UWB) encapsulate raw packet payloads
        res.set_packet(bytes.to_vec());
    }
    Ok(res)
}

// Convert PacketRequest to Bytes
pub(crate) fn packet_request_to_bytes(value: PacketRequest) -> Result<Bytes> {
    match value.request_type {
        Some(packet_streamer::packet_request::Request_type::HciPacket(hci)) => {
            let mut h4_packet = BytesMut::new();
            h4_packet.put_u8(hci.packet_type.value() as u8);
            h4_packet.put_slice(&hci.packet);
            Ok(h4_packet.freeze())
        }
        Some(packet_streamer::packet_request::Request_type::Packet(packet)) => {
            Ok(Bytes::from(packet))
        }
        _ => Err(PacketStreamError::InvalidConfig(
            "PacketRequest does not contain a supported packet type".to_string(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use netsim_proto::configuration::{
        Controller, ControllerFeatures, ControllerPreset, ControllerQuirks, VendorFeatures,
    };
    use protobuf::{Message, MessageField, MessageFull};

    use super::*;

    const ADB_SERIAL_FIELD_NUMBER: i32 = 10;
    const LENGTH_DELIMITED_WIRE_TYPE: u8 = 2;
    const ADB_SERIAL_WIRE_TAG: u8 =
        (ADB_SERIAL_FIELD_NUMBER as u8) << 3 | LENGTH_DELIMITED_WIRE_TYPE;

    #[test]
    fn test_proto_to_chip_info_populates_bt_properties() {
        let mut proto_chip = proto_startup::Chip::new();
        proto_chip.kind = protobuf::EnumOrUnknown::new(proto_common::ChipKind::BLUETOOTH);
        proto_chip.id = "chip-1".to_string();

        let mut ctrl = Controller::new();
        ctrl.set_preset(ControllerPreset::LAIRD_BL654);

        let mut feats = ControllerFeatures::new();
        feats.le_connected_isochronous_stream = Some(false);
        ctrl.features = MessageField::some(feats);

        let mut quirks = ControllerQuirks::new();
        quirks.hardware_error_before_reset = Some(true);
        ctrl.quirks = MessageField::some(quirks);

        ctrl.set_strict(true);

        let mut vendor = VendorFeatures::new();
        vendor.android = Some(true);
        ctrl.vendor = MessageField::some(vendor);

        proto_chip.bt_properties = MessageField::some(ctrl);

        let mut proto_info = proto_startup::ChipInfo::new();
        proto_info.name = "test-device".to_string();
        proto_info.chip = MessageField::some(proto_chip);

        let chip_info = proto_to_chip_info(proto_info);
        let chip = chip_info.chip.expect("chip present");
        let props = chip.bt_properties.expect("bt_properties present");

        assert_eq!(props.preset.as_deref(), Some("laird_bl654"));
        assert_eq!(
            props.features.as_ref().and_then(|f| f.le_connected_isochronous_stream),
            Some(false)
        );
        assert_eq!(props.quirks.as_ref().and_then(|q| q.hardware_error_before_reset), Some(true));
        assert_eq!(props.strict, Some(true));
        assert_eq!(props.vendor.as_ref().and_then(|v| v.android), Some(true));
    }

    #[test]
    fn test_proto_to_chip_info_empty_bt_properties_is_none() {
        let mut proto_chip = proto_startup::Chip::new();
        proto_chip.kind = protobuf::EnumOrUnknown::new(proto_common::ChipKind::BLUETOOTH);
        proto_chip.bt_properties = MessageField::some(Controller::new());

        let mut proto_info = proto_startup::ChipInfo::new();
        proto_info.chip = MessageField::some(proto_chip);

        let chip_info = proto_to_chip_info(proto_info);
        let chip = chip_info.chip.expect("chip present");
        assert!(chip.bt_properties.is_none());
    }

    #[test]
    fn test_proto_to_device_info_timezone_presence_and_empty_filter() {
        let mut proto_info = proto_startup::ChipInfo::new();
        proto_info.device_info = MessageField::some(proto_startup::DeviceInfo::new());
        let chip_info = proto_to_chip_info(proto_info);
        assert_eq!(chip_info.device_info.expect("device_info present").timezone, None);

        let mut empty_tz_device = proto_startup::DeviceInfo::new();
        empty_tz_device.timezone = Some(String::new());
        let mut proto_empty = proto_startup::ChipInfo::new();
        proto_empty.device_info = MessageField::some(empty_tz_device);
        let empty_chip_info = proto_to_chip_info(proto_empty);
        assert_eq!(empty_chip_info.device_info.expect("device_info present").timezone, None);

        let mut tz_device = proto_startup::DeviceInfo::new();
        tz_device.timezone = Some("America/New_York".to_string());
        let mut proto_with_tz = proto_startup::ChipInfo::new();
        proto_with_tz.device_info = MessageField::some(tz_device);
        let tz_chip_info = proto_to_chip_info(proto_with_tz);
        assert_eq!(
            tz_chip_info.device_info.expect("device_info present").timezone.as_deref(),
            Some("America/New_York")
        );
    }

    #[test]
    fn test_startup_device_info_adb_serial_descriptor_and_wire_roundtrip() {
        let descriptor = proto_startup::DeviceInfo::descriptor();
        let field = descriptor.field_by_name("adb_serial").expect("adb_serial descriptor field");
        assert_eq!(field.number(), ADB_SERIAL_FIELD_NUMBER);

        let empty = proto_startup::DeviceInfo::new();
        let empty_bytes = empty.write_to_bytes().expect("serialize default DeviceInfo");
        assert!(empty_bytes.is_empty());
        let decoded_empty =
            proto_startup::DeviceInfo::parse_from_bytes(&empty_bytes).expect("parse empty");
        assert_eq!(decoded_empty.adb_serial, "");

        let mut configured = proto_startup::DeviceInfo::new();
        configured.adb_serial = "emulator-5554".to_string();
        let wire_bytes = configured.write_to_bytes().expect("serialize configured DeviceInfo");
        assert_eq!(wire_bytes.first().copied(), Some(ADB_SERIAL_WIRE_TAG));

        let decoded =
            proto_startup::DeviceInfo::parse_from_bytes(&wire_bytes).expect("parse configured");
        assert_eq!(decoded.adb_serial, "emulator-5554");
        assert_eq!(decoded.special_fields.unknown_fields().iter().count(), 0);
    }

    #[test]
    fn test_proto_to_device_info_maps_adb_serial_and_preserves_empty_id() {
        let mut proto = proto_startup::DeviceInfo::new();
        proto.name = "Pixel_6".to_string();
        proto.adb_serial = "emulator-5554".to_string();

        let info = proto_to_device_info(proto);
        assert_eq!(info.name, "Pixel_6");
        assert_eq!(info.adb_serial, "emulator-5554");
        assert_eq!(info.id, "");
    }

    #[test]
    fn test_proto_to_device_info_unset_adb_serial_is_empty() {
        let mut unset_proto = proto_startup::DeviceInfo::new();
        unset_proto.name = "Pixel_6".to_string();
        let unset_info = proto_to_device_info(unset_proto);
        assert_eq!(unset_info.adb_serial, "");
    }

    const SIM_TYPE_NO_SIM: i32 = 0;
    const SIM_TYPE_NORMAL: i32 = 1;
    const SIM_TYPE_CTS_CARRIER: i32 = 2;

    #[test]
    fn test_proto_to_chip_legacy_cellular_fallback() {
        let proto = proto_startup::Chip {
            sim_type: SIM_TYPE_NORMAL,
            sim_profile: Some("custom.xml".to_string()),
            cellular_properties: MessageField::none(),
            ..Default::default()
        };

        let chip = proto_to_chip(proto);
        assert_eq!(
            chip.cellular_properties,
            Some(CellularProperties {
                sim_type: Some(SIM_TYPE_NORMAL),
                sim_profile: Some("custom.xml".to_string()),
                phone_number: None,
            })
        );
    }

    #[test]
    fn test_proto_to_chip_cellular_properties_overrides_legacy() {
        let proto = proto_startup::Chip {
            sim_type: SIM_TYPE_CTS_CARRIER,
            sim_profile: Some("legacy.xml".to_string()),
            cellular_properties: MessageField::some(proto_startup::CellularProperties {
                sim_type: Some(SIM_TYPE_NO_SIM),
                sim_profile: Some("   ".to_string()),
                phone_number: Some("+15550199999".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        };

        let chip = proto_to_chip(proto);
        assert_eq!(
            chip.cellular_properties,
            Some(CellularProperties {
                sim_type: Some(SIM_TYPE_NO_SIM),
                sim_profile: Some("legacy.xml".to_string()),
                phone_number: Some("+15550199999".to_string()),
            })
        );
    }

    #[test]
    fn test_proto_to_chip_empty_cellular_fields_yield_none() {
        let proto = proto_startup::Chip {
            sim_type: SIM_TYPE_NO_SIM,
            sim_profile: Some("   ".to_string()),
            cellular_properties: MessageField::some(proto_startup::CellularProperties {
                sim_type: None,
                sim_profile: Some(String::new()),
                phone_number: Some(" ".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        };

        let chip = proto_to_chip(proto);
        assert_eq!(chip.cellular_properties, None);
    }
}
