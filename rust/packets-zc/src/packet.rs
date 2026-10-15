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

//! `etherparse` provides a simplified interface for parsing a byte slice
//! into a structured `Packet` object. It handles the logic of identifying
//! and parsing different layers of a network packet, starting from the
//! Ethernet frame and drilling down into IP and transport layers.
//!
//! The main entry point is the `parse` function, which takes a raw byte
//! slice and returns an `Option<Packet>`. The `Packet` struct contains

use crate::ethernet::{EthernetPacket, ether_type};
use crate::icmp::IcmpHeader;
use crate::icmpv6::Icmpv6Header;
use crate::ip::{IP_P_HOPOPTS, IP_P_ICMP, IP_P_ICMPV6, Ipv4Header, Ipv6Header, Ipv6HopByHopHeader};
use zerocopy::Ref;

/// Represents the IP layer of a packet, supporting both IPv4 and IPv6.
/// It holds a reference to the header and a slice for the payload.
pub enum IpPacket<'a> {
    V4(Ref<&'a [u8], Ipv4Header>, &'a [u8]),
    V6(Ref<&'a [u8], Ipv6Header>, &'a [u8]),
}

/// Represents the transport layer of a packet, currently supporting ICMP and ICMPv6.
pub enum TransportPacket<'a> {
    Icmp(Ref<&'a [u8], IcmpHeader>, &'a [u8]),
    Icmpv6(Ref<&'a [u8], Icmpv6Header>, &'a [u8]),
}

/// A fully parsed packet, containing the Ethernet frame and optional
/// IP and transport layer data.
pub struct Packet<'a> {
    pub ethernet: EthernetPacket<'a>,
    pub ip: Option<IpPacket<'a>>,
    pub transport: Option<TransportPacket<'a>>,
}

