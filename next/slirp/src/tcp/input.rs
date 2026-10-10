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
    collections::VecDeque,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    time::Duration,
};

use bytes::Bytes;
use log::trace;
use netsim_packets::{Ipv4Header, Ipv6Header, MacAddr};

use super::{
    manager::{SendTcpPacketArgs, TcpManager},
    state::{RecvSequenceSpace, SendSequenceSpace, State, TcpConnection},
    util,
};
use crate::{
    ConnectionArgs, SlirpResponse, TcpConnectionArgs,
    packet::{ParsedPacket, TransportPacket},
    tcp::congestion::CongestionControl,
    timers::{TimerEvent, TimerManager},
};

const TIME_WAIT_DURATION: Duration = Duration::from_secs(30);
pub(crate) const INITIAL_RTO: Duration = Duration::from_secs(1);

/// A trait for extracting common IP header information for TCP processing.
pub(crate) trait IpHeader {
    fn source_ip(&self) -> IpAddr;
    fn dest_ip(&self) -> IpAddr;
}

impl IpHeader for Ipv4Header {
    fn source_ip(&self) -> IpAddr {
        IpAddr::V4(Ipv4Addr::from(self.source_addr))
    }
    fn dest_ip(&self) -> IpAddr {
        IpAddr::V4(Ipv4Addr::from(self.dest_addr))
    }
}

impl IpHeader for Ipv6Header {
    fn source_ip(&self) -> IpAddr {
        IpAddr::V6(self.source_addr.into())
    }
    fn dest_ip(&self) -> IpAddr {
        IpAddr::V6(self.dest_addr.into())
    }
}

impl TcpManager {
    pub fn handle_packet(
        &mut self,
        responses: &mut Vec<SlirpResponse>,
        timers: &mut TimerManager,
        packet: &ParsedPacket,
        ipv4_header: &Ipv4Header,
        payload: &[u8],
    ) {
        self.handle_packet_generic(responses, timers, packet, ipv4_header, payload);
    }

    pub fn handle_ipv6_packet(
        &mut self,
        responses: &mut Vec<SlirpResponse>,
        timers: &mut TimerManager,
        packet: &ParsedPacket,
        ipv6_header: &Ipv6Header,
        payload: &[u8],
    ) {
        self.handle_packet_generic(responses, timers, packet, ipv6_header, payload);
    }

