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

#![allow(clippy::unwrap_used)]

pub mod arp;
pub mod dhcp;
pub mod dhcpv6;
pub mod dns;
pub mod icmp;
pub mod ip_fragment;
pub mod ndp;
pub mod packet;
pub mod tcp;

pub mod logging;
pub mod tftp;
pub mod udp;
pub mod utils;

#[cfg(test)]
pub mod tests;

use std::{
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    time::Duration,
};

use bytes::Bytes;
use log::{info, trace, warn};
use netsim_packets::{
    DHCPV6_SERVER_PORT, EthernetFrame, EthernetPacket, IP_P_ICMP, IP_P_ICMPV6, IP_P_TCP, IP_P_UDP,
    Icmpv6Header, Ipv4Builder, Ipv4Header, Ipv6Builder, Ipv6Header, MacAddr, NeighborSolicitation,
    RouterAdvertisementBuilder, RouterSolicitation, UdpBuilder, ether_type, icmpv6_checksum,
};
use serde::{Deserialize, Serialize};
pub use utils::{clock, timers};
use zerocopy::FromBytes;

use crate::{
    arp::ArpTable,
    clock::{Clock, SystemClock},
    dhcp::DhcpManager,
    dhcpv6::Dhcpv6Manager,
    dns::DnsProxy,
    icmp::{IcmpManager, Icmpv6Manager},
    ip_fragment::Reassembler,
    ndp::NdpTable,
    packet::{IpPacket, NetworkPacket, ParsedPacket, TransportPacket},
    tcp::{State, TcpManager},
    tftp::TftpManager,
    timers::{TimerEvent, TimerManager},
    udp::UdpManager,
};

pub mod api;
pub mod proxy;
pub use api::*;
pub use dns::discover_host_dns_servers;

pub struct Slirp {
    config: Config,
    arp_table: ArpTable,
    ndp_table: NdpTable,
    tcp_manager: TcpManager,
    udp_manager: UdpManager,
    dhcp_manager: DhcpManager,
    icmp_manager: IcmpManager,
    icmpv6_manager: Icmpv6Manager,
    dhcpv6_manager: Dhcpv6Manager,
    timer_manager: TimerManager,
    reassembler: Reassembler,
    tftp_manager: TftpManager,
    dns_proxy: DnsProxy,
}

impl Slirp {
    pub fn new(config: Config) -> Self {
        Self::new_with_clock(config, Box::new(SystemClock))
    }

    pub fn new_with_clock(config: Config, clock: Box<dyn Clock>) -> Self {
        let mut arp_table = ArpTable::new();
        arp_table.add_entry(config.host_ipv4, config.gateway_mac);
        let mut ndp_table = NdpTable::new();
        ndp_table.add_entry(config.host_ipv6, config.gateway_mac);
        let gateway_ip = config.host_ipv4;

        let mut timer_manager = TimerManager::new(clock);
        timer_manager.schedule(Duration::from_secs(600), TimerEvent::NdpRouterAdvertisement);

        let mut tcp_manager = TcpManager::new(gateway_ip);
        tcp_manager.guestfwd = Self::get_effective_guestfwd(&config);

        Self {
            config,
            arp_table,
            ndp_table,
            tcp_manager,
            udp_manager: UdpManager::new(),
            dhcp_manager: DhcpManager::new(),
            icmp_manager: IcmpManager::new(),
            icmpv6_manager: Icmpv6Manager::new(),
            dhcpv6_manager: Dhcpv6Manager::new(),
            tftp_manager: TftpManager::new(),
            dns_proxy: DnsProxy::new(),
            timer_manager,
            reassembler: Reassembler::new(),
        }
    }

    fn get_effective_guestfwd(config: &Config) -> Vec<GuestFwdRule> {
        let mut guestfwd = config.guestfwd.clone();

        // Add implicit DNS-over-TCP redirection rule for IPv4
        if let Some(dns_ipv4) = config.dns_servers.iter().find(|ip| ip.is_ipv4()) {
            guestfwd.push(GuestFwdRule {
                virtual_addr: SocketAddr::new(IpAddr::V4(config.host_ipv4), 53),
                host_addr: SocketAddr::new(*dns_ipv4, 53),
            });
        }

        // Add implicit DNS-over-TCP redirection rule for IPv6
        if let Some(dns_ipv6) = config.dns_servers.iter().find(|ip| ip.is_ipv6()) {
            guestfwd.push(GuestFwdRule {
                virtual_addr: SocketAddr::new(IpAddr::V6(config.host_ipv6), 53),
                host_addr: SocketAddr::new(*dns_ipv6, 53),
            });
        }

        guestfwd
    }

