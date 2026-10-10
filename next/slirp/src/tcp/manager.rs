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
    collections::{HashMap, VecDeque},
    net::{IpAddr, Ipv4Addr, SocketAddr},
    time::Duration,
};

use log::{debug, warn};
use netsim_packets::MacAddr;
use serde::{Deserialize, Serialize};

use super::{
    congestion::CongestionControl,
    input::INITIAL_RTO,
    state::{RecvSequenceSpace, SendSequenceSpace, State, TcpConnection},
};
use crate::{SlirpResponse, TcpConnectionInfo, timers::TimerManager};

const DEFAULT_BUFFER_SIZE: usize = 2048;
const BUFFER_POOL_CAPACITY: usize = 100;
pub(crate) const MAX_DATA_RETRIES: u32 = 5;
pub(crate) const MAX_SYN_RETRIES: u32 = 5;

#[derive(Serialize, Deserialize)]
pub struct TcpManager {
    pub(crate) connections: HashMap<u64, TcpConnection>,
    #[serde(skip)]
    pub(crate) buffer_pool: Vec<Vec<u8>>,
    pub(crate) next_conn_id: u64,
    pub(crate) gateway_ip: Ipv4Addr,
    pub(crate) next_virtual_port: u16,
    #[serde(skip)]
    pub(crate) guestfwd: Vec<crate::GuestFwdRule>,
}

#[derive(Debug)]
pub(crate) struct SendTcpPacketArgs<'a> {
    pub(super) conn_id: Option<u64>,
    pub(super) sequence_num: u32,
    pub(super) ack_num: u32,
    pub(super) guest_addr: SocketAddr,
    pub(super) host_addr: SocketAddr,
    pub(super) ack: bool,
    pub(super) fin: bool,
    pub(super) syn: bool,
    pub(super) rst: bool,
    pub(super) payload: &'a [u8],
}

impl<'a> SendTcpPacketArgs<'a> {
    pub(super) fn ack(conn_id: u64, conn: &TcpConnection) -> SendTcpPacketArgs<'static> {
        SendTcpPacketArgs {
            conn_id: Some(conn_id),
            sequence_num: conn.send.nxt,
            ack_num: conn.recv.nxt,
            guest_addr: conn.guest_addr,
            host_addr: conn.host_addr,
            ack: true,
            fin: false,
            syn: false,
            rst: false,
            payload: &[],
        }
    }
}

impl TcpManager {
    pub fn new(gateway_ip: Ipv4Addr) -> Self {
        let mut buffer_pool = Vec::with_capacity(BUFFER_POOL_CAPACITY);
        for _ in 0..BUFFER_POOL_CAPACITY {
            buffer_pool.push(vec![0; DEFAULT_BUFFER_SIZE]);
        }
        Self {
            connections: HashMap::new(),
            buffer_pool,
            next_conn_id: 1,
            gateway_ip,
            next_virtual_port: 49152,
            guestfwd: Vec::new(),
        }
    }

