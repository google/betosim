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

use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    sync::{Arc, Mutex},
};

use crate::{
    SlirpResponse,
    clock::MockClock,
    packet::{IpPacket, NetworkPacket, ParsedPacket},
    tcp::{
        State,
        manager::TcpManager,
        tests::tcp_test_utils::{
            CreateTcpPacketArgs, MockHost, create_tcp_packet, establish_connection_for_test,
            parse_tcp_packet,
        },
    },
    timers::TimerManager,
};

#[test]
fn test_handle_syn_sends_syn_ack() {
    let mut tcp_manager = TcpManager::new(Ipv4Addr::new(10, 0, 2, 2));
    let clock = Arc::new(Mutex::new(MockClock::new()));
    let mut timers = TimerManager::new(Box::new(clock.clone()));
    let mut responses = Vec::new();

    let guest_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 2)), 12345);
    let host_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8)), 80);

    let ethernet_packet = create_tcp_packet(&CreateTcpPacketArgs {
        src_addr: guest_addr,
        dst_addr: host_addr,
        seq: 1000,
        ack: 0,
        syn: true,
        is_ack: false,
        fin: false,
        rst: false,
        options: &[],
        payload: &[],
    });

    let parsed_packet = ParsedPacket::parse(&ethernet_packet).unwrap();
    let (ipv4_header, tcp_packet) =
        if let Some(NetworkPacket::Ip(IpPacket::V4(h, p))) = parsed_packet.network {
            (h, p)
        } else {
            panic!();
        };

    tcp_manager.handle_packet(
        &mut responses,
        &mut timers,
        &parsed_packet,
        &ipv4_header,
        tcp_packet,
    );

    assert_eq!(responses.len(), 2);
    let response_packet = match responses.get(1).unwrap() {
        SlirpResponse::Packet(packet) => packet,
        _ => panic!("Expected a packet response"),
    };

    let (_eth, _ipv4, tcp_header, _payload) = parse_tcp_packet(response_packet);

    assert!(tcp_header.syn() && tcp_header.ack(), "Packet should be a SYN-ACK");
}

#[test]
fn test_handle_syn_sent_ack_with_payload() {
    let mut tcp_manager = TcpManager::new(Ipv4Addr::new(10, 0, 2, 2));
    let clock = Arc::new(Mutex::new(MockClock::new()));
    let mut timers = TimerManager::new(Box::new(clock.clone()));
    let mut responses = Vec::new();

    let guest_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 2)), 12345);
    let host_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8)), 80);

    let syn_packet = create_tcp_packet(&CreateTcpPacketArgs {
        src_addr: guest_addr,
        dst_addr: host_addr,
        seq: 1000,
        ack: 0,
        syn: true,
        is_ack: false,
        fin: false,
        rst: false,
        options: &[],
        payload: &[],
    });

    let parsed_syn = ParsedPacket::parse(&syn_packet).unwrap();
    let (ipv4_header, tcp_packet) =
        if let Some(NetworkPacket::Ip(IpPacket::V4(h, p))) = parsed_syn.network {
            (h, p)
        } else {
            panic!("Expected IPv4 packet");
        };

    tcp_manager.handle_packet(&mut responses, &mut timers, &parsed_syn, &ipv4_header, tcp_packet);

    assert_eq!(responses.len(), 2);
    let response_packet = match responses.get(1).unwrap() {
        SlirpResponse::Packet(packet) => packet,
        _ => panic!("Expected a packet response"),
    };

    let (_eth, _ipv4, syn_ack_header, _payload) = parse_tcp_packet(response_packet);
    assert!(syn_ack_header.syn() && syn_ack_header.ack(), "Packet should be a SYN-ACK");
    let syn_ack_seq = syn_ack_header.sequence_num.get();

    // Guest sends ACK with piggybacked payload to complete handshake
    responses.clear();
    let payload = b"hello world";
    let ack_packet = create_tcp_packet(&CreateTcpPacketArgs {
        src_addr: guest_addr,
        dst_addr: host_addr,
        seq: 1001,
        ack: syn_ack_seq.wrapping_add(1),
        syn: false,
        is_ack: true,
        fin: false,
        rst: false,
        options: &[],
        payload,
    });

    let parsed_ack = ParsedPacket::parse(&ack_packet).unwrap();
    let (ipv4_header, tcp_packet) =
        if let Some(NetworkPacket::Ip(IpPacket::V4(h, p))) = parsed_ack.network {
            (h, p)
        } else {
            panic!("Expected IPv4 packet");
        };

    tcp_manager.handle_packet(&mut responses, &mut timers, &parsed_ack, &ipv4_header, tcp_packet);

    let mut activated_fast_path = false;
    let mut wrote_payload = false;
    let mut ack_packet_found = false;

    for resp in &responses {
        match resp {
            SlirpResponse::ActivateFastPath { conn_id: _, guest_addr: g, host_addr: h } => {
                assert_eq!(*g, guest_addr);
                assert_eq!(*h, host_addr);
                activated_fast_path = true;
            }
            SlirpResponse::WriteToConnection(_conn_id, data) => {
                assert_eq!(data.as_ref(), payload);
                wrote_payload = true;
            }
            SlirpResponse::Packet(ack_back) => {
                let (_eth, _ipv4, ack_header, _p) = parse_tcp_packet(ack_back);
                assert!(ack_header.ack() && !ack_header.syn());
                assert_eq!(ack_header.ack_num.get(), 1001 + payload.len() as u32);
                ack_packet_found = true;
            }
            other => panic!("Unexpected SlirpResponse: {other:?}"),
        }
    }

    assert!(activated_fast_path, "ActivateFastPath response should be emitted");
    assert!(wrote_payload, "WriteToConnection response should be emitted");
    assert!(ack_packet_found, "ACK packet response should be emitted");

    let conn_id = tcp_manager.find_connection_by_addrs(guest_addr, host_addr).unwrap();
    let conn = tcp_manager.connections.get(&conn_id).unwrap();
    assert_eq!(conn.state, State::Established);
}

