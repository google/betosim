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
    EthernetFrame, Icmpv6Header, Icmpv6Type, Icmpv6UnreachableCode, Ipv6Header, MacAddr,
};
use zerocopy::{FromBytes, IntoBytes, U16};

use crate::{Config, SlirpResponse, icmp::Icmpv6Manager, packet::ParsedPacket};

fn create_icmpv6_echo_request(
    src_ip: Ipv6Addr,
    dest_ip: Ipv6Addr,
    payload: &[u8],
    hop_limit: u8,
) -> (Ipv6Header, Vec<u8>) {
    let icmp_len = 8 + payload.len();
    let mut icmp_buf = vec![0u8; icmp_len];
    let (header_slice, payload_slice) = icmp_buf.split_at_mut(8);

    let icmp_header = Icmpv6Header::mut_from_bytes(header_slice).unwrap();
    icmp_header.icmpv6_type = Icmpv6Type::EchoRequest as u8;
    icmp_header.icmpv6_code = 0;
    icmp_header.icmpv6_checksum = 0.into();
    icmp_header.set_echo_fields(1, 1);

    payload_slice[..payload.len()].copy_from_slice(payload);

    let ipv6_header = Ipv6Header {
        version_tc_fl: 0x60000000.into(),
        payload_length: U16::new(icmp_len as u16),
        next_header: netsim_packets::IP_P_ICMPV6,
        hop_limit,
        source_addr: src_ip.octets(),
        dest_addr: dest_ip.octets(),
    };

    (ipv6_header, icmp_buf)
}

#[test]
fn test_echo_reply() {
    let mut icmpv6_manager = Icmpv6Manager::new();
    let config = Config::default();
    let payload = b"hello";
    let (ipv6_header, icmp_packet) =
        create_icmpv6_echo_request(config.guest_ipv6, config.host_ipv6, payload, 64);
    let mut responses = Vec::new();

    let mut eth_buf = vec![0u8; 14 + 40 + icmp_packet.len()];
    let (eth_header_slice, eth_payload_slice) = eth_buf.split_at_mut(14);
    let eth_frame = EthernetFrame::mut_from_bytes(eth_header_slice).unwrap();
    eth_frame.dst_addr = MacAddr { bytes: [0x1, 0x2, 0x3, 0x4, 0x5, 0x6] };
    eth_frame.src_addr = MacAddr { bytes: [0x6, 0x5, 0x4, 0x3, 0x2, 0x1] };
    eth_frame.ethertype = 0x86DD.into(); // IPv6
    eth_payload_slice[..40].copy_from_slice(ipv6_header.as_bytes());
    eth_payload_slice[40..].copy_from_slice(&icmp_packet);
    let parsed_packet = ParsedPacket::parse(&eth_buf).unwrap();

    icmpv6_manager.handle_packet(&mut responses, &config, &parsed_packet, &icmp_packet);

    assert_eq!(responses.len(), 1);
    let reply_packet = match responses.pop().unwrap() {
        SlirpResponse::Packet(packet) => packet,
        _ => panic!("Expected a packet response"),
    };

    let (_, reply_ip) = EthernetFrame::parse(&reply_packet).unwrap();
    let (reply_ip_header, reply_icmp) = Ipv6Header::parse(reply_ip).unwrap();
    let (reply_icmp_header, reply_icmp_payload) = Icmpv6Header::parse(reply_icmp).unwrap();

    assert_eq!(Ipv6Addr::from(reply_ip_header.source_addr), config.host_ipv6);
    assert_eq!(Ipv6Addr::from(reply_ip_header.dest_addr), config.guest_ipv6);
    assert_eq!(reply_icmp_header.icmpv6_type, Icmpv6Type::EchoReply as u8);
    assert_eq!(reply_icmp_header.echo_fields(), Some((1, 1)));
    assert_eq!(reply_icmp_payload, payload);
}

