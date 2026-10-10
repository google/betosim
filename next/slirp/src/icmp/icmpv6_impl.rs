// Copyright 2026 The Android Open Source Project
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

//! ICMPv6 protocol implementation.

use std::{
    collections::HashMap,
    net::{IpAddr, Ipv6Addr},
};

use bytes::Bytes;
use netsim_packets::{
    EthernetFrame, Icmpv6Header, Icmpv6Type, Icmpv6UnreachableCode, Ipv6Builder, Ipv6Header,
    MacAddr,
};
use zerocopy::FromBytes;

use crate::{
    Config, ConnectionArgs, IcmpConnectionArgs, SlirpResponse,
    packet::{IpPacket, NetworkPacket, ParsedPacket, TransportPacket},
};

pub struct Icmpv6Manager {
    // Maps (guest_ip, target_ip, guest_id) -> (conn_id, real_dest_addr)
    pub(crate) flows: HashMap<(Ipv6Addr, Ipv6Addr, u16), (u64, Ipv6Addr)>,
    // Maps conn_id -> (guest_ip, target_ip, guest_id)
    pub(crate) id_to_flow: HashMap<u64, (Ipv6Addr, Ipv6Addr, u16)>,
    pub(crate) next_flow_id: u64,
}

impl Default for Icmpv6Manager {
    fn default() -> Self {
        Self::new()
    }
}

impl Icmpv6Manager {
    pub fn new() -> Self {
        Self {
            flows: HashMap::new(),
            id_to_flow: HashMap::new(),
            next_flow_id: (1 << 61) + (1 << 60), // Start IPv6 ICMP flow IDs after IPv4 ICMP
        }
    }

    /// Handles an incoming ICMPv6 packet.
    pub fn handle_packet(
        &mut self,
        responses: &mut Vec<SlirpResponse>,
        config: &Config,
        packet: &ParsedPacket,
        raw_icmpv6_packet: &[u8],
    ) {
        let Some(NetworkPacket::Ip(IpPacket::V6(ipv6_header, _))) = &packet.network else {
            return;
        };
        let Some(TransportPacket::Icmpv6(icmpv6_header, icmpv6_payload)) = &packet.transport else {
            return;
        };

        if icmpv6_header.icmpv6_type == Icmpv6Type::EchoRequest as u8 {
            let dest_ip = Ipv6Addr::from(ipv6_header.dest_addr);
            let guest_ip = Ipv6Addr::from(ipv6_header.source_addr);

            if dest_ip == config.host_ipv6 {
                let guest_mac = match packet.ethernet {
                    netsim_packets::EthernetPacket::Untagged { frame, .. } => frame.src_addr,
                    netsim_packets::EthernetPacket::Vlan { frame, .. } => frame.src_addr,
                };
                self.send_echo_reply(
                    responses,
                    config,
                    guest_mac,
                    ipv6_header,
                    icmpv6_header,
                    icmpv6_payload,
                );
            } else {
                // External IPv6 ping! Forward to host driver via Icmp
                // connection flow.
                if let Some((guest_id, _seq)) = icmpv6_header.echo_fields() {
                    let flow_key = (guest_ip, dest_ip, guest_id);
                    let (conn_id, _) = self.flows.entry(flow_key).or_insert_with(|| {
                        let id = self.next_flow_id;
                        self.next_flow_id += 1;
                        let conn_info = IcmpConnectionArgs {
                            destination: IpAddr::V6(dest_ip),
                            guest_ip: IpAddr::V6(guest_ip),
                            guest_id,
                        };
                        responses.push(SlirpResponse::EstablishConnection(
                            id,
                            ConnectionArgs::Icmp(conn_info),
                        ));

                        self.id_to_flow.insert(id, flow_key);
                        (id, dest_ip)
                    });

                    // Forward the entire raw ICMPv6 packet (raw_icmpv6_packet)
                    responses.push(SlirpResponse::WriteToConnection(
                        *conn_id,
                        Bytes::copy_from_slice(raw_icmpv6_packet),
                    ));
                }
            }
        }
    }