    pub fn handle_request(&mut self, request: SlirpRequest) -> Vec<SlirpResponse> {
        let mut responses = Vec::new();
        match request {
            SlirpRequest::Packet(packet) => {
                self.handle_packet(&packet, &mut responses);
            }
            SlirpRequest::WriteComplete(_conn_id) => {
                // TODO: Handle this
            }
            SlirpRequest::Data(conn_id, data) => {
                if conn_id >= (1 << 63) {
                    if let Some((src_addr, dst_addr, payload)) =
                        self.udp_manager.handle_reply(conn_id, &data)
                    {
                        // Intercept DNS replies to update cache!
                        let is_dns_reply = src_addr.port() == 53
                            && (src_addr.ip() == IpAddr::V4(self.config.host_ipv4)
                                || src_addr.ip() == IpAddr::V6(self.config.host_ipv6));
                        if is_dns_reply {
                            self.dns_proxy.handle_reply(&payload, self.timer_manager.clock().now());
                        }

                        self.send_udp_packet_to_guest(&mut responses, src_addr, dst_addr, &payload);
                    }
                } else if conn_id >= (1 << 61) + (1 << 60) {
                    self.icmpv6_manager.handle_reply(&mut responses, &self.config, conn_id, &data);
                } else if conn_id >= (1 << 61) {
                    self.icmp_manager.handle_reply(&mut responses, &self.config, conn_id, &data);
                } else {
                    self.tcp_manager.send_data(
                        &mut responses,
                        &mut self.timer_manager,
                        conn_id,
                        &data,
                    );
                }
            }
            SlirpRequest::ConnectionClosed(conn_id) => {
                if conn_id >= (1 << 63) {
                    self.udp_manager.remove_flow(conn_id);
                } else {
                    if let Some(conn) = self
                        .tcp_manager
                        .get_connection(conn_id)
                        .filter(|c| c.state == State::SynSent)
                    {
                        info!(
                            "TCP connection {conn_id} failed to connect, sending ICMP Host Unreachable"
                        );
                        let offending_packet =
                            synthesize_offending_tcp_packet(conn.guest_addr, conn.host_addr);
                        if conn.guest_addr.is_ipv6() {
                            self.icmpv6_manager.send_host_unreachable(
                                &mut responses,
                                &self.config,
                                &offending_packet,
                            );
                        } else {
                            self.icmp_manager.send_host_unreachable(
                                &mut responses,
                                &self.config,
                                &offending_packet,
                            );
                        }
                    }
                    self.tcp_manager.remove_connection(conn_id);
                }
            }
            SlirpRequest::RemoteClosed(conn_id) => {
                self.tcp_manager.handle_remote_closed(
                    &mut responses,
                    &mut self.timer_manager,
                    conn_id,
                );
            }
            SlirpRequest::Timer => {
                self.poll_timers(&mut responses);
            }
            SlirpRequest::DeactivateFastPath { conn_id } => {
                self.tcp_manager.deactivate_fast_path(conn_id);
            }
            SlirpRequest::AcceptIncoming { conn_id, host_addr: _, guest_addr } => {
                if let SocketAddr::V4(_) = guest_addr {
                    self.tcp_manager.accept_incoming(
                        &mut responses,
                        &mut self.timer_manager,
                        conn_id,
                        guest_addr,
                        self.config.guest_mac,
                        self.config.gateway_mac,
                    );
                }
            }
            SlirpRequest::SaveState(sender) => {
                if let Ok(state_bytes) = self.save_state() {
                    sender.send(state_bytes).ok();
                }
            }
            SlirpRequest::RestoreState(bytes) => match self.restore_state(&bytes) {
                Ok(mut restore_responses) => {
                    responses.append(&mut restore_responses);
                }
                Err(e) => {
                    warn!("Failed to restore Slirp state: {e}");
                }
            },
        }
        crate::ip_fragment::fragment_outgoing_packets(&mut responses);
        responses
    }

