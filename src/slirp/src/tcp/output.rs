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

use std::{collections::HashMap, net::IpAddr};

use bytes::Bytes;
use log::trace;
use netsim_packets::{
    EthernetFrame, Ipv4Builder, Ipv6Builder, MacAddr, TCP_FLAG_ACK, TCP_FLAG_FIN, TCP_FLAG_RST,
    TCP_FLAG_SYN, TcpBuilder, TcpOptionMss, TcpOptionSackPermitted, TcpOptionWindowScale,
    option_kind,
};
use zerocopy::FromBytes;

use super::{
    manager::{SendTcpPacketArgs, TcpManager},
    state::TcpConnection,
};
use crate::{
    SlirpResponse,
    timers::{TimerEvent, TimerManager},
};

impl TcpManager {
    pub(crate) fn send_tcp_packet(
        &mut self,
        responses: &mut Vec<SlirpResponse>,
        timers: &mut TimerManager,
        guest_mac: MacAddr,
        gateway_mac: MacAddr,
        args: &SendTcpPacketArgs,
    ) {
        let mut buffer = self.buffer_pool.pop().unwrap_or_default();
        Self::send_tcp_packet_helper(
            responses,
            timers,
            guest_mac,
            gateway_mac,
            args,
            &mut buffer,
            &mut self.connections,
        );
        if self.buffer_pool.len() < 100 {
            self.buffer_pool.push(buffer);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn send_tcp_packet_helper(
        responses: &mut Vec<SlirpResponse>,
        timers: &mut TimerManager,
        guest_mac: MacAddr,
        gateway_mac: MacAddr,
        args: &SendTcpPacketArgs,
        buffer: &mut Vec<u8>,
        connections: &mut HashMap<u64, TcpConnection>,
    ) {
        trace!("send_tcp_packet: guest_mac={guest_mac}, gateway_mac={gateway_mac}, args={args:?}");
        let eth_header_len = 14;
        let (ip_header_len, ethertype) = match args.host_addr.ip() {
            IpAddr::V4(_) => (20, 0x0800),
            IpAddr::V6(_) => (40, 0x86DD),
        };
        let has_options = args.syn;
        let tcp_header_len = if has_options { 32 } else { 20 };
        let header_len = eth_header_len + ip_header_len + tcp_header_len;
        let required_len = header_len + args.payload.len();

        if buffer.len() < required_len {
            buffer.resize(required_len, 0);
        }

        let (eth_header, eth_payload) = buffer.split_at_mut(eth_header_len);
        let eth_frame = EthernetFrame::mut_from_bytes(eth_header).unwrap();
        eth_frame.dst_addr = guest_mac;
        eth_frame.src_addr = gateway_mac;
        eth_frame.ethertype = ethertype.into();

        let (ip_header_slice, ip_payload) = eth_payload.split_at_mut(ip_header_len);
        match (args.host_addr.ip(), args.guest_addr.ip()) {
            (IpAddr::V4(host_ip), IpAddr::V4(guest_ip)) => {
                let mut ipv4_builder =
                    Ipv4Builder::new(ip_header_slice, netsim_packets::IP_P_TCP, host_ip, guest_ip)
                        .unwrap();
                ipv4_builder.payload_len(tcp_header_len + args.payload.len());
                ipv4_builder.build();
            }
            (IpAddr::V6(host_ip), IpAddr::V6(guest_ip)) => {
                let mut ipv6_builder =
                    Ipv6Builder::new(ip_header_slice, netsim_packets::IP_P_TCP, host_ip, guest_ip)
                        .unwrap();
                ipv6_builder.payload_len(tcp_header_len + args.payload.len());
                ipv6_builder.build();
            }
            _ => return,
        }

        let Some(tcp_segment_slice) = ip_payload.get_mut(..tcp_header_len + args.payload.len())
        else {
            return;
        };
        let mut tcp_builder = match (args.host_addr.ip(), args.guest_addr.ip()) {
            (IpAddr::V4(host_ip), IpAddr::V4(guest_ip)) => {
                TcpBuilder::new(tcp_segment_slice, host_ip, guest_ip).unwrap()
            }
            (IpAddr::V6(host_ip), IpAddr::V6(guest_ip)) => {
                TcpBuilder::new_v6(tcp_segment_slice, host_ip, guest_ip).unwrap()
            }
            _ => return,
        };

        if has_options {
            tcp_builder.data_offset(8); // 32 bytes header
            if let Some(opts) = tcp_builder.options_mut().filter(|opts| opts.len() >= 12) {
                let (mss_buf, rest) = opts.split_at_mut(4);
                let mss = TcpOptionMss::mut_from_bytes(mss_buf).unwrap();
                mss.kind = option_kind::MSS;
                mss.len = 4;
                mss.mss.set(1460);

                let (ws_buf, rest) = rest.split_at_mut(3);
                let ws = TcpOptionWindowScale::mut_from_bytes(ws_buf).unwrap();
                ws.kind = option_kind::WS;
                ws.len = 3;
                ws.shift = 7;

                let (sack_buf, rest) = rest.split_at_mut(2);
                let sack = TcpOptionSackPermitted::mut_from_bytes(sack_buf).unwrap();
                sack.kind = option_kind::SACK_PERMITTED;
                sack.len = 2;

                // NOP Padding
                rest.fill(option_kind::NOP);
            }
        }

        tcp_builder
            .source_port(args.host_addr.port())
            .dest_port(args.guest_addr.port())
            .sequence_num(args.sequence_num)
            .ack_num(args.ack_num)
            .flags(
                if args.syn { TCP_FLAG_SYN } else { 0 }
                    | if args.ack { TCP_FLAG_ACK } else { 0 }
                    | if args.fin { TCP_FLAG_FIN } else { 0 }
                    | if args.rst { TCP_FLAG_RST } else { 0 },
            )
            .window_size(65535)
            .payload(args.payload);

        tcp_builder.build();
        let total_len = eth_header_len + ip_header_len + tcp_header_len + args.payload.len();
        let Some(packet_slice) = buffer.get(..total_len) else {
            return;
        };
        responses.push(SlirpResponse::Packet(Bytes::copy_from_slice(packet_slice)));

        if let Some((conn_id, conn)) = args
            .conn_id
            .and_then(|id| connections.get_mut(&id).map(|c| (id, c)))
            .filter(|_| !args.payload.is_empty() || args.syn || args.fin)
        {
            let sequence_end = args.sequence_num.wrapping_add(args.payload.len() as u32);
            conn.sent_packets.push_back((sequence_end, timers.clock().now()));
            timers.schedule(conn.retransmission_timeout, TimerEvent::TcpRetransmit(conn_id));
        }
    }
}
