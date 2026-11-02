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

//! Integration Tests: IPv4 Fragment Reassembly, IGMP Multicast Join, & ICMPv6
//! Packet Too Big.

use std::net::{Ipv4Addr, Ipv6Addr};

use netsim_packets::{
    EthernetFrame, IP_P_ICMPV6, IP_P_UDP, Icmpv6Header, Icmpv6Type, Ipv4Builder, Ipv6Builder,
    MacAddr,
};
use slirp_actor::SlirpBackend;
use zerocopy::FromBytes;

use super::test_driver::SlirpTestDriver;

fn create_ipv4_fragment(id: u16, flags_offset: u16, payload_len: usize) -> bytes::Bytes {
    let payload = vec![0xABu8; payload_len];
    let mut ip_data = vec![0u8; 20 + payload_len];
    let mut ipv4_builder = Ipv4Builder::new(
        &mut ip_data,
        IP_P_UDP,
        Ipv4Addr::new(10, 0, 2, 15),
        Ipv4Addr::new(10, 0, 2, 2),
    )
    .unwrap();
    ipv4_builder.identification(id);
    ipv4_builder.flags_fragment_offset(flags_offset);
    ipv4_builder.payload(&payload).unwrap();
    ipv4_builder.build().unwrap();

    let mut eth_data = vec![0u8; 14 + ip_data.len()];
    let (eth_header_slice, eth_payload_slice) = eth_data.split_at_mut(14);
    let eth_frame = EthernetFrame::mut_from_bytes(eth_header_slice).unwrap();
    eth_frame.dst_addr = MacAddr { bytes: [0x52, 0x54, 0x00, 0x12, 0x34, 0x56] };
    eth_frame.src_addr = MacAddr { bytes: [0x02, 0x15, 0xb2, 0x00, 0x00, 0x00] };
    eth_frame.ethertype = 0x0800.into();
    eth_payload_slice.copy_from_slice(&ip_data);

    bytes::Bytes::from(eth_data)
}

fn create_igmp_v2_report_packet(group_ip: Ipv4Addr) -> bytes::Bytes {
    let mut igmp_data = vec![0u8; 8];
    igmp_data[0] = 0x16; // Type: IGMPv2 Membership Report
    igmp_data[1] = 0x00; // Max Response Time
    igmp_data[2..4].copy_from_slice(&[0, 0]); // Checksum zero initially
    igmp_data[4..8].copy_from_slice(&group_ip.octets());

    let checksum = netsim_packets::ipv4_checksum(&igmp_data);
    igmp_data[2..4].copy_from_slice(&checksum.to_be_bytes());

    let mut ip_data = vec![0u8; 20 + igmp_data.len()];
    let mut ipv4_builder = Ipv4Builder::new(
        &mut ip_data,
        2, // Protocol IGMP
        Ipv4Addr::new(10, 0, 2, 15),
        group_ip,
    )
    .unwrap();
    ipv4_builder.ttl(1);
    ipv4_builder.payload(&igmp_data).unwrap();
    ipv4_builder.build().unwrap();

    let mut eth_data = vec![0u8; 14 + ip_data.len()];
    let (eth_header_slice, eth_payload_slice) = eth_data.split_at_mut(14);
    let eth_frame = EthernetFrame::mut_from_bytes(eth_header_slice).unwrap();
    let group_octets = group_ip.octets();
    eth_frame.dst_addr = MacAddr {
        bytes: [0x01, 0x00, 0x5e, group_octets[1] & 0x7f, group_octets[2], group_octets[3]],
    };
    eth_frame.src_addr = MacAddr { bytes: [0x02, 0x15, 0xb2, 0x00, 0x00, 0x00] };
    eth_frame.ethertype = 0x0800.into();
    eth_payload_slice.copy_from_slice(&ip_data);

    bytes::Bytes::from(eth_data)
}