    fn handle_packet(&mut self, packet_buf: &[u8], responses: &mut Vec<SlirpResponse>) {
        let Some(packet) = ParsedPacket::parse(packet_buf) else {
            warn!("Failed to parse packet");
            return;
        };
        trace!("handle_packet: {packet:?}");

        // Handle IP fragmentation and reassembly
        if let Some(NetworkPacket::Ip(IpPacket::V4(ref ip_header, _))) = packet.network {
            let flags_offset = ip_header.flags_fragment_offset.get();
            let more_fragments = flags_offset & 0x2000 != 0;
            let offset = flags_offset & 0x1FFF;
            if more_fragments || offset > 0 {
                trace!("Received IP fragment: offset={offset}, more={more_fragments}");
                let eth_header_len = match packet.ethernet {
                    EthernetPacket::Untagged { .. } => 14,
                    EthernetPacket::Vlan { .. } => 18,
                };

                let Some((eth_header, ip_payload)) = packet_buf.split_at_checked(eth_header_len)
                else {
                    return;
                };
                if let Some(reassembled_ip) = self.reassembler.reassemble(ip_payload) {
                    trace!("IP reassembly complete, reconstructing Ethernet frame");
                    let mut reassembled_eth =
                        Vec::with_capacity(eth_header_len + reassembled_ip.len());
                    reassembled_eth.extend_from_slice(eth_header);
                    reassembled_eth.extend_from_slice(&reassembled_ip);

                    self.handle_packet(&reassembled_eth, responses);
                }
                return;
            }
        }

        let dst_mac = match packet.ethernet {
            EthernetPacket::Untagged { frame, .. } => frame.dst_addr,
            EthernetPacket::Vlan { frame, .. } => frame.dst_addr,
        };

        let is_ipv6_multicast = dst_mac.bytes.starts_with(&[0x33, 0x33]);
        if dst_mac.is_multicast() && !dst_mac.is_broadcast() && !is_ipv6_multicast {
            trace!("Dropping multicast packet to {dst_mac}");
            return;
        }

        if !dst_mac.is_multicast() && dst_mac != self.config.gateway_mac {
            trace!(
                "Dropping unicast packet not for gateway MAC ({}) but for {}",
                self.config.gateway_mac, dst_mac
            );
            return;
        }

        if let Some(ref network_packet) = packet.network {
            match network_packet {
                NetworkPacket::Ip(ip_packet) => match ip_packet {
                    IpPacket::V4(header, _) => {
                        let eth_header_len = match packet.ethernet {
                            EthernetPacket::Untagged { .. } => 14,
                            EthernetPacket::Vlan { .. } => 18,
                        };
                        let total_len = header.total_length.get() as usize;
                        let Some(ip_packet_slice) =
                            packet_buf.get(eth_header_len..eth_header_len + total_len)
                        else {
                            return;
                        };
                        self.handle_ipv4_packet(&packet, ip_packet_slice, responses)
                    }
                    IpPacket::V6(header, _) => {
                        let eth_header_len = match packet.ethernet {
                            EthernetPacket::Untagged { .. } => 14,
                            EthernetPacket::Vlan { .. } => 18,
                        };
                        let total_len = 40 + header.payload_length.get() as usize;
                        let Some(ip_packet_slice) =
                            packet_buf.get(eth_header_len..eth_header_len + total_len)
                        else {
                            return;
                        };
                        self.handle_ipv6_packet(&packet, ip_packet_slice, responses)
                    }
                },
                NetworkPacket::Arp(arp_packet) => {
                    if let Some(arp_reply_payload) = self.arp_table.handle_packet(arp_packet) {
                        let mut eth_reply_buf = Vec::with_capacity(14 + arp_reply_payload.len());
                        let dst_mac = match packet.ethernet {
                            EthernetPacket::Untagged { frame, .. } => frame.src_addr,
                            EthernetPacket::Vlan { frame, .. } => frame.src_addr,
                        };
                        let src_mac = self.config.gateway_mac;

                        eth_reply_buf.extend_from_slice(&dst_mac.bytes);
                        eth_reply_buf.extend_from_slice(&src_mac.bytes);
                        eth_reply_buf.extend_from_slice(&ether_type::ARP.to_be_bytes());
                        eth_reply_buf.extend_from_slice(&arp_reply_payload);

                        responses
                            .push(SlirpResponse::Packet(Bytes::copy_from_slice(&eth_reply_buf)));
                    }
                }
            }
        }
    }

    fn handle_ipv4_packet(
        &mut self,
        packet: &ParsedPacket,
        ipv4_packet: &[u8],
        responses: &mut Vec<SlirpResponse>,
    ) {
        let Some((header, payload)) = Ipv4Header::parse(ipv4_packet) else {
            warn!("Failed to parse IPv4 header");
            return;
        };

        let guest_mac = match packet.ethernet {
            EthernetPacket::Untagged { frame, .. } => frame.src_addr,
            EthernetPacket::Vlan { frame, .. } => frame.src_addr,
        };
        let source_ip = Ipv4Addr::from(header.source_addr);
        if !source_ip.is_unspecified() {
            self.arp_table.add_entry(source_ip, guest_mac);
        }

        let dest_ip = Ipv4Addr::from(header.dest_addr);
        let is_for_gateway = dest_ip == self.config.host_ipv4;

        // Check TTL expiration for routed packets
        if !is_for_gateway && header.ttl <= 1 {
            trace!("TTL expired for packet to {dest_ip}");
            self.icmp_manager.send_time_exceeded(responses, &self.config, ipv4_packet);
            return;
        }

        match header.protocol {
            IP_P_TCP => {
                self.tcp_manager.handle_packet(
                    responses,
                    &mut self.timer_manager,
                    packet,
                    &header,
                    payload,
                );
            }
            IP_P_UDP => {
                self.handle_udp_packet(packet, ipv4_packet, responses);
            }
            IP_P_ICMP => {
                self.icmp_manager.handle_packet(responses, &self.config, packet, payload);
            }
            _ => {}
        }
    }

