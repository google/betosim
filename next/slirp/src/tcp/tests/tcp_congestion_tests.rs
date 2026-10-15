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

use netsim_packets::{EthernetFrame, Ipv4Header, TcpHeader};

use crate::{
    TimerManager,
    clock::MockClock,
    packet::{IpPacket, NetworkPacket, ParsedPacket},
    tcp::{
        congestion::{CongestionControl, INITIAL_CWND_PACKETS, INITIAL_SSTHRESH},
        tests::tcp_test_utils::{
            CreateTcpPacketArgs, MockHost, create_tcp_packet, establish_connection_for_test,
            process_responses,
        },
        *,
    },
};

const TEST_MSS: u16 = 1460;

#[test]
fn test_initial_congestion_window() {
    let cc = CongestionControl::new(TEST_MSS);
    assert_eq!(cc.cwnd, INITIAL_CWND_PACKETS * TEST_MSS as u32);
    assert_eq!(cc.ssthresh, INITIAL_SSTHRESH);
}

#[test]
fn test_slow_start_on_ack() {
    let mut cc = CongestionControl::new(TEST_MSS);
    let initial_cwnd = cc.cwnd;

    // In slow start, cwnd should increase by MSS for each ACK.
    cc.on_ack(TEST_MSS);
    assert_eq!(cc.cwnd, initial_cwnd + TEST_MSS as u32);

    cc.on_ack(TEST_MSS);
    assert_eq!(cc.cwnd, initial_cwnd + 2 * TEST_MSS as u32);
}

#[test]
fn test_congestion_avoidance_on_ack() {
    let mut cc = CongestionControl::new(TEST_MSS);
    // Manually enter congestion avoidance
    cc.cwnd = cc.ssthresh + 1;
    let initial_cwnd = cc.cwnd;

    // In congestion avoidance, cwnd increases by roughly MSS^2 / cwnd
    cc.on_ack(TEST_MSS);
    let expected_increase = (TEST_MSS as u32 * TEST_MSS as u32) / initial_cwnd;
    assert_eq!(cc.cwnd, initial_cwnd + expected_increase);
}

#[test]
fn test_retransmission_updates_ssthresh_and_cwnd() {
    let mut cc = CongestionControl::new(TEST_MSS);
    cc.cwnd = 20 * TEST_MSS as u32; // Inflate the window
    let current_cwnd = cc.cwnd;

    cc.on_retransmission(TEST_MSS);

    assert_eq!(cc.ssthresh, current_cwnd / 2);
    assert_eq!(cc.cwnd, TEST_MSS as u32);
}

#[tokio::test]
async fn test_fast_retransmit() {
    let mut tcp_manager = TcpManager::new(Ipv4Addr::new(10, 0, 2, 2));
    let mut host = MockHost::new();
    let clock = Arc::new(Mutex::new(MockClock::new()));
    let mut timers = TimerManager::new(Box::new(clock.clone()));
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

    // 2. Send three segments from the host
    let data1 = [1; 100];
    let data2 = [2; 100];
    let data3 = [3; 100];
    let mut responses = Vec::new();
    tcp_manager.send_data(&mut responses, &mut timers, conn_id, &data1);
    tcp_manager.send_data(&mut responses, &mut timers, conn_id, &data2);
    tcp_manager.send_data(&mut responses, &mut timers, conn_id, &data3);
    process_responses(&mut host, responses);

    // 3. Simulate the guest ACKing the first segment, but dropping the second, and
    //    then sending duplicate ACKs for the first segment upon receiving the
    //    third.
    let conn = tcp_manager.connections.get(&conn_id).unwrap();
    let ack_for_seg1 = conn.send.una + data1.len() as u32;

    // First ACK (for seg1)
    let ethernet_packet = create_tcp_packet(&CreateTcpPacketArgs {
        src_addr: guest_addr,
        dst_addr: host_addr,
        seq: 2000,
        ack: ack_for_seg1,
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

    let packets_sent_before_dupacks = host.packets_to_guest.borrow().len();

    // Send 3 duplicate ACKs
    for _ in 0..3 {
        let ethernet_packet = create_tcp_packet(&CreateTcpPacketArgs {
            src_addr: guest_addr,
            dst_addr: host_addr,
            seq: 2000,
            ack: ack_for_seg1,
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
    }

    // 4. Assert that a retransmission was triggered
    let packets_sent_after_dupacks = host.packets_to_guest.borrow().len();
    assert_eq!(packets_sent_after_dupacks, packets_sent_before_dupacks + 1);

    // 5. Verify the retransmitted packet is the second segment
    let retransmitted_packet = host.packets_to_guest.borrow().last().unwrap().clone();
    let (_eth, eth_payload) = EthernetFrame::parse(&retransmitted_packet).unwrap();
    let (_ip, ip_payload) = Ipv4Header::parse(eth_payload).unwrap();
    let (_tcp, tcp_payload) = TcpHeader::parse(ip_payload).unwrap();
    assert_eq!(tcp_payload, &data2);
}