    fn handle_packet_generic<H: IpHeader>(
        &mut self,
        responses: &mut Vec<SlirpResponse>,
        timers: &mut TimerManager,
        packet: &ParsedPacket,
        ip_header: &H,
        payload: &[u8],
    ) {
        if let Some(TransportPacket::Tcp(tcp_header, tcp_payload)) = packet.transport {
            let is_syn = tcp_header.syn();
            let is_ack = tcp_header.ack();
            let is_fin = tcp_header.fin();
            let is_rst = tcp_header.rst();

            let src_addr = SocketAddr::new(ip_header.source_ip(), tcp_header.source_port.get());
            let dst_addr = SocketAddr::new(ip_header.dest_ip(), tcp_header.dest_port.get());
            crate::slirp_debug!(
                crate::logging::Topic::Tcp,
                "handle_tcp_packet: src={src_addr}, dst={dst_addr}, syn={is_syn}, ack={is_ack}, fin={is_fin}, rst={is_rst}"
            );

            if is_rst {
                if let Some((conn_id, conn)) = self
                    .find_connection_by_addrs(src_addr, dst_addr)
                    .and_then(|id| self.connections.remove(&id).map(|c| (id, c)))
                {
                    responses.push(SlirpResponse::CloseConnection {
                        conn_id,
                        guest_addr: conn.guest_addr,
                    });
                }
                return;
            }

            trace!(
                "connections: {:?}",
                self.connections.values().map(|c| (c.guest_addr, c.host_addr)).collect::<Vec<_>>()
            );
            if is_syn && !is_ack {
                crate::slirp_debug!(crate::logging::Topic::Tcp, "SYN packet");
                if let Some((conn_id, conn)) = self
                    .find_connection_by_addrs(src_addr, dst_addr)
                    .and_then(|id| self.connections.get(&id).map(|c| (id, c)))
                {
                    if conn.state == State::TimeWait {
                        // Silently drop SYN packets to connections in
                        // TIME_WAIT.
                        return;
                    } else {
                        // Drop unexpected SYN for existing active connections
                        // to prevent
                        // duplicate connection allocation
                        crate::slirp_debug!(
                            crate::logging::Topic::Tcp,
                            "SYN received for active connection {conn_id} in state {:?}, dropping.",
                            conn.state
                        );
                        return;
                    }
                }

                let conn_id = self.next_conn_id;
                self.next_conn_id += 1;

                let destination = self
                    .guestfwd
                    .iter()
                    .find(|rule| rule.virtual_addr == dst_addr)
                    .map(|rule| rule.host_addr)
                    .unwrap_or(dst_addr);

                let conn_info = TcpConnectionArgs {
                    destination,
                    guest_ip: src_addr.ip(),
                    guest_port: src_addr.port(),
                };
                responses.push(SlirpResponse::EstablishConnection(
                    conn_id,
                    ConnectionArgs::Tcp(conn_info),
                ));

                let iss = rand::random::<u32>();
                let (mss, window_scale, sack_blocks) =
                    util::parse_tcp_options(&tcp_header, payload);
                let send_window = if let Some(scale) = window_scale {
                    (tcp_header.window_size.get() as u32) << scale
                } else {
                    tcp_header.window_size.get() as u32
                };
                let guest_mac = match packet.ethernet {
                    netsim_packets::EthernetPacket::Untagged { frame, .. } => frame.src_addr,
                    netsim_packets::EthernetPacket::Vlan { frame, .. } => frame.src_addr,
                };
                let conn = TcpConnection {
                    state: State::SynSent,
                    guest_mac,
                    gateway_mac: MacAddr { bytes: [0xde, 0xad, 0xbe, 0xef, 0x12, 0x35] },
                    guest_addr: src_addr,
                    host_addr: dst_addr,
                    send: SendSequenceSpace { iss, una: iss, nxt: iss },
                    recv: RecvSequenceSpace { nxt: tcp_header.sequence_num.get().wrapping_add(1) },
                    unacked: VecDeque::new(),
                    retransmission_timeout: INITIAL_RTO,
                    retransmissions: 0,
                    mss,
                    srtt: Duration::from_secs(0),
                    rttvar: Duration::from_secs(0),
                    sent_packets: VecDeque::new(),
                    window_size: 8192,
                    send_window,
                    congestion_control: CongestionControl::new(mss.unwrap_or(536)),
                    dup_acks: 0,
                    recv_window_scale: window_scale,
                    sack_blocks,
                    recv_buffer: Vec::new(),
                };
                let guest_addr = conn.guest_addr;
                let host_addr = conn.host_addr;
                let iss = conn.send.iss;
                let recv_nxt = conn.recv.nxt;
                let gateway_mac = conn.gateway_mac;
                self.connections.insert(conn_id, conn);

                self.send_tcp_packet(
                    responses,
                    timers,
                    guest_mac,
                    gateway_mac,
                    &SendTcpPacketArgs {
                        conn_id: Some(conn_id),
                        sequence_num: iss,
                        ack_num: recv_nxt,
                        guest_addr,
                        host_addr,
                        ack: true,
                        fin: false,
                        syn: true,
                        rst: false,
                        payload: &[],
                    },
                );
                self.connections.get_mut(&conn_id).unwrap().send.nxt =
                    self.connections.get(&conn_id).unwrap().send.nxt.wrapping_add(1);
            } else if let Some(conn_id) = self.find_connection_by_addrs(src_addr, dst_addr) {
                crate::slirp_debug!(crate::logging::Topic::Tcp, "Found connection {conn_id}");
                let mut packet_to_send = None;
                let mut retransmit_payload = None;

                let mut should_remove = false;
                if let Some(mut conn) = self.connections.remove(&conn_id) {
                    let (_, _, sack_blocks) = util::parse_tcp_options(&tcp_header, payload);
                    if !sack_blocks.is_empty() {
                        conn.sack_blocks = sack_blocks.clone();
                    }
                    let ack_num = tcp_header.ack_num.get();
                    conn.window_size = tcp_header.window_size.get();
                    conn.send_window = if let Some(scale) = conn.recv_window_scale {
                        (conn.window_size as u32) << scale
                    } else {
                        conn.window_size as u32
                    };
                    let mut retransmit = false;
                    if is_ack {
                        if ack_num == conn.send.una {
                            conn.dup_acks += 1;
                            if conn.dup_acks == 3 {
                                retransmit = true;
                            }
                        } else {
                            conn.dup_acks = 0;
                        }

                        let acked_bytes = ack_num.wrapping_sub(conn.send.una);
                        if acked_bytes > 0 {
                            // RTT calculation
                            if let Some(pos) =
                                conn.sent_packets.iter().position(|(seq, _)| *seq == ack_num)
                            {
                                let (_, sent_at) = conn.sent_packets.remove(pos).unwrap();
                                let rtt = timers.clock().now().duration_since(sent_at);
                                if conn.srtt == Duration::from_secs(0) {
                                    // First measurement
                                    conn.srtt = rtt;
                                    conn.rttvar = rtt / 2;
                                } else {
                                    // Jacobson's algorithm
                                    let beta: u64 = 250_000; // 0.25
                                    let alpha: u64 = 125_000; // 0.125
                                    let srtt_micros = conn.srtt.as_micros() as u64;
                                    let rtt_micros = rtt.as_micros() as u64;
                                    let delta = srtt_micros.abs_diff(rtt_micros);
                                    conn.rttvar = Duration::from_micros(
                                        ((1_000_000 - beta) * conn.rttvar.as_micros() as u64
                                            + beta * delta)
                                            / 1_000_000,
                                    );
                                    conn.srtt = Duration::from_micros(
                                        ((1_000_000 - alpha) * srtt_micros + alpha * rtt_micros)
                                            / 1_000_000,
                                    );
                                }
                                conn.retransmission_timeout = conn.srtt + 4 * conn.rttvar;
                            }

                            let old_una = conn.send.una;
                            conn.send.una = ack_num;
                            conn.congestion_control.on_ack(conn.mss.unwrap_or(536));
                            let mut acked_len = conn.send.una.wrapping_sub(old_una) as usize;
                            while acked_len > 0 {
                                if let Some(front) = conn.unacked.front() {
                                    if acked_len >= front.len() {
                                        acked_len -= front.len();
                                        conn.unacked.pop_front();
                                    } else {
                                        // Partially acked segment, we don't
                                        // support this yet.
                                        break;
                                    }
                                } else {
                                    break;
                                }
                            }
                        }
                    }

                    if retransmit {
                        // Fast Retransmit
                        let mut retransmitted = false;
                        let mut current_seq = conn.send.una;
                        for segment in &conn.unacked {
                            let segment_end = current_seq.wrapping_add(segment.len() as u32);
                            let is_sacked = sack_blocks.iter().any(|(start, end)| {
                                // Check for any overlap between the segment and
                                // a SACK block.
                                current_seq < *end && segment_end > *start
                            });
                            if !is_sacked {
                                retransmit_payload = Some(segment.clone());
                                retransmitted = true;
                                break;
                            }
                            current_seq = segment_end;
                        }
                        if !retransmitted {
                            retransmit_payload = conn.unacked.front().cloned();
                        }
                    }
                    trace!("TCP state: {:?}", conn.state);
                    match conn.state {
                        State::SynSent => {
                            trace!("SYNSENT::on_packet tcp_header={tcp_header:?}");
                            if is_ack && ack_num == conn.send.nxt {
                                conn.state = State::Established;
                                timers.cancel_by_event(&TimerEvent::TcpRetransmit(conn_id));

                                if is_syn {
                                    // Host-initiated connection: guest replied
                                    // with SYN-ACK.
                                    // 1. Update recv.nxt to ack guest's SYN
                                    let guest_seq = tcp_header.sequence_num.get();
                                    conn.recv.nxt = guest_seq.wrapping_add(1);

                                    // 2. Send final ACK to guest
                                    packet_to_send = Some(SendTcpPacketArgs::ack(conn_id, &conn));

                                    // 3. Notify the host driver that the connection is established!
                                    responses.push(SlirpResponse::ConnectionEstablished(conn_id));
                                } else {
                                    // Guest-initiated connection: guest sent
                                    // ACK. Handshake
                                    // complete.
                                    responses.push(SlirpResponse::ActivateFastPath {
                                        conn_id,
                                        guest_addr: conn.guest_addr,
                                        host_addr: conn.host_addr,
                                    });
                                }
                            }
                        }
                        State::Established => {
                            trace!(
                                "ESTABLISHED::on_packet tcp_header={tcp_header:?}, is_fin={is_fin}"
                            );
                            if !tcp_payload.is_empty() {
                                conn.recv.nxt =
                                    conn.recv.nxt.wrapping_add(tcp_payload.len() as u32);
                                packet_to_send = Some(SendTcpPacketArgs::ack(conn_id, &conn));
                                responses.push(SlirpResponse::WriteToConnection(
                                    conn_id,
                                    Bytes::copy_from_slice(tcp_payload),
                                ));
                            }
                            if is_fin {
                                // Passive close
                                conn.state = State::CloseWait;
                                conn.recv.nxt = conn.recv.nxt.wrapping_add(1);
                                responses.push(SlirpResponse::CloseConnection {
                                    conn_id,
                                    guest_addr: conn.guest_addr,
                                });
                                if packet_to_send.is_none() {
                                    packet_to_send = Some(SendTcpPacketArgs::ack(conn_id, &conn));
                                }
                            }
                        }
                        State::FinWait1 => {
                            trace!("FINWAIT1::on_packet tcp_header={tcp_header:?}");
                            if is_fin {
                                // Simultaneous close
                                conn.state = State::Closing;
                                conn.recv.nxt = conn.recv.nxt.wrapping_add(1);
                                packet_to_send = Some(SendTcpPacketArgs::ack(conn_id, &conn));
                            } else if is_ack && ack_num == conn.send.nxt {
                                conn.state = State::FinWait2;
                            }
                        }
                        State::FinWait2 => {
                            trace!("FINWAIT2::on_packet tcp_header={tcp_header:?}");
                            if is_fin {
                                conn.state = State::TimeWait;
                                conn.recv.nxt = conn.recv.nxt.wrapping_add(1);
                                packet_to_send = Some(SendTcpPacketArgs::ack(conn_id, &conn));
                                timers.schedule(TIME_WAIT_DURATION, TimerEvent::Tcp(conn_id));
                            }
                        }
                        State::Closing => {
                            trace!("CLOSING::on_packet tcp_header={tcp_header:?}");
                            if is_ack && ack_num == conn.send.nxt {
                                conn.state = State::TimeWait;
                                timers.schedule(TIME_WAIT_DURATION, TimerEvent::Tcp(conn_id));
                            }
                        }
                        State::LastAck => {
                            trace!("LASTACK::on_packet tcp_header={tcp_header:?}");
                            if is_ack && ack_num == conn.send.nxt {
                                should_remove = true;
                            }
                        }
                        State::CloseWait => {
                            trace!("CLOSEWAIT::on_packet tcp_header={tcp_header:?}");
                        }
                        State::TimeWait => {
                            trace!("TIMEWAIT::on_packet tcp_header={tcp_header:?}");
                        }
                    }
                    if !should_remove {
                        self.connections.insert(conn_id, conn);
                    }
                }

                if let Some(args) = packet_to_send {
                    let guest_mac = match packet.ethernet {
                        netsim_packets::EthernetPacket::Untagged { frame, .. } => frame.src_addr,
                        netsim_packets::EthernetPacket::Vlan { frame, .. } => frame.src_addr,
                    };
                    if let Some(conn) = self.connections.get(&conn_id) {
                        self.send_tcp_packet(responses, timers, guest_mac, conn.gateway_mac, &args);
                    }
                }
                if let Some(payload) = retransmit_payload {
                    let (send_una, recv_nxt, guest_addr, host_addr, gateway_mac) =
                        if let Some(conn) = self.connections.get_mut(&conn_id) {
                            conn.congestion_control.on_retransmission(conn.mss.unwrap_or(536));
                            (
                                conn.send.una,
                                conn.recv.nxt,
                                conn.guest_addr,
                                conn.host_addr,
                                conn.gateway_mac,
                            )
                        } else {
                            return;
                        };
                    let guest_mac = match packet.ethernet {
                        netsim_packets::EthernetPacket::Untagged { frame, .. } => frame.src_addr,
                        netsim_packets::EthernetPacket::Vlan { frame, .. } => frame.src_addr,
                    };
                    let args = SendTcpPacketArgs {
                        conn_id: Some(conn_id),
                        sequence_num: send_una,
                        ack_num: recv_nxt,
                        guest_addr,
                        host_addr,
                        ack: true,
                        fin: false,
                        syn: false,
                        rst: false,
                        payload: &payload,
                    };
                    self.send_tcp_packet(responses, timers, guest_mac, gateway_mac, &args);
                }
            } else {
                crate::slirp_debug!(crate::logging::Topic::Tcp, "No connection found");
                // No connection found for a non-SYN packet. Send RST.
                // See RFC 793, page 37, "RESET Generation".
                let (sequence_num, ack_num, ack) = if is_ack {
                    // If the incoming segment has an ACK field, then
                    // <SEQ=SEG.ACK><CTL=RST>
                    (tcp_header.ack_num.get(), 0, false)
                } else {
                    // Otherwise
                    // <SEQ=0><ACK=SEG.SEQ+SEG.LEN><CTL=RST,ACK>
                    (
                        0,
                        tcp_header
                            .sequence_num
                            .get()
                            .wrapping_add(tcp_payload.len() as u32)
                            .wrapping_add(if is_fin { 1 } else { 0 }),
                        true,
                    )
                };

                let guest_mac = match packet.ethernet {
                    netsim_packets::EthernetPacket::Untagged { frame, .. } => frame.src_addr,
                    netsim_packets::EthernetPacket::Vlan { frame, .. } => frame.src_addr,
                };
                self.send_tcp_packet(
                    responses,
                    timers,
                    guest_mac,
                    MacAddr { bytes: [0xde, 0xad, 0xbe, 0xef, 0x12, 0x35] },
                    &SendTcpPacketArgs {
                        conn_id: None,
                        sequence_num,
                        ack_num,
                        guest_addr: src_addr,
                        host_addr: dst_addr,
                        ack,
                        fin: false,
                        syn: false,
                        rst: true,
                        payload: &[],
                    },
                );
            }
        }
    }
}
