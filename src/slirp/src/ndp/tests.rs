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
    EthernetFrame, IP_P_ICMPV6, Icmpv6Header, Icmpv6Type, Ipv6Header, MacAddr, NDP_HOP_LIMIT,
    NeighborAdvertisement, NeighborSolicitation, PrefixInformationOption, RdnssOption,
    RouterAdvertisement, SourceLinkLayerAddressOption, TargetLinkLayerAddressOption, ether_type,
    icmpv6_checksum, na_flags, ndp_option,
};
use zerocopy::{FromBytes, IntoBytes};

use super::{
    ALL_NODES_MAC, ALL_NODES_MULTICAST_V6, GATEWAY_LINK_LOCAL_IPV6, NdpReply, NdpTable,
    RA_CUR_HOP_LIMIT, RA_ROUTER_LIFETIME_SECS, SLAAC_PREFIX_LEN, handle_ndp_packet,
};
use crate::Config;

const GUEST_MAC: MacAddr = MacAddr { bytes: [0x02, 0x15, 0xb2, 0x00, 0x00, 0x00] };
const GUEST_LINK_LOCAL: Ipv6Addr = Ipv6Addr::new(0xfe80, 0, 0, 0, 0x0215, 0xb2ff, 0xfe00, 0);
const ALL_ROUTERS_MULTICAST: Ipv6Addr = Ipv6Addr::new(0xff02, 0, 0, 0, 0, 0, 0, 2);
const TEST_BUF_CAP: usize = 256;

#[test]
fn test_ndp_table_creation_and_lookup() {
    let mut ndp_table = NdpTable::new();
    let guest_ip = Ipv6Addr::new(0x2001, 0xdb8, 0, 0, 0, 0, 0, 1);
    let guest_mac = MacAddr::new([0x52, 0x54, 0x00, 0x12, 0x34, 0x56]);

    assert!(ndp_table.lookup(&guest_ip).is_none());
    ndp_table.add_entry(guest_ip, guest_mac);
    assert_eq!(ndp_table.lookup(&guest_ip), Some(guest_mac));

    let _default_table = NdpTable::default();
}

#[test]
fn test_handle_ndp_packet_neighbor_solicitation_host_ipv6() {
    let config = Config::default();
    let ipv6_hdr =
        Ipv6Header::new(GUEST_LINK_LOCAL.octets(), config.host_ipv6.octets(), IP_P_ICMPV6);

    let ns = NeighborSolicitation { target_addr: config.host_ipv6.octets() };

    let icmpv6_hdr = Icmpv6Header {
        icmpv6_type: Icmpv6Type::NeighborSolicitation as u8,
        icmpv6_code: 0,
        icmpv6_checksum: 0.into(),
        rest: [0; 4],
    };

    let reply = handle_ndp_packet(&config, GUEST_MAC, &ipv6_hdr, &icmpv6_hdr, ns.as_bytes());
    let Some(NdpReply::NeighborAdvertisement {
        src_mac,
        dst_mac,
        src_ip,
        dst_ip,
        target_ip,
        target_mac,
        flags,
    }) = reply
    else {
        panic!("Expected NeighborAdvertisement reply");
    };

    assert_eq!(src_mac, config.gateway_mac);
    assert_eq!(dst_mac, GUEST_MAC);
    assert_eq!(src_ip, config.host_ipv6);
    assert_eq!(dst_ip, GUEST_LINK_LOCAL);
    assert_eq!(target_ip, config.host_ipv6);
    assert_eq!(target_mac, config.gateway_mac);
    assert_eq!(flags, na_flags::ROUTER | na_flags::SOLICITED | na_flags::OVERRIDE);
}

#[test]
fn test_handle_ndp_packet_neighbor_solicitation_gateway_link_local() {
    let config = Config::default();
    let ipv6_hdr =
        Ipv6Header::new(GUEST_LINK_LOCAL.octets(), GATEWAY_LINK_LOCAL_IPV6.octets(), IP_P_ICMPV6);

    let ns = NeighborSolicitation { target_addr: GATEWAY_LINK_LOCAL_IPV6.octets() };

    let icmpv6_hdr = Icmpv6Header {
        icmpv6_type: Icmpv6Type::NeighborSolicitation as u8,
        icmpv6_code: 0,
        icmpv6_checksum: 0.into(),
        rest: [0; 4],
    };

    let reply = handle_ndp_packet(&config, GUEST_MAC, &ipv6_hdr, &icmpv6_hdr, ns.as_bytes());
    let Some(NdpReply::NeighborAdvertisement { src_ip, target_ip, .. }) = reply else {
        panic!("Expected NeighborAdvertisement reply");
    };

    assert_eq!(src_ip, GATEWAY_LINK_LOCAL_IPV6);
    assert_eq!(target_ip, GATEWAY_LINK_LOCAL_IPV6);
}

