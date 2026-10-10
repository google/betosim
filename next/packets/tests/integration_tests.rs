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

use netsim_packets::{NlAttrHdr, attr_id};
use zerocopy::IntoBytes;

#[test]
fn test_nl_attr_hdr_creation() {
    let hdr = NlAttrHdr::new(8, attr_id::IFACE_MAC);
    assert_eq!(hdr.nla_len.get(), 8);
    assert_eq!(hdr.attr_type(), attr_id::IFACE_MAC);
    assert_eq!(hdr.type_(), attr_id::IFACE_MAC);
}

#[test]
fn test_nl_attr_hdr_serialization() {
    let hdr = NlAttrHdr::new(8, attr_id::IFACE_MAC);
    let bytes = hdr.as_bytes();
    assert_eq!(bytes.len(), 4);
    assert_eq!(bytes[0], 8); // Length (low byte)
    assert_eq!(bytes[1], 0); // Length (high byte)
    assert_eq!(bytes[2], attr_id::IFACE_MAC as u8); // Type (low byte)
    assert_eq!(bytes[3], 0); // Type (high byte)
}

#[test]
fn test_parse_multi_layer_packet() {
    // Ethernet Header (14 bytes)
    // Dst: 00:00:00:00:00:02
    // Src: 00:00:00:00:00:01
    // Type: IPv4 (0x0800)
    let eth_header = [
        0x00, 0x00, 0x00, 0x00, 0x00, 0x02, // Dst
        0x00, 0x00, 0x00, 0x00, 0x00, 0x01, // Src
        0x08, 0x00, // Type
    ];

    // IPv4 Header (20 bytes)
    // Version: 4, IHL: 5
    // TOS: 0
    // Total Length: 40 (20 header + 20 TCP)
    // ID: 0
    // Flags/Frag: 0
    // TTL: 64
    // Protocol: TCP (6)
    // Checksum: 0 (ignored)
    // Src: 192.168.1.1 (C0 A8 01 01)
    // Dst: 192.168.1.2 (C0 A8 01 02)
    let ip_header = [
        0x45, 0x00, 0x00, 0x28, // Ver/IHL, TOS, Len
        0x00, 0x00, 0x00, 0x00, // ID, Flags/Frag
        0x40, 0x06, 0x00, 0x00, // TTL, Proto, Checksum
        0xC0, 0xA8, 0x01, 0x01, // Src
        0xC0, 0xA8, 0x01, 0x02, // Dst
    ];

    // TCP Header (20 bytes)
    // Src Port: 1234 (04 D2)
    // Dst Port: 80 (00 50)
    // Seq: 0
    // Ack: 0
    // Offset: 5, Flags: SYN (0x02)
    // Window: 1024
    // Checksum: 0
    // Urg: 0
    let tcp_header = [
        0x04, 0xD2, 0x00, 0x50, // Src, Dst
        0x00, 0x00, 0x00, 0x00, // Seq
        0x00, 0x00, 0x00, 0x00, // Ack
        0x50, 0x02, 0x04, 0x00, // Offset/Flags, Win
        0x00, 0x00, 0x00, 0x00, // Checksum, Urg
    ];

    let mut packet = Vec::new();
    packet.extend_from_slice(&eth_header);
    packet.extend_from_slice(&ip_header);
    packet.extend_from_slice(&tcp_header);

    // Parse using top-level parser
    use netsim_packets::{TransportPacket, parse};
    let packet = parse(&packet).expect("Failed to parse packet");

    // Verify Ethernet
    if let netsim_packets::EthernetPacket::Untagged { frame, .. } = packet.ethernet {
        assert_eq!(frame.ethertype.get(), 0x0800);
    } else {
        panic!("Expected untagged Ethernet frame");
    }

    // Verify IPv4
    let ip_packet = packet.ip.expect("Expected IP packet");
    if let netsim_packets::IpPacket::V4(header, _) = ip_packet {
        assert_eq!(header.protocol, 6); // TCP
    } else {
        panic!("Expected IPv4 packet");
    }

    // Verify TCP
    let transport_packet = packet.transport.expect("Expected transport packet");
    if let TransportPacket::Tcp(header, _) = transport_packet {
        assert_eq!(header.source_port.get(), 1234);
        assert!(header.syn());
    } else {
        panic!("Expected TCP packet");
    }
}

#[test]
fn test_create_le_ll_packet_payload_length_bounds() {
    use netsim_packets::create_le_ll_packet;

    let mut raw = vec![0u8; 16];
    raw[0] = 0x0B; // LE_LEGACY_ADVERTISING_PDU
    raw[1..7].copy_from_slice(&[0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF]);
    raw[13] = 1;
    raw[14] = 1;
    raw[15] = 0x02;

    // 249 bytes (valid boundary)
    let mut valid_raw = raw.clone();
    valid_raw.extend(vec![0x42; 249]);
    assert!(create_le_ll_packet(&valid_raw).is_some());

    // 250 bytes (oversized: 6 + 250 = 256 > 255)
    let mut invalid_raw = raw;
    invalid_raw.extend(vec![0x42; 250]);
    assert!(create_le_ll_packet(&invalid_raw).is_none());
}
