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

use std::net::Ipv4Addr;

use netsim_packets::{
    EthernetFrame, IP_P_TCP, IP_P_UDP, Ipv4Builder, MacAddr, PacketBuilder, TCP_FLAG_ACK,
    TCP_FLAG_FIN, TCP_FLAG_RST, TCP_FLAG_SYN, TcpBuilder, ether_type,
};
use zerocopy::FromBytes;

const DEFAULT_DST_MAC: [u8; 6] = [0x52, 0x54, 0x00, 0x12, 0x34, 0x56];
const DEFAULT_SRC_MAC: [u8; 6] = [0x02, 0x15, 0xb2, 0x00, 0x00, 0x00];

#[allow(clippy::too_many_arguments)]
pub fn create_tcp_packet(
    src_ip: Ipv4Addr,
    dst_ip: Ipv4Addr,
    src_port: u16,
    dst_port: u16,
    seq: u32,
    ack: u32,
    syn: bool,
    ack_flag: bool,
    fin: bool,
    rst: bool,
    window_size: u16,
    payload: &[u8],
) -> bytes::Bytes {
    let mut flags = 0u16;
    if syn {
        flags |= TCP_FLAG_SYN;
    }
    if ack_flag {
        flags |= TCP_FLAG_ACK;
    }
    if fin {
        flags |= TCP_FLAG_FIN;
    }
    if rst {
        flags |= TCP_FLAG_RST;
    }

    bytes::Bytes::from(
        PacketBuilder::new(DEFAULT_DST_MAC, DEFAULT_SRC_MAC, ether_type::IPV4)
            .ipv4(src_ip.octets(), dst_ip.octets(), IP_P_TCP)
            .tcp(src_port, dst_port, seq, ack, flags, window_size)
            .payload(payload),
    )
}

#[allow(clippy::too_many_arguments)]
pub fn create_tcp_packet_with_options(
    src_ip: Ipv4Addr,
    dst_ip: Ipv4Addr,
    src_port: u16,
    dst_port: u16,
    seq: u32,
    ack: u32,
    syn: bool,
    ack_flag: bool,
    fin: bool,
    rst: bool,
    window_size: u16,
    options: &[u8],
    payload: &[u8],
) -> bytes::Bytes {
    if options.is_empty() {
        return create_tcp_packet(
            src_ip,
            dst_ip,
            src_port,
            dst_port,
            seq,
            ack,
            syn,
            ack_flag,
            fin,
            rst,
            window_size,
            payload,
        );
    }

    let mut flags = 0u16;
    if syn {
        flags |= TCP_FLAG_SYN;
    }
    if ack_flag {
        flags |= TCP_FLAG_ACK;
    }
    if fin {
        flags |= TCP_FLAG_FIN;
    }
    if rst {
        flags |= TCP_FLAG_RST;
    }

    // The TCP data offset counts 32-bit words, so the options field has to be
    // padded to a 4-byte boundary.
    let padded_options_len = options.len().div_ceil(4) * 4;
    let header_len = 20 + padded_options_len;
    let data_offset = (header_len / 4) as u8;
    let mut tcp_data = vec![0u8; header_len + payload.len()];
    let mut tcp_builder = TcpBuilder::new(&mut tcp_data, src_ip, dst_ip).unwrap();
    tcp_builder
        .source_port(src_port)
        .dest_port(dst_port)
        .sequence_num(seq)
        .ack_num(ack)
        .flags(flags)
        .window_size(window_size);

    if data_offset > 5 {
        tcp_builder.data_offset(data_offset);
        if let Some(opts) = tcp_builder.options_mut() {
            opts[..options.len()].copy_from_slice(options);
        }
    }
    tcp_builder.payload(payload).unwrap();
    tcp_builder.build().unwrap();

    let mut ip_data = vec![0u8; 20 + tcp_data.len()];
    let mut ipv4_builder = Ipv4Builder::new(&mut ip_data, IP_P_TCP, src_ip, dst_ip).unwrap();
    ipv4_builder.ttl(64);
    ipv4_builder.payload(&tcp_data).unwrap();
    ipv4_builder.build().unwrap();

    let mut eth_data = vec![0u8; 14 + ip_data.len()];
    let (eth_header_slice, eth_payload_slice) = eth_data.split_at_mut(14);
    let eth_frame = EthernetFrame::mut_from_bytes(eth_header_slice).unwrap();
    eth_frame.dst_addr = MacAddr { bytes: DEFAULT_DST_MAC };
    eth_frame.src_addr = MacAddr { bytes: DEFAULT_SRC_MAC };
    eth_frame.ethertype = ether_type::IPV4.into();
    eth_payload_slice.copy_from_slice(&ip_data);

    bytes::Bytes::from(eth_data)
}