#[test]
fn test_handle_ndp_packet_neighbor_solicitation_dad() {
    let config = Config::default();
    let ipv6_hdr =
        Ipv6Header::new(Ipv6Addr::UNSPECIFIED.octets(), config.host_ipv6.octets(), IP_P_ICMPV6);

    let ns = NeighborSolicitation { target_addr: config.host_ipv6.octets() };

    let icmpv6_hdr = Icmpv6Header {
        icmpv6_type: Icmpv6Type::NeighborSolicitation as u8,
        icmpv6_code: 0,
        icmpv6_checksum: 0.into(),
        rest: [0; 4],
    };

    let reply = handle_ndp_packet(&config, GUEST_MAC, &ipv6_hdr, &icmpv6_hdr, ns.as_bytes());
    let Some(NdpReply::NeighborAdvertisement { dst_mac, dst_ip, flags, .. }) = reply else {
        panic!("Expected NeighborAdvertisement reply");
    };

    assert_eq!(dst_mac, ALL_NODES_MAC);
    assert_eq!(dst_ip, ALL_NODES_MULTICAST_V6);
    assert_eq!(flags, na_flags::ROUTER | na_flags::OVERRIDE);
}

#[test]
fn test_handle_ndp_packet_neighbor_solicitation_ignored() {
    let config = Config::default();
    let other_ip = Ipv6Addr::new(0x2001, 0xdb8, 0, 0, 0, 0, 0, 99);
    let ipv6_hdr = Ipv6Header::new(GUEST_LINK_LOCAL.octets(), other_ip.octets(), IP_P_ICMPV6);

    let ns = NeighborSolicitation { target_addr: other_ip.octets() };

    let icmpv6_hdr = Icmpv6Header {
        icmpv6_type: Icmpv6Type::NeighborSolicitation as u8,
        icmpv6_code: 0,
        icmpv6_checksum: 0.into(),
        rest: [0; 4],
    };

    let reply = handle_ndp_packet(&config, GUEST_MAC, &ipv6_hdr, &icmpv6_hdr, ns.as_bytes());
    assert!(reply.is_none());
}

#[test]
fn test_handle_ndp_packet_router_solicitation_unicast() {
    let config = Config::default();
    let ipv6_hdr =
        Ipv6Header::new(GUEST_LINK_LOCAL.octets(), ALL_ROUTERS_MULTICAST.octets(), IP_P_ICMPV6);

    let icmpv6_hdr = Icmpv6Header {
        icmpv6_type: Icmpv6Type::RouterSolicitation as u8,
        icmpv6_code: 0,
        icmpv6_checksum: 0.into(),
        rest: [0; 4],
    };

    let reply = handle_ndp_packet(&config, GUEST_MAC, &ipv6_hdr, &icmpv6_hdr, &[]);
    let Some(NdpReply::RouterAdvertisement {
        src_mac,
        dst_mac,
        src_ip,
        dst_ip,
        prefix,
        prefix_len,
        dns_server,
    }) = reply
    else {
        panic!("Expected RouterAdvertisement reply");
    };

    assert_eq!(src_mac, config.gateway_mac);
    assert_eq!(dst_mac, GUEST_MAC);
    assert_eq!(src_ip, config.host_ipv6);
    assert_eq!(dst_ip, GUEST_LINK_LOCAL);
    assert_eq!(prefix, Ipv6Addr::new(0xfec0, 0, 0, 0, 0, 0, 0, 0).octets());
    assert_eq!(prefix_len, SLAAC_PREFIX_LEN);
    assert_eq!(dns_server, config.host_ipv6);
}

