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
    time::Duration,
};

use crate::{
    TimerManager,
    clock::MockClock,
    packet::{IpPacket, NetworkPacket, ParsedPacket},
    tcp::{
        input::INITIAL_RTO,
        tests::tcp_test_utils::{
            CreateTcpPacketArgs, MockHost, create_tcp_packet, establish_connection_for_test,
            process_responses,
        },
        *,
    },
};

#[tokio::test]
async fn test_rto_calculation_on_ack() {
    let mut tcp_manager = TcpManager::new(Ipv4Addr::new(10, 0, 2, 2));
    let mut host = MockHost::new();
    let clock = Arc::new(Mutex::new(MockClock::new()));
    let mut timers = TimerManager::new(Box::new(clock.clone()));
    let guest_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 2)), 12345);
    let host_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8)), 80);

    // 1. Establish connection
    let (conn_id, sequence_num) = establish_connection_for_test(
        &mut tcp_manager,
        &mut host,
        &mut timers,
        guest_addr,
        host_addr,
    );

    // 2. Send data from host to guest
    let payload_to_guest = b"hello guest";
    let mut responses = Vec::new();
    tcp_manager.send_data(&mut responses, &mut timers, conn_id, payload_to_guest);
    process_responses(&mut host, responses);

    // 3. Advance time and ACK the data
    clock.lock().unwrap().advance(Duration::from_millis(100));
    let ethernet_packet = create_tcp_packet(&CreateTcpPacketArgs {
        src_addr: guest_addr,
        dst_addr: host_addr,
        seq: 1001,
        ack: sequence_num + 1 + payload_to_guest.len() as u32,
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

    // 4. Assert that RTO was updated
    let conn = tcp_manager.connections.get(&conn_id).unwrap();
    assert_ne!(conn.retransmission_timeout, INITIAL_RTO);
    assert_ne!(conn.srtt, Duration::from_secs(0));
}