    fn handle_ipv6_packet(
        &mut self,
        packet: &ParsedPacket,
        ipv6_packet: &[u8],
        responses: &mut Vec<SlirpResponse>,
    ) {
        let Some(NetworkPacket::Ip(IpPacket::V6(header, payload))) = &packet.network else {
            return;
        };
        let guest_mac = match packet.ethernet {
            EthernetPacket::Untagged { frame, .. } => frame.src_addr,
            EthernetPacket::Vlan { frame, .. } => frame.src_addr,
        };
        let source_ip = Ipv6Addr::from(header.source_addr);
        if !source_ip.is_unspecified() {
            self.ndp_table.add_entry(source_ip, guest_mac);
        }
        if header.next_header == IP_P_ICMPV6 {
            self.handle_icmpv6_packet(packet, header, payload, responses);
        } else if header.next_header == IP_P_TCP {
            self.tcp_manager.handle_ipv6_packet(
                responses,
                &mut self.timer_manager,
                packet,
                header,
                payload,
            );
        } else if header.next_header == IP_P_UDP {
            self.handle_udp_packet(packet, ipv6_packet, responses);
        }
    }

    fn handle_icmpv6_packet(
        &mut self,
        packet: &ParsedPacket,
        header: &Ipv6Header,
        payload: &[u8],
        responses: &mut Vec<SlirpResponse>,
    ) {
        let Some((icmpv6_header, icmpv6_payload)) = Icmpv6Header::parse(payload) else {
            trace!("Failed to parse ICMPv6 header");
            return;
        };

        // Neighbor Solicitation is type 135
        if icmpv6_header.icmpv6_type == 135 {
            let Some(ns) = NeighborSolicitation::parse(icmpv6_payload) else {
                trace!("Failed to parse Neighbor Solicitation");
                return;
            };

            if let Some(na_payload) = self.ndp_table.handle_packet(&ns) {
                self.send_neighbor_advertisement(responses, packet, header, &na_payload);
            }
        // Router Solicitation is type 133
        } else if icmpv6_header.icmpv6_type == 133 {
            let Some(_rs) = RouterSolicitation::parse(icmpv6_payload) else {
                trace!("Failed to parse Router Solicitation");
                return;
            };
            self.send_router_advertisement(responses, packet, header);
        } else {
            self.icmpv6_manager.handle_packet(responses, &self.config, packet, payload);
        }
    }

    fn send_neighbor_advertisement(
        &mut self,
        responses: &mut Vec<SlirpResponse>,
        original_packet: &ParsedPacket,
        original_ipv6_header: &Ipv6Header,
        na_payload: &[u8],
    ) {
        let eth_header_len = 14;
        let ipv6_header_len = 40;
        let icmpv6_header_len = 8;
        let total_icmpv6_len = icmpv6_header_len + na_payload.len();
        let total_ipv6_len = ipv6_header_len + total_icmpv6_len;
        let total_len = eth_header_len + total_ipv6_len;

        let mut buffer = vec![0u8; total_len];

        // 1. Fill Ethernet Header
        let guest_mac = match original_packet.ethernet {
            EthernetPacket::Untagged { frame, .. } => frame.src_addr,
            EthernetPacket::Vlan { frame, .. } => frame.src_addr,
        };
        let (eth_slice, eth_payload) = buffer.split_at_mut(eth_header_len);
        let eth_frame = EthernetFrame::mut_from_bytes(eth_slice).unwrap();
        eth_frame.dst_addr = guest_mac;
        eth_frame.src_addr = self.config.gateway_mac;
        eth_frame.ethertype = 0x86DD.into(); // IPv6

        // 2. Build IPv6 Header
        let (ipv6_slice, ipv6_payload) = eth_payload.split_at_mut(ipv6_header_len);
        let mut ipv6_builder = Ipv6Builder::new(
            ipv6_slice,
            IP_P_ICMPV6,
            self.config.host_ipv6,
            Ipv6Addr::from(original_ipv6_header.source_addr),
        )
        .unwrap();
        ipv6_builder.hop_limit(255); // NDP requires Hop Limit = 255

        // 3. Build ICMPv6 Header and copy NA payload
        let (icmpv6_slice, icmpv6_payload_slice) = ipv6_payload.split_at_mut(icmpv6_header_len);
        let icmpv6_header = Icmpv6Header::mut_from_bytes(icmpv6_slice).unwrap();
        icmpv6_header.icmpv6_type = 136; // Neighbor Advertisement
        icmpv6_header.icmpv6_code = 0;
        icmpv6_header.icmpv6_checksum = 0.into();
        icmpv6_header.rest = [0u8; 4];

        icmpv6_payload_slice.copy_from_slice(na_payload);

        // 4. Calculate ICMPv6 Checksum (uses pseudo-header)
        let checksum = icmpv6_checksum(
            ipv6_payload,
            self.config.host_ipv6,
            Ipv6Addr::from(original_ipv6_header.source_addr),
        );
        let (icmpv6_hdr_slice, _) = ipv6_payload.split_at_mut(icmpv6_header_len);
        let mut_icmpv6_header = Icmpv6Header::mut_from_bytes(icmpv6_hdr_slice).unwrap();
        mut_icmpv6_header.icmpv6_checksum.set(checksum);

        // 5. Finalize IPv6 Header
        ipv6_builder.payload_len(total_icmpv6_len);
        ipv6_builder.build();

        responses.push(SlirpResponse::Packet(Bytes::from(buffer)));
    }

