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
    BtControllerProperties, Chip, ChipInfo, ChipKind, DeviceInfo,
    error::{PacketStreamError, Result},
};
use protobuf::Enum;

fn proto_to_chip_kind(proto: protobuf::EnumOrUnknown<proto_common::ChipKind>) -> ChipKind {
    crate::frontend_converter::from_proto_chip_kind(proto.enum_value_or_default())
        .unwrap_or(ChipKind::UNSPECIFIED)
}

fn proto_to_chip(proto: proto_startup::Chip) -> Chip {
    let bt_properties = proto.bt_properties.as_ref().and_then(BtControllerProperties::from_proto);
    Chip {
        kind: proto_to_chip_kind(proto.kind),
        id: proto.id,
        name: String::new(), // Not available in proto Chip
        manufacturer: proto.manufacturer,
        product_name: proto.product_name,
        address: proto.address,
        sim_type: if proto.sim_type != 0 { Some(proto.sim_type) } else { None },
        sim_profile: proto.sim_profile,
        loopback: proto.loopback,
        bt_properties,
    }
}

fn proto_to_device_info(proto: proto_startup::DeviceInfo) -> DeviceInfo {
    DeviceInfo {
        name: proto.name,
        id: String::new(), // Not available in proto DeviceInfo
        avd_path: proto.avd_path,
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
            let hci_bytes = hci.packet.to_vec();
            h4_packet.put_slice(&hci_bytes);
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
    use protobuf::MessageField;

    use super::*;

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
}
