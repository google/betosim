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

use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use crate::{
    TimerManager,
    clock::MockClock,
    packet::{IpPacket, NetworkPacket, ParsedPacket},
    tcp::{
        tests::tcp_test_utils::{
            CreateTcpPacketArgs, MockHost, create_tcp_packet, establish_connection_for_test,
            process_responses,
        },
        *,
    },
};

#[tokio::test]
async fn test_syn_sent_to_established() {
    let mut tcp_manager = TcpManager::new(Ipv4Addr::new(10, 0, 2, 2));
    let mut host = MockHost::new();
    let mut timers = TimerManager::new(Box::new(MockClock::new()));
    let guest_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 2)), 12345);
    let host_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8)), 80);

    // 1. Guest sends SYN
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
    let mut responses = Vec::new();
    tcp_manager.handle_packet(
        &mut responses,
        &mut timers,
        &parsed_packet,
        &ipv4_header,
        tcp_packet,
    );
    process_responses(&mut host, responses);

    // 2. Assert we sent a SYN-ACK and are in SYN_SENT state
    assert_eq!(host.packets_to_guest.borrow().len(), 1);
    let conn_id = tcp_manager.find_connection_by_addrs(guest_addr, host_addr).unwrap();
    let conn = tcp_manager.connections.get(&conn_id).unwrap();
    assert_eq!(conn.state, State::SynSent);

    // 3. Guest sends ACK
    let ethernet_packet = create_tcp_packet(&CreateTcpPacketArgs {
        src_addr: guest_addr,
        dst_addr: host_addr,
        seq: 1001,
        ack: conn.send.nxt,
        syn: false,
        is_ack: true,
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
    let mut responses = Vec::new();
    tcp_manager.handle_packet(
        &mut responses,
        &mut timers,
        &parsed_packet,
        &ipv4_header,
        tcp_packet,
    );
    process_responses(&mut host, responses);

    // 4. Assert we are in ESTABLISHED state
    let conn = tcp_manager.connections.get(&conn_id).unwrap();
    assert_eq!(conn.state, State::Established);
}

#[tokio::test]
async fn test_data_transfer() {
    let mut tcp_manager = TcpManager::new(Ipv4Addr::new(10, 0, 2, 2));
    let mut host = MockHost::new();
    let mut timers = TimerManager::new(Box::new(MockClock::new()));
    let guest_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 2)), 12345);
    let host_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8)), 80);

    // 1. Establish connection
    let (conn_id, _sequence_num) = establish_connection_for_test(
        &mut tcp_manager,
        &mut host,
        &mut timers,
        guest_addr,
        host_addr,
    );

    // 2. Send data from guest to host
    let payload_to_host = b"hello host";
    let ethernet_packet = create_tcp_packet(&CreateTcpPacketArgs {
        src_addr: guest_addr,
        dst_addr: host_addr,
        seq: 1001,
        ack: 1,
        syn: false,
        is_ack: true,
        fin: false,
        rst: false,
        options: &[],
        payload: payload_to_host,
    });
    let parsed_packet = ParsedPacket::parse(&ethernet_packet).unwrap();
    let (ipv4_header, tcp_packet) =
        if let Some(NetworkPacket::Ip(IpPacket::V4(h, p))) = parsed_packet.network {
            (h, p)
        } else {
            panic!();
        };
    let mut responses = Vec::new();
    tcp_manager.handle_packet(
        &mut responses,
        &mut timers,
        &parsed_packet,
        &ipv4_header,
        tcp_packet,
    );
    process_responses(&mut host, responses);

    // 3. Send data from host to guest
    let payload_to_guest = b"hello guest";
    let mut responses = Vec::new();
    tcp_manager.send_data(&mut responses, &mut timers, conn_id, payload_to_guest);
    process_responses(&mut host, responses);

    // 4. Verify packets were sent to guest
    assert_eq!(host.packets_to_guest.borrow().len(), 3); // SYN-ACK, ACK, Data
}
