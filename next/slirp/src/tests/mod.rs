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

use bytes::Bytes;
use netsim_packets::{EthernetFrame, UdpBuilder};
use zerocopy::FromBytes;

use crate::{Config, Slirp, SlirpRequest, SlirpResponse};

pub struct TestSlirp {
    pub slirp: Slirp,
}

impl Default for TestSlirp {
    fn default() -> Self {
        Self::new()
    }
}

impl TestSlirp {
    pub fn new() -> Self {
        let config = Config::default();
        Self { slirp: Slirp::new(config) }
    }

    pub fn send_udp_packet_to_guest(
        &mut self,
        payload: &[u8],
        src_port: u16,
        dst_port: u16,
    ) -> Vec<SlirpResponse> {
        let mut udp_packet = vec![0u8; 8 + payload.len()];
        let mut udp_builder = UdpBuilder::new(
            &mut udp_packet,
            self.slirp.config.host_ipv4,
            self.slirp.config.guest_ipv4,
            src_port,
            dst_port,
        )
        .unwrap();
        udp_builder.payload(payload).unwrap();
        udp_builder.build().unwrap();

        let mut eth_packet = vec![0u8; 14 + 20 + udp_packet.len()];
        let (eth_header, eth_payload) = eth_packet.split_at_mut(14);
        let eth_frame = EthernetFrame::mut_from_bytes(eth_header).unwrap();
        eth_frame.dst_addr = self.slirp.config.guest_mac;
        eth_frame.src_addr =
            netsim_packets::MacAddr { bytes: [0x52, 0x54, 0x00, 0x12, 0x34, 0x56] };
        eth_frame.ethertype = 0x0800.into();

        let (ipv4_header, ipv4_payload) = eth_payload.split_at_mut(20);
        let mut ipv4_builder = netsim_packets::Ipv4Builder::new(
            ipv4_header,
            netsim_packets::IP_P_UDP,
            self.slirp.config.host_ipv4,
            self.slirp.config.guest_ipv4,
        )
        .unwrap();
        ipv4_builder.payload_len(udp_packet.len());
        ipv4_builder.build();
        ipv4_payload[..udp_packet.len()].copy_from_slice(&udp_packet);

        self.slirp.handle_request(SlirpRequest::Packet(Bytes::copy_from_slice(&eth_packet)))
    }

    pub fn send_ipv4_packet_to_guest(&mut self, packet: &[u8]) -> Vec<SlirpResponse> {
        let mut eth_packet = vec![0u8; 14 + packet.len()];
        let (eth_header, eth_payload) = eth_packet.split_at_mut(14);
        let eth_frame = EthernetFrame::mut_from_bytes(eth_header).unwrap();
        eth_frame.dst_addr = self.slirp.config.guest_mac;
        eth_frame.src_addr =
            netsim_packets::MacAddr { bytes: [0x52, 0x54, 0x00, 0x12, 0x34, 0x56] };
        eth_frame.ethertype = 0x0800.into();
        eth_payload.copy_from_slice(packet);

        self.slirp.handle_request(SlirpRequest::Packet(Bytes::copy_from_slice(&eth_packet)))
    }
}

#[cfg(test)]
mod tftp_tests;