    fn send_router_advertisement(
        &mut self,
        responses: &mut Vec<SlirpResponse>,
        original_packet: &ParsedPacket,
        original_ipv6_header: &Ipv6Header,
    ) {
        let source_ip = Ipv6Addr::from(original_ipv6_header.source_addr);
        let (dest_ip, dest_mac) = if source_ip.is_unspecified() {
            (
                Ipv6Addr::new(0xff02, 0, 0, 0, 0, 0, 0, 1),
                MacAddr { bytes: [0x33, 0x33, 0x00, 0x00, 0x00, 0x01] },
            )
        } else {
            let guest_mac = match original_packet.ethernet {
                EthernetPacket::Untagged { frame, .. } => frame.src_addr,
                EthernetPacket::Vlan { frame, .. } => frame.src_addr,
            };
            (source_ip, guest_mac)
        };

        self.send_router_advertisement_raw(responses, dest_ip, dest_mac);
    }

    fn send_periodic_router_advertisement(&mut self, responses: &mut Vec<SlirpResponse>) {
        let dest_ip = Ipv6Addr::new(0xff02, 0, 0, 0, 0, 0, 0, 1);
        let dest_mac = MacAddr { bytes: [0x33, 0x33, 0x00, 0x00, 0x00, 0x01] };
        self.send_router_advertisement_raw(responses, dest_ip, dest_mac);
    }

    fn send_router_advertisement_raw(
        &mut self,
        responses: &mut Vec<SlirpResponse>,
        dest_ip: Ipv6Addr,
        dest_mac: MacAddr,
    ) {
        let eth_header_len = 14;
        let ipv6_header_len = 40;
        let icmpv6_header_len = 8;
        let ra_payload_len = 76; // SLLA (8) + PIO (32) + RDNSS (24) + RA (12) = 76 bytes!
        let total_icmpv6_len = icmpv6_header_len + ra_payload_len;
        let total_ipv6_len = ipv6_header_len + total_icmpv6_len;
        let total_len = eth_header_len + total_ipv6_len;

        let mut buffer = vec![0u8; total_len];

        // 1. Fill Ethernet Header
        let (eth_slice, eth_payload) = buffer.split_at_mut(eth_header_len);
        let eth_frame = EthernetFrame::mut_from_bytes(eth_slice).unwrap();
        eth_frame.dst_addr = dest_mac;
        eth_frame.src_addr = self.config.gateway_mac;
        eth_frame.ethertype = 0x86DD.into(); // IPv6

        // 2. Build IPv6 Header
        let (ipv6_slice, ipv6_payload) = eth_payload.split_at_mut(ipv6_header_len);
        let mut ipv6_builder =
            Ipv6Builder::new(ipv6_slice, IP_P_ICMPV6, self.config.host_ipv6, dest_ip).unwrap();
        ipv6_builder.hop_limit(255); // NDP requires Hop Limit = 255

        // 3. Build ICMPv6 Header
        let (icmpv6_slice, icmpv6_payload_slice) = ipv6_payload.split_at_mut(icmpv6_header_len);
        let icmpv6_header = Icmpv6Header::mut_from_bytes(icmpv6_slice).unwrap();
        icmpv6_header.icmpv6_type = 134; // Router Advertisement
        icmpv6_header.icmpv6_code = 0;
        icmpv6_header.icmpv6_checksum = 0.into();
        icmpv6_header.rest = [0u8; 4];

        // 4. Build RA Payload (RA Header + Options)
        let prefix = (u128::from(self.config.guest_ipv6) & (u128::MAX << 64)).to_be_bytes();

        let ra_builder = RouterAdvertisementBuilder::new(icmpv6_payload_slice).unwrap();
        ra_builder
            .build(
                64,
                0,
                1800,
                0,
                0,
                self.config.gateway_mac.bytes,
                prefix,
                64,
                Some(self.config.host_ipv6.octets()), // Announce gateway as DNS server!
            )
            .unwrap();

        // 5. Calculate ICMPv6 Checksum
        let checksum = icmpv6_checksum(ipv6_payload, self.config.host_ipv6, dest_ip);
        let (icmpv6_hdr_slice, _) = ipv6_payload.split_at_mut(icmpv6_header_len);
        let mut_icmpv6_header = Icmpv6Header::mut_from_bytes(icmpv6_hdr_slice).unwrap();
        mut_icmpv6_header.icmpv6_checksum.set(checksum);

        // 6. Finalize IPv6 Header
        ipv6_builder.payload_len(total_icmpv6_len);
        ipv6_builder.build();

        responses.push(SlirpResponse::Packet(Bytes::from(buffer)));
    }

