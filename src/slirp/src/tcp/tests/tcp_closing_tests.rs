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

use bytes::Bytes;
use netsim_packets::{
    EthernetFrame, IP_P_TCP, Ipv4Builder, Ipv4Header, TcpBuilder, TcpHeader, ether_type,
};
use zerocopy::{FromBytes, Ref};

use crate::{
    Config, ConnectionArgs, Slirp, SlirpRequest, SlirpResponse, TimerManager,
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
async fn test_fin_wait() {
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

    // 2. Close connection from our side -> FIN_WAIT_1
    let mut responses = Vec::new();
    tcp_manager.close(&mut responses, &mut timers, conn_id);
    process_responses(&mut host, responses);
    let conn = tcp_manager.connections.get(&conn_id).unwrap();
    assert_eq!(conn.state, State::FinWait1);
    let response = host.packets_to_guest.borrow().last().unwrap().clone();
    let (_eth_frame, eth_payload) = EthernetFrame::parse(&response).unwrap();
    let (_ipv4_header_resp, ipv4_payload) = Ipv4Header::parse(eth_payload).unwrap();
    let (tcp_header_resp, _) = Ref::<_, TcpHeader>::from_prefix(ipv4_payload).unwrap();
    assert!(tcp_header_resp.fin());

    // 3. Guest sends ACK for our FIN -> FIN_WAIT_2
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
    let conn = tcp_manager.connections.get(&conn_id).unwrap();
    assert_eq!(conn.state, State::FinWait2);
}

#[tokio::test]
async fn test_close_wait() {
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

    // 2. Guest sends FIN -> CLOSE_WAIT
    let ethernet_packet = create_tcp_packet(&CreateTcpPacketArgs {
        src_addr: guest_addr,
        dst_addr: host_addr,
        seq: 1001,
        ack: 1,
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
    let conn = tcp_manager.connections.get(&conn_id).unwrap();
    assert_eq!(conn.state, State::CloseWait);
}

#[tokio::test]
async fn test_simultaneous_close() {
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

    // 2. Close from our side -> FIN_WAIT_1
    let mut responses = Vec::new();
    tcp_manager.close(&mut responses, &mut timers, conn_id);
    process_responses(&mut host, responses);

    // 3. Guest sends FIN (before ACK) -> CLOSING
    let ethernet_packet = create_tcp_packet(&CreateTcpPacketArgs {
        src_addr: guest_addr,
        dst_addr: host_addr,
        seq: 1001,
        ack: 1,
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
    let conn = tcp_manager.connections.get(&conn_id).unwrap();
    assert_eq!(conn.state, State::Closing);
}

#[tokio::test]
async fn test_rst_closes_connection() {
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

    // 2. Guest sends RST
    let ethernet_packet = create_tcp_packet(&CreateTcpPacketArgs {
        src_addr: guest_addr,
        dst_addr: host_addr,
        seq: 1001,
        ack: 1,
        syn: false,
        is_ack: false,
        fin: false,
        rst: true,
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

    // 3. Assert connection is removed
    assert!(!tcp_manager.connections.contains_key(&conn_id));
    assert_eq!(host.closed_connections.borrow().len(), 1);
    assert_eq!(host.closed_connections.borrow()[0], conn_id);
}

#[tokio::test]
async fn test_fin_wait_2_to_time_wait() {
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

    // 2. Close connection from our side -> FIN_WAIT_1
    let mut responses = Vec::new();
    tcp_manager.close(&mut responses, &mut timers, conn_id);
    process_responses(&mut host, responses);

    // 3. Guest sends ACK for our FIN -> FIN_WAIT_2
    let conn = tcp_manager.connections.get(&conn_id).unwrap();
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

    // Assert we are in FIN_WAIT_2
    let conn = tcp_manager.connections.get(&conn_id).unwrap();
    assert_eq!(conn.state, State::FinWait2);

    // 4. Guest sends FIN -> TIME_WAIT
    let ethernet_packet = create_tcp_packet(&CreateTcpPacketArgs {
        src_addr: guest_addr,
        dst_addr: host_addr,
        seq: 1001,
        ack: conn.send.nxt,
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

    // Assert we transitioned to TIME_WAIT
    let conn = tcp_manager.connections.get(&conn_id).unwrap();
    assert_eq!(conn.state, State::TimeWait);
}

#[tokio::test]
async fn test_close_wait_to_last_ack_to_closed() {
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

    // 2. Guest sends FIN -> CLOSE_WAIT
    let ethernet_packet = create_tcp_packet(&CreateTcpPacketArgs {
        src_addr: guest_addr,
        dst_addr: host_addr,
        seq: 1001,
        ack: 1,
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

    // Assert we are in CLOSE_WAIT
    let conn = tcp_manager.connections.get(&conn_id).unwrap();
    assert_eq!(conn.state, State::CloseWait);

    // 3. Host closes connection -> sends FIN and transitions to LAST_ACK
    let mut responses = Vec::new();
    tcp_manager.close(&mut responses, &mut timers, conn_id);
    process_responses(&mut host, responses);

    let conn = tcp_manager.connections.get(&conn_id).unwrap();
    assert_eq!(conn.state, State::LastAck);

    // 4. Guest sends ACK for our FIN -> CLOSED (connection removed)
    let ethernet_packet = create_tcp_packet(&CreateTcpPacketArgs {
        src_addr: guest_addr,
        dst_addr: host_addr,
        seq: 1002, // FIN consumed 1 seq number, so next is 1002
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

    // Assert connection is completely removed
    assert!(!tcp_manager.connections.contains_key(&conn_id));
}

#[test]
fn test_tcp_connection_closed_established_sends_rst() {
    let config = Config::default();
    let mut slirp = Slirp::new(config.clone());

    let guest_port = 12345;
    let remote_port = 80;
    let remote_ip = Ipv4Addr::new(93, 184, 216, 34);
    let guest_iss = 1000;

    let build_tcp_packet = |seq: u32, ack: u32, flags: u16| -> Vec<u8> {
        let mut tcp_data = vec![0u8; 20];
        let mut tcp_builder = TcpBuilder::new(&mut tcp_data, config.guest_ipv4, remote_ip).unwrap();
        tcp_builder
            .source_port(guest_port)
            .dest_port(remote_port)
            .sequence_num(seq)
            .ack_num(ack)
            .flags(flags)
            .window_size(8192);
        tcp_builder.build();

        let mut ip_data = vec![0u8; 20 + tcp_data.len()];
        let (ip_header_slice, ip_payload_slice) = ip_data.split_at_mut(20);
        let mut ipv4_builder =
            Ipv4Builder::new(ip_header_slice, IP_P_TCP, config.guest_ipv4, remote_ip).unwrap();
        ipv4_builder.payload_len(tcp_data.len());
        ipv4_builder.build();
        ip_payload_slice.copy_from_slice(&tcp_data);

        let mut eth_data = vec![0u8; 14 + ip_data.len()];
        let (eth_header_slice, eth_payload_slice) = eth_data.split_at_mut(14);
        let eth_frame = EthernetFrame::mut_from_bytes(eth_header_slice).unwrap();
        eth_frame.dst_addr = config.gateway_mac;
        eth_frame.src_addr = config.guest_mac;
        eth_frame.ethertype = ether_type::IPV4.into();
        eth_payload_slice.copy_from_slice(&ip_data);
        eth_data
    };

    // 1. Guest sends SYN
    let syn_frame = build_tcp_packet(guest_iss, 0, netsim_packets::TCP_FLAG_SYN);
    let responses = slirp.handle_request(SlirpRequest::Packet(Bytes::from(syn_frame)));

    // Verify it returns EstablishConnection and a SYN-ACK packet
    assert_eq!(responses.len(), 2);
    let mut conn_id = None;
    let mut syn_ack_packet = None;
    for resp in &responses {
        match resp {
            SlirpResponse::EstablishConnection(id, ConnectionArgs::Tcp(_)) => {
                conn_id = Some(*id);
            }
            SlirpResponse::Packet(p) => {
                syn_ack_packet = Some(p.clone());
            }
            _ => {}
        }
    }
    let conn_id = conn_id.expect("Should have returned EstablishConnection");
    let syn_ack_packet = syn_ack_packet.expect("Should have returned SYN-ACK packet");

    // Parse SYN-ACK to get syn_ack_seq
    let (_eth_frame, ip_payload) = EthernetFrame::parse(&syn_ack_packet).unwrap();
    let (_ip_header, tcp_payload) = Ipv4Header::parse(ip_payload).unwrap();
    let (tcp_header, _) = TcpHeader::parse(tcp_payload).unwrap();
    let syn_ack_seq = tcp_header.sequence_num.get();

    // 2. Guest sends ACK to complete handshake
    let ack_frame =
        build_tcp_packet(guest_iss + 1, syn_ack_seq.wrapping_add(1), netsim_packets::TCP_FLAG_ACK);
    let _ = slirp.handle_request(SlirpRequest::Packet(Bytes::from(ack_frame)));

    // 3. Host signals connection closed on established flow
    let replies = slirp.handle_request(SlirpRequest::ConnectionClosed(conn_id));

    // 4. Verify a TCP RST packet is sent to the guest
    assert_eq!(replies.len(), 1);
    let reply_packet = match replies.first() {
        Some(SlirpResponse::Packet(p)) => p,
        _ => panic!("Expected a packet response, got {replies:?}"),
    };

    let (eth_frame, ip_payload) = EthernetFrame::parse(reply_packet).unwrap();
    assert_eq!(eth_frame.dst_addr, config.guest_mac);

    let (ip_header, tcp_payload) = Ipv4Header::parse(ip_payload).unwrap();
    assert_eq!(ip_header.protocol, IP_P_TCP);
    assert_eq!(Ipv4Addr::from(ip_header.dest_addr), config.guest_ipv4);
    assert_eq!(Ipv4Addr::from(ip_header.source_addr), remote_ip);

    let (tcp_header, _) = TcpHeader::parse(tcp_payload).unwrap();
    assert!(tcp_header.rst());
    assert!(tcp_header.ack());
    assert!(!tcp_header.syn());
    assert!(!tcp_header.fin());
    assert_eq!(tcp_header.source_port.get(), remote_port);
    assert_eq!(tcp_header.dest_port.get(), guest_port);
    assert_eq!(tcp_header.sequence_num.get(), syn_ack_seq.wrapping_add(1));
    assert_eq!(tcp_header.ack_num.get(), guest_iss + 1);
    assert!(slirp.connection_info().next().is_none());
}

#[tokio::test]
async fn test_abort_established_connection_sends_rst() {
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

    let (expected_seq, expected_ack) = {
        let conn = tcp_manager.connections.get(&conn_id).unwrap();
        (conn.send.nxt, conn.recv.nxt)
    };

    // 2. Abort connection
    let mut responses = Vec::new();
    tcp_manager.abort_connection(&mut responses, &mut timers, conn_id);

    // 3. Assert a RST packet is emitted and the connection is removed
    assert_eq!(responses.len(), 1);
    let packet = match responses.first() {
        Some(SlirpResponse::Packet(p)) => p,
        other => panic!("Expected a packet response, got {other:?}"),
    };

    let (_eth_frame, eth_payload) = EthernetFrame::parse(packet).unwrap();
    let (_ipv4_header_resp, ipv4_payload) = Ipv4Header::parse(eth_payload).unwrap();
    let (tcp_header_resp, _) = Ref::<_, TcpHeader>::from_prefix(ipv4_payload).unwrap();

    assert!(tcp_header_resp.rst());
    assert!(tcp_header_resp.ack());
    assert!(!tcp_header_resp.syn());
    assert!(!tcp_header_resp.fin());
    assert_eq!(tcp_header_resp.sequence_num.get(), expected_seq);
    assert_eq!(tcp_header_resp.ack_num.get(), expected_ack);
    assert_eq!(tcp_header_resp.source_port.get(), 80);
    assert_eq!(tcp_header_resp.dest_port.get(), 12345);

    assert!(!tcp_manager.connections.contains_key(&conn_id));
}

#[tokio::test]
async fn test_abort_syn_sent_connection_sends_no_rst() {
    let mut tcp_manager = TcpManager::new(Ipv4Addr::new(10, 0, 2, 2));
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

    let conn_id = tcp_manager.find_connection_by_addrs(guest_addr, host_addr).unwrap();
    assert_eq!(tcp_manager.connections.get(&conn_id).unwrap().state, State::SynSent);

    // 2. Abort connection while in SynSent
    responses.clear();
    tcp_manager.abort_connection(&mut responses, &mut timers, conn_id);

    // 3. Assert no RST packet is sent and the connection is removed
    assert!(responses.is_empty());
    assert!(!tcp_manager.connections.contains_key(&conn_id));
}

#[tokio::test]
async fn test_abort_close_wait_connection_sends_rst() {
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

    // 2. Guest sends FIN -> CLOSE_WAIT
    let ethernet_packet = create_tcp_packet(&CreateTcpPacketArgs {
        src_addr: guest_addr,
        dst_addr: host_addr,
        seq: 1001,
        ack: 1,
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

    let (expected_seq, expected_ack) = {
        let conn = tcp_manager.connections.get(&conn_id).unwrap();
        assert_eq!(conn.state, State::CloseWait);
        (conn.send.nxt, conn.recv.nxt)
    };

    // 3. Abort connection
    let mut responses = Vec::new();
    tcp_manager.abort_connection(&mut responses, &mut timers, conn_id);

    // 4. Assert a RST packet is emitted and the connection is removed
    assert_eq!(responses.len(), 1);
    let packet = match responses.first() {
        Some(SlirpResponse::Packet(p)) => p,
        other => panic!("Expected a packet response, got {other:?}"),
    };

    let (_eth_frame, eth_payload) = EthernetFrame::parse(packet).unwrap();
    let (_ipv4_header_resp, ipv4_payload) = Ipv4Header::parse(eth_payload).unwrap();
    let (tcp_header_resp, _) = Ref::<_, TcpHeader>::from_prefix(ipv4_payload).unwrap();

    assert!(tcp_header_resp.rst());
    assert!(tcp_header_resp.ack());
    assert!(!tcp_header_resp.syn());
    assert!(!tcp_header_resp.fin());
    assert_eq!(tcp_header_resp.sequence_num.get(), expected_seq);
    assert_eq!(tcp_header_resp.ack_num.get(), expected_ack);
    assert_eq!(tcp_header_resp.source_port.get(), 80);
    assert_eq!(tcp_header_resp.dest_port.get(), 12345);

    assert!(!tcp_manager.connections.contains_key(&conn_id));
}

#[tokio::test]
async fn test_abort_connection_time_wait_sends_no_rst() {
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

    // 2. Close connection from our side -> FIN_WAIT_1
    let mut responses = Vec::new();
    tcp_manager.close(&mut responses, &mut timers, conn_id);
    process_responses(&mut host, responses);

    // 3. Guest sends ACK for our FIN -> FIN_WAIT_2
    let conn = tcp_manager.connections.get(&conn_id).unwrap();
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

    // 4. Guest sends FIN -> TIME_WAIT
    let conn = tcp_manager.connections.get(&conn_id).unwrap();
    let ethernet_packet = create_tcp_packet(&CreateTcpPacketArgs {
        src_addr: guest_addr,
        dst_addr: host_addr,
        seq: 1001,
        ack: conn.send.nxt,
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

    // Assert we transitioned to TIME_WAIT
    assert_eq!(tcp_manager.connections.get(&conn_id).unwrap().state, State::TimeWait);

    // 5. Abort connection while in TimeWait
    let mut responses = Vec::new();
    tcp_manager.abort_connection(&mut responses, &mut timers, conn_id);

    // 6. Assert responses is empty (no RST sent) and TCB is removed
    assert!(responses.is_empty());
    assert!(!tcp_manager.connections.contains_key(&conn_id));
}