/// Parses a raw byte slice into a `Packet`.
///
/// This function serves as the main entry point for packet parsing. It starts
/// by parsing the Ethernet layer and then iteratively decodes the encapsulated
/// IP and transport layers based on the `ethertype` and `protocol` fields.
pub fn parse(data: &[u8]) -> Option<Packet> {
    if let Some(ethernet) = EthernetPacket::parse(data) {
        let (ethertype, payload) = match &ethernet {
            EthernetPacket::Untagged { frame, payload } => (frame.ethertype.get(), *payload),
            EthernetPacket::Vlan { vlan_header, payload, .. } => {
                (vlan_header.ethertype.get(), *payload)
            }
        };

        let ip = match ethertype {
            ether_type::IPV4 => {
                if let Some((ipv4_header, ipv4_payload)) = Ipv4Header::parse(payload) {
                    Some(IpPacket::V4(ipv4_header, ipv4_payload))
                } else {
                    None
                }
            }
            ether_type::IPV6 => {
                if let Some((ipv6_header, ipv6_payload)) = Ipv6Header::parse(payload) {
                    Some(IpPacket::V6(ipv6_header, ipv6_payload))
                } else {
                    None
                }
            }
            _ => None,
        };

        let transport = if let Some(ip_packet) = &ip {
            match ip_packet {
                IpPacket::V4(header, payload) if header.protocol == IP_P_ICMP => {
                    if let Some((icmp_header, icmp_payload)) = IcmpHeader::parse(payload) {
                        Some(TransportPacket::Icmp(icmp_header, icmp_payload))
                    } else {
                        None
                    }
                }
                IpPacket::V6(header, payload) => {
                    let mut next_header = header.next_header;
                    let mut current_payload = *payload;
                    if next_header == IP_P_HOPOPTS {
                        if let Some((hbh_header, hbh_payload)) =
                            Ipv6HopByHopHeader::parse(current_payload)
                        {
                            next_header = hbh_header.next_header;
                            current_payload = hbh_payload;
                        } else {
                            // Malformed packet, can't parse HopByHop
                            return Some(Packet { ethernet, ip, transport: None });
                        }
                    }
                    if next_header == IP_P_ICMPV6 {
                        if let Some((icmpv6_header, icmpv6_payload)) =
                            Icmpv6Header::parse(current_payload)
                        {
                            Some(TransportPacket::Icmpv6(icmpv6_header, icmpv6_payload))
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                }
                _ => None,
            }
        } else {
            None
        };

        Some(Packet { ethernet, ip, transport })
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ethernet::ether_type;

    fn build_ipv4_icmp_packet() -> Vec<u8> {
        let mut bytes = Vec::new();
        // Ethernet
        bytes.extend_from_slice(&[0x00, 0x11, 0x22, 0x33, 0x44, 0x55]); // Dst
        bytes.extend_from_slice(&[0x66, 0x77, 0x88, 0x99, 0xAA, 0xBB]); // Src
        bytes.extend_from_slice(&ether_type::IPV4.to_be_bytes());
        // IPv4
        bytes.extend_from_slice(&[0x45, 0x00]); // Version, IHL, ToS
        bytes.extend_from_slice(&28u16.to_be_bytes()); // Total Length
        bytes.extend_from_slice(&0u16.to_be_bytes()); // Identification
        bytes.extend_from_slice(&0u16.to_be_bytes()); // Flags, Frag Offset
        bytes.extend_from_slice(&[64, IP_P_ICMP]); // TTL, Protocol
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

    fn build_ipv6_icmpv6_packet() -> Vec<u8> {
        let mut bytes = Vec::new();
        // Ethernet
        bytes.extend_from_slice(&[0x00, 0x11, 0x22, 0x33, 0x44, 0x55]); // Dst
        bytes.extend_from_slice(&[0x66, 0x77, 0x88, 0x99, 0xAA, 0xBB]); // Src
        bytes.extend_from_slice(&ether_type::IPV6.to_be_bytes());
        // IPv6
        bytes.extend_from_slice(&0x60000000u32.to_be_bytes()); // Version, TC, Flow Label
        bytes.extend_from_slice(&8u16.to_be_bytes()); // Payload Length
        bytes.extend_from_slice(&[IP_P_ICMPV6, 64]); // Next Header, Hop Limit
        bytes.extend_from_slice(&[0xfe, 0x80, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1]); // Src
        bytes.extend_from_slice(&[0xff, 0x02, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1]); // Dst
        // ICMPv6
        bytes.extend_from_slice(&[128, 0]); // Type, Code
        bytes.extend_from_slice(&0u16.to_be_bytes()); // Checksum
        bytes.extend_from_slice(&[0, 0, 0, 0]); // Body
        bytes
    }

    #[test]
    fn test_parse_ipv4_icmp() {
        let bytes = build_ipv4_icmp_packet();
        let packet = parse(&bytes).expect("Failed to parse packet");

        if let EthernetPacket::Untagged { frame, .. } = packet.ethernet {
            assert_eq!(frame.ethertype.get(), ether_type::IPV4);
        } else {
            panic!("Expected untagged frame");
        }

        let ip_packet = packet.ip.expect("No IP packet found");
        if let IpPacket::V4(header, payload) = ip_packet {
            assert_eq!(header.protocol, IP_P_ICMP);
            assert_eq!(payload.len(), 8);
        } else {
            panic!("Expected IPv4 packet");
        }

        let transport_packet = packet.transport.expect("No transport packet found");
        if let TransportPacket::Icmp(header, payload) = transport_packet {
            assert_eq!(header.icmp_type, 8);
            assert_eq!(payload.len(), 0);
        } else {
            panic!("Expected ICMP packet");
        }
    }

    #[test]
    fn test_parse_ipv6_icmpv6() {
        let bytes = build_ipv6_icmpv6_packet();
        let packet = parse(&bytes).expect("Failed to parse packet");

        if let EthernetPacket::Untagged { frame, .. } = packet.ethernet {
            assert_eq!(frame.ethertype.get(), ether_type::IPV6);
        } else {
            panic!("Expected untagged frame");
        }

        let ip_packet = packet.ip.expect("No IP packet found");
        if let IpPacket::V6(header, payload) = ip_packet {
            assert_eq!(header.next_header, IP_P_ICMPV6);
            assert_eq!(payload.len(), 8);
        } else {
            panic!("Expected IPv6 packet");
        }

        let transport_packet = packet.transport.expect("No transport packet found");
        if let TransportPacket::Icmpv6(header, payload) = transport_packet {
            assert_eq!(header.icmpv6_type, 128);
            assert_eq!(payload.len(), 0);
        } else {
            panic!("Expected ICMPv6 packet");
        }
    }

    #[test]
    fn test_parse_vlan() {
        let mut bytes = Vec::new();
        // Ethernet w/ VLAN
        bytes.extend_from_slice(&[0x00, 0x11, 0x22, 0x33, 0x44, 0x55]); // Dst
        bytes.extend_from_slice(&[0x66, 0x77, 0x88, 0x99, 0xAA, 0xBB]); // Src
        bytes.extend_from_slice(&ether_type::VLAN.to_be_bytes());
        bytes.extend_from_slice(&0u16.to_be_bytes()); // TCI
        bytes.extend_from_slice(&ether_type::IPV4.to_be_bytes());
        // IPv4
        bytes.extend_from_slice(&[0x45, 0x00]); // Version, IHL, ToS
        bytes.extend_from_slice(&20u16.to_be_bytes()); // Total Length
        bytes.extend_from_slice(&0u16.to_be_bytes()); // Identification
        bytes.extend_from_slice(&0u16.to_be_bytes()); // Flags, Frag Offset
        bytes.extend_from_slice(&[64, 17]); // TTL, Protocol (UDP)
        bytes.extend_from_slice(&0u16.to_be_bytes()); // Checksum
        bytes.extend_from_slice(&[192, 168, 0, 1]); // Src Addr
        bytes.extend_from_slice(&[192, 168, 0, 2]); // Dst Addr

        let packet = parse(&bytes).expect("Failed to parse packet");
        assert!(matches!(packet.ethernet, EthernetPacket::Vlan { .. }));
        assert!(packet.ip.is_some());
        assert!(packet.transport.is_none()); // UDP parsing not implemented
    }
}
