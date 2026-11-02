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
//! parsed representations of the Ethernet layer, network layer, and transport
//! layer.

use netsim_packets::{
    ArpPacket, EthernetPacket, IP_P_HOPOPTS, IP_P_ICMP, IP_P_ICMPV6, IP_P_TCP, IP_P_UDP,
    IcmpHeader, Icmpv6Header, Ipv4Header, Ipv6Header, Ipv6HopByHopHeader, TcpHeader, UdpHeader,
    ether_type,
};
use zerocopy::Ref;

/// Represents the IP layer of a packet, supporting both IPv4 and IPv6.
/// It holds a reference to the header and a slice for the payload.
#[derive(Debug)]
pub enum IpPacket<'a> {
    V4(Ref<&'a [u8], Ipv4Header>, &'a [u8]),
    V6(Ref<&'a [u8], Ipv6Header>, &'a [u8]),
}

/// Represents the network layer of a packet (Layer 3).
#[derive(Debug)]
pub enum NetworkPacket<'a> {
    Ip(IpPacket<'a>),
    Arp(Ref<&'a [u8], ArpPacket>),
}

/// Represents the transport layer of a packet, currently supporting ICMP and
/// ICMPv6.
#[derive(Debug)]
pub enum TransportPacket<'a> {
    Icmp(Ref<&'a [u8], IcmpHeader>, &'a [u8]),
    Icmpv6(Ref<&'a [u8], Icmpv6Header>, &'a [u8]),
    Tcp(Ref<&'a [u8], TcpHeader>, &'a [u8]),
    Udp(Ref<&'a [u8], UdpHeader>, &'a [u8]),
}

/// A fully parsed packet, containing the Ethernet frame and optional
/// IP and transport layer data.
#[derive(Debug)]
pub struct ParsedPacket<'a> {
    pub ethernet: EthernetPacket<'a>,
    pub network: Option<NetworkPacket<'a>>,
    pub transport: Option<TransportPacket<'a>>,
    pub payload: &'a [u8],
}

impl<'a> ParsedPacket<'a> {
    /// Parses a raw byte slice into a `ParsedPacket`.
    ///
    /// This function serves as the main entry point for packet parsing. It
    /// starts by parsing the Ethernet layer and then iteratively decodes
    /// the encapsulated IP and transport layers based on the `ethertype`
    /// and `protocol` fields.
    pub fn parse(data: &'a [u8]) -> Option<Self> {
        let ethernet = EthernetPacket::parse(data)?;
        let (ethertype, eth_payload) = match &ethernet {
            EthernetPacket::Untagged { frame, payload } => (frame.ethertype.get(), *payload),
            EthernetPacket::Vlan { vlan_header, payload, .. } => {
                (vlan_header.ethertype.get(), *payload)
            }
        };

        let (network, ip_payload) = match ethertype {
            ether_type::IPV4 => {
                let (ipv4_header, ipv4_payload) = Ipv4Header::parse(eth_payload)?;
                (Some(NetworkPacket::Ip(IpPacket::V4(ipv4_header, ipv4_payload))), ipv4_payload)
            }
            ether_type::IPV6 => {
                let (ipv6_header, ipv6_payload) = Ipv6Header::parse(eth_payload)?;
                (Some(NetworkPacket::Ip(IpPacket::V6(ipv6_header, ipv6_payload))), ipv6_payload)
            }
            ether_type::ARP => {
                let arp_packet = ArpPacket::parse(eth_payload)?;
                (Some(NetworkPacket::Arp(arp_packet)), &[] as &[u8])
            }
            _ => (None, eth_payload),
        };

        let (transport, transport_payload) = if let Some(NetworkPacket::Ip(ip_packet)) = &network {
            let (protocol, ip_payload) = match ip_packet {
                IpPacket::V4(header, payload) => (header.protocol, *payload),
                IpPacket::V6(header, payload) => {
                    let mut next_header = header.next_header;
                    let mut ip_payload = *payload;
                    if next_header == IP_P_HOPOPTS {
                        let (hbh_header, hbh_payload) = Ipv6HopByHopHeader::parse(ip_payload)?;
                        next_header = hbh_header.next_header;
                        ip_payload = hbh_payload;
                    }
                    (next_header, ip_payload)
                }
            };
            parse_transport_packet(protocol, ip_payload)?
        } else {
            (None, ip_payload)
        };

        Some(ParsedPacket { ethernet, network, transport, payload: transport_payload })
    }
}

