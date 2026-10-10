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

use std::net::Ipv4Addr;

use netsim_packets::{ArpPacket, ArpPacketBuilder, MacAddr, ether_type};

use crate::arp::arp_impl::ArpTable;

#[test]
fn test_arp_table_creation() {
    let _arp_table = ArpTable::new();
}

#[test]
fn test_arp_reply_generation() {
    let mut arp_table = ArpTable::new();
    let my_ip = Ipv4Addr::new(10, 0, 2, 2);
    let my_mac = MacAddr::new([0x52, 0x54, 0x00, 0x12, 0x34, 0x56]);
    arp_table.add_entry(my_ip, my_mac);

    // Create a mock ARP request packet.
    let mut request_buf = [0u8; 28];
    let sender_mac = MacAddr::new([1, 2, 3, 4, 5, 6]);
    let sender_ip = [10, 0, 2, 15];
    let builder = ArpPacketBuilder::new(&mut request_buf).unwrap();
    builder
        .hardware_type(1)
        .protocol_type(ether_type::IPV4)
        .hardware_addr_len(6)
        .protocol_addr_len(4)
        .opcode(1) // Request
        .sender_hardware_addr(sender_mac)
        .sender_protocol_addr(sender_ip)
        .target_hardware_addr(MacAddr::new([0; 6]))
        .target_protocol_addr(my_ip.octets())
        .build();

    let request_packet = ArpPacket::parse(&request_buf).unwrap();
    let reply = arp_table.handle_packet(&request_packet).unwrap();
    let reply_packet = ArpPacket::parse(&reply).unwrap();

    // Assert that the reply is a valid "is-at" packet.
    assert_eq!(reply_packet.opcode.get(), 2); // Opcode for reply
    assert_eq!(reply_packet.sender_hardware_addr.bytes, my_mac.bytes);
    assert_eq!(reply_packet.sender_protocol_addr, my_ip.octets());
    assert_eq!(reply_packet.target_hardware_addr.bytes, sender_mac.bytes);
    assert_eq!(reply_packet.target_protocol_addr, sender_ip);
}

#[test]
fn test_arp_default() {
    let _default_table = ArpTable::default();
}

#[test]
fn test_arp_handle_packet_edge_cases() {
    let mut arp_table = ArpTable::new();
    let my_ip = Ipv4Addr::new(10, 0, 2, 2);
    let my_mac = MacAddr::new([0x52, 0x54, 0x00, 0x12, 0x34, 0x56]);
    arp_table.add_entry(my_ip, my_mac);

    let sender_mac = MacAddr::new([1, 2, 3, 4, 5, 6]);
    let sender_ip = [10, 0, 2, 15];

    // Case 1: Hardware type is not 1 (e.g. 2)
    let mut buf = [0u8; 28];
    let builder = ArpPacketBuilder::new(&mut buf).unwrap();
    builder
        .hardware_type(2) // Invalid hardware type
        .protocol_type(ether_type::IPV4)
        .hardware_addr_len(6)
        .protocol_addr_len(4)
        .opcode(1)
        .sender_hardware_addr(sender_mac)
        .sender_protocol_addr(sender_ip)
        .target_hardware_addr(MacAddr::new([0; 6]))
        .target_protocol_addr(my_ip.octets())
        .build();
    let packet = ArpPacket::parse(&buf).unwrap();
    assert!(arp_table.handle_packet(&packet).is_none());

    // Case 2: Protocol type is not 0x0800 (e.g. 0x0806)
    let mut buf = [0u8; 28];
    let builder = ArpPacketBuilder::new(&mut buf).unwrap();
    builder
        .hardware_type(1)
        .protocol_type(0x0806) // Invalid protocol type
        .hardware_addr_len(6)
        .protocol_addr_len(4)
        .opcode(1)
        .sender_hardware_addr(sender_mac)
        .sender_protocol_addr(sender_ip)
        .target_hardware_addr(MacAddr::new([0; 6]))
        .target_protocol_addr(my_ip.octets())
        .build();
    let packet = ArpPacket::parse(&buf).unwrap();
    assert!(arp_table.handle_packet(&packet).is_none());

    // Case 3: Opcode is not 1 (e.g. 2 - Reply)
    let mut buf = [0u8; 28];
    let builder = ArpPacketBuilder::new(&mut buf).unwrap();
    builder
        .hardware_type(1)
        .protocol_type(ether_type::IPV4)
        .hardware_addr_len(6)
        .protocol_addr_len(4)
        .opcode(2) // Reply instead of Request
        .sender_hardware_addr(sender_mac)
        .sender_protocol_addr(sender_ip)
        .target_hardware_addr(MacAddr::new([0; 6]))
        .target_protocol_addr(my_ip.octets())
        .build();
    let packet = ArpPacket::parse(&buf).unwrap();
    assert!(arp_table.handle_packet(&packet).is_none());

    // Case 4: Target IP is not in cache (e.g. 10.0.2.3)
    let mut buf = [0u8; 28];
    let builder = ArpPacketBuilder::new(&mut buf).unwrap();
    builder
        .hardware_type(1)
        .protocol_type(ether_type::IPV4)
        .hardware_addr_len(6)
        .protocol_addr_len(4)
        .opcode(1)
        .sender_hardware_addr(sender_mac)
        .sender_protocol_addr(sender_ip)
        .target_hardware_addr(MacAddr::new([0; 6]))
        .target_protocol_addr([10, 0, 2, 3]) // IP not in cache
        .build();
    let packet = ArpPacket::parse(&buf).unwrap();
    assert!(arp_table.handle_packet(&packet).is_none());
}
