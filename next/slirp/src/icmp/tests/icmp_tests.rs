// Copyright 2023-2025 The Android Open Source Project
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

use netsim_packets::{EthernetFrame, IcmpHeader, IcmpType, Ipv4Header, MacAddr, UnreachableCode};
use zerocopy::{FromBytes, IntoBytes, U16};

use crate::{Config, SlirpResponse, icmp::*, packet::ParsedPacket};

fn create_echo_request(payload: &[u8], ttl: u8) -> (Ipv4Header, Vec<u8>) {
    let icmp_header_len = std::mem::size_of::<IcmpHeader>();
    let icmp_len = icmp_header_len + payload.len();
    let mut icmp_buf = vec![0u8; icmp_len];
    let (header_slice, payload_slice) = icmp_buf.split_at_mut(icmp_header_len);

    let icmp_header = IcmpHeader::mut_from_bytes(header_slice).unwrap();
    icmp_header.icmp_type = IcmpType::EchoRequest as u8;
    icmp_header.icmp_code = 0;
    icmp_header.icmp_checksum = 0.into();

    icmp_header.rest[..2].copy_from_slice(&1u16.to_be_bytes());
    icmp_header.rest[2..].copy_from_slice(&1u16.to_be_bytes());

    payload_slice[..payload.len()].copy_from_slice(payload);

    let ipv4_header = Ipv4Header {
        version_ihl: 0x45,
        dscp_ecn: 0,
        total_length: U16::new((20 + icmp_len) as u16),
        identification: U16::new(0),
        flags_fragment_offset: U16::new(0),
        ttl,
        protocol: 1,
        header_checksum: U16::new(0),
        source_addr: Ipv4Addr::new(10, 0, 2, 15).octets(),
        dest_addr: Ipv4Addr::new(10, 0, 2, 2).octets(),
    };

    (ipv4_header, icmp_buf)
}

#[test]
fn test_echo_reply() {
    let mut icmp_manager = IcmpManager::new();
    let config = Config::default();
    let payload = b"hello";
    let (ipv4_header, icmp_packet) = create_echo_request(payload, 64);
    let mut responses = Vec::new();

    let mut eth_buf = vec![0u8; 14 + 20 + icmp_packet.len()];
    let (eth_header_slice, eth_payload_slice) = eth_buf.split_at_mut(14);
    let eth_frame = EthernetFrame::mut_from_bytes(eth_header_slice).unwrap();
    eth_frame.dst_addr = MacAddr { bytes: [0x1, 0x2, 0x3, 0x4, 0x5, 0x6] };
    eth_frame.src_addr = MacAddr { bytes: [0x6, 0x5, 0x4, 0x3, 0x2, 0x1] };
    eth_frame.ethertype = 0x0800.into();
    eth_payload_slice[..20].copy_from_slice(ipv4_header.as_bytes());
    eth_payload_slice[20..].copy_from_slice(&icmp_packet);
    let parsed_packet = ParsedPacket::parse(&eth_buf).unwrap();

    icmp_manager.handle_packet(&mut responses, &config, &parsed_packet, &icmp_packet);

    assert_eq!(responses.len(), 1);
    let reply_packet = match responses.pop().unwrap() {
        SlirpResponse::Packet(packet) => packet,
        _ => panic!("Expected a packet response"),
    };

    let (_, reply_ip) = EthernetFrame::parse(&reply_packet).unwrap();
    let (reply_ip_header, reply_icmp) = Ipv4Header::parse(reply_ip).unwrap();
    let (reply_icmp_header, reply_icmp_payload) = IcmpHeader::parse(reply_icmp).unwrap();

    assert_eq!(reply_ip_header.source_addr, config.host_ipv4.octets());
    assert_eq!(reply_ip_header.dest_addr, config.guest_ipv4.octets());
    assert_eq!(reply_icmp_header.icmp_type, IcmpType::EchoReply as u8);

    let identifier = u16::from_be_bytes([reply_icmp_header.rest[0], reply_icmp_header.rest[1]]);
    let sequence_number =
        u16::from_be_bytes([reply_icmp_header.rest[2], reply_icmp_header.rest[3]]);
    assert_eq!(identifier, 1);
    assert_eq!(sequence_number, 1);
    assert_eq!(reply_icmp_payload, payload);
}

#[test]
fn test_time_exceeded() {
    let mut icmp_manager = IcmpManager::new();
    let config = Config::default();
    let payload = b"hello";
    let (ipv4_header, icmp_packet) = create_echo_request(payload, 1);
    let mut responses = Vec::new();

    let mut ip_buf = vec![0u8; 20 + icmp_packet.len()];
    ip_buf[..20].copy_from_slice(ipv4_header.as_bytes());
    ip_buf[20..].copy_from_slice(&icmp_packet);

    icmp_manager.send_time_exceeded(&mut responses, &config, &ip_buf);

    assert_eq!(responses.len(), 1);
    let reply_packet = match responses.pop().unwrap() {
        SlirpResponse::Packet(packet) => packet,
        _ => panic!("Expected a packet response"),
    };

    let (_, reply_ip) = EthernetFrame::parse(&reply_packet).unwrap();
    let (_reply_ip_header, reply_icmp) = Ipv4Header::parse(reply_ip).unwrap();
    let (reply_icmp_header, _) = IcmpHeader::parse(reply_icmp).unwrap();

    assert_eq!(reply_icmp_header.icmp_type, IcmpType::TimeExceeded as u8);
}

#[test]
fn test_host_unreachable() {
    let mut icmp_manager = IcmpManager::new();
    let config = Config::default();
    let payload = b"hello";
    let (ipv4_header, icmp_packet) = create_echo_request(payload, 64);
    let mut responses = Vec::new();

    let mut ip_buf = vec![0u8; 20 + icmp_packet.len()];
    ip_buf[..20].copy_from_slice(ipv4_header.as_bytes());
    ip_buf[20..].copy_from_slice(&icmp_packet);

    icmp_manager.send_host_unreachable(&mut responses, &config, &ip_buf);

    assert_eq!(responses.len(), 1);
    let reply_packet = match responses.pop().unwrap() {
        SlirpResponse::Packet(packet) => packet,
        _ => panic!("Expected a packet response"),
    };

    let (_, reply_ip) = EthernetFrame::parse(&reply_packet).unwrap();
    let (_reply_ip_header, reply_icmp) = Ipv4Header::parse(reply_ip).unwrap();
    let (reply_icmp_header, _) = IcmpHeader::parse(reply_icmp).unwrap();

    assert_eq!(reply_icmp_header.icmp_type, IcmpType::DestUnreachable as u8);
    assert_eq!(reply_icmp_header.icmp_code, UnreachableCode::HostUnreachable as u8);
}
