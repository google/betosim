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
    Chip, ChipInfo, ChipKind, DeviceInfo,
    error::{PacketStreamError, Result},
};
use protobuf::Enum;

fn proto_to_chip_kind(proto: protobuf::EnumOrUnknown<proto_common::ChipKind>) -> ChipKind {
    crate::frontend_converter::from_proto_chip_kind(proto.enum_value_or_default())
        .unwrap_or(ChipKind::UNSPECIFIED)
}

fn proto_to_chip(proto: proto_startup::Chip) -> Chip {
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
