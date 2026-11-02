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

use std::net::Ipv6Addr;

use netsim_packets::{
    MacAddr, NeighborAdvertisement, NeighborSolicitation, NeighborSolicitationBuilder,
};
use zerocopy::Ref;

use crate::ndp::ndp_impl::NdpTable;

#[test]
fn test_ndp_table_creation() {
    let _ndp_table = NdpTable::new();
}

#[test]
fn test_ndp_reply_generation() {
    let mut ndp_table = NdpTable::new();
    let my_ip = Ipv6Addr::new(0xfe80, 0, 0, 0, 0, 0, 0, 1);
    let my_mac = MacAddr::new([0x52, 0x54, 0x00, 0x12, 0x34, 0x56]);
    ndp_table.add_entry(my_ip, my_mac);

    // Create a mock Neighbor Solicitation packet.
    let mut request_buf = [0u8; std::mem::size_of::<NeighborSolicitation>()];
    let builder = NeighborSolicitationBuilder::new(&mut request_buf).unwrap();
    builder.target_addr(my_ip.octets()).build();
    let (request_packet, _) =
        Ref::<_, NeighborSolicitation>::from_prefix(&request_buf[..]).unwrap();

    let reply = ndp_table.handle_packet(&request_packet).unwrap();
    let (reply_packet, _) = Ref::<_, NeighborAdvertisement>::from_prefix(&reply[..]).unwrap();

    // Assert that the reply is a valid "is-at" packet.
    assert_eq!(reply_packet.flags, 0b01100000); // Router, Solicited, Override
    assert_eq!(reply_packet.target_addr, my_ip.octets());
}

#[test]
fn test_ndp_default() {
    let _default_table = NdpTable::default();
}

#[test]
fn test_ndp_handle_packet_edge_cases() {
    let mut ndp_table = NdpTable::new();
    let my_ip = Ipv6Addr::new(0xfe80, 0, 0, 0, 0, 0, 0, 1);
    let my_mac = MacAddr::new([0x52, 0x54, 0x00, 0x12, 0x34, 0x56]);
    ndp_table.add_entry(my_ip, my_mac);

    // Case 1: Target IP not in cache (e.g. fe80::2)
    let mut request_buf = [0u8; std::mem::size_of::<NeighborSolicitation>()];
    let builder = NeighborSolicitationBuilder::new(&mut request_buf).unwrap();
    builder.target_addr(Ipv6Addr::new(0xfe80, 0, 0, 0, 0, 0, 0, 2).octets()).build();
    let (request_packet, _) =
        Ref::<_, NeighborSolicitation>::from_prefix(&request_buf[..]).unwrap();

    assert!(ndp_table.handle_packet(&request_packet).is_none());
}