    /// Handles a reply from the host for an ICMPv6 flow.
    pub fn handle_reply(
        &self,
        responses: &mut Vec<SlirpResponse>,
        config: &Config,
        conn_id: u64,
        icmp_reply: &[u8],
    ) {
        let Some(&(guest_ip, target_ip, _guest_id)) = self.id_to_flow.get(&conn_id) else {
            log::warn!("Received ICMPv6 reply for unknown flow {conn_id}");
            return;
        };

        // Reconstruct the IPv6 packet containing the ICMPv6 reply
        let icmp_len = icmp_reply.len();
        let ipv6_header_len = 40;
        let total_ipv6_len = ipv6_header_len + icmp_len;
        let eth_header_len = 14;
        let total_len = eth_header_len + total_ipv6_len;

        let mut buffer = vec![0u8; total_len];

        // 1. Fill Ethernet Header
        let (eth_slice, eth_payload) = buffer.split_at_mut(eth_header_len);
        let eth_frame = EthernetFrame::mut_from_bytes(eth_slice).unwrap();
        eth_frame.dst_addr = config.guest_mac;
        eth_frame.src_addr = config.gateway_mac;
        eth_frame.ethertype = 0x86DD.into(); // IPv6

        // 2. Build IPv6 Header
        let (ipv6_slice, ipv6_payload) = eth_payload.split_at_mut(ipv6_header_len);
        let mut ipv6_builder =
            Ipv6Builder::new(ipv6_slice, netsim_packets::IP_P_ICMPV6, target_ip, guest_ip).unwrap();
        ipv6_builder.hop_limit(64);

        // 3. Copy ICMPv6 reply
        let Some(dest) = ipv6_payload.get_mut(..icmp_len) else {
            return;
        };
        dest.copy_from_slice(icmp_reply);

        // 4. Finalize IPv6 Header
        ipv6_builder.payload_len(icmp_len);
        ipv6_builder.build();

        responses.push(SlirpResponse::Packet(Bytes::copy_from_slice(&buffer)));
    }

    fn send_echo_reply(
        &mut self,
        responses: &mut Vec<SlirpResponse>,
        config: &Config,
        guest_mac: MacAddr,
        original_ipv6_header: &Ipv6Header,
        echo_request: &Icmpv6Header,
        echo_payload: &[u8],
    ) {
        let eth_header_len = 14;
        let ipv6_header_len = 40;
        let icmpv6_header_len = 8;
        let echo_payload_len = echo_payload.len();
        let total_icmpv6_len = icmpv6_header_len + echo_payload_len;
        let total_ipv6_len = ipv6_header_len + total_icmpv6_len;
        let total_len = eth_header_len + total_ipv6_len;

        let mut buffer = vec![0u8; total_len];

        let dest_ip = Ipv6Addr::from(original_ipv6_header.source_addr);
        let src_ip = Ipv6Addr::from(original_ipv6_header.dest_addr);

        // 1. Fill Ethernet Header
        let (eth_slice, eth_payload) = buffer.split_at_mut(eth_header_len);
        let eth_frame = EthernetFrame::mut_from_bytes(eth_slice).unwrap();
        eth_frame.dst_addr = guest_mac;
        eth_frame.src_addr = config.gateway_mac;
        eth_frame.ethertype = 0x86DD.into(); // IPv6

        // 2. Build IPv6 Header
        let (ipv6_slice, ipv6_payload) = eth_payload.split_at_mut(ipv6_header_len);
        let mut ipv6_builder =
            Ipv6Builder::new(ipv6_slice, netsim_packets::IP_P_ICMPV6, src_ip, dest_ip).unwrap();
        ipv6_builder.hop_limit(64);

        // 3. Build ICMPv6 Header
        let (icmpv6_slice, icmpv6_payload_slice) = ipv6_payload.split_at_mut(icmpv6_header_len);
        let icmpv6_header = Icmpv6Header::mut_from_bytes(icmpv6_slice).unwrap();
        icmpv6_header.icmpv6_type = Icmpv6Type::EchoReply as u8;
        icmpv6_header.icmpv6_code = 0;
        icmpv6_header.icmpv6_checksum = 0.into();
        icmpv6_header.rest = echo_request.rest; // Copy ID and Seq

        // Copy payload
        let Some(dest) = icmpv6_payload_slice.get_mut(..echo_payload_len) else {
            return;
        };
        dest.copy_from_slice(echo_payload);

        // 4. Calculate ICMPv6 Checksum
        let Some(full_icmpv6_slice) = ipv6_payload.get_mut(..total_icmpv6_len) else {
            return;
        };
        let checksum = netsim_packets::icmpv6_checksum(full_icmpv6_slice, src_ip, dest_ip);
        let Some(header_slice) = full_icmpv6_slice.get_mut(..icmpv6_header_len) else {
            return;
        };
        let mut_icmpv6_header = Icmpv6Header::mut_from_bytes(header_slice).unwrap();
        mut_icmpv6_header.icmpv6_checksum.set(checksum);

        // 5. Finalize IPv6 Header
        ipv6_builder.payload_len(total_icmpv6_len);
        ipv6_builder.build();

        responses.push(SlirpResponse::Packet(Bytes::copy_from_slice(&buffer)));
    }