/// Parses the transport layer packet from an IP payload given its protocol or next header.
fn parse_transport_packet<'a>(
    protocol: u8,
    payload: &'a [u8],
) -> Option<(Option<TransportPacket<'a>>, &'a [u8])> {
    match protocol {
        IP_P_ICMP => {
            let (icmp_header, icmp_payload) = IcmpHeader::parse(payload)?;
            Some((Some(TransportPacket::Icmp(icmp_header, icmp_payload)), icmp_payload))
        }
        IP_P_ICMPV6 => {
            let (icmpv6_header, icmpv6_payload) = Icmpv6Header::parse(payload)?;
            Some((Some(TransportPacket::Icmpv6(icmpv6_header, icmpv6_payload)), icmpv6_payload))
        }
        IP_P_TCP => {
            let (tcp_header, tcp_payload) = TcpHeader::parse(payload)?;
            Some((Some(TransportPacket::Tcp(tcp_header, tcp_payload)), tcp_payload))
        }
        IP_P_UDP => {
            let (udp_header, udp_payload) = UdpHeader::parse(payload)?;
            Some((Some(TransportPacket::Udp(udp_header, udp_payload)), udp_payload))
        }
        _ => Some((None, payload)),
    }
}

#[cfg(test)]
mod tests {
    use netsim_packets::ether_type;

