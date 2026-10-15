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
use zerocopy::Ref;

use crate::{
    TimerManager,
    clock::MockClock,
    packet::{IpPacket, NetworkPacket, ParsedPacket},
    tcp::{
        manager::{MAX_DATA_RETRIES, MAX_SYN_RETRIES, TcpManager},
        state::State,
        tests::tcp_test_utils::{
            CreateTcpPacketArgs, MockHost, create_tcp_packet, process_responses,
        },
    },
    timers::TimerEvent,
};

#[tokio::test]
async fn test_syn_sent_timeout() {
    let mut tcp_manager = TcpManager::new(Ipv4Addr::new(10, 0, 2, 2));
    let mut host = MockHost::new();
    let clock = Arc::new(Mutex::new(MockClock::new()));
    let mut timers = TimerManager::new(Box::new(clock.clone()));
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

    // 2. Verify we are in SYN_SENT and a retransmit timer is scheduled
    let conn_id = tcp_manager.find_connection_by_addrs(guest_addr, host_addr).unwrap();
    let conn = tcp_manager.connections.get(&conn_id).unwrap();
    assert_eq!(conn.state, State::SynSent);
    let mut next_delay = timers.next_event_in().unwrap();

    // 3. Repeatedly trigger the timer, simulating no ACK from the guest
    for i in 0..MAX_SYN_RETRIES {
        // Advance time to trigger the timer
        clock.lock().unwrap().advance(next_delay);
        let next_event = timers.tick();

        assert_eq!(next_event.len(), 1);
        let conn_id_from_timer = match next_event[0] {
            TimerEvent::TcpRetransmit(id) => id,
            _ => panic!("Expected TcpRetransmit event"),
        };
        assert_eq!(conn_id_from_timer, conn_id);

        // Handle the timer event
        let mut responses = Vec::new();
        tcp_manager.handle_timer(&mut responses, &mut timers, conn_id_from_timer);
        process_responses(&mut host, responses);

        if i < MAX_SYN_RETRIES - 1 {
            assert!(tcp_manager.connections.contains_key(&conn_id));
            next_delay = timers.next_event_in().unwrap();
        }
    }

    // After the last retry, the connection should be gone
    assert!(!tcp_manager.connections.contains_key(&conn_id));
    assert_eq!(host.closed_connections.borrow().last().unwrap(), &conn_id);

    // No more timers should be scheduled for this connection
    assert!(timers.tick().is_empty());
}