    pub fn send_port_unreachable(
        &mut self,
        responses: &mut Vec<SlirpResponse>,
        config: &Config,
        offending_packet: &[u8],
    ) {
        self.send_error_packet(
            responses,
            config,
            offending_packet,
            Icmpv6Type::DestinationUnreachable,
            Icmpv6UnreachableCode::PortUnreachable as u8,
            0, // rest is unused
        );
    }

    pub fn send_host_unreachable(
        &mut self,
        responses: &mut Vec<SlirpResponse>,
        config: &Config,
        offending_packet: &[u8],
    ) {
        self.send_error_packet(
            responses,
            config,
            offending_packet,
            Icmpv6Type::DestinationUnreachable,
            Icmpv6UnreachableCode::AddressUnreachable as u8,
            0, // rest is unused
        );
    }

    pub fn send_packet_too_big(
        &mut self,
        responses: &mut Vec<SlirpResponse>,
        config: &Config,
        offending_packet: &[u8],
        mtu: u32,
    ) {
        self.send_error_packet(
            responses,
            config,
            offending_packet,
            Icmpv6Type::PacketTooBig,
            0,   // code is 0
            mtu, // rest contains MTU
        );
    }

    fn send_error_packet(
        &mut self,
        responses: &mut Vec<SlirpResponse>,
        config: &Config,
        offending_packet: &[u8],
        error_type: Icmpv6Type,
        code: u8,
        rest_val: u32,
    ) {
        // Minimum IPv6 MTU is 1280. We should not exceed it for ICMPv6 errors.
        let max_total_len = 1280;
        let eth_header_len = 14;
        let ipv6_header_len = 40;
        let icmpv6_header_len = 8;

        let max_payload_len = max_total_len - eth_header_len - ipv6_header_len - icmpv6_header_len;
        let offending_slice = offending_packet.get(..max_payload_len).unwrap_or(offending_packet);
        let payload_len = offending_slice.len();

        let total_icmpv6_len = icmpv6_header_len + payload_len;
        let total_ipv6_len = ipv6_header_len + total_icmpv6_len;
        let total_len = eth_header_len + total_ipv6_len;

        let mut buffer = vec![0u8; total_len];

        // Parse offending IPv6 header to get source/destination
        let (original_ipv6_header, _) = Ipv6Header::parse(offending_packet).unwrap();
        let dest_ip = Ipv6Addr::from(original_ipv6_header.source_addr);
        // ICMPv6 error source should be the gateway's IPv6
        let src_ip = config.host_ipv6;

        // 1. Fill Ethernet Header
        let (eth_slice, eth_payload) = buffer.split_at_mut(eth_header_len);
        let eth_frame = EthernetFrame::mut_from_bytes(eth_slice).unwrap();
        eth_frame.dst_addr = config.guest_mac;
        // Use a generic QEMU/Slirp gateway MAC for error messages to match
        // ICMPv4
        eth_frame.src_addr = MacAddr { bytes: [0x52, 0x54, 0x00, 0x12, 0x34, 0x56] };
        eth_frame.ethertype = 0x86DD.into(); // IPv6

        // 2. Build IPv6 Header
        let (ipv6_slice, ipv6_payload) = eth_payload.split_at_mut(ipv6_header_len);
        let mut ipv6_builder =
            Ipv6Builder::new(ipv6_slice, netsim_packets::IP_P_ICMPV6, src_ip, dest_ip).unwrap();
        ipv6_builder.hop_limit(64);

        // 3. Build ICMPv6 Header
        let (icmpv6_slice, icmpv6_payload_slice) = ipv6_payload.split_at_mut(icmpv6_header_len);
        let icmpv6_header = Icmpv6Header::mut_from_bytes(icmpv6_slice).unwrap();
        icmpv6_header.icmpv6_type = error_type as u8;
        icmpv6_header.icmpv6_code = code;
        icmpv6_header.icmpv6_checksum = 0.into();
        icmpv6_header.rest = rest_val.to_be_bytes();

        // Copy offending packet as payload
        icmpv6_payload_slice.copy_from_slice(offending_slice);

        // 4. Calculate ICMPv6 Checksum
        let checksum = netsim_packets::icmpv6_checksum(ipv6_payload, src_ip, dest_ip);
        let (header_slice, _) = ipv6_payload.split_at_mut(icmpv6_header_len);
        let mut_icmpv6_header = Icmpv6Header::mut_from_bytes(header_slice).unwrap();
        mut_icmpv6_header.icmpv6_checksum.set(checksum);

        // 5. Finalize IPv6 Header
        ipv6_builder.payload_len(total_icmpv6_len);
        ipv6_builder.build();

        responses.push(SlirpResponse::Packet(Bytes::from(buffer)));
    }
}