    use super::*;

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
        bytes.extend_from_slice(&12u16.to_be_bytes()); // Payload Length
        bytes.extend_from_slice(&[IP_P_ICMPV6, 64]); // Next Header, Hop Limit
        bytes.extend_from_slice(&[0xfe, 0x80, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1]); // Src
        bytes.extend_from_slice(&[0xff, 0x02, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1]); // Dst
        // ICMPv6
        bytes.extend_from_slice(&[128, 0]); // Type, Code
        bytes.extend_from_slice(&0u16.to_be_bytes()); // Checksum
        bytes.extend_from_slice(&[0, 0, 0, 0]); // Body
        bytes.extend_from_slice(&[0, 0, 0, 0]); // Body
        bytes
    }

    #[test]
    fn test_parse_ipv4_icmp() {
        let bytes = build_ipv4_icmp_packet();
        let packet = ParsedPacket::parse(&bytes).expect("Failed to parse packet");

        if let EthernetPacket::Untagged { frame, .. } = packet.ethernet {
            assert_eq!(frame.ethertype.get(), ether_type::IPV4);
        } else {
            panic!("Expected untagged frame");
        }

        let network_packet = packet.network.expect("No network packet found");
        let ip_packet = match network_packet {
            NetworkPacket::Ip(ip) => ip,
            _ => panic!("Expected IP packet"),
        };
        if let IpPacket::V4(header, payload) = ip_packet {
            assert_eq!(header.protocol, IP_P_ICMP);
            assert_eq!(payload.len(), 8);
        } else {
            panic!("Expected IPv4 packet");
        }

        let transport_packet = packet.transport.expect("No transport packet found");
        if let TransportPacket::Icmp(header, payload) = transport_packet {
            assert_eq!(header.icmp_type, 8);
            assert_eq!(header.icmp_code, 0);
            assert_eq!(payload.len(), 0); // 8-byte header consumes all 8 bytes
            let identifier = u16::from_be_bytes([header.rest[0], header.rest[1]]);
            let sequence = u16::from_be_bytes([header.rest[2], header.rest[3]]);
            assert_eq!(identifier, 0);
            assert_eq!(sequence, 1);
        } else {
            panic!("Expected ICMP packet");
        }
    }

    #[test]
    fn test_parse_ipv6_icmpv6() {
        let bytes = build_ipv6_icmpv6_packet();
        let packet = ParsedPacket::parse(&bytes).expect("Failed to parse packet");

        if let EthernetPacket::Untagged { frame, .. } = packet.ethernet {
            assert_eq!(frame.ethertype.get(), ether_type::IPV6);
        } else {
            panic!("Expected untagged frame");
        }

        let network_packet = packet.network.expect("No network packet found");
        let ip_packet = match network_packet {
            NetworkPacket::Ip(ip) => ip,
            _ => panic!("Expected IP packet"),
        };
        if let IpPacket::V6(header, payload) = ip_packet {
            assert_eq!(header.next_header, IP_P_ICMPV6);
            assert_eq!(payload.len(), 12);
        } else {
            panic!("Expected IPv6 packet");
        }

        let transport_packet = packet.transport.expect("No transport packet found");
        if let TransportPacket::Icmpv6(header, payload) = transport_packet {
            assert_eq!(header.icmpv6_type, 128);
            assert_eq!(payload.len(), 4);
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
        bytes.extend_from_slice(&28u16.to_be_bytes()); // Total Length
        bytes.extend_from_slice(&0u16.to_be_bytes()); // Identification
        bytes.extend_from_slice(&0u16.to_be_bytes()); // Flags, Frag Offset
        bytes.extend_from_slice(&[64, 17]); // TTL, Protocol (UDP)
        bytes.extend_from_slice(&0u16.to_be_bytes()); // Checksum
        bytes.extend_from_slice(&[192, 168, 0, 1]); // Src Addr
        bytes.extend_from_slice(&[192, 168, 0, 2]); // Dst Addr
        // UDP
        bytes.extend_from_slice(&12345u16.to_be_bytes());
        bytes.extend_from_slice(&54321u16.to_be_bytes());
        bytes.extend_from_slice(&8u16.to_be_bytes());
        bytes.extend_from_slice(&0u16.to_be_bytes());

        let packet = ParsedPacket::parse(&bytes).expect("Failed to parse packet");
        assert!(matches!(packet.ethernet, EthernetPacket::Vlan { .. }));
        assert!(packet.network.is_some());
        assert!(packet.transport.is_some());
        assert!(matches!(packet.transport.unwrap(), TransportPacket::Udp(..)));
    }

    #[test]
    fn test_parse_ipv4_tcp() {
        let mut bytes = Vec::new();
        // Ethernet
        bytes.extend_from_slice(&[0x00, 0x11, 0x22, 0x33, 0x44, 0x55]);
        bytes.extend_from_slice(&[0x66, 0x77, 0x88, 0x99, 0xAA, 0xBB]);
        bytes.extend_from_slice(&ether_type::IPV4.to_be_bytes());
        // IPv4 (20 bytes header + 20 bytes TCP + 5 bytes payload = 45 bytes total)
        bytes.extend_from_slice(&[0x45, 0x00]);
        bytes.extend_from_slice(&45u16.to_be_bytes());
        bytes.extend_from_slice(&1u16.to_be_bytes());
        bytes.extend_from_slice(&0u16.to_be_bytes());
        bytes.extend_from_slice(&[64, IP_P_TCP]);
        bytes.extend_from_slice(&0u16.to_be_bytes());
        bytes.extend_from_slice(&[192, 168, 0, 1]);
        bytes.extend_from_slice(&[192, 168, 0, 2]);
        // TCP
        bytes.extend_from_slice(&8080u16.to_be_bytes());
        bytes.extend_from_slice(&80u16.to_be_bytes());
        bytes.extend_from_slice(&1000u32.to_be_bytes());
        bytes.extend_from_slice(&0u32.to_be_bytes());
        bytes.extend_from_slice(&(5u16 << 12).to_be_bytes()); // Data offset 5 words = 20 bytes
        bytes.extend_from_slice(&65535u16.to_be_bytes());
        bytes.extend_from_slice(&0u16.to_be_bytes());
        bytes.extend_from_slice(&0u16.to_be_bytes());
        bytes.extend_from_slice(b"hello");

        let packet = ParsedPacket::parse(&bytes).expect("Failed to parse packet");
        assert!(packet.network.is_some());
        let transport = packet.transport.expect("No transport packet found");
        if let TransportPacket::Tcp(header, payload) = transport {
            assert_eq!(header.source_port.get(), 8080);
            assert_eq!(header.dest_port.get(), 80);
            assert_eq!(payload, b"hello");
        } else {
            panic!("Expected TCP packet");
        }
        assert_eq!(packet.payload, b"hello");
    }

    #[test]
    fn test_parse_ipv6_tcp() {
        let mut bytes = Vec::new();
        // Ethernet
        bytes.extend_from_slice(&[0x00, 0x11, 0x22, 0x33, 0x44, 0x55]);
        bytes.extend_from_slice(&[0x66, 0x77, 0x88, 0x99, 0xAA, 0xBB]);
        bytes.extend_from_slice(&ether_type::IPV6.to_be_bytes());
        // IPv6 (40 bytes header, payload length = 20 + 4 = 24)
        bytes.extend_from_slice(&0x60000000u32.to_be_bytes());
        bytes.extend_from_slice(&24u16.to_be_bytes());
        bytes.extend_from_slice(&[IP_P_TCP, 64]);
        bytes.extend_from_slice(&[0xfe, 0x80, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1]);
        bytes.extend_from_slice(&[0xfe, 0x80, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2]);
        // TCP
        bytes.extend_from_slice(&9000u16.to_be_bytes());
        bytes.extend_from_slice(&443u16.to_be_bytes());
        bytes.extend_from_slice(&2000u32.to_be_bytes());
        bytes.extend_from_slice(&0u32.to_be_bytes());
        bytes.extend_from_slice(&(5u16 << 12).to_be_bytes());
        bytes.extend_from_slice(&32768u16.to_be_bytes());
        bytes.extend_from_slice(&0u16.to_be_bytes());
        bytes.extend_from_slice(&0u16.to_be_bytes());
        bytes.extend_from_slice(b"ping");

        let packet = ParsedPacket::parse(&bytes).expect("Failed to parse packet");
        let transport = packet.transport.expect("No transport packet found");
        if let TransportPacket::Tcp(header, payload) = transport {
            assert_eq!(header.source_port.get(), 9000);
            assert_eq!(header.dest_port.get(), 443);
            assert_eq!(payload, b"ping");
        } else {
            panic!("Expected TCP packet");
        }
        assert_eq!(packet.payload, b"ping");
    }

    #[test]
    fn test_parse_ipv6_udp() {
        let mut bytes = Vec::new();
        // Ethernet
        bytes.extend_from_slice(&[0x00, 0x11, 0x22, 0x33, 0x44, 0x55]);
        bytes.extend_from_slice(&[0x66, 0x77, 0x88, 0x99, 0xAA, 0xBB]);
        bytes.extend_from_slice(&ether_type::IPV6.to_be_bytes());
        // IPv6 (40 bytes header, payload length = 8 + 4 = 12)
        bytes.extend_from_slice(&0x60000000u32.to_be_bytes());
        bytes.extend_from_slice(&12u16.to_be_bytes());
        bytes.extend_from_slice(&[IP_P_UDP, 64]);
        bytes.extend_from_slice(&[0xfe, 0x80, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1]);
        bytes.extend_from_slice(&[0xfe, 0x80, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2]);
        // UDP
        bytes.extend_from_slice(&5353u16.to_be_bytes());
        bytes.extend_from_slice(&5353u16.to_be_bytes());
        bytes.extend_from_slice(&12u16.to_be_bytes());
        bytes.extend_from_slice(&0u16.to_be_bytes());
        bytes.extend_from_slice(b"mdns");

        let packet = ParsedPacket::parse(&bytes).expect("Failed to parse packet");
        let transport = packet.transport.expect("No transport packet found");
        if let TransportPacket::Udp(header, payload) = transport {
            assert_eq!(header.source_port.get(), 5353);
            assert_eq!(header.dest_port.get(), 5353);
            assert_eq!(payload, b"mdns");
        } else {
            panic!("Expected UDP packet");
        }
        assert_eq!(packet.payload, b"mdns");
    }

    #[test]
    fn test_parse_unknown_transport() {
        let mut bytes = Vec::new();
        // Ethernet
        bytes.extend_from_slice(&[0x00, 0x11, 0x22, 0x33, 0x44, 0x55]);
        bytes.extend_from_slice(&[0x66, 0x77, 0x88, 0x99, 0xAA, 0xBB]);
        bytes.extend_from_slice(&ether_type::IPV4.to_be_bytes());
        // IPv4 with unhandled protocol 253
        bytes.extend_from_slice(&[0x45, 0x00]);
        bytes.extend_from_slice(&24u16.to_be_bytes());
        bytes.extend_from_slice(&1u16.to_be_bytes());
        bytes.extend_from_slice(&0u16.to_be_bytes());
        bytes.extend_from_slice(&[64, 253]);
        bytes.extend_from_slice(&0u16.to_be_bytes());
        bytes.extend_from_slice(&[192, 168, 0, 1]);
        bytes.extend_from_slice(&[192, 168, 0, 2]);
        bytes.extend_from_slice(b"raw!");

        let packet = ParsedPacket::parse(&bytes).expect("Failed to parse packet");
        assert!(packet.network.is_some());
        assert!(packet.transport.is_none());
        assert_eq!(packet.payload, b"raw!");
    }
}