    fn handle_udp_packet(
        &mut self,
        packet: &ParsedPacket,
        l3_packet: &[u8],
        responses: &mut Vec<SlirpResponse>,
    ) {
        let Some(NetworkPacket::Ip(ip_packet)) = &packet.network else {
            return;
        };
        let Some(TransportPacket::Udp(udp_header, udp_payload)) = &packet.transport else {
            return;
        };

        let (src_ip, dest_ip, is_to_gateway, is_ipv6) = match ip_packet {
            IpPacket::V4(h, _) => (
                IpAddr::V4(Ipv4Addr::from(h.source_addr)),
                IpAddr::V4(Ipv4Addr::from(h.dest_addr)),
                Ipv4Addr::from(h.dest_addr) == self.config.host_ipv4,
                false,
            ),
            IpPacket::V6(h, _) => (
                IpAddr::V6(Ipv6Addr::from(h.source_addr)),
                IpAddr::V6(Ipv6Addr::from(h.dest_addr)),
                Ipv6Addr::from(h.dest_addr) == self.config.host_ipv6,
                true,
            ),
        };

        let dest_port = udp_header.dest_port.get();
        let src_port = udp_header.source_port.get();

        // 1. Handle DHCP (v4 and v6)
        if !is_ipv6 && dest_port == 67 {
            self.dhcp_manager.handle_packet(
                responses,
                &self.config,
                self.timer_manager.clock(),
                udp_payload,
            );
            return;
        } else if is_ipv6 && dest_port == DHCPV6_SERVER_PORT {
            if let IpAddr::V6(guest_ip) = src_ip {
                self.dhcpv6_manager.handle_packet(
                    responses,
                    &self.config,
                    packet,
                    guest_ip,
                    udp_header,
                    udp_payload,
                );
            }
            return;
        }

        let src_addr = SocketAddr::new(src_ip, src_port);
        let dst_addr = SocketAddr::new(dest_ip, dest_port);

        // 1a. Handle TFTP
        if dest_port == 69 || self.tftp_manager.has_session_with_tid(dest_port) {
            let guest_mac = match packet.ethernet {
                EthernetPacket::Untagged { frame, .. } => frame.src_addr,
                EthernetPacket::Vlan { frame, .. } => frame.src_addr,
            };
            self.tftp_manager.handle_packet(
                responses,
                &self.config,
                udp_payload,
                src_addr,
                dest_port,
                guest_mac,
            );
            return;
        }

        // 2. Handle DNS Redirection
        if is_to_gateway && dest_port == 53 {
            use crate::dns::DnsQueryResult;
            let result = self.dns_proxy.handle_query(
                udp_payload,
                src_addr,
                self.timer_manager.clock().now(),
                &self.config.dns_servers,
            );
            match result {
                DnsQueryResult::Cached(reply) => {
                    self.send_udp_packet_to_guest(responses, dst_addr, src_addr, &reply);
                }
                DnsQueryResult::Forward(server_ip) => {
                    let redirect_dest = SocketAddr::new(server_ip, 53);
                    self.udp_manager.handle_packet(
                        responses,
                        udp_payload,
                        src_addr,
                        dst_addr,
                        Some(redirect_dest),
                    );
                }
                DnsQueryResult::Ignore => {}
            }
            return;
        }

        // 3. Handle Closed Gateway Ports
        if is_to_gateway {
            if !is_ipv6 {
                trace!("UDP packet to closed gateway port: {dest_port}");
                self.icmp_manager.send_port_unreachable(responses, &self.config, l3_packet);
            } else {
                trace!("UDP packet to closed IPv6 gateway port: {dest_port}");
                self.icmpv6_manager.send_port_unreachable(responses, &self.config, l3_packet);
            }
            return;
        }

        // 4. Normal UDP Forwarding
        let redirect_dest = self
            .config
            .guestfwd
            .iter()
            .find(|rule| rule.virtual_addr == dst_addr)
            .map(|rule| rule.host_addr);

        self.udp_manager.handle_packet(responses, udp_payload, src_addr, dst_addr, redirect_dest);
    }

