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
    cell::RefCell,
    collections::HashMap,
    net::{IpAddr, SocketAddr},
};

use netsim_packets::{
    EthernetFrame, IP_P_TCP, Ipv4Header, MacAddr, TcpBuilder, TcpHeader, ether_type,
};
use zerocopy::{FromBytes, Ref};

use crate::{
    ConnectionArgs, SlirpResponse,
    packet::{IpPacket, NetworkPacket, ParsedPacket},
    tcp::*,
    timers::TimerManager,
};

pub struct MockHost {
    pub packets_to_guest: RefCell<Vec<Vec<u8>>>,
    pub data_received_by_host: RefCell<HashMap<u64, Vec<u8>>>,
    pub closed_connections: RefCell<Vec<u64>>,
    pub established_connections: RefCell<HashMap<u64, ConnectionArgs>>,
}

impl MockHost {
    pub fn new() -> Self {
        Self {
            packets_to_guest: RefCell::new(Vec::new()),
            data_received_by_host: RefCell::new(HashMap::new()),
            closed_connections: RefCell::new(Vec::new()),
            established_connections: RefCell::new(HashMap::new()),
        }
    }
}

pub fn process_responses(host: &mut MockHost, responses: Vec<SlirpResponse>) {
    for response in responses {
        match response {
            SlirpResponse::Packet(p) => host.packets_to_guest.borrow_mut().push(p.to_vec()),
            SlirpResponse::CloseConnection { conn_id, .. } => {
                host.closed_connections.borrow_mut().push(conn_id)
            }
            SlirpResponse::WriteToConnection(conn_id, data) => host
                .data_received_by_host
                .borrow_mut()
                .entry(conn_id)
                .or_default()
                .extend_from_slice(&data),
            SlirpResponse::EstablishConnection(conn_id, conn_args) => {
                host.established_connections.borrow_mut().insert(conn_id, conn_args);
            }
            _ => {}
        }
    }
}

pub type ParsedTcpPacket<'a> =
    (Ref<&'a [u8], EthernetFrame>, Ref<&'a [u8], Ipv4Header>, Ref<&'a [u8], TcpHeader>, &'a [u8]);

pub fn parse_tcp_packet(packet: &[u8]) -> ParsedTcpPacket<'_> {
    let (eth, eth_payload) = EthernetFrame::parse(packet).unwrap();
    let (ipv4, ipv4_payload) = Ipv4Header::parse(eth_payload).unwrap();
    let (tcp, tcp_payload) = TcpHeader::parse(ipv4_payload).unwrap();
    (eth, ipv4, tcp, tcp_payload)
}

pub struct CreateTcpPacketArgs<'a> {
    pub src_addr: SocketAddr,
    pub dst_addr: SocketAddr,
    pub seq: u32,
    pub ack: u32,
    pub syn: bool,
    pub is_ack: bool,
    pub fin: bool,
    pub rst: bool,
    pub options: &'a [u8],
    pub payload: &'a [u8],
}

