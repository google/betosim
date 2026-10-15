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

use netsim_packets::{EthernetFrame, Ipv4Header, TcpHeader};
use zerocopy::Ref;

use crate::{
    TimerManager,
    clock::MockClock,
    tcp::{
        tests::tcp_test_utils::{MockHost, establish_connection_for_test, process_responses},
        *,
    },
};

#[tokio::test]
async fn test_sliding_window_prevents_sending_data() {
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

    // 2. Set a small window size on the connection
    let conn = tcp_manager.connections.get_mut(&conn_id).unwrap();
    conn.send_window = 50;

    // 3. Try to send 100 bytes (more than the window)
    let data_to_send = vec![0; 100];
    let mut responses = Vec::new();
    tcp_manager.send_data(&mut responses, &mut timers, conn_id, &data_to_send);
    process_responses(&mut host, responses);

    // 4. Assert that only a packet matching the window size was sent
    let packets = host.packets_to_guest.borrow();
    let last_packet = packets.last().unwrap();
    let (_eth_frame, eth_payload) = EthernetFrame::parse(last_packet).unwrap();
    let (_ipv4_header_resp, ipv4_payload) = Ipv4Header::parse(eth_payload).unwrap();
    let (_tcp_header_resp, tcp_payload) = Ref::<_, TcpHeader>::from_prefix(ipv4_payload).unwrap();

    assert_eq!(tcp_payload.len(), 50);
}