fn create_icmpv6_pkt_too_big_packet() -> bytes::Bytes {
    let src_ip = Ipv6Addr::new(0xfe80, 0, 0, 0, 0x0215, 0xb2ff, 0xfe00, 0x0015);
    let dst_ip = Ipv6Addr::new(0xfe80, 0, 0, 0, 0, 0, 0, 2);

    let icmp_header_len = std::mem::size_of::<Icmpv6Header>();
    let payload = vec![0u8; 40];
    let total_icmp_len = icmp_header_len + payload.len();

    let mut icmp_data = vec![0u8; total_icmp_len];
    let (icmp_hdr_slice, icmp_payload_slice) = icmp_data.split_at_mut(icmp_header_len);
    let icmp_header = Icmpv6Header::mut_from_bytes(icmp_hdr_slice).unwrap();
    icmp_header.icmpv6_type = Icmpv6Type::PacketTooBig as u8;
    icmp_header.icmpv6_code = 0;
    icmp_header.icmpv6_checksum.set(0);
    icmp_header.set_mtu(1280);
    icmp_payload_slice.copy_from_slice(&payload);

    let checksum = netsim_packets::icmpv6_checksum(&icmp_data, src_ip, dst_ip);
    let icmp_header_mut = Icmpv6Header::mut_from_bytes(&mut icmp_data[..icmp_header_len]).unwrap();
    icmp_header_mut.icmpv6_checksum.set(checksum);

    let mut ip_data = vec![0u8; 40 + total_icmp_len];
    let mut ipv6_builder = Ipv6Builder::new(&mut ip_data, IP_P_ICMPV6, src_ip, dst_ip).unwrap();
    ipv6_builder.payload(&icmp_data).unwrap();
    ipv6_builder.build().unwrap();

    let mut eth_data = vec![0u8; 14 + ip_data.len()];
    let (eth_header_slice, eth_payload_slice) = eth_data.split_at_mut(14);
    let eth_frame = EthernetFrame::mut_from_bytes(eth_header_slice).unwrap();
    eth_frame.dst_addr = MacAddr { bytes: [0x52, 0x54, 0x00, 0x12, 0x34, 0x56] };
    eth_frame.src_addr = MacAddr { bytes: [0x02, 0x15, 0xb2, 0x00, 0x00, 0x00] };
    eth_frame.ethertype = 0x86DD.into();
    eth_payload_slice.copy_from_slice(&ip_data);

    bytes::Bytes::from(eth_data)
}

// -----------------------------------------------------------------------------
// 1. IPv4 Fragment Reassembly
// -----------------------------------------------------------------------------

async fn run_ipv4_fragment_reassembly_test(backend: SlirpBackend) {
    // Given a Slirp driver running the specified backend
    let mut driver = SlirpTestDriver::new(backend, None).await;

    // When guest sends 2 IPv4 fragments (MF=1 with fragment offset 0, followed by
    // MF=0 with fragment offset 185)
    let frag1 = create_ipv4_fragment(0x1234, 0x2000, 1480);
    let frag2 = create_ipv4_fragment(0x1234, 185, 100);

    let result1 = driver.send_packet(frag1).await;
    let result2 = driver.send_packet(frag2).await;

    // Then Slirp accepts both fragments successfully
    assert!(result1.is_ok());
    assert!(result2.is_ok());
}

#[tokio::test]
async fn test_ipv4_fragment_reassembly() {
    for backend in [SlirpBackend::CFfi, SlirpBackend::Native] {
        run_ipv4_fragment_reassembly_test(backend).await;
    }
}

// -----------------------------------------------------------------------------
// 2. IGMP Multicast Join
// -----------------------------------------------------------------------------

async fn run_igmp_multicast_join_test(backend: SlirpBackend) {
    // Given a Slirp driver running the specified backend
    let mut driver = SlirpTestDriver::new(backend, None).await;

    // When guest sends an IGMPv2 Membership Report packet for multicast group
    // 224.0.0.22
    let igmp_pkt = create_igmp_v2_report_packet(Ipv4Addr::new(224, 0, 0, 22));
    let result = driver.send_packet(igmp_pkt).await;

    // Then Slirp processes the IGMP Membership Report packet
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_igmp_multicast_join() {
    for backend in [SlirpBackend::CFfi, SlirpBackend::Native] {
        run_igmp_multicast_join_test(backend).await;
    }
}

// -----------------------------------------------------------------------------
// 3. ICMPv6 Packet Too Big
// -----------------------------------------------------------------------------

async fn run_icmpv6_pkt_too_big_test(backend: SlirpBackend) {
    // Given a Slirp driver running the specified backend
    let mut driver = SlirpTestDriver::new(backend, None).await;

    // When guest sends an ICMPv6 Packet Too Big message with MTU 1280
    let icmpv6_pkt = create_icmpv6_pkt_too_big_packet();
    let result = driver.send_packet(icmpv6_pkt).await;

    // Then Slirp accepts the ICMPv6 Packet Too Big message
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_icmpv6_pkt_too_big() {
    for backend in [SlirpBackend::CFfi, SlirpBackend::Native] {
        run_icmpv6_pkt_too_big_test(backend).await;
    }
}
