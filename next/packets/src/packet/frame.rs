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

//! This module provides a simplified interface for parsing a byte slice
//! into a structured `Packet` object. It handles the logic of identifying
//! and parsing different layers of a network packet, starting from the
//! Ethernet frame and drilling down into IP and transport layers.
//!
//! The main entry point is the `parse` function, which takes a raw byte
//! slice and returns an `Option<Packet>`. The `Packet` struct contains

use zerocopy::Ref;

use crate::{
    ethernet::{EthernetPacket, ether_type},
    icmp::{IcmpHeader, v6::Icmpv6Header},
    ip::{
        IP_P_HOPOPTS, IP_P_ICMP, IP_P_ICMPV6, IP_P_TCP, IP_P_UDP, Ipv4Header, Ipv6Header,
        Ipv6HopByHopHeader,
    },
    transport::{tcp::TcpHeader, udp::UdpHeader},
};

/// Represents the IP layer of a packet, supporting both IPv4 and IPv6.
/// It holds a reference to the header and a slice for the payload.
pub enum IpPacket<'a> {
    V4(Ref<&'a [u8], Ipv4Header>, &'a [u8]),
    V6(Ref<&'a [u8], Ipv6Header>, &'a [u8]),
}

/// Represents the transport layer of a packet, currently supporting ICMP,
/// ICMPv6, TCP, and UDP.
pub enum TransportPacket<'a> {
    Icmp(Ref<&'a [u8], IcmpHeader>, &'a [u8]),
    Icmpv6(Ref<&'a [u8], Icmpv6Header>, &'a [u8]),
    Tcp(Ref<&'a [u8], TcpHeader>, &'a [u8]),
    Udp(Ref<&'a [u8], UdpHeader>, &'a [u8]),
}

use crate::llc::frame::{LlcHeader, LlcSnapHeader};

/// Represents the LLC layer of a packet.
pub enum LlcPacket<'a> {
    Llc(Ref<&'a [u8], LlcHeader>, &'a [u8]),
    LlcSnap(Ref<&'a [u8], LlcSnapHeader>, &'a [u8]),
}

/// A fully parsed packet, containing the Ethernet frame and optional
/// IP and transport layer data.
pub struct Packet<'a> {
    pub ethernet: EthernetPacket<'a>,
    pub llc: Option<LlcPacket<'a>>,
    pub ip: Option<IpPacket<'a>>,
    pub transport: Option<TransportPacket<'a>>,
}

/// Parses a raw byte slice into a `Packet`.
///
/// This function serves as the main entry point for packet parsing. It starts
/// by parsing the Ethernet layer and then iteratively decodes the encapsulated
/// IP and transport layers based on the `ethertype` and `protocol` fields.
pub fn parse(data: &[u8]) -> Option<Packet<'_>> {
    if let Some(ethernet) = EthernetPacket::parse(data) {
        let (mut ethertype, mut payload) = match &ethernet {
            EthernetPacket::Untagged { frame, payload } => (frame.ethertype.get(), *payload),
            EthernetPacket::Vlan { vlan_header, payload, .. } => {
                (vlan_header.ethertype.get(), *payload)
            }
        };

        let mut llc = None;

        // Check for LLC (Length field <= 1500)
        if ethertype <= 1500 {
            // Try to parse as LLC/SNAP first (most common for IP over LLC)
            if let Some((header, rest)) = LlcSnapHeader::parse(payload) {
                // If SNAP, the protocol ID is the new EtherType
                ethertype = header.snap.protocol_id.get();
                payload = rest;
                llc = Some(LlcPacket::LlcSnap(header, rest));
            } else if let Some((header, rest)) = LlcHeader::parse(payload) {
                // If just LLC, we might not have an EtherType unless it's SNAP (handled above)
                // or we infer from SAPs.
                // For now, we just store it.
                llc = Some(LlcPacket::Llc(header, rest));
                payload = rest;
                // If not SNAP, we don't have a standard EtherType for IP parsing usually.
                // We could check for specific SAPs if needed.
                ethertype = 0; // Unknown/Consumed
            }
        }

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
                IpPacket::V4(header, payload) => parse_transport(header.protocol, payload, false),
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
                            return Some(Packet { ethernet, llc, ip, transport: None });
                        }
                    }
                    parse_transport(next_header, current_payload, true)
                }
            }
        } else {
            None
        };

        Some(Packet { ethernet, llc, ip, transport })
    } else {
        None
    }
}