#[test]
fn test_handle_ndp_packet_router_solicitation_unspecified() {
    let config = Config::default();
    let ipv6_hdr = Ipv6Header::new(
        Ipv6Addr::UNSPECIFIED.octets(),
        ALL_ROUTERS_MULTICAST.octets(),
        IP_P_ICMPV6,
    );

    let icmpv6_hdr = Icmpv6Header {
        icmpv6_type: Icmpv6Type::RouterSolicitation as u8,
        icmpv6_code: 0,
        icmpv6_checksum: 0.into(),
        rest: [0; 4],
    };

    let reply = handle_ndp_packet(&config, GUEST_MAC, &ipv6_hdr, &icmpv6_hdr, &[]);
    let Some(NdpReply::RouterAdvertisement { dst_mac, dst_ip, .. }) = reply else {
        panic!("Expected RouterAdvertisement reply");
    };

    assert_eq!(dst_mac, ALL_NODES_MAC);
    assert_eq!(dst_ip, ALL_NODES_MULTICAST_V6);
}

#[test]
fn test_handle_ndp_packet_non_ndp_ignored() {
    let config = Config::default();
    let ipv6_hdr =
        Ipv6Header::new(GUEST_LINK_LOCAL.octets(), config.host_ipv6.octets(), IP_P_ICMPV6);
    let icmpv6_hdr = Icmpv6Header {
        icmpv6_type: Icmpv6Type::EchoRequest as u8,
        icmpv6_code: 0,
        icmpv6_checksum: 0.into(),
        rest: [0; 4],
    };

    let reply = handle_ndp_packet(&config, GUEST_MAC, &ipv6_hdr, &icmpv6_hdr, &[]);
    assert!(reply.is_none());
}

#[test]
fn test_periodic_router_advertisement() {
    let config = Config::default();
    let reply = NdpReply::periodic_router_advertisement(&config);
    let NdpReply::RouterAdvertisement { dst_mac, dst_ip, .. } = &reply else {
        panic!("Expected RouterAdvertisement");
    };
    assert_eq!(*dst_mac, ALL_NODES_MAC);
    assert_eq!(*dst_ip, ALL_NODES_MULTICAST_V6);
}

#[test]
fn test_encode_neighbor_advertisement() {
    let config = Config::default();
    let reply = NdpReply::NeighborAdvertisement {
        src_mac: config.gateway_mac,
        dst_mac: GUEST_MAC,
        src_ip: config.host_ipv6,
        dst_ip: GUEST_LINK_LOCAL,
        target_ip: config.host_ipv6,
        target_mac: config.gateway_mac,
        flags: na_flags::ROUTER | na_flags::SOLICITED | na_flags::OVERRIDE,
    };

    let mut frame_buf = [0u8; TEST_BUF_CAP];
    let written = reply.encode(&mut frame_buf).unwrap();

    let (eth, rest) = EthernetFrame::ref_from_prefix(&frame_buf[..written]).unwrap();
    assert_eq!(eth.dst_addr, GUEST_MAC);
    assert_eq!(eth.src_addr, config.gateway_mac);
    assert_eq!(eth.ethertype.get(), ether_type::IPV6);

    let (ip_hdr, ip_payload) = Ipv6Header::ref_from_prefix(rest).unwrap();
    assert_eq!(Ipv6Addr::from(ip_hdr.source_addr), config.host_ipv6);
    assert_eq!(Ipv6Addr::from(ip_hdr.dest_addr), GUEST_LINK_LOCAL);
    assert_eq!(ip_hdr.next_header, IP_P_ICMPV6);
    assert_eq!(ip_hdr.hop_limit, NDP_HOP_LIMIT);
    assert_eq!(usize::from(ip_hdr.payload_length.get()), ip_payload.len());

    let (icmp_hdr, rest) = Icmpv6Header::ref_from_prefix(ip_payload).unwrap();
    assert_eq!(icmp_hdr.icmpv6_type, Icmpv6Type::NeighborAdvertisement as u8);
    assert_eq!(icmp_hdr.icmpv6_code, 0);
    assert_eq!(icmp_hdr.na_flags(), na_flags::ROUTER | na_flags::SOLICITED | na_flags::OVERRIDE);

    let (na, rest) = NeighborAdvertisement::ref_from_prefix(rest).unwrap();
    assert_eq!(Ipv6Addr::from(na.target_addr), config.host_ipv6);

    let (tlla, rest) = TargetLinkLayerAddressOption::ref_from_prefix(rest).unwrap();
    assert_eq!(tlla.option_type, ndp_option::TARGET_LINK_LAYER_ADDR);
    assert_eq!(tlla.length, TargetLinkLayerAddressOption::LENGTH_UNITS);
    assert_eq!(tlla.addr, config.gateway_mac.bytes);
    assert!(rest.is_empty());

    assert_eq!(icmpv6_checksum(ip_payload, config.host_ipv6, GUEST_LINK_LOCAL), 0);
}

