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

//! Integration Tests: IPv6 NDP & SLAAC Auto-Configuration (b/560172253).

use std::net::Ipv6Addr;

use netsim_packets::{
    DHCPV6_CLIENT_PORT, DHCPV6_SERVER_PORT, Dhcpv6Header, EthernetFrame, IP_P_ICMPV6, IP_P_UDP,
    Icmpv6Header, Icmpv6Type, Ipv6Header, MSG_INFORMATION_REQUEST, MSG_REPLY, NDP_HOP_LIMIT,
    NeighborAdvertisement, NeighborSolicitation, OPTION_CLIENTID, PacketBuilder,
    RouterAdvertisement, SourceLinkLayerAddressOption, TargetLinkLayerAddressOption, UdpHeader,
    ether_type, na_flags, ndp_option, udp_checksum_v6,
};
use slirp_actor::SlirpBackend;
use zerocopy::FromBytes;

use super::test_driver::SlirpTestDriver;

/// Guest MAC address, matching the EUI-64 host bits of `GUEST_LINK_LOCAL`.
const GUEST_MAC: [u8; 6] = [0x02, 0x15, 0xb2, 0x00, 0x00, 0x00];
/// Guest link-local address, derived from the guest MAC via EUI-64.
const GUEST_LINK_LOCAL: Ipv6Addr = Ipv6Addr::new(0xfe80, 0, 0, 0, 0x0215, 0xb2ff, 0xfe00, 0);
/// ff02::2, the all-routers multicast group.
const ALL_ROUTERS: Ipv6Addr = Ipv6Addr::new(0xff02, 0, 0, 0, 0, 0, 0, 2);
/// All-routers multicast MAC address (33:33:00:00:00:02).
const ALL_ROUTERS_MAC: [u8; 6] = [0x33, 0x33, 0x00, 0x00, 0x00, 0x02];
/// ff02::1:ff00:2, the solicited-node multicast group for fec0::2.
const SOLICITED_NODE: Ipv6Addr = Ipv6Addr::new(0xff02, 0, 0, 0, 0, 1, 0xff00, 2);
/// Solicited-node multicast MAC address (33:33:ff:00:00:02).
const SOLICITED_NODE_MAC: [u8; 6] = [0x33, 0x33, 0xff, 0x00, 0x00, 0x02];
/// fec0::2, the gateway address being resolved.
const GATEWAY_IPV6: Ipv6Addr = Ipv6Addr::new(0xfec0, 0, 0, 0, 0, 0, 0, 2);
const GATEWAY_LINK_LOCAL: Ipv6Addr = Ipv6Addr::new(0xfe80, 0, 0, 0, 0, 0, 0, 2);
const RA_CUR_HOP_LIMIT: u8 = 64;
const TEST_BUF_CAP: usize = 256;

fn create_router_solicitation_packet() -> bytes::Bytes {
    let frame = PacketBuilder::new(ALL_ROUTERS_MAC, GUEST_MAC, ether_type::IPV6)
        .ipv6(GUEST_LINK_LOCAL.octets(), ALL_ROUTERS.octets(), IP_P_ICMPV6)
        .hop_limit(NDP_HOP_LIMIT)
        .icmpv6(Icmpv6Type::RouterSolicitation as u8, 0, [0; 4])
        .build();

    bytes::Bytes::from(frame)
}

fn create_neighbor_solicitation_packet(target_ip: Ipv6Addr) -> bytes::Bytes {
    let mut ns_buf = [0u8; TEST_BUF_CAP];
    let initial_len = ns_buf.len();
    let (ns, rest) =
        NeighborSolicitation::mut_from_prefix(&mut ns_buf).expect("buffer too small for NS");
    ns.target_addr = target_ip.octets();
    let (slla, rest) =
        SourceLinkLayerAddressOption::mut_from_prefix(rest).expect("buffer too small for SLLA");
    *slla = SourceLinkLayerAddressOption::new(GUEST_MAC);
    let ns_len = initial_len - rest.len();
    let ns_payload = ns_buf.get(..ns_len).expect("slice NS payload");

    let frame = PacketBuilder::new(SOLICITED_NODE_MAC, GUEST_MAC, ether_type::IPV6)
        .ipv6(GUEST_LINK_LOCAL.octets(), SOLICITED_NODE.octets(), IP_P_ICMPV6)
        .hop_limit(NDP_HOP_LIMIT)
        .icmpv6(Icmpv6Type::NeighborSolicitation as u8, 0, [0; 4])
        .payload(ns_payload);

    bytes::Bytes::from(frame)
}

// -----------------------------------------------------------------------------
// 1. IPv6 Router Solicitation & SLAAC Test
// -----------------------------------------------------------------------------