/// Parses the transport layer for the given IP protocol number.
///
/// ICMP (RFC 792) is only accepted over IPv4 and ICMPv6 (RFC 4443) only over
/// IPv6; `is_ipv6` selects which one applies.
fn parse_transport(protocol: u8, payload: &[u8], is_ipv6: bool) -> Option<TransportPacket<'_>> {
    match protocol {
        IP_P_ICMP if !is_ipv6 => IcmpHeader::parse(payload)
            .map(|(header, payload)| TransportPacket::Icmp(header, payload)),
        IP_P_ICMPV6 if is_ipv6 => Icmpv6Header::parse(payload)
            .map(|(header, payload)| TransportPacket::Icmpv6(header, payload)),
        IP_P_TCP => {
            TcpHeader::parse(payload).map(|(header, payload)| TransportPacket::Tcp(header, payload))
        }
        IP_P_UDP => {
            UdpHeader::parse(payload).map(|(header, payload)| TransportPacket::Udp(header, payload))
        }
        _ => None,
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
        assert!(packet.transport.is_none()); // UDP parsing fails due to missing
        // header
    }

    #[test]
    fn test_parse_tcp() {
        let mut bytes = Vec::new();
        // Ethernet
        bytes.extend_from_slice(&[0x00, 0x11, 0x22, 0x33, 0x44, 0x55]);
        bytes.extend_from_slice(&[0x66, 0x77, 0x88, 0x99, 0xAA, 0xBB]);
        bytes.extend_from_slice(&ether_type::IPV4.to_be_bytes());
        // IPv4
        bytes.extend_from_slice(&[0x45, 0x00]);
        bytes.extend_from_slice(&40u16.to_be_bytes()); // Total Length (20 IP + 20 TCP)
        bytes.extend_from_slice(&0u16.to_be_bytes());
        bytes.extend_from_slice(&0u16.to_be_bytes());
        bytes.extend_from_slice(&[64, IP_P_TCP]);
        bytes.extend_from_slice(&0u16.to_be_bytes());
        bytes.extend_from_slice(&[192, 168, 0, 1]);
        bytes.extend_from_slice(&[192, 168, 0, 2]);
        // TCP
        bytes.extend_from_slice(&1234u16.to_be_bytes()); // Src Port
        bytes.extend_from_slice(&80u16.to_be_bytes()); // Dst Port
        bytes.extend_from_slice(&0u32.to_be_bytes()); // Seq
        bytes.extend_from_slice(&0u32.to_be_bytes()); // Ack
        bytes.extend_from_slice(&[0x50, 0x02]); // Offset (5), Flags (SYN)
        bytes.extend_from_slice(&0u16.to_be_bytes()); // Window
        bytes.extend_from_slice(&0u16.to_be_bytes()); // Checksum
        bytes.extend_from_slice(&0u16.to_be_bytes()); // Urgent Pointer

        let packet = parse(&bytes).expect("Failed to parse packet");
        let transport = packet.transport.expect("No transport packet found");
        if let TransportPacket::Tcp(header, payload) = transport {
            assert_eq!(header.source_port.get(), 1234);
            assert_eq!(header.dest_port.get(), 80);
            assert_eq!(payload.len(), 0);
        } else {
            panic!("Expected TCP packet");
        }
    }

    #[test]
    fn test_parse_udp() {
        let mut bytes = Vec::new();
        // Ethernet
        bytes.extend_from_slice(&[0x00, 0x11, 0x22, 0x33, 0x44, 0x55]);
        bytes.extend_from_slice(&[0x66, 0x77, 0x88, 0x99, 0xAA, 0xBB]);
        bytes.extend_from_slice(&ether_type::IPV4.to_be_bytes());
        // IPv4
        bytes.extend_from_slice(&[0x45, 0x00]);
        bytes.extend_from_slice(&28u16.to_be_bytes()); // Total Length (20 IP + 8 UDP)
        bytes.extend_from_slice(&0u16.to_be_bytes());
        bytes.extend_from_slice(&0u16.to_be_bytes());
        bytes.extend_from_slice(&[64, IP_P_UDP]);
        bytes.extend_from_slice(&0u16.to_be_bytes());
        bytes.extend_from_slice(&[192, 168, 0, 1]);
        bytes.extend_from_slice(&[192, 168, 0, 2]);
        // UDP
        bytes.extend_from_slice(&1234u16.to_be_bytes()); // Src Port
        bytes.extend_from_slice(&53u16.to_be_bytes()); // Dst Port
        bytes.extend_from_slice(&8u16.to_be_bytes()); // Length
        bytes.extend_from_slice(&0u16.to_be_bytes()); // Checksum

        let packet = parse(&bytes).expect("Failed to parse packet");
        let transport = packet.transport.expect("No transport packet found");
        if let TransportPacket::Udp(header, payload) = transport {
            assert_eq!(header.source_port.get(), 1234);
            assert_eq!(header.dest_port.get(), 53);
            assert_eq!(payload.len(), 0);
        } else {
            panic!("Expected UDP packet");
        }
    }

    #[test]
    fn test_parse_ipv6_tcp() {
        let mut bytes = Vec::new();
        // Ethernet
        bytes.extend_from_slice(&[0x00, 0x11, 0x22, 0x33, 0x44, 0x55]);
        bytes.extend_from_slice(&[0x66, 0x77, 0x88, 0x99, 0xAA, 0xBB]);
        bytes.extend_from_slice(&ether_type::IPV6.to_be_bytes());
        // IPv6
        bytes.extend_from_slice(&0x60000000u32.to_be_bytes());
        bytes.extend_from_slice(&20u16.to_be_bytes()); // Payload Length (20 TCP)
        bytes.extend_from_slice(&[IP_P_TCP, 64]); // Next Header, Hop Limit
        bytes.extend_from_slice(&[0xfe, 0x80, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1]);
        bytes.extend_from_slice(&[0xff, 0x02, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1]);
        // TCP
        bytes.extend_from_slice(&1234u16.to_be_bytes()); // Src Port
        bytes.extend_from_slice(&80u16.to_be_bytes()); // Dst Port
        bytes.extend_from_slice(&0u32.to_be_bytes()); // Seq
        bytes.extend_from_slice(&0u32.to_be_bytes()); // Ack
        bytes.extend_from_slice(&[0x50, 0x02]); // Offset (5), Flags (SYN)
        bytes.extend_from_slice(&0u16.to_be_bytes()); // Window
        bytes.extend_from_slice(&0u16.to_be_bytes()); // Checksum
        bytes.extend_from_slice(&0u16.to_be_bytes()); // Urgent Pointer

        let packet = parse(&bytes).expect("Failed to parse packet");
        let transport = packet.transport.expect("No transport packet found");
        if let TransportPacket::Tcp(header, payload) = transport {
            assert_eq!(header.source_port.get(), 1234);
            assert_eq!(header.dest_port.get(), 80);
            assert_eq!(payload.len(), 0);
        } else {
            panic!("Expected TCP packet");
        }
    }

    #[test]
    fn test_parse_ipv6_udp() {
        let mut bytes = Vec::new();
        // Ethernet
        bytes.extend_from_slice(&[0x00, 0x11, 0x22, 0x33, 0x44, 0x55]);
        bytes.extend_from_slice(&[0x66, 0x77, 0x88, 0x99, 0xAA, 0xBB]);
        bytes.extend_from_slice(&ether_type::IPV6.to_be_bytes());
        // IPv6
        bytes.extend_from_slice(&0x60000000u32.to_be_bytes());
        bytes.extend_from_slice(&8u16.to_be_bytes()); // Payload Length (8 UDP)
        bytes.extend_from_slice(&[IP_P_UDP, 64]); // Next Header, Hop Limit
        bytes.extend_from_slice(&[0xfe, 0x80, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1]);
        bytes.extend_from_slice(&[0xff, 0x02, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1]);
        // UDP
        bytes.extend_from_slice(&1234u16.to_be_bytes()); // Src Port
        bytes.extend_from_slice(&53u16.to_be_bytes()); // Dst Port
        bytes.extend_from_slice(&8u16.to_be_bytes()); // Length
        bytes.extend_from_slice(&0u16.to_be_bytes()); // Checksum

        let packet = parse(&bytes).expect("Failed to parse packet");
        let transport = packet.transport.expect("No transport packet found");
        if let TransportPacket::Udp(header, payload) = transport {
            assert_eq!(header.source_port.get(), 1234);
            assert_eq!(header.dest_port.get(), 53);
            assert_eq!(payload.len(), 0);
        } else {
            panic!("Expected UDP packet");
        }
    }

    #[test]
    fn test_parse_ipv4_rejects_icmpv6() {
        let mut bytes = build_ipv4_icmp_packet();
        // Rewrite the IPv4 protocol field (Ethernet 14 + offset 9) to ICMPv6.
        bytes[14 + 9] = IP_P_ICMPV6;

        let packet = parse(&bytes).expect("Failed to parse packet");
        assert!(matches!(packet.ip, Some(IpPacket::V4(..))));
        assert!(packet.transport.is_none());
    }

    #[test]
    fn test_parse_ipv6_rejects_icmp() {
        let mut bytes = build_ipv6_icmpv6_packet();
        // Rewrite the IPv6 next header field (Ethernet 14 + offset 6) to ICMP.
        bytes[14 + 6] = IP_P_ICMP;

        let packet = parse(&bytes).expect("Failed to parse packet");
        assert!(matches!(packet.ip, Some(IpPacket::V6(..))));
        assert!(packet.transport.is_none());
    }

    #[test]
    fn test_parse_ether_padded_ipv4_udp_tftp_ack() {
        let mut bytes = Vec::new();
        // Ethernet: 14 bytes
        bytes.extend_from_slice(&[0x00, 0x11, 0x22, 0x33, 0x44, 0x55]);
        bytes.extend_from_slice(&[0x66, 0x77, 0x88, 0x99, 0xAA, 0xBB]);
        bytes.extend_from_slice(&ether_type::IPV4.to_be_bytes());
        // IPv4: 20 bytes, total length = 32 (20 IP + 8 UDP + 4 payload)
        bytes.extend_from_slice(&[0x45, 0x00]);
        bytes.extend_from_slice(&32u16.to_be_bytes());
        bytes.extend_from_slice(&0u16.to_be_bytes());
        bytes.extend_from_slice(&0u16.to_be_bytes());
        bytes.extend_from_slice(&[64, IP_P_UDP]);
        bytes.extend_from_slice(&0u16.to_be_bytes());
        bytes.extend_from_slice(&[192, 168, 0, 1]);
        bytes.extend_from_slice(&[192, 168, 0, 2]);
        // UDP: 8 bytes, length = 12 (8 UDP + 4 payload)
        bytes.extend_from_slice(&1234u16.to_be_bytes()); // Src Port
        bytes.extend_from_slice(&69u16.to_be_bytes()); // Dst Port (TFTP)
        bytes.extend_from_slice(&12u16.to_be_bytes()); // Length
        bytes.extend_from_slice(&0u16.to_be_bytes()); // Checksum
        // TFTP ACK: 4 bytes (Opcode 4, Block 1)
        bytes.extend_from_slice(&[0x00, 0x04, 0x00, 0x01]);
        // Ethernet trailing padding to reach 60 bytes minimum frame size (60 - 46 = 14 bytes)
        bytes.extend_from_slice(&[0x00; 14]);

        assert_eq!(bytes.len(), 60);

        let packet = parse(&bytes).expect("Failed to parse packet");
        let ip_packet = packet.ip.expect("No IP packet found");
        if let IpPacket::V4(header, payload) = ip_packet {
            assert_eq!(header.total_length.get(), 32);
            assert_eq!(payload.len(), 12);
        } else {
            panic!("Expected IPv4 packet");
        }

        let transport = packet.transport.expect("No transport packet found");
        if let TransportPacket::Udp(header, payload) = transport {
            assert_eq!(header.length.get(), 12);
            assert_eq!(payload, &[0x00, 0x04, 0x00, 0x01]);
        } else {
            panic!("Expected UDP packet");
        }
    }

    #[test]
    fn test_parse_ipv4_with_options_udp() {
        let mut bytes = Vec::new();
        // Ethernet: 14 bytes
        bytes.extend_from_slice(&[0x00, 0x11, 0x22, 0x33, 0x44, 0x55]);
        bytes.extend_from_slice(&[0x66, 0x77, 0x88, 0x99, 0xAA, 0xBB]);
        bytes.extend_from_slice(&ether_type::IPV4.to_be_bytes());
        // IPv4 with IHL = 6: 24 bytes header (20 fixed + 4 options), total length = 36
        bytes.extend_from_slice(&[0x46, 0x00]);
        bytes.extend_from_slice(&36u16.to_be_bytes());
        bytes.extend_from_slice(&0u16.to_be_bytes());
        bytes.extend_from_slice(&0u16.to_be_bytes());
        bytes.extend_from_slice(&[64, IP_P_UDP]);
        bytes.extend_from_slice(&0u16.to_be_bytes());
        bytes.extend_from_slice(&[192, 168, 0, 1]);
        bytes.extend_from_slice(&[192, 168, 0, 2]);
        // 4 bytes of options
        bytes.extend_from_slice(&[0x01, 0x01, 0x01, 0x01]);
        // UDP: 8 bytes, length = 12
        bytes.extend_from_slice(&1234u16.to_be_bytes());
        bytes.extend_from_slice(&53u16.to_be_bytes());
        bytes.extend_from_slice(&12u16.to_be_bytes());
        bytes.extend_from_slice(&0u16.to_be_bytes());
        // Payload: 4 bytes
        bytes.extend_from_slice(&[10, 20, 30, 40]);
        // 14 bytes trailing padding
        bytes.extend_from_slice(&[0x00; 14]);

        let packet = parse(&bytes).expect("Failed to parse packet");
        let ip_packet = packet.ip.expect("No IP packet found");
        if let IpPacket::V4(header, payload) = ip_packet {
            assert_eq!(header.ihl(), 6);
            assert_eq!(header.header_length(), 24);
            assert_eq!(header.total_length.get(), 36);
            assert_eq!(payload.len(), 12);
        } else {
            panic!("Expected IPv4 packet");
        }

        let transport = packet.transport.expect("No transport packet found");
        if let TransportPacket::Udp(header, payload) = transport {
            assert_eq!(header.length.get(), 12);
            assert_eq!(payload, &[10, 20, 30, 40]);
        } else {
            panic!("Expected UDP packet");
        }
    }
}