    pub fn get_connections(&self) -> impl Iterator<Item = TcpConnectionInfo> + '_ {
        self.connections.values().map(|c| TcpConnectionInfo {
            local_addr: c.guest_addr,
            peer_addr: c.host_addr,
            state: c.state,
        })
    }

    pub(crate) fn get_connection(&self, conn_id: u64) -> Option<&TcpConnection> {
        self.connections.get(&conn_id)
    }

    pub fn find_connection_by_addrs(
        &self,
        src_addr: SocketAddr,
        dst_addr: SocketAddr,
    ) -> Option<u64> {
        self.connections
            .iter()
            .find(|(_, c)| c.guest_addr == src_addr && c.host_addr == dst_addr)
            .map(|(k, _)| *k)
    }

    pub fn close(
        &mut self,
        responses: &mut Vec<SlirpResponse>,
        timers: &mut TimerManager,
        conn_id: u64,
    ) {
        let mut packet_to_send = None;
        let mut macs = None;
        if let Some(conn) = self.connections.get_mut(&conn_id) {
            match conn.state {
                State::Established => {
                    conn.state = State::FinWait1;
                    packet_to_send = Some(SendTcpPacketArgs {
                        conn_id: Some(conn_id),
                        sequence_num: conn.send.nxt,
                        ack_num: conn.recv.nxt,
                        guest_addr: conn.guest_addr,
                        host_addr: conn.host_addr,
                        ack: true,
                        fin: true,
                        syn: false,
                        rst: false,
                        payload: &[],
                    });
                    conn.send.nxt = conn.send.nxt.wrapping_add(1);
                    macs = Some((conn.guest_mac, conn.gateway_mac));
                }
                State::CloseWait => {
                    conn.state = State::LastAck;
                    packet_to_send = Some(SendTcpPacketArgs {
                        conn_id: Some(conn_id),
                        sequence_num: conn.send.nxt,
                        ack_num: conn.recv.nxt,
                        guest_addr: conn.guest_addr,
                        host_addr: conn.host_addr,
                        ack: true,
                        fin: true,
                        syn: false,
                        rst: false,
                        payload: &[],
                    });
                    conn.send.nxt = conn.send.nxt.wrapping_add(1);
                    macs = Some((conn.guest_mac, conn.gateway_mac));
                }
                _ => {}
            }
        }
        if let (Some(args), Some((guest_mac, gateway_mac))) = (packet_to_send, macs) {
            self.send_tcp_packet(responses, timers, guest_mac, gateway_mac, &args);
        }
    }

    pub fn send_data(
        &mut self,
        responses: &mut Vec<SlirpResponse>,
        timers: &mut TimerManager,
        conn_id: u64,
        data: &[u8],
    ) {
        let mut packets_to_send = Vec::new();
        let mut conn_info = None;
        if let Some(conn) = self.connections.get_mut(&conn_id) {
            conn.recv_buffer.extend_from_slice(data);

            loop {
                let available_window = conn.send_window as usize;
                let congestion_window = conn.congestion_control.cwnd as usize;
                let effective_window = std::cmp::min(available_window, congestion_window);
                let mss = conn.mss.unwrap_or(536) as usize;

                let mut send_len = std::cmp::min(conn.recv_buffer.len(), effective_window);
                send_len = std::cmp::min(send_len, mss);

                if send_len == 0 {
                    break;
                }

                let data_to_send = conn.recv_buffer.drain(..send_len).collect::<Vec<_>>();
                conn.unacked.push_back(data_to_send.clone());

                let sequence_num = conn.send.nxt;
                conn.send.nxt = conn.send.nxt.wrapping_add(data_to_send.len() as u32);
                packets_to_send.push((sequence_num, data_to_send));
            }
            if !packets_to_send.is_empty() {
                conn_info = Some((
                    conn.guest_mac,
                    conn.gateway_mac,
                    conn.recv.nxt,
                    conn.guest_addr,
                    conn.host_addr,
                ));
            }
        }

        if let Some((guest_mac, gateway_mac, recv_nxt, guest_addr, host_addr)) = conn_info {
            for (sequence_num, data_to_send) in packets_to_send {
                self.send_tcp_packet(
                    responses,
                    timers,
                    guest_mac,
                    gateway_mac,
                    &SendTcpPacketArgs {
                        conn_id: Some(conn_id),
                        sequence_num,
                        ack_num: recv_nxt,
                        guest_addr,
                        host_addr,
                        ack: true,
                        fin: false,
                        syn: false,
                        rst: false,
                        payload: &data_to_send,
                    },
                );
            }
        }
    }

    pub fn remove_connection(&mut self, conn_id: u64) {
        self.connections.remove(&conn_id);
    }

    pub fn handle_remote_closed(
        &mut self,
        responses: &mut Vec<SlirpResponse>,
        timers: &mut TimerManager,
        conn_id: u64,
    ) {
        if let Some(conn) = self.connections.get_mut(&conn_id) {
            match conn.state {
                State::Established | State::SynSent => {
                    self.close(responses, timers, conn_id);
                }
                _ => {}
            }
        }
    }

    pub fn handle_timer(
        &mut self,
        responses: &mut Vec<SlirpResponse>,
        timers: &mut TimerManager,
        conn_id: u64,
    ) {
        if let Some(mut conn) = self.connections.remove(&conn_id) {
            match conn.state {
                State::TimeWait => {
                    // Connection is already closed, just remove it.
                    return;
                }
                State::SynSent => {
                    conn.retransmissions += 1;
                    if conn.retransmissions >= MAX_SYN_RETRIES {
                        responses.push(SlirpResponse::CloseConnection {
                            conn_id,
                            guest_addr: conn.guest_addr,
                        });
                        return;
                    }
                }
                _ => {
                    conn.retransmissions += 1;
                    if conn.retransmissions >= MAX_DATA_RETRIES {
                        responses.push(SlirpResponse::CloseConnection {
                            conn_id,
                            guest_addr: conn.guest_addr,
                        });
                        return;
                    }
                }
            }

            // Retransmit
            conn.congestion_control.on_retransmission(conn.mss.unwrap_or(536));
            conn.retransmission_timeout *= 2;

            let retransmit_conn = conn.clone();
            self.connections.insert(conn_id, conn);

            match retransmit_conn.state {
                State::SynSent => {
                    self.send_syn_ack(responses, timers, &retransmit_conn, conn_id);
                }
                _ => {
                    if let Some(segment_to_retransmit) = retransmit_conn.unacked.front() {
                        self.send_tcp_packet(
                            responses,
                            timers,
                            retransmit_conn.guest_mac,
                            retransmit_conn.gateway_mac,
                            &SendTcpPacketArgs {
                                conn_id: Some(conn_id),
                                sequence_num: retransmit_conn.send.una,
                                ack_num: retransmit_conn.recv.nxt,
                                guest_addr: retransmit_conn.guest_addr,
                                host_addr: retransmit_conn.host_addr,
                                ack: true,
                                fin: false,
                                syn: false,
                                rst: false,
                                payload: segment_to_retransmit,
                            },
                        );
                    }
                }
            }
        }
    }

    fn send_syn_ack(
        &mut self,
        responses: &mut Vec<SlirpResponse>,
        timers: &mut TimerManager,
        conn: &TcpConnection,
        conn_id: u64,
    ) {
        self.send_tcp_packet(
            responses,
            timers,
            conn.guest_mac,
            conn.gateway_mac,
            &SendTcpPacketArgs {
                conn_id: Some(conn_id),
                sequence_num: conn.send.iss,
                ack_num: conn.recv.nxt,
                guest_addr: conn.guest_addr,
                host_addr: conn.host_addr,
                ack: true,
                fin: false,
                syn: true,
                rst: false,
                payload: &[],
            },
        );
    }

    pub fn deactivate_fast_path(&mut self, conn_id: u64) {
        if let Some(_conn) = self.connections.get_mut(&conn_id) {
            // Nothing to do here yet, but this is where we would disable the
            // fast path if we were tracking it in the connection
            // struct.
        }
    }

    pub fn accept_incoming(
        &mut self,
        responses: &mut Vec<SlirpResponse>,
        timers: &mut TimerManager,
        conn_id: u64,
        guest_addr: SocketAddr,
        guest_mac: MacAddr,
        gateway_mac: MacAddr,
    ) {
        // 1. Allocate a unique virtual port on the gateway
        let gateway_ip = self.gateway_ip;
        let mut port = self.next_virtual_port;
        let mut port_allocated = false;
        let start_port = port;

        loop {
            self.next_virtual_port =
                if self.next_virtual_port == 65535 { 49152 } else { self.next_virtual_port + 1 };

            // Check if this port is already in use
            let port_in_use = self.connections.values().any(|c| {
                if let SocketAddr::V4(addr) = c.host_addr {
                    *addr.ip() == gateway_ip && addr.port() == port
                } else {
                    false
                }
            });

            if !port_in_use {
                port_allocated = true;
                break;
            }

            if self.next_virtual_port == start_port {
                break; // We looped through all ports!
            }
            port = self.next_virtual_port;
        }

        if !port_allocated {
            warn!("TCP hostfwd: No virtual ports available on gateway!");
            return;
        }

        let virtual_host_addr = SocketAddr::new(IpAddr::V4(gateway_ip), port);
        debug!("TCP hostfwd: Allocated virtual gateway port: {virtual_host_addr}");

        // 2. Initialize connection in SYN_SENT state
        let iss = 1000 + conn_id as u32;
        let mss_val = 1460;
        let mss = Some(mss_val); // Default MSS for local ethernet

        let conn = TcpConnection {
            state: State::SynSent,
            guest_mac,
            gateway_mac,
            guest_addr,
            host_addr: virtual_host_addr,
            send: SendSequenceSpace { iss, una: iss, nxt: iss },
            recv: RecvSequenceSpace {
                nxt: 0, // We don't know guest's seq yet
            },
            unacked: VecDeque::new(),
            retransmission_timeout: INITIAL_RTO,
            retransmissions: 0,
            mss,
            srtt: Duration::from_secs(0),
            rttvar: Duration::from_secs(0),
            sent_packets: VecDeque::new(),
            window_size: 8192,
            send_window: 8192,
            congestion_control: CongestionControl::new(mss_val),
            dup_acks: 0,
            recv_window_scale: None,
            sack_blocks: Vec::new(),
            recv_buffer: Vec::new(),
        };

        self.connections.insert(conn_id, conn);

        // 3. Send SYN packet to guest
        let args = SendTcpPacketArgs {
            conn_id: Some(conn_id),
            sequence_num: iss,
            ack_num: 0,
            guest_addr,
            host_addr: virtual_host_addr,
            ack: false,
            fin: false,
            syn: true,
            rst: false,
            payload: &[],
        };

        self.send_tcp_packet(responses, timers, guest_mac, gateway_mac, &args);

        // Update send.nxt to iss + 1 (since SYN consumes 1 seq)
        if let Some(c) = self.connections.get_mut(&conn_id) {
            c.send.nxt = c.send.nxt.wrapping_add(1);
        }
    }
}