    fn poll_timers(&mut self, responses: &mut Vec<SlirpResponse>) {
        for event in self.timer_manager.tick() {
            match event {
                TimerEvent::Tcp(conn_id) => {
                    self.tcp_manager.handle_timer(responses, &mut self.timer_manager, conn_id);
                }
                TimerEvent::TcpRetransmit(conn_id) => {
                    self.tcp_manager.handle_timer(responses, &mut self.timer_manager, conn_id);
                }
                TimerEvent::NdpRouterAdvertisement => {
                    self.send_periodic_router_advertisement(responses);
                    self.timer_manager
                        .schedule(Duration::from_secs(600), TimerEvent::NdpRouterAdvertisement);
                }
                _ => {}
            }
        }
        if let Some(duration) = self.timer_manager.next_event_in() {
            responses.push(SlirpResponse::SetTimer(duration));
        }
    }

    pub fn connection_info(&self) -> impl Iterator<Item = ConnectionInfo> + '_ {
        let tcp_info = self.tcp_manager.get_connections().map(ConnectionInfo::Tcp);
        let udp_info = self.udp_manager.get_connections().map(ConnectionInfo::Udp);
        tcp_info.chain(udp_info)
    }

    fn send_udp_packet_to_guest(
        &self,
        responses: &mut Vec<SlirpResponse>,
        src_addr: SocketAddr,
        dst_addr: SocketAddr,
        payload: &[u8],
    ) {
        let eth_header_len = 14;
        let udp_header_len = 8;

        let (ip_header_len, ethertype) = match src_addr {
            SocketAddr::V4(_) => (20, 0x0800),
            SocketAddr::V6(_) => (40, 0x86DD),
        };

        let dst_mac = match dst_addr {
            SocketAddr::V4(dst_v4) => {
                self.arp_table.lookup(dst_v4.ip()).unwrap_or(self.config.guest_mac)
            }
            SocketAddr::V6(dst_v6) => {
                self.ndp_table.lookup(dst_v6.ip()).unwrap_or(self.config.guest_mac)
            }
        };

        let total_len = eth_header_len + ip_header_len + udp_header_len + payload.len();
        let mut buffer = vec![0u8; total_len];

        // 1. Fill Ethernet Header
        let (eth_slice, eth_payload) = buffer.split_at_mut(eth_header_len);
        let eth_frame = EthernetFrame::mut_from_bytes(eth_slice).unwrap();
        eth_frame.dst_addr = dst_mac;
        eth_frame.src_addr = self.config.gateway_mac;
        eth_frame.ethertype = ethertype.into();

        // 2. Build IP Header
        let (ip_header_slice, ip_payload) = eth_payload.split_at_mut(ip_header_len);
        match (src_addr, dst_addr) {
            (SocketAddr::V4(src_v4), SocketAddr::V4(dst_v4)) => {
                let mut ipv4_builder =
                    Ipv4Builder::new(ip_header_slice, IP_P_UDP, *src_v4.ip(), *dst_v4.ip())
                        .unwrap();
                ipv4_builder.payload_len(udp_header_len + payload.len());
                ipv4_builder.build();
            }
            (SocketAddr::V6(src_v6), SocketAddr::V6(dst_v6)) => {
                let mut ipv6_builder =
                    Ipv6Builder::new(ip_header_slice, IP_P_UDP, *src_v6.ip(), *dst_v6.ip())
                        .unwrap();
                ipv6_builder.payload_len(udp_header_len + payload.len());
                ipv6_builder.build();
            }
            _ => return, // Mismatched family
        }

        // 3. Build UDP Header and Payload
        match (src_addr, dst_addr) {
            (SocketAddr::V4(src_v4), SocketAddr::V4(dst_v4)) => {
                let mut udp_builder = UdpBuilder::new(
                    ip_payload,
                    *src_v4.ip(),
                    *dst_v4.ip(),
                    src_addr.port(),
                    dst_addr.port(),
                )
                .unwrap();
                udp_builder.payload(payload).unwrap();
                udp_builder.build().unwrap();
            }
            (SocketAddr::V6(src_v6), SocketAddr::V6(dst_v6)) => {
                let mut udp_builder = UdpBuilder::new_v6(
                    ip_payload,
                    *src_v6.ip(),
                    *dst_v6.ip(),
                    src_addr.port(),
                    dst_addr.port(),
                )
                .unwrap();
                udp_builder.payload(payload).unwrap();
                udp_builder.build().unwrap();
            }
            _ => return, // Mismatched family
        }

        responses.push(SlirpResponse::Packet(Bytes::copy_from_slice(&buffer)));
    }

    pub fn save_state(&self) -> Result<Vec<u8>, serde_json::Error> {
        let state = SlirpStateRef {
            arp_table: &self.arp_table,
            ndp_table: &self.ndp_table,
            dhcp_manager: &self.dhcp_manager,
            tcp_manager: &self.tcp_manager,
            udp_manager: &self.udp_manager,
        };
        serde_json::to_vec(&state)
    }

    pub fn restore_state(
        &mut self,
        state_bytes: &[u8],
    ) -> Result<Vec<SlirpResponse>, serde_json::Error> {
        let state: SlirpStateOwned = serde_json::from_slice(state_bytes)?;

        self.arp_table = state.arp_table;
        self.ndp_table = state.ndp_table;
        self.dhcp_manager = state.dhcp_manager;
        self.tcp_manager = state.tcp_manager;
        self.tcp_manager.guestfwd = Self::get_effective_guestfwd(&self.config);
        self.udp_manager = state.udp_manager;
        self.tftp_manager = tftp::TftpManager::new();
        self.dns_proxy = dns::DnsProxy::new();

        // Reconstruct timers!
        self.timer_manager.clear();

        self.timer_manager.schedule(Duration::from_secs(600), TimerEvent::NdpRouterAdvertisement);

        // For each TCP connection, if it has unacked data, schedule a
        // retransmission timer!
        for (&conn_id, conn) in &self.tcp_manager.connections {
            if !conn.unacked.is_empty() || conn.state == State::SynSent {
                self.timer_manager
                    .schedule(conn.retransmission_timeout, TimerEvent::TcpRetransmit(conn_id));
            }
        }

        // Re-initialize reassembler
        self.reassembler = Reassembler::new();

        let mut responses = vec![SlirpResponse::Reset];

        // 1. Re-establish TCP connections
        for (&conn_id, conn) in &self.tcp_manager.connections {
            responses.push(SlirpResponse::EstablishConnection(
                conn_id,
                ConnectionArgs::Tcp(TcpConnectionArgs {
                    destination: conn.host_addr,
                    guest_ip: conn.guest_addr.ip(),
                    guest_port: conn.guest_addr.port(),
                }),
            ));
        }

        // 2. Re-establish UDP connections (flows)
        for (&conn_id, flow) in &self.udp_manager.id_to_flow {
            let destination = self
                .udp_manager
                .flows
                .get(&flow.guest_addr)
                .map(|(_, real_dest)| *real_dest)
                .unwrap_or(flow.original_dest);

            responses.push(SlirpResponse::EstablishConnection(
                conn_id,
                ConnectionArgs::Udp(UdpConnectionArgs {
                    destination,
                    guest_ip: flow.guest_addr.ip(),
                    guest_port: flow.guest_addr.port(),
                }),
            ));
        }

        Ok(responses)
    }
}

