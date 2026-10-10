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

//! `etherparse_json` provides functionality to serialize a parsed `Packet`
//! object into a `serde_json::Value`. This is designed to produce JSON
//! output that is compatible with systems like Elasticsearch, and mimics
//! the layer-based structure of tools like `tshark`.
//!
//! The main entry point is the `to_json` function, which takes a `Packet`
//! reference and returns a `Value` representing the full packet structure.

use crate::ethernet_json;
use crate::icmp_json;
use crate::icmpv6_json;
use crate::ip_json;
use crate::json_common;
use crate::packet::{IpPacket, Packet, TransportPacket};
use serde_json::Value;

/// Converts a parsed `Packet` into a `serde_json::Value`.
///
/// This function orchestrates the serialization of each protocol layer
/// present in the `Packet`. It calls the respective `to_json` function
/// for each layer (Ethernet, IP, transport) and assembles them into a
/// single JSON object.
pub fn to_json(packet: &Packet) -> Value {
    let mut layers = serde_json::Map::new();
    let eth_map = ethernet_json::to_json(&packet.ethernet, 0);
    layers.insert("eth".to_string(), eth_map);

    if let Some(ip_packet) = &packet.ip {
        let (ip_json, ip_layer_name) = match ip_packet {
            IpPacket::V4(header, payload) => {
                (serde_json::to_value(ip_json::ipv4_to_json(header, payload)), "ip")
            }
            IpPacket::V6(header, payload) => {
                (serde_json::to_value(ip_json::ipv6_to_json(header, payload)), "ipv6")
            }
        };
        if let Ok(ip_map) = ip_json {
            layers.insert(ip_layer_name.to_string(), ip_map);
        }
    }
    if let Some(transport_packet) = &packet.transport {
        let (transport_json, transport_layer_name) = match transport_packet {
            TransportPacket::Icmp(header, _) => {
                (serde_json::to_value(icmp_json::to_json(header)), "icmp")
            }
            TransportPacket::Icmpv6(header, _) => {
                (serde_json::to_value(icmpv6_json::to_json(header)), "icmpv6")
            }
        };
        if let Ok(transport_map) = transport_json {
            layers.insert(transport_layer_name.to_string(), transport_map);
        }
    }

    json_common::build_packet_json(Value::Object(layers))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::packet;

    fn build_ipv4_icmp_packet() -> Vec<u8> {
        let mut bytes = Vec::new();
        // Ethernet
        bytes.extend_from_slice(&[0x00, 0x11, 0x22, 0x33, 0x44, 0x55]); // Dst
        bytes.extend_from_slice(&[0x66, 0x77, 0x88, 0x99, 0xAA, 0xBB]); // Src
        bytes.extend_from_slice(&[0x08, 0x00]); // EtherType (IPv4)
        // IPv4
        bytes.extend_from_slice(&[0x45, 0x00]); // Version, IHL, ToS
        bytes.extend_from_slice(&28u16.to_be_bytes()); // Total Length
        bytes.extend_from_slice(&0u16.to_be_bytes()); // Identification
        bytes.extend_from_slice(&0u16.to_be_bytes()); // Flags, Frag Offset
        bytes.extend_from_slice(&[64, 1]); // TTL, Protocol (ICMP)
        bytes.extend_from_slice(&0u16.to_be_bytes()); // Checksum
        bytes.extend_from_slice(&[192, 168, 0, 1]); // Src Addr
        bytes.extend_from_slice(&[192, 168, 0, 2]); // Dst Addr
        // ICMP
        bytes.extend_from_slice(&[8, 0]); // Type, Code
        bytes.extend_from_slice(&0u16.to_be_bytes()); // Checksum
        bytes.extend_from_slice(&0u16.to_be_bytes()); // Identifier
        bytes.extend_from_slice(&1u16.to_be_bytes()); // Sequence
        bytes
    }

    #[test]
    fn test_to_json_ipv4_icmp() {
        let bytes = build_ipv4_icmp_packet();
        let packet = packet::parse(&bytes).unwrap();
        let json = to_json(&packet);

        assert_eq!(json[0]["_source"]["layers"]["eth"]["eth.dst"], "00:11:22:33:44:55");
        assert_eq!(json[0]["_source"]["layers"]["ip"]["ip.proto"], "1");
        assert_eq!(json[0]["_source"]["layers"]["icmp"]["icmp.type"], "8");
    }

    #[test]
    fn test_to_json_string() {
        let bytes = build_ipv4_icmp_packet();
        let packet = packet::parse(&bytes).unwrap();
        let json_string = to_json(&packet).to_string();
        assert!(json_string.contains("eth.dst"));
    }
}
