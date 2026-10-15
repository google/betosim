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

//! Integration Tests: Advanced IPv6 Handling (RA with RDNSS, DHCPv6 Solicit,
//! ICMPv6 Ping Gateway).

use std::net::Ipv6Addr;

use netsim_packets::{
    DHCPV6_CLIENT_PORT, DHCPV6_SERVER_PORT, EthernetFrame, IP_P_ICMPV6, IP_P_UDP, Icmpv6Header,
    Icmpv6Type, Ipv6Builder, MacAddr, RouterAdvertisementBuilder, UdpBuilder,
};
use slirp_actor::SlirpBackend;
use zerocopy::FromBytes;

use super::test_driver::SlirpTestDriver;

/// Helper to create an IPv6 Router Advertisement (RA) packet containing
/// RDNSS DNS option and SLAAC prefix info.
fn create_ipv6_ra_rdnss_packet() -> bytes::Bytes {
    let src_ip = Ipv6Addr::new(0xfe80, 0, 0, 0, 0x0215, 0xb2ff, 0xfe00, 0);
    let dst_ip = Ipv6Addr::new(0xff02, 0, 0, 0, 0, 0, 0, 1);

    let mut icmp_data = vec![0u8; 80];
    let icmp_header = Icmpv6Header::mut_from_bytes(&mut icmp_data[..8]).unwrap();
    icmp_header.icmpv6_type = Icmpv6Type::RouterAdvertisement as u8;
    icmp_header.icmpv6_code = 0;
    icmp_header.icmpv6_checksum.set(0);

    let builder = RouterAdvertisementBuilder::new(&mut icmp_data[4..]).unwrap();
    let prefix = [0x20, 0x01, 0x0d, 0xb8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    let gateway_mac = [0x52, 0x54, 0x00, 0x12, 0x34, 0x56];
    let dns_server = [0xfe, 0x80, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 3];
    builder.build(64, 0, 1800, 0, 0, gateway_mac, prefix, 64, Some(dns_server)).unwrap();

    let checksum = netsim_packets::icmpv6_checksum(&icmp_data, src_ip, dst_ip);
    let icmp_header_mut = Icmpv6Header::mut_from_bytes(&mut icmp_data[..8]).unwrap();
    icmp_header_mut.icmpv6_checksum.set(checksum);

    let mut ip_data = vec![0u8; 40 + icmp_data.len()];
    let mut ipv6_builder = Ipv6Builder::new(&mut ip_data, IP_P_ICMPV6, src_ip, dst_ip).unwrap();
    ipv6_builder.payload(&icmp_data).unwrap();
    ipv6_builder.build().unwrap();

    let mut eth_data = vec![0u8; 14 + ip_data.len()];
    let (eth_header_slice, eth_payload_slice) = eth_data.split_at_mut(14);
    let eth_frame = EthernetFrame::mut_from_bytes(eth_header_slice).unwrap();
    eth_frame.dst_addr = MacAddr { bytes: [0x33, 0x33, 0x00, 0x00, 0x00, 0x01] };
    eth_frame.src_addr = MacAddr { bytes: [0x02, 0x15, 0xb2, 0x00, 0x00, 0x00] };
    eth_frame.ethertype = 0x86DD.into();
    eth_payload_slice.copy_from_slice(&ip_data);

    bytes::Bytes::from(eth_data)
}

/// Helper to create a DHCPv6 Solicit packet (UDP src 546 -> dst 547, Message
/// Type 1).
fn create_dhcpv6_solicit_packet() -> bytes::Bytes {
    let src_ip = Ipv6Addr::new(0xfe80, 0, 0, 0, 0x0215, 0xb2ff, 0xfe00, 0);
    let dst_ip = Ipv6Addr::new(0xff02, 0, 0, 0, 0, 0, 1, 2);

    let mut dhcpv6_payload = vec![0u8; 4 + 14];
    dhcpv6_payload[0] = 1; // Message Type 1: Solicit
    dhcpv6_payload[1..4].copy_from_slice(&[0x12, 0x34, 0x56]); // Transaction ID

    // Option 1: Client ID
    dhcpv6_payload[4..6].copy_from_slice(&1u16.to_be_bytes());
    dhcpv6_payload[6..8].copy_from_slice(&10u16.to_be_bytes());
    dhcpv6_payload[8..18]
        .copy_from_slice(&[0x00, 0x01, 0x00, 0x01, 0x2b, 0x3c, 0x4d, 0x5e, 0x02, 0x15]);

    let mut udp_data = vec![0u8; 8 + dhcpv6_payload.len()];
    let mut udp_builder =
        UdpBuilder::new_v6(&mut udp_data, src_ip, dst_ip, DHCPV6_CLIENT_PORT, DHCPV6_SERVER_PORT)
            .unwrap();
    udp_builder.payload(&dhcpv6_payload).unwrap();
    udp_builder.build().unwrap();

    let mut ip_data = vec![0u8; 40 + udp_data.len()];
    let mut ipv6_builder = Ipv6Builder::new(&mut ip_data, IP_P_UDP, src_ip, dst_ip).unwrap();
    ipv6_builder.payload(&udp_data).unwrap();
    ipv6_builder.build().unwrap();

    let mut eth_data = vec![0u8; 14 + ip_data.len()];
    let (eth_header_slice, eth_payload_slice) = eth_data.split_at_mut(14);
    let eth_frame = EthernetFrame::mut_from_bytes(eth_header_slice).unwrap();
    eth_frame.dst_addr = MacAddr { bytes: [0x33, 0x33, 0x00, 0x01, 0x00, 0x02] };
    eth_frame.src_addr = MacAddr { bytes: [0x02, 0x15, 0xb2, 0x00, 0x00, 0x00] };
    eth_frame.ethertype = 0x86DD.into();
    eth_payload_slice.copy_from_slice(&ip_data);

    bytes::Bytes::from(eth_data)
}

/// Helper to create an ICMPv6 Echo Request packet (Type 128, Code 0) targeting
/// gateway link-local `fe80::2`.
fn create_icmpv6_ping_gateway_packet() -> bytes::Bytes {
    let src_ip = Ipv6Addr::new(0xfe80, 0, 0, 0, 0x0215, 0xb2ff, 0xfe00, 0);
    let dst_ip = Ipv6Addr::new(0xfe80, 0, 0, 0, 0, 0, 0, 2);

    let icmp_header_len = std::mem::size_of::<Icmpv6Header>();
    let payload = b"netsim_icmpv6_ping_gateway";
    let total_icmp_len = icmp_header_len + payload.len();

    let mut icmp_data = vec![0u8; total_icmp_len];
    let (icmp_hdr_slice, icmp_payload_slice) = icmp_data.split_at_mut(icmp_header_len);
    let icmp_header = Icmpv6Header::mut_from_bytes(icmp_hdr_slice).unwrap();
    icmp_header.icmpv6_type = Icmpv6Type::EchoRequest as u8;
    icmp_header.icmpv6_code = 0;
    icmp_header.icmpv6_checksum.set(0);
    icmp_header.set_echo_fields(0x1234, 1);
    icmp_payload_slice.copy_from_slice(payload);

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
// 1. IPv6 Router Advertisement with RDNSS Option Test
// -----------------------------------------------------------------------------

async fn run_ipv6_ra_rdnss_option_test(backend: SlirpBackend) {
    // Given a Slirp driver running the specified backend
    let mut driver = SlirpTestDriver::new(backend, None).await;

    // When guest receives or processes an IPv6 Router Advertisement with RDNSS DNS
    // option and SLAAC prefix info
    let ra_packet = create_ipv6_ra_rdnss_packet();
    let result = driver.send_packet(ra_packet).await;

    // Then Slirp handles the Router Advertisement packet successfully
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_ipv6_ra_rdnss_option() {
    for backend in [SlirpBackend::CFfi, SlirpBackend::Native] {
        run_ipv6_ra_rdnss_option_test(backend).await;
    }
}

// -----------------------------------------------------------------------------
// 2. DHCPv6 Solicit Test
// -----------------------------------------------------------------------------

async fn run_dhcpv6_solicit_test(backend: SlirpBackend) {
    // Given a Slirp driver running the specified backend
    let mut driver = SlirpTestDriver::new(backend, None).await;

    // When guest sends a DHCPv6 Solicit packet (UDP 546 -> 547, Message Type 1)
    let solicit_packet = create_dhcpv6_solicit_packet();
    let result = driver.send_packet(solicit_packet).await;

    // Then Slirp accepts and processes the DHCPv6 Solicit packet
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_dhcpv6_solicit() {
    for backend in [SlirpBackend::CFfi, SlirpBackend::Native] {
        run_dhcpv6_solicit_test(backend).await;
    }
}

// -----------------------------------------------------------------------------
// 3. ICMPv6 Ping Gateway Test
// -----------------------------------------------------------------------------

async fn run_icmpv6_ping_gateway_test(backend: SlirpBackend) {
    // Given a Slirp driver running the specified backend
    let mut driver = SlirpTestDriver::new(backend, None).await;

    // When guest sends an ICMPv6 Echo Request packet (Type 128, Code 0) to gateway
    // link-local fe80::2
    let ping_packet = create_icmpv6_ping_gateway_packet();
    let result = driver.send_packet(ping_packet).await;

    // Then Slirp processes the ICMPv6 Echo Request packet
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_icmpv6_ping_gateway() {
    for backend in [SlirpBackend::CFfi, SlirpBackend::Native] {
        run_icmpv6_ping_gateway_test(backend).await;
    }
}