#[tokio::test]
async fn test_data_retransmission_timeout() {
    let mut tcp_manager = TcpManager::new(Ipv4Addr::new(10, 0, 2, 2));
    let mut host = MockHost::new();
    let clock = Arc::new(Mutex::new(MockClock::new()));
    let mut timers = TimerManager::new(Box::new(clock.clone()));
    let guest_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 2)), 12345);
    let host_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8)), 80);

    // 1. Manually establish connection
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

    let conn_id = tcp_manager.find_connection_by_addrs(guest_addr, host_addr).unwrap();
    let conn = tcp_manager.connections.get(&conn_id).unwrap();
    let seq = conn.send.iss;
    let ethernet_packet = create_tcp_packet(&CreateTcpPacketArgs {
        src_addr: guest_addr,
        dst_addr: host_addr,
        seq: 1001,
        ack: seq + 1,
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

    // 2. Send data from host to guest, which will be unacked
    let payload_to_guest = b"hello guest";
    let mut responses = Vec::new();
    tcp_manager.send_data(&mut responses, &mut timers, conn_id, payload_to_guest);
    process_responses(&mut host, responses);
    let mut next_delay = timers.next_event_in().unwrap();

    // 3. Repeatedly trigger the retransmission timer
    for i in 0..MAX_DATA_RETRIES {
        clock.lock().unwrap().advance(next_delay);
        let next_event = timers.tick();
        assert_eq!(next_event.len(), 1);
        let conn_id_from_timer = match next_event[0] {
            TimerEvent::TcpRetransmit(id) => id,
            _ => panic!("Expected TcpRetransmit event"),
        };
        assert_eq!(conn_id_from_timer, conn_id);

        let mut responses = Vec::new();
        tcp_manager.handle_timer(&mut responses, &mut timers, conn_id_from_timer);
        process_responses(&mut host, responses);

        if i < MAX_DATA_RETRIES - 1 {
            assert!(tcp_manager.connections.contains_key(&conn_id));
            next_delay = timers.next_event_in().unwrap();
        }
    }

    // 4. Assert connection is dropped
    assert!(!tcp_manager.connections.contains_key(&conn_id));
    assert_eq!(host.closed_connections.borrow().last().unwrap(), &conn_id);
    assert!(timers.tick().is_empty());
}

#[tokio::test]
async fn test_time_wait_prevents_new_connection() {
    let mut tcp_manager = TcpManager::new(Ipv4Addr::new(10, 0, 2, 2));
    let mut host = MockHost::new();
    let clock = Arc::new(Mutex::new(MockClock::new()));
    let mut timers = TimerManager::new(Box::new(clock.clone()));
    let guest_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 2)), 12345);
    let host_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8)), 80);

    // 1. Establish connection
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
    let conn_id = tcp_manager.find_connection_by_addrs(guest_addr, host_addr).unwrap();
    let conn = tcp_manager.connections.get(&conn_id).unwrap();
    let seq = conn.send.iss;
    let ethernet_packet = create_tcp_packet(&CreateTcpPacketArgs {
        src_addr: guest_addr,
        dst_addr: host_addr,
        seq: 1001,
        ack: seq + 1,
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

    // 2. Close connection from our side -> FIN_WAIT_1
    let mut responses = Vec::new();
    tcp_manager.close(&mut responses, &mut timers, conn_id);
    process_responses(&mut host, responses);
    let conn = tcp_manager.connections.get(&conn_id).unwrap();
    let expected_ack = conn.send.nxt;

    // 3. Guest sends ACK for our FIN -> FIN_WAIT_2
    let ethernet_packet = create_tcp_packet(&CreateTcpPacketArgs {
        src_addr: guest_addr,
        dst_addr: host_addr,
        seq: 1001,
        ack: expected_ack,
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

    // 4. Guest sends FIN -> TIME_WAIT
    let ethernet_packet = create_tcp_packet(&CreateTcpPacketArgs {
        src_addr: guest_addr,
        dst_addr: host_addr,
        seq: 1001,
        ack: expected_ack,
        syn: false,
        is_ack: false,
        fin: true,
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

    // 5. Assert we are in TIME_WAIT
    let conn = tcp_manager.connections.get(&conn_id).unwrap();
    assert_eq!(conn.state, State::TimeWait);
    let packets_before_syn = host.packets_to_guest.borrow().len();

    // 6. Send a new SYN with the same 4-tuple
    let ethernet_packet = create_tcp_packet(&CreateTcpPacketArgs {
        src_addr: guest_addr,
        dst_addr: host_addr,
        seq: 2000, // New sequence number
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

    // 7. Assert no new connection was created and no SYN-ACK was sent
    assert_eq!(tcp_manager.connections.len(), 1);
    assert_eq!(host.packets_to_guest.borrow().len(), packets_before_syn);
}

#[tokio::test]
async fn test_rst_for_non_existent_connection() {
    let mut tcp_manager = TcpManager::new(Ipv4Addr::new(10, 0, 2, 2));
    let mut host = MockHost::new();
    let mut timers = TimerManager::new(Box::new(MockClock::new()));
    let guest_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 2)), 12345);
    let host_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8)), 80);

    // Send a FIN packet for a connection that doesn't exist
    let ethernet_packet = create_tcp_packet(&CreateTcpPacketArgs {
        src_addr: guest_addr,
        dst_addr: host_addr,
        seq: 1000,
        ack: 0,
        syn: false,
        is_ack: false,
        fin: true,
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

    // Assert that a RST packet was sent back
    assert_eq!(host.packets_to_guest.borrow().len(), 1);
    let rst_packet = host.packets_to_guest.borrow().last().unwrap().clone();
    let (_eth, eth_payload) = EthernetFrame::parse(&rst_packet).unwrap();
    let (_ipv4, ipv4_payload) = Ipv4Header::parse(eth_payload).unwrap();
    let (tcp, _) = Ref::<_, TcpHeader>::from_prefix(ipv4_payload).unwrap();
    assert!(tcp.rst());
}

#[tokio::test]
async fn test_tcp_manager_edge_cases() {
    let mut tcp_manager = TcpManager::new(Ipv4Addr::new(10, 0, 2, 2));
    let mut timers = TimerManager::new(Box::new(MockClock::new()));
    let mut responses = Vec::new();

    // 1. deactivate_fast_path for non-existent connection (should not panic)
    tcp_manager.deactivate_fast_path(999);

    // 2. handle_timer for non-existent connection (should not panic/do nothing)
    tcp_manager.handle_timer(&mut responses, &mut timers, 999);
    assert!(responses.is_empty());
}

#[tokio::test]
async fn test_hostfwd_port_exhaustion() {
    use std::{collections::VecDeque, time::Duration};

    use netsim_packets::MacAddr;

    use crate::tcp::congestion::CongestionControl;

    let mut tcp_manager = TcpManager::new(Ipv4Addr::new(10, 0, 2, 2));
    let mut timers = TimerManager::new(Box::new(MockClock::new()));
    let mut responses = Vec::new();

    let guest_mac = MacAddr::new([1, 2, 3, 4, 5, 6]);
    let gateway_mac = MacAddr::new([10, 20, 30, 40, 50, 60]);

    // Fill the virtual port pool (49152 to 65535, total 16384 ports)
    let gateway_ip = tcp_manager.gateway_ip;
    for port in 49152..=65535 {
        let conn_id = (port - 49152) as u64;
        let virtual_host_addr = SocketAddr::new(IpAddr::V4(gateway_ip), port);

        let conn = crate::tcp::state::TcpConnection {
            state: State::Established,
            guest_mac,
            gateway_mac,
            guest_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 2)), port),
            host_addr: virtual_host_addr,
            send: crate::tcp::state::SendSequenceSpace { iss: 0, una: 0, nxt: 0 },
            recv: crate::tcp::state::RecvSequenceSpace { nxt: 0 },
            unacked: VecDeque::new(),
            retransmission_timeout: Duration::from_secs(1),
            retransmissions: 0,
            mss: None,
            srtt: Duration::ZERO,
            rttvar: Duration::ZERO,
            sent_packets: VecDeque::new(),
            window_size: 8192,
            send_window: 8192,
            congestion_control: CongestionControl::new(536),
            dup_acks: 0,
            recv_window_scale: None,
            sack_blocks: Vec::new(),
            recv_buffer: Vec::new(),
        };
        tcp_manager.connections.insert(conn_id, conn);
    }

    // Now try to accept a new incoming connection. It should fail to allocate a
    // port
    let new_conn_id = 99999;
    let guest_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 3)), 55555);
    tcp_manager.accept_incoming(
        &mut responses,
        &mut timers,
        new_conn_id,
        guest_addr,
        guest_mac,
        gateway_mac,
    );

    assert!(!tcp_manager.connections.contains_key(&new_conn_id));
    assert!(responses.is_empty());
}