async fn run_ipv6_router_solicitation_test(backend: SlirpBackend) {
    // Given a Slirp driver running the specified backend
    let mut driver = SlirpTestDriver::new(backend, None).await;

    // When guest sends IPv6 Router Solicitation
    let rs_packet = create_router_solicitation_packet();
    assert!(driver.send_packet(rs_packet).await.is_ok());

    // Then Slirp answers Router Solicitation with Router Advertisement
    let is_ra = |packet: &bytes::Bytes| {
        EthernetFrame::parse(packet)
            .filter(|(eth, _)| eth.ethertype.get() == ether_type::IPV6)
            .and_then(|(_, payload)| Ipv6Header::parse(payload))
            .filter(|(ip, _)| ip.next_header == IP_P_ICMPV6)
            .and_then(|(_, payload)| Icmpv6Header::parse(payload))
            .is_some_and(|(icmp, _)| icmp.icmpv6_type == Icmpv6Type::RouterAdvertisement as u8)
    };
    let reply =
        tokio::time::timeout(std::time::Duration::from_millis(500), driver.recv_matching(is_ra))
            .await
            .expect("Timeout waiting for Router Advertisement reply")
            .expect("Expected a Router Advertisement packet");

    let (_, eth_payload) =
        EthernetFrame::parse(&reply).expect("Failed to parse reply Ethernet frame");
    let (_, ip_payload) = Ipv6Header::parse(eth_payload).expect("Failed to parse IPv6 header");
    let (icmpv6, icmpv6_payload) =
        Icmpv6Header::parse(ip_payload).expect("Failed to parse ICMPv6 header");
    assert_eq!(icmpv6.ra_fields().0, RA_CUR_HOP_LIMIT, "Expected cur_hop_limit == 64");

    let _ra = RouterAdvertisement::parse(icmpv6_payload)
        .expect("Failed to parse Router Advertisement payload");
}

#[tokio::test]
async fn test_ipv6_router_solicitation() {
    for backend in [SlirpBackend::CFfi, SlirpBackend::Native] {
        run_ipv6_router_solicitation_test(backend).await;
    }
}

// -----------------------------------------------------------------------------
// 2. IPv6 Neighbor Solicitation Test
// -----------------------------------------------------------------------------

async fn run_ipv6_neighbor_solicitation_test(backend: SlirpBackend, target_ip: Ipv6Addr) {
    // Given a Slirp driver running the specified backend
    let mut driver = SlirpTestDriver::new(backend, None).await;

    // When guest sends Neighbor Solicitation for gateway
    let ns_packet = create_neighbor_solicitation_packet(target_ip);
    assert!(driver.send_packet(ns_packet).await.is_ok());

    // Then Slirp answers Neighbor Solicitation with a Neighbor Advertisement for the gateway
    let is_na = |packet: &bytes::Bytes| {
        EthernetFrame::parse(packet)
            .filter(|(eth, _)| eth.ethertype.get() == ether_type::IPV6)
            .and_then(|(_, payload)| Ipv6Header::parse(payload))
            .filter(|(ip, _)| ip.next_header == IP_P_ICMPV6)
            .and_then(|(_, payload)| Icmpv6Header::parse(payload))
            .is_some_and(|(icmp, _)| icmp.icmpv6_type == Icmpv6Type::NeighborAdvertisement as u8)
    };
    let reply =
        tokio::time::timeout(std::time::Duration::from_millis(500), driver.recv_matching(is_na))
            .await
            .expect("Timeout waiting for Neighbor Advertisement reply")
            .expect("Expected a Neighbor Advertisement packet");

    let (eth, eth_payload) =
        EthernetFrame::parse(&reply).expect("Failed to parse reply Ethernet frame");
    assert_eq!(eth.dst_addr.bytes, GUEST_MAC, "Advertisement must target the guest");

    let (ip, ip_payload) = Ipv6Header::parse(eth_payload).expect("Failed to parse IPv6 header");
    assert_eq!(
        Ipv6Addr::from(ip.source_addr),
        target_ip,
        "Advertisement must come from the solicited target"
    );

    let (icmpv6, icmpv6_payload) =
        Icmpv6Header::parse(ip_payload).expect("Failed to parse ICMPv6 header");
    assert_eq!(
        icmpv6.na_flags(),
        na_flags::ROUTER | na_flags::SOLICITED | na_flags::OVERRIDE,
        "Expected R|S|O flags"
    );

    let (na, rest) = NeighborAdvertisement::ref_from_prefix(icmpv6_payload)
        .expect("Failed to parse Neighbor Advertisement");
    assert_eq!(Ipv6Addr::from(na.target_addr), target_ip, "Target address must match solicited IP");

    let (tlla, rest) = TargetLinkLayerAddressOption::ref_from_prefix(rest)
        .expect("Failed to parse Target Link-Layer Address option");
    assert_eq!(tlla.option_type, ndp_option::TARGET_LINK_LAYER_ADDR);
    assert_eq!(tlla.length, TargetLinkLayerAddressOption::LENGTH_UNITS);
    assert_eq!(tlla.addr, eth.src_addr.bytes);
    assert!(rest.is_empty());
}