pub fn create_tcp_packet(args: &CreateTcpPacketArgs) -> Vec<u8> {
    // Round up options length to the nearest multiple of 4 to ensure header is
    // 32-bit aligned.
    let options_len_padded = (args.options.len() + 3) & !3;
    let tcp_header_len = 20 + options_len_padded;
    let ipv4_header_len = 20;
    let ethernet_header_len = 14;
    let total_len = ethernet_header_len + ipv4_header_len + tcp_header_len + args.payload.len();

    let mut ethernet_packet = vec![0u8; total_len];

    let (eth_header, eth_payload) = ethernet_packet.split_at_mut(ethernet_header_len);
    let eth_frame = EthernetFrame::mut_from_bytes(eth_header).unwrap();
    eth_frame.dst_addr = MacAddr { bytes: [0xde, 0xad, 0xbe, 0xef, 0x12, 0x35] };
    eth_frame.src_addr = MacAddr { bytes: [0xde, 0xad, 0xbe, 0xef, 0x12, 0x34] };
    eth_frame.ethertype = ether_type::IPV4.into();

    let (ipv4_header_slice, ipv4_payload_slice) = eth_payload.split_at_mut(ipv4_header_len);

    let mut ipv4_builder = netsim_packets::Ipv4Builder::new(
        ipv4_header_slice,
        IP_P_TCP,
        match args.src_addr.ip() {
            IpAddr::V4(ip) => ip,
            _ => panic!("IPv6 not supported"),
        },
        match args.dst_addr.ip() {
            IpAddr::V4(ip) => ip,
            _ => panic!("IPv6 not supported"),
        },
    )
    .unwrap();
    ipv4_builder.payload_len(tcp_header_len + args.payload.len());
    ipv4_builder.build();

    let data_offset_words = (tcp_header_len / 4) as u8;

    let tcp_segment_slice = &mut ipv4_payload_slice[..tcp_header_len + args.payload.len()];
    let mut tcp_builder = TcpBuilder::new(
        tcp_segment_slice,
        match args.src_addr.ip() {
            IpAddr::V4(ip) => ip,
            _ => panic!("IPv6 not supported"),
        },
        match args.dst_addr.ip() {
            IpAddr::V4(ip) => ip,
            _ => panic!("IPv6 not supported"),
        },
    )
    .unwrap();

    // Manually copy options into the header.
    if !args.options.is_empty() {
        let options_slice = &mut tcp_builder.payload_mut().unwrap()[..options_len_padded];
        options_slice[..args.options.len()].copy_from_slice(args.options);
    }

    tcp_builder
        .data_offset(data_offset_words)
        .source_port(args.src_addr.port())
        .dest_port(args.dst_addr.port())
        .sequence_num(args.seq)
        .ack_num(args.ack)
        .window_size(8192)
        .flags(
            if args.syn { netsim_packets::TCP_FLAG_SYN } else { 0 }
                | if args.is_ack { netsim_packets::TCP_FLAG_ACK } else { 0 }
                | if args.fin { netsim_packets::TCP_FLAG_FIN } else { 0 }
                | if args.rst { netsim_packets::TCP_FLAG_RST } else { 0 },
        )
        .payload(args.payload);

    tcp_builder.build();

    ethernet_packet
}

pub fn establish_connection_for_test(
    tcp_manager: &mut TcpManager,
    host: &mut MockHost,
    timers: &mut TimerManager,
    guest_addr: SocketAddr,
    host_addr: SocketAddr,
) -> (u64, u32) {
    let mut responses = Vec::new();
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

    tcp_manager.handle_packet(&mut responses, timers, &parsed_packet, &ipv4_header, tcp_packet);
    process_responses(host, responses);

    let sequence_num;
    // Check that we sent a SYN-ACK
    {
        let response = &host.packets_to_guest.borrow()[0];
        let (_eth_frame, eth_payload) = EthernetFrame::parse(response).unwrap();
        let (_ipv4_header_resp, ipv4_payload) = Ipv4Header::parse(eth_payload).unwrap();
        let (tcp_header_resp, _) = Ref::<_, TcpHeader>::from_prefix(ipv4_payload).unwrap();
        sequence_num = tcp_header_resp.sequence_num.get();
    }

    // 2. Guest sends ACK
    let ethernet_packet = create_tcp_packet(&CreateTcpPacketArgs {
        src_addr: guest_addr,
        dst_addr: host_addr,
        seq: 1001,
        ack: sequence_num + 1,
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
    tcp_manager.handle_packet(&mut responses, timers, &parsed_packet, &ipv4_header, tcp_packet);
    process_responses(host, responses);

    // Check connection is established
    let conn_id = tcp_manager.find_connection_by_addrs(guest_addr, host_addr).unwrap();
    (conn_id, sequence_num)
}