pub fn create_udp_packet(
    src_ip: Ipv4Addr,
    dst_ip: Ipv4Addr,
    src_port: u16,
    dst_port: u16,
    payload: &[u8],
) -> bytes::Bytes {
    create_udp_packet_with_mac(DEFAULT_SRC_MAC, src_ip, dst_ip, src_port, dst_port, payload)
}

pub fn create_udp_packet_with_mac(
    src_mac: [u8; 6],
    src_ip: Ipv4Addr,
    dst_ip: Ipv4Addr,
    src_port: u16,
    dst_port: u16,
    payload: &[u8],
) -> bytes::Bytes {
    bytes::Bytes::from(
        PacketBuilder::new(DEFAULT_DST_MAC, src_mac, ether_type::IPV4)
            .ipv4(src_ip.octets(), dst_ip.octets(), IP_P_UDP)
            .udp(src_port, dst_port)
            .payload(payload),
    )
}

#[cfg(test)]
mod tests {
    use netsim_packets::{EthernetPacket, IpPacket, TransportPacket, parse};

    use super::*;

    #[test]
    fn test_create_tcp_packet() {
        let src_ip = Ipv4Addr::new(192, 168, 0, 1);
        let dst_ip = Ipv4Addr::new(192, 168, 0, 2);
        let src_port = 12345;
        let dst_port = 80;
        let seq = 100;
        let ack = 0;
        let syn = true;
        let ack_flag = false;
        let fin = false;
        let rst = false;
        let window_size = 8192;
        let payload = b"hello";

        let packet_bytes = create_tcp_packet(
            src_ip,
            dst_ip,
            src_port,
            dst_port,
            seq,
            ack,
            syn,
            ack_flag,
            fin,
            rst,
            window_size,
            payload,
        );

        // Verify we can parse it back
        let parsed = parse(&packet_bytes).unwrap();

        // Verify Ethernet layer
        if let EthernetPacket::Untagged { frame, .. } = parsed.ethernet {
            assert_eq!(frame.ethertype.get(), ether_type::IPV4);
        } else {
            panic!("Expected untagged ethernet frame");
        }

        // Verify IP layer
        let ip_packet = parsed.ip.unwrap();
        if let IpPacket::V4(ip_header, _ip_payload) = ip_packet {
            assert_eq!(ip_header.source_addr, src_ip.octets());
            assert_eq!(ip_header.dest_addr, dst_ip.octets());
            assert_eq!(ip_header.protocol, IP_P_TCP);
        } else {
            panic!("Expected IPv4 packet");
        }

        // Verify Transport layer
        let transport_packet = parsed.transport.unwrap();
        if let TransportPacket::Tcp(tcp_header, tcp_payload) = transport_packet {
            assert_eq!(tcp_header.source_port.get(), src_port);
            assert_eq!(tcp_header.dest_port.get(), dst_port);
            assert_eq!(tcp_header.sequence_num.get(), seq);
            assert_eq!(tcp_header.ack_num.get(), ack);
            assert_eq!(tcp_header.syn(), syn);
            assert_eq!(tcp_header.ack(), ack_flag);
            assert_eq!(tcp_header.fin(), fin);
            assert_eq!(tcp_header.rst(), rst);
            assert_eq!(tcp_header.window_size.get(), window_size);
            assert_eq!(tcp_payload, payload);
        } else {
            panic!("Expected TCP packet");
        }
    }

    #[test]
    fn test_create_udp_packet() {
        let src_ip = Ipv4Addr::new(10, 0, 2, 15);
        let dst_ip = Ipv4Addr::new(8, 8, 8, 8);
        let src_port = 54321;
        let dst_port = 53;
        let payload = b"dns_query";

        let packet_bytes = create_udp_packet(src_ip, dst_ip, src_port, dst_port, payload);
        let parsed = parse(&packet_bytes).unwrap();

        if let EthernetPacket::Untagged { frame, .. } = parsed.ethernet {
            assert_eq!(frame.ethertype.get(), ether_type::IPV4);
        } else {
            panic!("Expected untagged ethernet frame");
        }

        if let Some(IpPacket::V4(ip_header, _)) = parsed.ip {
            assert_eq!(ip_header.source_addr, src_ip.octets());
            assert_eq!(ip_header.dest_addr, dst_ip.octets());
            assert_eq!(ip_header.protocol, IP_P_UDP);
        } else {
            panic!("Expected IPv4 packet");
        }

        if let Some(TransportPacket::Udp(udp_header, udp_payload)) = parsed.transport {
            assert_eq!(udp_header.source_port.get(), src_port);
            assert_eq!(udp_header.dest_port.get(), dst_port);
            assert_eq!(udp_payload, payload);
        } else {
            panic!("Expected UDP packet");
        }
    }
}