#[tokio::test]
async fn test_ipv6_neighbor_solicitation_c_ffi() {
    run_ipv6_neighbor_solicitation_test(SlirpBackend::CFfi, GATEWAY_IPV6).await;
}

#[tokio::test]
async fn test_ipv6_neighbor_solicitation_native() {
    run_ipv6_neighbor_solicitation_test(SlirpBackend::Native, GATEWAY_IPV6).await;
}

#[tokio::test]
async fn test_ipv6_neighbor_solicitation_link_local_c_ffi() {
    run_ipv6_neighbor_solicitation_test(SlirpBackend::CFfi, GATEWAY_LINK_LOCAL).await;
}

#[tokio::test]
async fn test_ipv6_neighbor_solicitation_link_local_native() {
    run_ipv6_neighbor_solicitation_test(SlirpBackend::Native, GATEWAY_LINK_LOCAL).await;
}

// -----------------------------------------------------------------------------
// 3. DHCPv6 Information-Request & Reply UDP Checksum Test (b/565540890)
// -----------------------------------------------------------------------------

#[tokio::test]
async fn test_dhcpv6_information_request_reply_udp_checksum() {
    let mut driver = SlirpTestDriver::new(SlirpBackend::Native, None).await;

    // Build DHCPv6 Information-Request carrying Option Client ID
    let client_duid = [&[0x00, 0x03, 0x00, 0x01][..], &GUEST_MAC].concat();
    let transaction_id = [0x12, 0x34, 0x56];
    let dhcpv6_payload = [
        &[MSG_INFORMATION_REQUEST][..],
        &transaction_id,
        &OPTION_CLIENTID.to_be_bytes(),
        &(client_duid.len() as u16).to_be_bytes(),
        &client_duid,
    ]
    .concat();

    let dst_ipv6 = Ipv6Addr::new(0xff02, 0, 0, 0, 0, 0, 1, 2); // All_DHCP_Relay_Agents_and_Servers

    let req_packet = bytes::Bytes::from(
        PacketBuilder::new([0x33, 0x33, 0x00, 0x01, 0x00, 0x02], GUEST_MAC, ether_type::IPV6)
            .ipv6(GUEST_LINK_LOCAL.octets(), dst_ipv6.octets(), IP_P_UDP)
            .hop_limit(64)
            .udp(DHCPV6_CLIENT_PORT, DHCPV6_SERVER_PORT)
            .payload(&dhcpv6_payload),
    );

    driver.send_packet(req_packet).await.expect("Failed to send DHCPv6 Information-Request");

    // Wait for DHCPv6 Reply (UDP 547 -> 546)
    let is_dhcpv6_reply = |frame: &bytes::Bytes| {
        EthernetFrame::parse(frame)
            .filter(|(eth, _)| eth.ethertype.get() == ether_type::IPV6)
            .and_then(|(_, payload)| Ipv6Header::parse(payload))
            .filter(|(ip, _)| ip.next_header == IP_P_UDP)
            .and_then(|(_, payload)| UdpHeader::parse(payload))
            .is_some_and(|(udp, _)| {
                udp.source_port.get() == DHCPV6_SERVER_PORT
                    && udp.dest_port.get() == DHCPV6_CLIENT_PORT
            })
    };
    let reply_opt = tokio::time::timeout(
        std::time::Duration::from_millis(500),
        driver.recv_matching(is_dhcpv6_reply),
    )
    .await;

    let reply_frame = reply_opt
        .expect("Timed out waiting for DHCPv6 Reply")
        .expect("Failed to receive DHCPv6 Reply frame");

    let (_, eth_payload) = EthernetFrame::parse(&reply_frame).unwrap();
    let (reply_ipv6, ip_payload) = Ipv6Header::parse(eth_payload).unwrap();
    let (reply_udp, udp_payload) = UdpHeader::parse(ip_payload).unwrap();

    assert_eq!(reply_udp.source_port.get(), DHCPV6_SERVER_PORT);
    assert_eq!(reply_udp.dest_port.get(), DHCPV6_CLIENT_PORT);
    assert_ne!(reply_udp.checksum.get(), 0, "DHCPv6 Reply UDP checksum must not be 0");

    let src_ip = Ipv6Addr::from(reply_ipv6.source_addr);
    let dst_ip = Ipv6Addr::from(reply_ipv6.dest_addr);

    assert_eq!(
        udp_checksum_v6(ip_payload, src_ip, dst_ip),
        u16::MAX,
        "UDP checksum over packet including checksum must be 0xFFFF"
    );

    let (dhcpv6_hdr, _) = Dhcpv6Header::parse(udp_payload).unwrap();
    assert_eq!(dhcpv6_hdr.msg_type, MSG_REPLY);
    assert_eq!(dhcpv6_hdr.transaction_id, transaction_id);
}