#[test]
fn test_handle_syn_sent_ack_with_payload_and_fin() {
    let mut tcp_manager = TcpManager::new(Ipv4Addr::new(10, 0, 2, 2));
    let clock = Arc::new(Mutex::new(MockClock::new()));
    let mut timers = TimerManager::new(Box::new(clock.clone()));
    let mut responses = Vec::new();

    let guest_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 2)), 12345);
    let host_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8)), 80);

    let syn_packet = create_tcp_packet(&CreateTcpPacketArgs {
        src_addr: guest_addr,
        dst_addr: host_addr,
        seq: 1000,
        ack: 0,
        syn: true,
        is_ack: false,
        fin: false,
        rst: false,
        options: &[],
        payload: &[],
    });

    let parsed_syn = ParsedPacket::parse(&syn_packet).unwrap();
    let (ipv4_header, tcp_packet) =
        if let Some(NetworkPacket::Ip(IpPacket::V4(h, p))) = parsed_syn.network {
            (h, p)
        } else {
            panic!("Expected IPv4 packet");
        };

    tcp_manager.handle_packet(&mut responses, &mut timers, &parsed_syn, &ipv4_header, tcp_packet);

    assert_eq!(responses.len(), 2);
    let response_packet = match responses.get(1).unwrap() {
        SlirpResponse::Packet(packet) => packet,
        _ => panic!("Expected a packet response"),
    };

    let (_eth, _ipv4, syn_ack_header, _payload) = parse_tcp_packet(response_packet);
    assert!(syn_ack_header.syn() && syn_ack_header.ack(), "Packet should be a SYN-ACK");
    let syn_ack_seq = syn_ack_header.sequence_num.get();

    // Guest sends ACK with both piggybacked payload and FIN
    responses.clear();
    let payload = b"bye";
    let ack_packet = create_tcp_packet(&CreateTcpPacketArgs {
        src_addr: guest_addr,
        dst_addr: host_addr,
        seq: 1001,
        ack: syn_ack_seq.wrapping_add(1),
        syn: false,
        is_ack: true,
        fin: true,
        rst: false,
        options: &[],
        payload,
    });

    let parsed_ack = ParsedPacket::parse(&ack_packet).unwrap();
    let (ipv4_header, tcp_packet) =
        if let Some(NetworkPacket::Ip(IpPacket::V4(h, p))) = parsed_ack.network {
            (h, p)
        } else {
            panic!("Expected IPv4 packet");
        };

    tcp_manager.handle_packet(&mut responses, &mut timers, &parsed_ack, &ipv4_header, tcp_packet);

    let mut activated_fast_path = false;
    let mut wrote_payload = false;
    let mut closed_connection = false;
    let mut ack_packet_found = false;

    for resp in &responses {
        match resp {
            SlirpResponse::ActivateFastPath { conn_id: _, guest_addr: g, host_addr: h } => {
                assert_eq!(*g, guest_addr);
                assert_eq!(*h, host_addr);
                activated_fast_path = true;
            }
            SlirpResponse::WriteToConnection(_conn_id, data) => {
                assert_eq!(data.as_ref(), payload);
                wrote_payload = true;
            }
            SlirpResponse::CloseConnection { conn_id: _, guest_addr: g } => {
                assert_eq!(*g, guest_addr);
                closed_connection = true;
            }
            SlirpResponse::Packet(ack_back) => {
                let (_eth, _ipv4, ack_header, _p) = parse_tcp_packet(ack_back);
                assert!(ack_header.ack() && !ack_header.syn());
                // The ACK number must acknowledge both the payload bytes
                // and the FIN control bit (+1)
                assert_eq!(ack_header.ack_num.get(), 1001 + payload.len() as u32 + 1);
                ack_packet_found = true;
            }
            other => panic!("Unexpected SlirpResponse: {other:?}"),
        }
    }

    assert!(activated_fast_path, "ActivateFastPath response should be emitted");
    assert!(wrote_payload, "WriteToConnection response should be emitted");
    assert!(closed_connection, "CloseConnection response should be emitted");
    assert!(ack_packet_found, "ACK packet response should be emitted");

    let conn_id = tcp_manager.find_connection_by_addrs(guest_addr, host_addr).unwrap();
    let conn = tcp_manager.connections.get(&conn_id).unwrap();
    assert_eq!(conn.state, State::CloseWait);
}

