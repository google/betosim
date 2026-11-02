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

use std::{fs::File, io::Write};

use bytes::Bytes;
use netsim_packets::{EthernetFrame, Ipv4Header, UdpHeader};
use tempfile::TempDir;
use zerocopy::FromBytes;

use crate::{
    Slirp,
    api::{Config, SlirpRequest, SlirpResponse},
};

fn build_rrq_packet(filename: &str) -> Vec<u8> {
    let mut packet = vec![0u8, 1]; // Opcode 1 (RRQ)
    packet.extend_from_slice(filename.as_bytes());
    packet.push(0);
    packet.extend_from_slice(b"octet");
    packet.push(0);
    packet
}

fn build_ack_packet(block_num: u16) -> Vec<u8> {
    let mut packet = vec![0u8, 4]; // Opcode 4 (ACK)
    packet.extend_from_slice(&block_num.to_be_bytes());
    packet
}

fn inject_udp_packet(
    slirp: &mut Slirp,
    payload: &[u8],
    src_port: u16,
    dst_port: u16,
) -> Vec<SlirpResponse> {
    use netsim_packets::{Ipv4Builder, UdpBuilder};

    let mut udp_packet = vec![0u8; 8 + payload.len()];
    let mut udp_builder = UdpBuilder::new(
        &mut udp_packet,
        slirp.config.guest_ipv4,
        slirp.config.host_ipv4,
        src_port,
        dst_port,
    )
    .unwrap();
    udp_builder.payload(payload).unwrap();
    udp_builder.build().unwrap();

    let mut ip_packet = vec![0u8; 20 + udp_packet.len()];
    let (ip_header, ip_payload) = ip_packet.split_at_mut(20);
    let mut ipv4_builder = Ipv4Builder::new(
        ip_header,
        netsim_packets::IP_P_UDP,
        slirp.config.guest_ipv4,
        slirp.config.host_ipv4, // gateway
    )
    .unwrap();
    ipv4_builder.payload_len(udp_packet.len());
    ipv4_builder.build();
    ip_payload.copy_from_slice(&udp_packet);

    let mut eth_packet = vec![0u8; 14 + ip_packet.len()];
    let (eth_header, eth_payload) = eth_packet.split_at_mut(14);
    let eth_frame = EthernetFrame::mut_from_bytes(eth_header).unwrap();
    eth_frame.dst_addr = slirp.config.gateway_mac;
    eth_frame.src_addr = slirp.config.guest_mac;
    eth_frame.ethertype = 0x0800.into();
    eth_payload.copy_from_slice(&ip_packet);

    slirp.handle_request(SlirpRequest::Packet(Bytes::copy_from_slice(&eth_packet)))
}

fn parse_tftp_packet(packet: &[u8]) -> (u16, u16, Vec<u8>) {
    let (_, eth_payload) = EthernetFrame::parse(packet).unwrap();
    let (_, ip_payload) = Ipv4Header::parse(eth_payload).unwrap();
    let (_, udp_payload) = UdpHeader::parse(ip_payload).unwrap();

    assert!(udp_payload.len() >= 4);
    let opcode = u16::from_be_bytes([udp_payload[0], udp_payload[1]]);
    let block_or_err = u16::from_be_bytes([udp_payload[2], udp_payload[3]]);
    let data = udp_payload[4..].to_vec();
    (opcode, block_or_err, data)
}

fn get_udp_dest_port(packet: &[u8]) -> u16 {
    let (_, eth_payload) = EthernetFrame::parse(packet).unwrap();
    let (_, ip_payload) = Ipv4Header::parse(eth_payload).unwrap();
    // The source port of the reply packet is our TID!
    u16::from_be_bytes([ip_payload[0], ip_payload[1]])
}

#[test]
fn test_tftp_rrq_success() {
    let tmp_dir = TempDir::new().unwrap();
    let file_path = tmp_dir.path().join("test.txt");
    let file_content = b"hello tftp world!";
    {
        let mut file = File::create(&file_path).unwrap();
        file.write_all(file_content).unwrap();
    }

    let config = Config { tftp_root: Some(tmp_dir.path().to_path_buf()), ..Config::default() };
    let mut slirp = Slirp::new(config);

    // 1. Send RRQ for test.txt
    let rrq = build_rrq_packet("test.txt");
    let responses = inject_udp_packet(&mut slirp, &rrq, 23456, 69);

    // Should return 1 packet (DATA block 1)
    assert_eq!(responses.len(), 1);
    let packet = match &responses[0] {
        SlirpResponse::Packet(p) => p,
        _ => panic!("Expected Packet response"),
    };

    let (opcode, block_num, data) = parse_tftp_packet(packet);
    assert_eq!(opcode, 3); // DATA
    assert_eq!(block_num, 1); // block 1
    assert_eq!(data, file_content);

    let tid = get_udp_dest_port(packet);
    assert!(tid >= 50000);

    // 2. Send ACK for block 1
    let ack = build_ack_packet(1);
    let responses2 = inject_udp_packet(&mut slirp, &ack, 23456, tid);

    // Since the file was small, transfer is complete, no more packets should be
    // sent!
    assert!(responses2.is_empty());
}

