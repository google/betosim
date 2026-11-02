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
        manager::TcpManager,
        tests::tcp_test_utils::{
            CreateTcpPacketArgs, MockHost, create_tcp_packet, process_responses,
        },
    },
};

#[tokio::test]
async fn test_mss_option_is_parsed() {
    let mut tcp_manager = TcpManager::new(Ipv4Addr::new(10, 0, 2, 2));
    let mut host = MockHost::new();
    let mut timers = TimerManager::new(Box::new(MockClock::new()));
    let guest_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 2)), 12345);
    let host_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8)), 80);

    // 1. Manually construct a 24-byte TCP header with an MSS option.
    let mss_value: u16 = 1460;
    let mut tcp_options = vec![0u8; 4];
    tcp_options[0] = 2; // MSS Kind
    tcp_options[1] = 4; // Length
    tcp_options[2..4].copy_from_slice(&mss_value.to_be_bytes());

    let ethernet_packet = create_tcp_packet(&CreateTcpPacketArgs {
        src_addr: guest_addr,
        dst_addr: host_addr,
        seq: 1000,
        ack: 0,
        syn: true,
        is_ack: false,
        fin: false,
        rst: false,
        options: &tcp_options,
        payload: &[],
    });

    let parsed_packet = ParsedPacket::parse(&ethernet_packet).unwrap();
    let (ipv4_header, tcp_packet) =
        if let Some(NetworkPacket::Ip(IpPacket::V4(h, p))) = parsed_packet.network {
            (h, p)
        } else {
            panic!();
        };

    // 2. Handle the packet.
    let mut responses = Vec::new();
    tcp_manager.handle_packet(
        &mut responses,
        &mut timers,
        &parsed_packet,
        &ipv4_header,
        tcp_packet,
    );
    process_responses(&mut host, responses);

    // 3. Assert that the MSS was parsed and stored.
    let conn_id = tcp_manager.find_connection_by_addrs(guest_addr, host_addr).unwrap();
    let conn = tcp_manager.connections.get(&conn_id).unwrap();
    assert_eq!(conn.mss, Some(mss_value));
}