#[test]
fn test_handle_established_payload_and_fin_acknowledges_both() {
    let mut tcp_manager = TcpManager::new(Ipv4Addr::new(10, 0, 2, 2));
    let mut host = MockHost::new();
    let mut timers = TimerManager::new(Box::new(MockClock::new()));
    let guest_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 2)), 12345);
    let host_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8)), 80);

    let (conn_id, syn_ack_seq) = establish_connection_for_test(
        &mut tcp_manager,
        &mut host,
        &mut timers,
        guest_addr,
        host_addr,
    );
    assert_eq!(tcp_manager.connections.get(&conn_id).unwrap().state, State::Established);

    // Guest sends an Established-state segment carrying both payload and FIN
    let mut responses = Vec::new();
    let payload = b"bye";
    let fin_packet = create_tcp_packet(&CreateTcpPacketArgs {
        src_addr: guest_addr,
        dst_addr: host_addr,
        seq: 1001,
        ack: syn_ack_seq.wrapping_add(1),
        syn: false,
        is_ack: true,
        fin: true,
        rst: false,
        options: &[],
        payload,
    });

    let parsed_fin = ParsedPacket::parse(&fin_packet).unwrap();
    let (ipv4_header, tcp_packet) =
        if let Some(NetworkPacket::Ip(IpPacket::V4(h, p))) = parsed_fin.network {
            (h, p)
        } else {
            panic!("Expected IPv4 packet");
        };

    tcp_manager.handle_packet(&mut responses, &mut timers, &parsed_fin, &ipv4_header, tcp_packet);

    let mut wrote_payload = false;
    let mut closed_connection = false;
    let mut ack_packet_found = false;

    for resp in &responses {
        match resp {
            SlirpResponse::WriteToConnection(_conn_id, data) => {
                assert_eq!(data.as_ref(), payload);
                wrote_payload = true;
            }
            SlirpResponse::CloseConnection { conn_id: _, guest_addr: g } => {
                assert_eq!(*g, guest_addr);
                closed_connection = true;
            }
            SlirpResponse::Packet(ack_back) => {
                let (_eth, _ipv4, ack_header, _p) = parse_tcp_packet(ack_back);
                assert!(ack_header.ack() && !ack_header.syn());
                assert_eq!(ack_header.ack_num.get(), 1001 + payload.len() as u32 + 1);
                ack_packet_found = true;
            }
            other => panic!("Unexpected SlirpResponse: {other:?}"),
        }
    }

    assert!(wrote_payload, "WriteToConnection response should be emitted");
    assert!(closed_connection, "CloseConnection response should be emitted");
    assert!(ack_packet_found, "ACK packet response should be emitted");

    let conn = tcp_manager.connections.get(&conn_id).unwrap();
    assert_eq!(conn.state, State::CloseWait);
}