#[test]
fn test_port_unreachable() {
    let mut icmpv6_manager = Icmpv6Manager::new();
    let config = Config::default();
    let payload = b"hello";
    let (ipv6_header, icmp_packet) =
        create_icmpv6_echo_request(config.guest_ipv6, config.host_ipv6, payload, 64);
    let mut responses = Vec::new();

    let mut ip_buf = vec![0u8; 40 + icmp_packet.len()];
    ip_buf[..40].copy_from_slice(ipv6_header.as_bytes());
    ip_buf[40..].copy_from_slice(&icmp_packet);

    icmpv6_manager.send_port_unreachable(&mut responses, &config, &ip_buf);

    assert_eq!(responses.len(), 1);
    let reply_packet = match responses.pop().unwrap() {
        SlirpResponse::Packet(packet) => packet,
        _ => panic!("Expected a packet response"),
    };

    let (_, reply_ip) = EthernetFrame::parse(&reply_packet).unwrap();
    let (reply_ip_header, reply_icmp) = Ipv6Header::parse(reply_ip).unwrap();
    let (reply_icmp_header, reply_icmp_payload) = Icmpv6Header::parse(reply_icmp).unwrap();

    assert_eq!(reply_icmp_header.icmpv6_type, Icmpv6Type::DestinationUnreachable as u8);
    assert_eq!(reply_icmp_header.icmpv6_code, Icmpv6UnreachableCode::PortUnreachable as u8);
    assert_eq!(Ipv6Addr::from(reply_ip_header.source_addr), config.host_ipv6);
    assert_eq!(Ipv6Addr::from(reply_ip_header.dest_addr), Ipv6Addr::from(ipv6_header.source_addr));

    // Verify offending packet is copied back in payload
    assert_eq!(&reply_icmp_payload[..ip_buf.len()], &ip_buf[..]);
}

#[test]
fn test_packet_too_big() {
    let mut icmpv6_manager = Icmpv6Manager::new();
    let config = Config::default();
    let payload = b"hello";
    let (ipv6_header, icmp_packet) =
        create_icmpv6_echo_request(config.guest_ipv6, config.host_ipv6, payload, 64);
    let mut responses = Vec::new();

    let mut ip_buf = vec![0u8; 40 + icmp_packet.len()];
    ip_buf[..40].copy_from_slice(ipv6_header.as_bytes());
    ip_buf[40..].copy_from_slice(&icmp_packet);

    icmpv6_manager.send_packet_too_big(&mut responses, &config, &ip_buf, 1280);

    assert_eq!(responses.len(), 1);
    let reply_packet = match responses.pop().unwrap() {
        SlirpResponse::Packet(packet) => packet,
        _ => panic!("Expected a packet response"),
    };

    let (_, reply_ip) = EthernetFrame::parse(&reply_packet).unwrap();
    let (reply_ip_header, reply_icmp) = Ipv6Header::parse(reply_ip).unwrap();
    let (reply_icmp_header, reply_icmp_payload) = Icmpv6Header::parse(reply_icmp).unwrap();

    assert_eq!(reply_icmp_header.icmpv6_type, Icmpv6Type::PacketTooBig as u8);
    assert_eq!(reply_icmp_header.icmpv6_code, 0);
    assert_eq!(reply_icmp_header.mtu(), Some(1280));
    assert_eq!(Ipv6Addr::from(reply_ip_header.source_addr), config.host_ipv6);
    assert_eq!(Ipv6Addr::from(reply_ip_header.dest_addr), Ipv6Addr::from(ipv6_header.source_addr));

    // Verify offending packet is copied back in payload
    assert_eq!(&reply_icmp_payload[..ip_buf.len()], &ip_buf[..]);
}

#[test]
fn test_host_unreachable() {
    let mut icmpv6_manager = Icmpv6Manager::new();
    let config = Config::default();
    let payload = b"hello";
    let (ipv6_header, icmp_packet) =
        create_icmpv6_echo_request(config.guest_ipv6, config.host_ipv6, payload, 64);
    let mut responses = Vec::new();

    let mut ip_buf = vec![0u8; 40 + icmp_packet.len()];
    ip_buf[..40].copy_from_slice(ipv6_header.as_bytes());
    ip_buf[40..].copy_from_slice(&icmp_packet);

    icmpv6_manager.send_host_unreachable(&mut responses, &config, &ip_buf);

    assert_eq!(responses.len(), 1);
    let reply_packet = match responses.pop().unwrap() {
        SlirpResponse::Packet(packet) => packet,
        _ => panic!("Expected a packet response"),
    };

    let (_, reply_ip) = EthernetFrame::parse(&reply_packet).unwrap();
    let (reply_ip_header, reply_icmp) = Ipv6Header::parse(reply_ip).unwrap();
    let (reply_icmp_header, reply_icmp_payload) = Icmpv6Header::parse(reply_icmp).unwrap();

    assert_eq!(reply_icmp_header.icmpv6_type, Icmpv6Type::DestinationUnreachable as u8);
    assert_eq!(reply_icmp_header.icmpv6_code, Icmpv6UnreachableCode::AddressUnreachable as u8);
    assert_eq!(Ipv6Addr::from(reply_ip_header.source_addr), config.host_ipv6);
    assert_eq!(Ipv6Addr::from(reply_ip_header.dest_addr), Ipv6Addr::from(ipv6_header.source_addr));

    // Verify offending packet is copied back in payload
    assert_eq!(&reply_icmp_payload[..ip_buf.len()], &ip_buf[..]);
}