#[test]
fn test_tftp_large_file() {
    let tmp_dir = TempDir::new().unwrap();
    let file_path = tmp_dir.path().join("large.bin");
    // Create a 1000 bytes file (DATA 1: 512, DATA 2: 488)
    let file_content = vec![0x41u8; 1000];
    {
        let mut file = File::create(&file_path).unwrap();
        file.write_all(&file_content).unwrap();
    }

    let config = Config { tftp_root: Some(tmp_dir.path().to_path_buf()), ..Config::default() };
    let mut slirp = Slirp::new(config);

    // 1. Send RRQ
    let rrq = build_rrq_packet("large.bin");
    let responses = inject_udp_packet(&mut slirp, &rrq, 23456, 69);
    assert_eq!(responses.len(), 1);

    let packet1 = match &responses[0] {
        SlirpResponse::Packet(p) => p,
        _ => panic!("Expected Packet"),
    };
    let (opcode, block_num, data) = parse_tftp_packet(packet1);
    assert_eq!(opcode, 3);
    assert_eq!(block_num, 1);
    assert_eq!(data.len(), 512);
    assert_eq!(data, file_content[..512]);

    let tid = get_udp_dest_port(packet1);

    // 2. Send ACK 1
    let ack1 = build_ack_packet(1);
    let responses2 = inject_udp_packet(&mut slirp, &ack1, 23456, tid);
    assert_eq!(responses2.len(), 1);

    let packet2 = match &responses2[0] {
        SlirpResponse::Packet(p) => p,
        _ => panic!("Expected Packet"),
    };
    let (opcode, block_num, data) = parse_tftp_packet(packet2);
    assert_eq!(opcode, 3);
    assert_eq!(block_num, 2);
    assert_eq!(data.len(), 488);
    assert_eq!(data, file_content[512..]);

    // 3. Send ACK 2
    let ack2 = build_ack_packet(2);
    let responses3 = inject_udp_packet(&mut slirp, &ack2, 23456, tid);
    assert!(responses3.is_empty()); // Done!
}

#[test]
fn test_tftp_retransmission() {
    let tmp_dir = TempDir::new().unwrap();
    let file_path = tmp_dir.path().join("large.bin");
    let file_content = vec![0x41u8; 1000];
    {
        let mut file = File::create(&file_path).unwrap();
        file.write_all(&file_content).unwrap();
    }

    let config = Config { tftp_root: Some(tmp_dir.path().to_path_buf()), ..Config::default() };
    let mut slirp = Slirp::new(config);

    // 1. Send RRQ -> DATA 1
    let rrq = build_rrq_packet("large.bin");
    let responses = inject_udp_packet(&mut slirp, &rrq, 23456, 69);
    let packet1 = match &responses[0] {
        SlirpResponse::Packet(p) => p,
        _ => panic!("Expected Packet"),
    };
    let tid = get_udp_dest_port(packet1);

    // 2. Send ACK 1 -> DATA 2
    let ack1 = build_ack_packet(1);
    let responses2 = inject_udp_packet(&mut slirp, &ack1, 23456, tid);
    let packet2 = match &responses2[0] {
        SlirpResponse::Packet(p) => p,
        _ => panic!("Expected Packet"),
    };
    let (_, block_num, _) = parse_tftp_packet(packet2);
    assert_eq!(block_num, 2);

    // 3. Simulate client missing DATA 2 and sending duplicate ACK 1 again!
    let responses3 = inject_udp_packet(&mut slirp, &ack1, 23456, tid);
    assert_eq!(responses3.len(), 1);

    // Server must retransmit DATA 2!
    let packet3 = match &responses3[0] {
        SlirpResponse::Packet(p) => p,
        _ => panic!("Expected Packet"),
    };
    let (opcode, block_num, data) = parse_tftp_packet(packet3);
    assert_eq!(opcode, 3);
    assert_eq!(block_num, 2);
    assert_eq!(data.len(), 488);
}

#[test]
fn test_tftp_security_traversal() {
    let tmp_dir = TempDir::new().unwrap();

    let config = Config { tftp_root: Some(tmp_dir.path().to_path_buf()), ..Config::default() };
    let mut slirp = Slirp::new(config);

    // Try to escape using ..
    let rrq = build_rrq_packet("../escape.txt");
    let responses = inject_udp_packet(&mut slirp, &rrq, 23456, 69);

    assert_eq!(responses.len(), 1);
    let packet = match &responses[0] {
        SlirpResponse::Packet(p) => p,
        _ => panic!("Expected Packet"),
    };

    let (opcode, err_code, _) = parse_tftp_packet(packet);
    assert_eq!(opcode, 5); // ERROR
    assert_eq!(err_code, 2); // Access violation
}