#[derive(Serialize)]
struct SlirpStateRef<'a> {
    arp_table: &'a ArpTable,
    ndp_table: &'a NdpTable,
    dhcp_manager: &'a DhcpManager,
    tcp_manager: &'a TcpManager,
    udp_manager: &'a UdpManager,
}

#[derive(Deserialize)]
struct SlirpStateOwned {
    arp_table: ArpTable,
    ndp_table: NdpTable,
    dhcp_manager: DhcpManager,
    tcp_manager: TcpManager,
    udp_manager: UdpManager,
}

fn synthesize_offending_tcp_packet(guest_addr: SocketAddr, host_addr: SocketAddr) -> Vec<u8> {
    match (guest_addr, host_addr) {
        (SocketAddr::V4(guest_v4), SocketAddr::V4(host_v4)) => [
            // Version=4, IHL=5, DSCP/ECN=0, Total Length=28, ID=0, Flags/Frag=0, TTL=0, Proto=TCP(6), Checksum=0
            &[0x45, 0, 0, 28, 0, 0, 0, 0, 0, 6, 0, 0][..],
            &guest_v4.ip().octets(),
            &host_v4.ip().octets(),
            &guest_v4.port().to_be_bytes(),
            &host_v4.port().to_be_bytes(),
            &[0, 0, 0, 0], // Sequence Number = 0
        ]
        .concat(),
        (SocketAddr::V6(guest_v6), SocketAddr::V6(host_v6)) => [
            // Version=6, Payload Length=8, Next Header=TCP (6), Hop Limit=64
            &[0x60, 0x00, 0x00, 0x00, 0x00, 0x08, 6, 64][..],
            &guest_v6.ip().octets(),
            &host_v6.ip().octets(),
            &guest_v6.port().to_be_bytes(),
            &host_v6.port().to_be_bytes(),
            &[0, 0, 0, 0], // Sequence Number = 0
        ]
        .concat(),
        _ => Vec::new(), // Mismatched families (should not happen)
    }
}