#[test]
fn test_encode_router_advertisement() {
    let config = Config::default();
    let reply = NdpReply::periodic_router_advertisement(&config);

    let mut frame_buf = [0u8; TEST_BUF_CAP];
    let written = reply.encode(&mut frame_buf).unwrap();

    let (eth, rest) = EthernetFrame::ref_from_prefix(&frame_buf[..written]).unwrap();
    assert_eq!(eth.dst_addr, ALL_NODES_MAC);
    assert_eq!(eth.src_addr, config.gateway_mac);
    assert_eq!(eth.ethertype.get(), ether_type::IPV6);

    let (ip_hdr, ip_payload) = Ipv6Header::ref_from_prefix(rest).unwrap();
    assert_eq!(Ipv6Addr::from(ip_hdr.source_addr), config.host_ipv6);
    assert_eq!(Ipv6Addr::from(ip_hdr.dest_addr), ALL_NODES_MULTICAST_V6);
    assert_eq!(ip_hdr.next_header, IP_P_ICMPV6);
    assert_eq!(ip_hdr.hop_limit, NDP_HOP_LIMIT);
    assert_eq!(usize::from(ip_hdr.payload_length.get()), ip_payload.len());

    let (icmp_hdr, rest) = Icmpv6Header::ref_from_prefix(ip_payload).unwrap();
    assert_eq!(icmp_hdr.icmpv6_type, Icmpv6Type::RouterAdvertisement as u8);
    assert_eq!(icmp_hdr.icmpv6_code, 0);
    assert_eq!(icmp_hdr.ra_fields(), (RA_CUR_HOP_LIMIT, 0, RA_ROUTER_LIFETIME_SECS));

    let (ra, rest) = RouterAdvertisement::ref_from_prefix(rest).unwrap();
    assert_eq!(ra.reachable_time.get(), 0);
    assert_eq!(ra.retrans_timer.get(), 0);

    let (slla, rest) = SourceLinkLayerAddressOption::ref_from_prefix(rest).unwrap();
    assert_eq!(slla.option_type, ndp_option::SOURCE_LINK_LAYER_ADDR);
    assert_eq!(slla.length, SourceLinkLayerAddressOption::LENGTH_UNITS);
    assert_eq!(slla.addr, config.gateway_mac.bytes);

    let (pio, rest) = PrefixInformationOption::ref_from_prefix(rest).unwrap();
    assert_eq!(pio.option_type, ndp_option::PREFIX_INFORMATION);
    assert_eq!(pio.length, PrefixInformationOption::LENGTH_UNITS);
    assert_eq!(pio.prefix_length, SLAAC_PREFIX_LEN);

    let (rdnss, rest) = RdnssOption::ref_from_prefix(rest).unwrap();
    assert_eq!(rdnss.option_type, ndp_option::RDNSS);
    assert_eq!(rdnss.length, RdnssOption::LENGTH_UNITS);
    assert_eq!(rdnss.lifetime.get(), u32::from(RA_ROUTER_LIFETIME_SECS));
    assert_eq!(rdnss.dns_servers, [config.host_ipv6.octets()]);
    assert!(rest.is_empty());

    assert_eq!(icmpv6_checksum(ip_payload, config.host_ipv6, ALL_NODES_MULTICAST_V6), 0);
}

#[test]
fn test_ndp_reply_to_packet() {
    let config = Config::default();
    let reply = NdpReply::periodic_router_advertisement(&config);
    let packet = reply.to_packet().unwrap();
    assert!(!packet.is_empty());
}

#[test]
fn test_ndp_reply_encode_short_buffers() {
    let config = Config::default();
    let na = NdpReply::NeighborAdvertisement {
        src_mac: config.gateway_mac,
        dst_mac: GUEST_MAC,
        src_ip: config.host_ipv6,
        dst_ip: GUEST_LINK_LOCAL,
        target_ip: config.host_ipv6,
        target_mac: config.gateway_mac,
        flags: 0,
    };
    let mut buf = [0u8; TEST_BUF_CAP];
    let full_na_len = na.encode(&mut buf).unwrap();
    assert!(na.encode(&mut buf[..full_na_len - 1]).is_none());

    let ra = NdpReply::periodic_router_advertisement(&config);
    let full_ra_len = ra.encode(&mut buf).unwrap();
    assert!(ra.encode(&mut buf[..full_ra_len - 1]).is_none());
}
