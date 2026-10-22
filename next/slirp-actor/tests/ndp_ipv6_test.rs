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
    Icmpv6Header, Icmpv6Type, Ipv6Header, MSG_INFORMATION_REQUEST, MSG_REPLY, MacAddr,
    OPTION_CLIENTID, PacketBuilder, UdpHeader, ether_type, icmpv6_checksum, udp_checksum_v6,
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
/// ff02::1:ff00:2, the solicited-node multicast group for fec0::2.
const SOLICITED_NODE: Ipv6Addr = Ipv6Addr::new(0xff02, 0, 0, 0, 0, 1, 0xff00, 2);
/// fec0::2, the gateway address being resolved.
const GATEWAY_IPV6: Ipv6Addr = Ipv6Addr::new(0xfec0, 0, 0, 0, 0, 0, 0, 2);

fn create_router_solicitation_packet() -> bytes::Bytes {
    let mut eth_data = vec![0u8; 14 + 40 + 8];
    let (eth_header_slice, eth_payload_slice) = eth_data.split_at_mut(14);
    let eth_frame = EthernetFrame::mut_from_bytes(eth_header_slice).unwrap();
    eth_frame.dst_addr = MacAddr { bytes: [0x33, 0x33, 0x00, 0x00, 0x00, 0x02] }; // All-Routers multicast
    eth_frame.src_addr = MacAddr { bytes: [0x02, 0x15, 0xb2, 0x00, 0x00, 0x00] };
    eth_frame.ethertype = 0x86DD.into();

    eth_payload_slice[0] = 0x60;
    eth_payload_slice[4] = 0x00;
    eth_payload_slice[5] = 0x08;
    eth_payload_slice[6] = 58; // ICMPv6
    eth_payload_slice[7] = 255;

    eth_payload_slice[8..24].copy_from_slice(&GUEST_LINK_LOCAL.octets());
    eth_payload_slice[24..40].copy_from_slice(&ALL_ROUTERS.octets());

    // ICMPv6 Router Solicitation: Type 133, Code 0
    eth_payload_slice[40] = 133;
    eth_payload_slice[41] = 0;

    // ICMPv6 mandates a checksum over the IPv6 pseudo-header (RFC 4443 2.3);
    // unlike UDP over IPv4 there is no "zero means unchecked" exemption.
    let checksum = icmpv6_checksum(&eth_payload_slice[40..], GUEST_LINK_LOCAL, ALL_ROUTERS);
    eth_payload_slice[42..44].copy_from_slice(&checksum.to_be_bytes());

    bytes::Bytes::from(eth_data)
}

fn create_neighbor_solicitation_packet() -> bytes::Bytes {
    let mut eth_data = vec![0u8; 14 + 40 + 32];
    let (eth_header_slice, eth_payload_slice) = eth_data.split_at_mut(14);
    let eth_frame = EthernetFrame::mut_from_bytes(eth_header_slice).unwrap();
    eth_frame.dst_addr = MacAddr { bytes: [0x33, 0x33, 0xff, 0x00, 0x00, 0x02] }; // Solicited-node multicast
    eth_frame.src_addr = MacAddr { bytes: GUEST_MAC };
    eth_frame.ethertype = 0x86DD.into();

    eth_payload_slice[0] = 0x60;
    eth_payload_slice[4] = 0x00;
    eth_payload_slice[5] = 32; // ICMPv6 length: 24-byte NS + 8-byte option
    eth_payload_slice[6] = 58; // ICMPv6
    eth_payload_slice[7] = 255;

    eth_payload_slice[8..24].copy_from_slice(&GUEST_LINK_LOCAL.octets());
    eth_payload_slice[24..40].copy_from_slice(&SOLICITED_NODE.octets());

    // ICMPv6 Neighbor Solicitation: Type 135, Code 0
    eth_payload_slice[40] = 135;
    eth_payload_slice[41] = 0;
    eth_payload_slice[48..64].copy_from_slice(&GATEWAY_IPV6.octets());

    // Source Link-Layer Address option (type 1, length 1 unit of 8 bytes).
    // RFC 4861 4.3 says a solicitation sent from a unicast address should carry
    // it, and it is what lets the responder answer without soliciting back.
    eth_payload_slice[64] = 1;
    eth_payload_slice[65] = 1;
    eth_payload_slice[66..72].copy_from_slice(&GUEST_MAC);

    // ICMPv6 mandates a checksum over the IPv6 pseudo-header (RFC 4443 2.3).
    let checksum = icmpv6_checksum(&eth_payload_slice[40..], GUEST_LINK_LOCAL, SOLICITED_NODE);
    eth_payload_slice[42..44].copy_from_slice(&checksum.to_be_bytes());

    bytes::Bytes::from(eth_data)
}

// -----------------------------------------------------------------------------
// 1. IPv6 Router Solicitation & SLAAC Test
// -----------------------------------------------------------------------------

async fn run_ipv6_router_solicitation_test(backend: SlirpBackend) {
    // Given a Slirp driver running the specified backend
    let mut driver = SlirpTestDriver::new(backend, None).await;

    // When guest sends IPv6 Router Solicitation
    let rs_packet = create_router_solicitation_packet();
    let result = driver.send_packet(rs_packet).await;

    // Then Slirp accepts and processes the Router Solicitation
    assert!(result.is_ok());
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

async fn run_ipv6_neighbor_solicitation_test(backend: SlirpBackend) {
    // Given a Slirp driver running the specified backend
    let mut driver = SlirpTestDriver::new(backend, None).await;

    // When guest sends Neighbor Solicitation for gateway fe80::2
    let ns_packet = create_neighbor_solicitation_packet();
    let result = driver.send_packet(ns_packet).await;

    // Then Slirp accepts Neighbor Solicitation packet
    assert!(result.is_ok());

    // And, if it answers, the answer must be a Neighbor Advertisement for the
    // gateway addressed back to the guest.
    //
    // NOTE: only the C backend answers today; the native backend stays silent,
    // tracked in b/565480441. Make this unconditional once that is fixed.
    if let Ok(Some(reply)) =
        tokio::time::timeout(std::time::Duration::from_millis(500), driver.recv_packet()).await
    {
        let (eth, eth_payload) =
            EthernetFrame::parse(&reply).expect("Failed to parse reply Ethernet frame");
        assert_eq!(eth.ethertype.get(), 0x86DD, "Reply must be IPv6");
        assert_eq!(eth.dst_addr.bytes, GUEST_MAC, "Advertisement must target the guest");

        let (ip, ip_payload) = Ipv6Header::parse(eth_payload).expect("Failed to parse IPv6 header");
        assert_eq!(ip.next_header, IP_P_ICMPV6, "Reply must be ICMPv6");

        let (icmpv6, _) = Icmpv6Header::parse(ip_payload).expect("Failed to parse ICMPv6 header");
        assert_eq!(
            icmpv6.icmpv6_type,
            Icmpv6Type::NeighborAdvertisement as u8,
            "Expected a Neighbor Advertisement"
        );
        assert_eq!(
            Ipv6Addr::from(ip.source_addr),
            GATEWAY_IPV6,
            "Advertisement must come from the solicited target"
        );
    }
}

#[tokio::test]
async fn test_ipv6_neighbor_solicitation() {
    for backend in [SlirpBackend::CFfi, SlirpBackend::Native] {
        run_ipv6_neighbor_solicitation_test(backend).await;
    }
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
