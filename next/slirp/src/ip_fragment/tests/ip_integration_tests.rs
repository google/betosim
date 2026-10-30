// Copyright 2023-2026 The Android Open Source Project
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
    collections::HashMap,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    sync::{Arc, Mutex},
    time::Duration,
};

use bytes::Bytes;
use netsim_packets::{
    EthernetFrame, IP_P_ICMP, IP_P_ICMPV6, IP_P_TCP, IP_P_UDP, IcmpHeader, Icmpv6Header,
    Ipv4Builder, Ipv4Header, Ipv6Builder, Ipv6Header, MacAddr, NeighborAdvertisement,
    NeighborSolicitation, NeighborSolicitationBuilder, TcpBuilder, TcpHeader, UdpHeader,
    UdpPacketBuilder, ether_type,
};
use zerocopy::{FromBytes, IntoBytes};

use crate::{
    Config, ConnectionArgs, Slirp, SlirpRequest, SlirpResponse,
    clock::MockClock,
    dhcp::{DHCP_MAGIC_COOKIE, DhcpMessageType, DhcpPacket},
    ip_fragment::{Reassembler, fragment},
};

#[test]
fn test_fragmented_udp_packet_integration() {
    let config = Config::default();
    let mut slirp = Slirp::new(config.clone());

    // 1. Create a large UDP payload (e.g., 2000 bytes) to force fragmentation
    let mut udp_payload = vec![0u8; 2000];
    for (i, item) in udp_payload.iter_mut().enumerate() {
        *item = (i % 256) as u8;
    }

    let src_port = 12345;
    let dst_port = 53;
    let dst_ip = Ipv4Addr::new(8, 8, 8, 8);

    // 2. Build the UDP packet
    let mut udp_packet = vec![0u8; 8 + udp_payload.len()];
    let mut udp_builder = UdpPacketBuilder::new(&mut udp_packet, src_port, dst_port).unwrap();
    udp_builder.payload_mut().copy_from_slice(&udp_payload);
    udp_builder.build();

    // 3. Build the IPv4 packet (from guest to external IP)
    let mut ipv4_packet = vec![0u8; 20 + udp_packet.len()];
    let (ipv4_header_slice, ipv4_payload_slice) = ipv4_packet.split_at_mut(20);
    let mut ipv4_builder =
        Ipv4Builder::new(ipv4_header_slice, IP_P_UDP, config.guest_ipv4, dst_ip).unwrap();
    ipv4_builder.payload_len(udp_packet.len());
    ipv4_builder.build();
    ipv4_payload_slice.copy_from_slice(&udp_packet);

    // 4. Fragment the IP packet
    let fragments = fragment(&ipv4_packet);
    assert!(fragments.len() > 1, "Packet should be fragmented");

    // 5. Send all but the last fragment, wrapped in Ethernet frames.
    for fragment in fragments.iter().take(fragments.len() - 1) {
        let mut eth_packet = vec![0u8; 14 + fragment.len()];
        let (eth_header, eth_payload) = eth_packet.split_at_mut(14);
        let eth_frame = EthernetFrame::mut_from_bytes(eth_header).unwrap();
        eth_frame.dst_addr = config.gateway_mac;
        eth_frame.src_addr = config.guest_mac;
        eth_frame.ethertype = 0x0800.into();
        eth_payload.copy_from_slice(fragment);

        let responses =
            slirp.handle_request(SlirpRequest::Packet(Bytes::copy_from_slice(&eth_packet)));
        assert!(responses.is_empty(), "Intermediate fragments should not trigger responses");
    }

    // 6. Send the last fragment.
    let last_fragment = fragments.last().unwrap();
    let mut eth_packet = vec![0u8; 14 + last_fragment.len()];
    let (eth_header, eth_payload) = eth_packet.split_at_mut(14);
    let eth_frame = EthernetFrame::mut_from_bytes(eth_header).unwrap();
    eth_frame.dst_addr = config.gateway_mac;
    eth_frame.src_addr = config.guest_mac;
    eth_frame.ethertype = 0x0800.into();
    eth_payload.copy_from_slice(last_fragment);

    let responses = slirp.handle_request(SlirpRequest::Packet(Bytes::copy_from_slice(&eth_packet)));

    assert_eq!(responses.len(), 2, "Last fragment should trigger connection and write");

    let has_establish = responses.iter().any(|r| match r {
        SlirpResponse::EstablishConnection(_, ConnectionArgs::Udp(args)) => {
            args.destination == (dst_ip, dst_port).into()
                && args.guest_ip == config.guest_ipv4
                && args.guest_port == src_port
        }
        _ => false,
    });
    assert!(has_establish, "Should trigger EstablishConnection for UDP");

    let has_write = responses.iter().any(|r| match r {
        SlirpResponse::WriteToConnection(_, data) => data[..] == udp_payload[..],
        _ => false,
    });
    assert!(has_write, "Should trigger WriteToConnection with full payload");
}

#[test]
fn test_fragmented_icmp_echo_integration() {
    let config = Config::default();
    let mut slirp = Slirp::new(config.clone());

    // 1. Create a large ICMP payload (e.g., 2000 bytes) to force fragmentation
    let mut icmp_payload = vec![0u8; 2000];
    for (i, item) in icmp_payload.iter_mut().enumerate() {
        *item = (i % 256) as u8;
    }

    // 2. Build the ICMP Echo Request
    let icmp_header_len = std::mem::size_of::<IcmpHeader>();
    let mut icmp_packet = vec![0u8; icmp_header_len + icmp_payload.len()];

    let (icmp_header_slice, echo_payload_slice) = icmp_packet.split_at_mut(icmp_header_len);
    let icmp_header = IcmpHeader::mut_from_bytes(icmp_header_slice).unwrap();
    icmp_header.icmp_type = 8; // Echo Request
    icmp_header.icmp_code = 0;
    icmp_header.icmp_checksum = 0.into();

    icmp_header.rest[..2].copy_from_slice(&0x1234u16.to_be_bytes());
    icmp_header.rest[2..].copy_from_slice(&0x5678u16.to_be_bytes());
    echo_payload_slice.copy_from_slice(&icmp_payload);

    // Calculate ICMP checksum
    let checksum = netsim_packets::ipv4_checksum(&icmp_packet);
    let icmp_header = IcmpHeader::mut_from_bytes(&mut icmp_packet[..icmp_header_len]).unwrap();
    icmp_header.icmp_checksum.set(checksum);

    // 3. Build the IPv4 packet (from guest to host/gateway IP)
    let mut ipv4_packet = vec![0u8; 20 + icmp_packet.len()];
    let (ipv4_header_slice, ipv4_payload_slice) = ipv4_packet.split_at_mut(20);
    let mut ipv4_builder =
        Ipv4Builder::new(ipv4_header_slice, IP_P_ICMP, config.guest_ipv4, config.host_ipv4)
            .unwrap();
    ipv4_builder.payload_len(icmp_packet.len());
    ipv4_builder.build();
    ipv4_payload_slice.copy_from_slice(&icmp_packet);

    // 4. Fragment the IP packet
    let fragments = fragment(&ipv4_packet);
    assert!(fragments.len() > 1, "Packet should be fragmented");

    // 5. Send all but the last fragment.
    for fragment in fragments.iter().take(fragments.len() - 1) {
        let mut eth_packet = vec![0u8; 14 + fragment.len()];
        let (eth_header, eth_payload) = eth_packet.split_at_mut(14);
        let eth_frame = EthernetFrame::mut_from_bytes(eth_header).unwrap();
        eth_frame.dst_addr = config.gateway_mac;
        eth_frame.src_addr = config.guest_mac;
        eth_frame.ethertype = 0x0800.into();
        eth_payload.copy_from_slice(fragment);

        let responses =
            slirp.handle_request(SlirpRequest::Packet(Bytes::copy_from_slice(&eth_packet)));
        assert!(responses.is_empty(), "Intermediate fragments should not trigger responses");
    }

    // 6. Send the last fragment.
    let last_fragment = fragments.last().unwrap();
    let mut eth_packet = vec![0u8; 14 + last_fragment.len()];
    let (eth_header, eth_payload) = eth_packet.split_at_mut(14);
    let eth_frame = EthernetFrame::mut_from_bytes(eth_header).unwrap();
    eth_frame.dst_addr = config.gateway_mac;
    eth_frame.src_addr = config.guest_mac;
    eth_frame.ethertype = 0x0800.into();
    eth_payload.copy_from_slice(last_fragment);

    let responses = slirp.handle_request(SlirpRequest::Packet(Bytes::copy_from_slice(&eth_packet)));

    assert!(responses.len() > 1, "Should return multiple packets (fragments)");

    // Reassemble the responses to verify the Echo Reply
    let mut reply_reassembler = Reassembler::new();
    let mut reassembled_reply_ip = None;

    for response in responses {
        let packet_bytes = match response {
            SlirpResponse::Packet(p) => p,
            _ => panic!("Expected Packet response"),
        };

        let (eth_frame, eth_payload) = EthernetFrame::parse(&packet_bytes).unwrap();
        assert_eq!(eth_frame.dst_addr, config.guest_mac, "Reply should be sent to guest");
        assert_eq!(eth_frame.src_addr, config.gateway_mac, "Reply should be sent from gateway");

        if let Some(reassembled) = reply_reassembler.reassemble(eth_payload) {
            reassembled_reply_ip = Some(reassembled);
        }
    }

    let reassembled_reply_ip =
        reassembled_reply_ip.expect("Should have successfully reassembled the reply");

    // Verify the reassembled IP packet
    let (ip_header, ip_payload) = Ipv4Header::parse(&reassembled_reply_ip).unwrap();
    assert_eq!(ip_header.protocol, IP_P_ICMP);
    assert_eq!(Ipv4Addr::from(ip_header.source_addr), config.host_ipv4);
    assert_eq!(Ipv4Addr::from(ip_header.dest_addr), config.guest_ipv4);

    // Parse ICMP manually
    let (icmp_header, icmp_payload_slice) = IcmpHeader::parse(ip_payload).unwrap();
    assert_eq!(icmp_header.icmp_type, 0, "Should be Echo Reply");
    assert_eq!(icmp_header.icmp_code, 0);

    let identifier = u16::from_be_bytes([icmp_header.rest[0], icmp_header.rest[1]]);
    let sequence_number = u16::from_be_bytes([icmp_header.rest[2], icmp_header.rest[3]]);
    assert_eq!(identifier, 0x1234);
    assert_eq!(sequence_number, 0x5678);
    assert_eq!(icmp_payload_slice, icmp_payload, "Payload should match original");
}

#[test]
fn test_icmp_port_unreachable_integration() {
    let config = Config::default();
    let mut slirp = Slirp::new(config.clone());

    // Send a UDP packet from guest to an unserved gateway port
    // (port 0 is reserved and never translated).
    let src_port = 12345;
    let dst_port = 0;
    let udp_payload = b"hello";

    let mut udp_packet = vec![0u8; 8 + udp_payload.len()];
    let mut udp_builder = UdpPacketBuilder::new(&mut udp_packet, src_port, dst_port).unwrap();
    udp_builder.payload_mut().copy_from_slice(udp_payload);
    udp_builder.build();

    let mut ipv4_packet = vec![0u8; 20 + udp_packet.len()];
    let (ipv4_header_slice, ipv4_payload_slice) = ipv4_packet.split_at_mut(20);
    let mut ipv4_builder =
        Ipv4Builder::new(ipv4_header_slice, IP_P_UDP, config.guest_ipv4, config.host_ipv4).unwrap();
    ipv4_builder.payload_len(udp_packet.len());
    ipv4_builder.build();
    ipv4_payload_slice.copy_from_slice(&udp_packet);

    let mut eth_packet = vec![0u8; 14 + ipv4_packet.len()];
    let (eth_header, eth_payload) = eth_packet.split_at_mut(14);
    let eth_frame = EthernetFrame::mut_from_bytes(eth_header).unwrap();
    eth_frame.dst_addr = config.gateway_mac;
    eth_frame.src_addr = config.guest_mac;
    eth_frame.ethertype = 0x0800.into();
    eth_payload.copy_from_slice(&ipv4_packet);

    let responses = slirp.handle_request(SlirpRequest::Packet(Bytes::copy_from_slice(&eth_packet)));

    assert_eq!(responses.len(), 1, "Should return exactly one response");

    let response_packet = match &responses[0] {
        SlirpResponse::Packet(p) => p,
        _ => panic!("Expected Packet response"),
    };

    let (eth_frame, eth_payload) = EthernetFrame::parse(response_packet).unwrap();
    assert_eq!(eth_frame.dst_addr, config.guest_mac);
    assert_eq!(eth_frame.src_addr.bytes, [0x52, 0x54, 0x00, 0x12, 0x34, 0x56]);

    let (ip_header, ip_payload) = Ipv4Header::parse(eth_payload).unwrap();
    assert_eq!(ip_header.protocol, IP_P_ICMP);
    assert_eq!(Ipv4Addr::from(ip_header.source_addr), config.host_ipv4);
    assert_eq!(Ipv4Addr::from(ip_header.dest_addr), config.guest_ipv4);

    let (icmp_header, icmp_payload) = IcmpHeader::parse(ip_payload).unwrap();
    assert_eq!(icmp_header.icmp_type, 3); // Dest Unreachable
    assert_eq!(icmp_header.icmp_code, 3); // Port Unreachable

    // Verify payload contains the original IP header + UDP header
    assert!(icmp_payload.len() >= 28);
    let offending_packet_reply = icmp_payload;
    let (original_ip_header_reply, original_ip_payload_reply) =
        Ipv4Header::parse(offending_packet_reply).unwrap();
    assert_eq!(original_ip_header_reply.protocol, IP_P_UDP);
    assert_eq!(Ipv4Addr::from(original_ip_header_reply.source_addr), config.guest_ipv4);
    assert_eq!(Ipv4Addr::from(original_ip_header_reply.dest_addr), config.host_ipv4);

    let (original_udp_header, _) = UdpHeader::parse(original_ip_payload_reply).unwrap();
    assert_eq!(original_udp_header.source_port.get(), src_port);
    assert_eq!(original_udp_header.dest_port.get(), dst_port);
}

#[test]
fn test_icmp_time_exceeded_integration() {
    let config = Config::default();
    let mut slirp = Slirp::new(config.clone());

    // Send a UDP packet to an external IP with TTL = 1
    let src_port = 12345;
    let dst_port = 80;
    let dst_ip = Ipv4Addr::new(8, 8, 8, 8);
    let udp_payload = b"hello";

    let mut udp_packet = vec![0u8; 8 + udp_payload.len()];
    let mut udp_builder = UdpPacketBuilder::new(&mut udp_packet, src_port, dst_port).unwrap();
    udp_builder.payload_mut().copy_from_slice(udp_payload);
    udp_builder.build();

    let mut ipv4_packet = vec![0u8; 20 + udp_packet.len()];
    let (ipv4_header_slice, ipv4_payload_slice) = ipv4_packet.split_at_mut(20);
    let mut ipv4_builder =
        Ipv4Builder::new(ipv4_header_slice, IP_P_UDP, config.guest_ipv4, dst_ip).unwrap();
    ipv4_builder.ttl(1); // TTL = 1
    ipv4_builder.payload_len(udp_packet.len());
    ipv4_builder.build();
    ipv4_payload_slice.copy_from_slice(&udp_packet);

    let mut eth_packet = vec![0u8; 14 + ipv4_packet.len()];
    let (eth_header, eth_payload) = eth_packet.split_at_mut(14);
    let eth_frame = EthernetFrame::mut_from_bytes(eth_header).unwrap();
    eth_frame.dst_addr = config.gateway_mac;
    eth_frame.src_addr = config.guest_mac;
    eth_frame.ethertype = 0x0800.into();
    eth_payload.copy_from_slice(&ipv4_packet);

    let responses = slirp.handle_request(SlirpRequest::Packet(Bytes::copy_from_slice(&eth_packet)));

    assert_eq!(responses.len(), 1, "Should return exactly one response");

    let response_packet = match &responses[0] {
        SlirpResponse::Packet(p) => p,
        _ => panic!("Expected Packet response"),
    };

    let (eth_frame, eth_payload) = EthernetFrame::parse(response_packet).unwrap();
    assert_eq!(eth_frame.dst_addr, config.guest_mac);
    assert_eq!(eth_frame.src_addr.bytes, [0x52, 0x54, 0x00, 0x12, 0x34, 0x56]);

    let (ip_header, ip_payload) = Ipv4Header::parse(eth_payload).unwrap();
    assert_eq!(ip_header.protocol, IP_P_ICMP);
    assert_eq!(Ipv4Addr::from(ip_header.source_addr), config.host_ipv4);
    assert_eq!(Ipv4Addr::from(ip_header.dest_addr), config.guest_ipv4);

    let (icmp_header, icmp_payload) = IcmpHeader::parse(ip_payload).unwrap();
    assert_eq!(icmp_header.icmp_type, 11); // Time Exceeded
    assert_eq!(icmp_header.icmp_code, 0); // TTL expired in transit

    assert!(icmp_payload.len() >= 20);
    let offending_packet_reply = icmp_payload;
    let (original_ip_header_reply, _) = Ipv4Header::parse(offending_packet_reply).unwrap();
    assert_eq!(original_ip_header_reply.protocol, IP_P_UDP);
    assert_eq!(original_ip_header_reply.ttl, 1);
    assert_eq!(Ipv4Addr::from(original_ip_header_reply.dest_addr), dst_ip);
}

#[test]
fn test_ndp_neighbor_solicitation_integration() {
    let config = Config::default();
    let mut slirp = Slirp::new(config.clone());

    // 1. Build Neighbor Solicitation payload
    let ns_len = std::mem::size_of::<NeighborSolicitation>();
    let mut ns_payload = vec![0u8; ns_len];
    let ns_builder = NeighborSolicitationBuilder::new(&mut ns_payload).unwrap();
    ns_builder.target_addr(config.host_ipv6.octets()).build();

    // 2. Build ICMPv6 Header
    let icmpv6_header_len = 8;
    let mut icmpv6_packet = vec![0u8; icmpv6_header_len + ns_len];
    let (icmpv6_slice, icmpv6_payload_slice) = icmpv6_packet.split_at_mut(icmpv6_header_len);
    let icmpv6_header = Icmpv6Header::mut_from_bytes(icmpv6_slice).unwrap();
    icmpv6_header.icmpv6_type = 135; // Neighbor Solicitation
    icmpv6_header.icmpv6_code = 0;
    icmpv6_header.icmpv6_checksum = 0.into();
    icmpv6_header.rest = [0u8; 4];
    icmpv6_payload_slice.copy_from_slice(&ns_payload);

    // Calculate ICMPv6 checksum (uses pseudo-header)
    let checksum =
        netsim_packets::icmpv6_checksum(&icmpv6_packet, config.guest_ipv6, config.host_ipv6);
    let icmpv6_header =
        Icmpv6Header::mut_from_bytes(&mut icmpv6_packet[..icmpv6_header_len]).unwrap();
    icmpv6_header.icmpv6_checksum.set(checksum);

    // 3. Build IPv6 Header
    let ipv6_header_len = 40;
    let mut ipv6_packet = vec![0u8; ipv6_header_len + icmpv6_packet.len()];
    let (ipv6_slice, ipv6_payload_slice) = ipv6_packet.split_at_mut(ipv6_header_len);
    let mut ipv6_builder =
        Ipv6Builder::new(ipv6_slice, IP_P_ICMPV6, config.guest_ipv6, config.host_ipv6).unwrap();
    ipv6_builder.hop_limit(255);
    ipv6_builder.payload_len(icmpv6_packet.len());
    ipv6_builder.build();
    ipv6_payload_slice.copy_from_slice(&icmpv6_packet);

    // 4. Build Ethernet Frame
    let mut eth_packet = vec![0u8; 14 + ipv6_packet.len()];
    let (eth_header, eth_payload) = eth_packet.split_at_mut(14);
    let eth_frame = EthernetFrame::mut_from_bytes(eth_header).unwrap();
    eth_frame.dst_addr = config.gateway_mac;
    eth_frame.src_addr = config.guest_mac;
    eth_frame.ethertype = 0x86DD.into(); // IPv6
    eth_payload.copy_from_slice(&ipv6_packet);

    // 5. Send to Slirp
    let responses = slirp.handle_request(SlirpRequest::Packet(Bytes::copy_from_slice(&eth_packet)));

    // 6. Verify response
    assert_eq!(responses.len(), 1, "Should return exactly one response");

    let response_packet = match &responses[0] {
        SlirpResponse::Packet(p) => p,
        _ => panic!("Expected Packet response"),
    };

    // Parse Ethernet
    let (eth_frame_reply, eth_payload_reply) = EthernetFrame::parse(response_packet).unwrap();
    assert_eq!(eth_frame_reply.dst_addr, config.guest_mac);
    assert_eq!(eth_frame_reply.src_addr, config.gateway_mac);
    assert_eq!(eth_frame_reply.ethertype.get(), 0x86DD);

    // Parse IPv6
    let (ipv6_header_reply, ipv6_payload_reply) = Ipv6Header::parse(eth_payload_reply).unwrap();
    assert_eq!(ipv6_header_reply.next_header, IP_P_ICMPV6);
    assert_eq!(ipv6_header_reply.hop_limit, 255);
    assert_eq!(Ipv6Addr::from(ipv6_header_reply.source_addr), config.host_ipv6);
    assert_eq!(Ipv6Addr::from(ipv6_header_reply.dest_addr), config.guest_ipv6);

    // Parse ICMPv6
    let (icmpv6_header_reply, icmpv6_payload_reply) =
        Icmpv6Header::parse(ipv6_payload_reply).unwrap();
    assert_eq!(icmpv6_header_reply.icmpv6_type, 136); // Neighbor Advertisement
    assert_eq!(icmpv6_header_reply.icmpv6_code, 0);

    // Verify ICMPv6 checksum
    let calculated_checksum =
        netsim_packets::icmpv6_checksum(ipv6_payload_reply, config.host_ipv6, config.guest_ipv6);
    assert_eq!(calculated_checksum, 0, "ICMPv6 checksum should be valid");

    // Parse Neighbor Advertisement
    let na_reply = NeighborAdvertisement::parse(icmpv6_payload_reply).unwrap();
    assert_eq!(na_reply.flags, 0b01100000); // Solicited, Override
    assert_eq!(Ipv6Addr::from(na_reply.target_addr), config.host_ipv6);
}

#[test]
fn test_dns_redirection_and_reply_integration() {
    let config = Config::default();
    let mut slirp = Slirp::new(config.clone());

    // 1. Build a DNS query UDP packet (sent to gateway on port 53)
    let src_port = 12345;
    let dst_port = 53;
    let dns_query = b"dns_query_payload";

    let mut udp_packet = vec![0u8; 8 + dns_query.len()];
    let mut udp_builder = UdpPacketBuilder::new(&mut udp_packet, src_port, dst_port).unwrap();
    udp_builder.payload_mut().copy_from_slice(dns_query);
    udp_builder.build();

    let mut ipv4_packet = vec![0u8; 20 + udp_packet.len()];
    let (ipv4_header_slice, ipv4_payload_slice) = ipv4_packet.split_at_mut(20);
    let mut ipv4_builder = Ipv4Builder::new(
        ipv4_header_slice,
        IP_P_UDP,
        config.guest_ipv4,
        config.host_ipv4, // Sent to gateway IP
    )
    .unwrap();
    ipv4_builder.payload_len(udp_packet.len());
    ipv4_builder.build();
    ipv4_payload_slice.copy_from_slice(&udp_packet);

    let mut eth_packet = vec![0u8; 14 + ipv4_packet.len()];
    let (eth_header, eth_payload) = eth_packet.split_at_mut(14);
    let eth_frame = EthernetFrame::mut_from_bytes(eth_header).unwrap();
    eth_frame.dst_addr = config.gateway_mac;
    eth_frame.src_addr = config.guest_mac;
    eth_frame.ethertype = 0x0800.into();
    eth_payload.copy_from_slice(&ipv4_packet);

    // 2. Send DNS query to Slirp
    let responses = slirp.handle_request(SlirpRequest::Packet(Bytes::copy_from_slice(&eth_packet)));

    // 3. Verify it is intercepted and redirected to the host DNS server (default
    //    8.8.8.8)
    assert_eq!(responses.len(), 2, "Should return EstablishConnection and WriteToConnection");

    let mut conn_id = None;
    let has_establish = responses.iter().any(|r| match r {
        SlirpResponse::EstablishConnection(id, ConnectionArgs::Udp(args)) => {
            conn_id = Some(*id);
            // Verify destination is redirected to 8.8.8.8:53!
            args.destination == SocketAddr::new(IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8)), 53)
                && args.guest_ip == config.guest_ipv4
                && args.guest_port == src_port
        }
        _ => false,
    });
    assert!(has_establish, "Should trigger EstablishConnection to redirected DNS server");

    let conn_id = conn_id.expect("Should have allocated a connection ID");
    assert!(conn_id >= (1 << 63), "UDP connection ID should have MSB set");

    let has_write = responses.iter().any(|r| match r {
        SlirpResponse::WriteToConnection(id, data) => *id == conn_id && data[..] == dns_query[..],
        _ => false,
    });
    assert!(has_write, "Should trigger WriteToConnection with query payload");

    // 4. Simulate a DNS reply from the host DNS server
    let dns_reply = b"dns_reply_payload";
    let reply_responses =
        slirp.handle_request(SlirpRequest::Data(conn_id, Bytes::copy_from_slice(dns_reply)));

    // 5. Verify the reply is translated back and sent to the guest
    assert_eq!(reply_responses.len(), 1, "Should return exactly one reply packet");

    let response_packet = match &reply_responses[0] {
        SlirpResponse::Packet(p) => p,
        _ => panic!("Expected Packet response"),
    };

    // Parse Ethernet
    let (eth_frame_reply, eth_payload_reply) = EthernetFrame::parse(response_packet).unwrap();
    assert_eq!(eth_frame_reply.dst_addr, config.guest_mac);
    assert_eq!(eth_frame_reply.src_addr, config.gateway_mac);
    assert_eq!(eth_frame_reply.ethertype.get(), 0x0800); // IPv4

    // Parse IP
    let (ip_header_reply, ip_payload_reply) = Ipv4Header::parse(eth_payload_reply).unwrap();
    assert_eq!(ip_header_reply.protocol, IP_P_UDP);
    // Source IP must be translated back to gateway IP!
    assert_eq!(Ipv4Addr::from(ip_header_reply.source_addr), config.host_ipv4);
    assert_eq!(Ipv4Addr::from(ip_header_reply.dest_addr), config.guest_ipv4);

    // Parse UDP
    let (udp_header_reply, udp_payload_reply) = UdpHeader::parse(ip_payload_reply).unwrap();
    // Source port must be translated back to 53!
    assert_eq!(udp_header_reply.source_port.get(), 53);
    assert_eq!(udp_header_reply.dest_port.get(), src_port);

    // Verify payload
    assert_eq!(udp_payload_reply, dns_reply);
}

#[test]
fn test_host_port_forwarding_integration() {
    let config = Config::default();
    let mut slirp = Slirp::new(config.clone());

    let conn_id = 100;
    let host_addr = "127.0.0.1:2222".parse().unwrap();
    let guest_addr = "10.0.2.15:22".parse().unwrap();

    // 1. Simulate incoming connection from host
    let responses =
        slirp.handle_request(SlirpRequest::AcceptIncoming { conn_id, host_addr, guest_addr });

    // 2. Verify Slirp sends SYN to guest
    assert_eq!(responses.len(), 1, "Should return exactly one response (SYN packet)");
    let syn_packet = match &responses[0] {
        SlirpResponse::Packet(p) => p,
        _ => panic!("Expected Packet response"),
    };

    // Parse Ethernet
    let (eth_frame, eth_payload) = EthernetFrame::parse(syn_packet).unwrap();
    assert_eq!(eth_frame.dst_addr, config.guest_mac);
    assert_eq!(eth_frame.src_addr, config.gateway_mac);
    assert_eq!(eth_frame.ethertype.get(), 0x0800); // IPv4

    // Parse IP
    let (ip_header, ip_payload) = Ipv4Header::parse(eth_payload).unwrap();
    assert_eq!(ip_header.protocol, IP_P_TCP);
    assert_eq!(Ipv4Addr::from(ip_header.source_addr), config.host_ipv4); // Sent from gateway IP
    assert_eq!(Ipv4Addr::from(ip_header.dest_addr), config.guest_ipv4);

    // Parse TCP
    let (tcp_header, _tcp_payload_data) = TcpHeader::parse(ip_payload).unwrap();
    assert_eq!(tcp_header.dest_port.get(), 22); // Target guest port
    let virtual_port = tcp_header.source_port.get();
    assert!(virtual_port >= 49152, "Should allocate virtual port in ephemeral range");

    // Verify flags: only SYN should be set
    assert!(tcp_header.syn(), "SYN flag should be set");
    assert!(!tcp_header.ack(), "ACK flag should NOT be set");

    let iss = tcp_header.sequence_num.get();
    assert_eq!(iss, 1100); // 1000 + conn_id

    // 3. Simulate guest replying with SYN-ACK
    let guest_iss = 5000;

    let mut tcp_reply = vec![0u8; 20];
    let mut tcp_builder =
        TcpBuilder::new(&mut tcp_reply, config.guest_ipv4, config.host_ipv4).unwrap();
    tcp_builder
        .source_port(22)
        .dest_port(virtual_port)
        .sequence_num(guest_iss)
        .ack_num(iss + 1) // Acks our SYN
        .flags(netsim_packets::TCP_FLAG_SYN | netsim_packets::TCP_FLAG_ACK)
        .window_size(8192);
    tcp_builder.build();

    let mut ip_reply = vec![0u8; 20 + tcp_reply.len()];
    let (ip_header_slice, ip_payload_slice) = ip_reply.split_at_mut(20);
    let mut ipv4_builder =
        Ipv4Builder::new(ip_header_slice, IP_P_TCP, config.guest_ipv4, config.host_ipv4).unwrap();
    ipv4_builder.payload_len(tcp_reply.len());
    ipv4_builder.build();
    ip_payload_slice.copy_from_slice(&tcp_reply);

    let mut eth_reply = vec![0u8; 14 + ip_reply.len()];
    let (eth_header_slice, eth_payload_slice) = eth_reply.split_at_mut(14);
    let eth_frame_reply = EthernetFrame::mut_from_bytes(eth_header_slice).unwrap();
    eth_frame_reply.dst_addr = config.gateway_mac;
    eth_frame_reply.src_addr = config.guest_mac;
    eth_frame_reply.ethertype = 0x0800.into();
    eth_payload_slice.copy_from_slice(&ip_reply);

    // Send SYN-ACK to Slirp
    let responses2 = slirp.handle_request(SlirpRequest::Packet(Bytes::copy_from_slice(&eth_reply)));

    // 4. Verify Slirp sends ACK to guest and notifies host driver
    assert_eq!(responses2.len(), 2, "Should return ACK packet and ConnectionEstablished");

    let mut has_established = false;
    let mut final_ack_packet = None;

    for resp in &responses2 {
        match resp {
            SlirpResponse::ConnectionEstablished(id) => {
                assert_eq!(*id, conn_id);
                has_established = true;
            }
            SlirpResponse::Packet(p) => {
                final_ack_packet = Some(p);
            }
            other => panic!("Unexpected response type: {other:?}"),
        }
    }

    assert!(has_established, "Should notify host driver that connection is established");
    let final_ack_packet = final_ack_packet.expect("Should have sent final ACK packet");

    // Parse final ACK
    let (_, eth_payload_ack) = EthernetFrame::parse(final_ack_packet).unwrap();
    let (_, ip_payload_ack) = Ipv4Header::parse(eth_payload_ack).unwrap();
    let (tcp_header_ack, _) = TcpHeader::parse(ip_payload_ack).unwrap();

    assert_eq!(tcp_header_ack.sequence_num.get(), iss + 1);
    assert_eq!(tcp_header_ack.ack_num.get(), guest_iss + 1); // Acks guest's SYN
    assert!(tcp_header_ack.ack(), "ACK flag should be set");
    assert!(!tcp_header_ack.syn(), "SYN flag should NOT be set");

    // 5. Verify Bi-directional Data Flow
    // A. Host to Guest
    let host_data = b"hello_guest";
    let responses_data =
        slirp.handle_request(SlirpRequest::Data(conn_id, Bytes::copy_from_slice(host_data)));

    assert_eq!(responses_data.len(), 1, "Should send data packet to guest");
    let data_packet = match &responses_data[0] {
        SlirpResponse::Packet(p) => p,
        _ => panic!("Expected Packet response"),
    };

    let (_, eth_payload_data) = EthernetFrame::parse(data_packet).unwrap();
    let (_, ip_payload_data) = Ipv4Header::parse(eth_payload_data).unwrap();
    let (tcp_header_data, tcp_payload_data) = TcpHeader::parse(ip_payload_data).unwrap();

    assert_eq!(tcp_header_data.sequence_num.get(), iss + 1);
    assert_eq!(tcp_header_data.ack_num.get(), guest_iss + 1);
    assert_eq!(tcp_payload_data, host_data);

    // B. Guest to Host
    let guest_data = b"hello_host";
    let mut tcp_data = vec![0u8; 20 + guest_data.len()];
    let mut tcp_builder =
        TcpBuilder::new(&mut tcp_data, config.guest_ipv4, config.host_ipv4).unwrap();
    tcp_builder
        .source_port(22)
        .dest_port(virtual_port)
        .sequence_num(guest_iss + 1)
        .ack_num(iss + 1 + host_data.len() as u32) // Acks host data
        .flags(netsim_packets::TCP_FLAG_ACK)
        .window_size(8192)
        .payload(guest_data);
    tcp_builder.build();

    let mut ip_data = vec![0u8; 20 + tcp_data.len()];
    let (ip_header_slice, ip_payload_slice) = ip_data.split_at_mut(20);
    let mut ipv4_builder =
        Ipv4Builder::new(ip_header_slice, IP_P_TCP, config.guest_ipv4, config.host_ipv4).unwrap();
    ipv4_builder.payload_len(tcp_data.len());
    ipv4_builder.build();
    ip_payload_slice.copy_from_slice(&tcp_data);

    let mut eth_data = vec![0u8; 14 + ip_data.len()];
    let (eth_header_slice, eth_payload_slice) = eth_data.split_at_mut(14);
    let eth_frame_data = EthernetFrame::mut_from_bytes(eth_header_slice).unwrap();
    eth_frame_data.dst_addr = config.gateway_mac;
    eth_frame_data.src_addr = config.guest_mac;
    eth_frame_data.ethertype = 0x0800.into();
    eth_payload_slice.copy_from_slice(&ip_data);

    let responses_guest_data =
        slirp.handle_request(SlirpRequest::Packet(Bytes::copy_from_slice(&eth_data)));

    // Verify Slirp forwards data to host and acks it
    assert_eq!(responses_guest_data.len(), 2, "Should return WriteToConnection and ACK packet");

    let mut has_write = false;
    for resp in &responses_guest_data {
        match resp {
            SlirpResponse::WriteToConnection(id, data) => {
                assert_eq!(*id, conn_id);
                assert_eq!(data[..], guest_data[..]);
                has_write = true;
            }
            SlirpResponse::Packet(_) => {} // The ACK packet to guest
            _ => panic!("Unexpected response type"),
        }
    }
    assert!(has_write, "Should forward guest data to host driver");
}

fn parse_dhcp_options(options: &[u8]) -> HashMap<u8, Vec<u8>> {
    let mut map = HashMap::new();
    let mut i = 0;
    while i < options.len() {
        let option_code = options[i];
        if option_code == 0 {
            i += 1;
            continue;
        }
        if option_code == 255 {
            break;
        }
        if i + 1 >= options.len() {
            break;
        }
        let len = options[i + 1] as usize;
        if i + 1 + len >= options.len() {
            break;
        }
        let data = options[i + 2..i + 2 + len].to_vec();
        map.insert(option_code, data);
        i += len + 2;
    }
    map
}

fn create_discover_packet() -> DhcpPacket {
    let mut options = [0; 308];
    options[0] = 53; // DHCP Message Type
    options[1] = 1;
    options[2] = DhcpMessageType::Discover as u8;
    options[3] = 255; // End
    DhcpPacket {
        op: 1,
        htype: 1,
        hlen: 6,
        hops: 0,
        xid: [1, 2, 3, 4],
        secs: [0; 2],
        flags: [0; 2],
        ciaddr: [0; 4],
        yiaddr: [0; 4],
        siaddr: [0; 4],
        giaddr: [0; 4],
        chaddr: [0xde, 0xad, 0xbe, 0xef, 0x12, 0x34, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        sname: [0; 64],
        file: [0; 128],
        magic_cookie: DHCP_MAGIC_COOKIE,
        options,
    }
}

fn create_request_packet(requested_ip: Ipv4Addr) -> DhcpPacket {
    let mut options = [0; 308];
    options[0] = 53; // DHCP Message Type
    options[1] = 1;
    options[2] = DhcpMessageType::Request as u8;
    options[3] = 50; // Requested IP address
    options[4] = 4;
    options[5..9].copy_from_slice(&requested_ip.octets());
    options[9] = 255; // End
    DhcpPacket {
        op: 1,
        htype: 1,
        hlen: 6,
        hops: 0,
        xid: [1, 2, 3, 5],
        secs: [0; 2],
        flags: [0; 2],
        ciaddr: [0; 4],
        yiaddr: [0; 4],
        siaddr: [0; 4],
        giaddr: [0; 4],
        chaddr: [0xde, 0xad, 0xbe, 0xef, 0x12, 0x34, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        sname: [0; 64],
        file: [0; 128],
        magic_cookie: DHCP_MAGIC_COOKIE,
        options,
    }
}

fn wrap_dhcp_packet(dhcp_packet: &DhcpPacket, guest_mac: MacAddr) -> Bytes {
    let mut buffer = vec![0u8; 1500];
    let eth_header_len = 14;
    let ipv4_header_len = 20;
    let dhcp_len = std::mem::size_of::<DhcpPacket>();

    let (eth_header_slice, eth_payload) = buffer.split_at_mut(eth_header_len);
    let eth_frame = EthernetFrame::mut_from_bytes(eth_header_slice).unwrap();
    eth_frame.dst_addr = MacAddr { bytes: [0xff; 6] }; // Broadcast
    eth_frame.src_addr = guest_mac;
    eth_frame.ethertype = 0x0800.into();

    let (ipv4_header_slice, ipv4_payload) = eth_payload.split_at_mut(ipv4_header_len);
    let mut ipv4_builder =
        Ipv4Builder::new(ipv4_header_slice, 17, [0; 4].into(), [255; 4].into()).unwrap();

    let mut udp_builder = UdpPacketBuilder::new(&mut ipv4_payload[..8 + dhcp_len], 68, 67).unwrap();
    udp_builder.payload_mut().copy_from_slice(dhcp_packet.as_bytes());
    let udp_len = udp_builder.build();
    ipv4_builder.payload_len(udp_len);
    ipv4_builder.build();

    let packet_len = eth_header_len + ipv4_header_len + udp_len;
    Bytes::copy_from_slice(&buffer[..packet_len])
}

#[test]
fn test_save_restore_integration() {
    let config = Config::default();
    let mut slirp_a = Slirp::new(config.clone());

    // 1. Populate DHCP State (Lease)
    // Send DHCP Discover
    let discover = create_discover_packet();
    let eth_discover = wrap_dhcp_packet(&discover, config.guest_mac);
    let responses_discover = slirp_a.handle_request(SlirpRequest::Packet(eth_discover));
    assert_eq!(responses_discover.len(), 1, "Should reply with DHCPOFFER");

    // Send DHCP Request for the leased IP (10.0.2.15)
    let request = create_request_packet(config.guest_ipv4);
    let eth_request = wrap_dhcp_packet(&request, config.guest_mac);
    let responses_request = slirp_a.handle_request(SlirpRequest::Packet(eth_request));
    assert_eq!(responses_request.len(), 1, "Should reply with DHCPACK");

    // 2. Populate TCP State (Established Connection)
    let conn_id = 1;
    let guest_port = 12345;
    let host_port = 80;

    // Guest sends SYN
    let guest_iss = 1000;
    let mut tcp_syn = vec![0u8; 20];
    let mut tcp_builder =
        TcpBuilder::new(&mut tcp_syn, config.guest_ipv4, config.host_ipv4).unwrap();
    tcp_builder
        .source_port(guest_port)
        .dest_port(host_port)
        .sequence_num(guest_iss)
        .flags(netsim_packets::TCP_FLAG_SYN)
        .window_size(8192);
    tcp_builder.build();

    let mut ip_syn = vec![0u8; 20 + tcp_syn.len()];
    let (ip_header_slice, ip_payload_slice) = ip_syn.split_at_mut(20);
    let mut ipv4_builder =
        Ipv4Builder::new(ip_header_slice, IP_P_TCP, config.guest_ipv4, config.host_ipv4).unwrap();
    ipv4_builder.payload_len(tcp_syn.len());
    ipv4_builder.build();
    ip_payload_slice.copy_from_slice(&tcp_syn);

    let mut eth_syn = vec![0u8; 14 + ip_syn.len()];
    let (eth_header_slice, eth_payload_slice) = eth_syn.split_at_mut(14);
    let eth_frame_syn = EthernetFrame::mut_from_bytes(eth_header_slice).unwrap();
    eth_frame_syn.dst_addr = config.gateway_mac;
    eth_frame_syn.src_addr = config.guest_mac;
    eth_frame_syn.ethertype = 0x0800.into();
    eth_payload_slice.copy_from_slice(&ip_syn);

    let responses_syn =
        slirp_a.handle_request(SlirpRequest::Packet(Bytes::copy_from_slice(&eth_syn)));

    // Slirp replies with EstablishConnection and SYN-ACK packet
    assert_eq!(responses_syn.len(), 2);
    let mut syn_ack_packet = None;
    for resp in &responses_syn {
        if let SlirpResponse::Packet(p) = resp {
            syn_ack_packet = Some(p);
        }
    }
    let syn_ack_packet = syn_ack_packet.expect("Should have sent SYN-ACK packet");

    // Parse SYN-ACK to get Slirp's ISS
    let (_, eth_payload_syn_ack) = EthernetFrame::parse(syn_ack_packet).unwrap();
    let (_, ip_payload_syn_ack) = Ipv4Header::parse(eth_payload_syn_ack).unwrap();
    let (tcp_header_syn_ack, _) = TcpHeader::parse(ip_payload_syn_ack).unwrap();
    let slirp_iss = tcp_header_syn_ack.sequence_num.get();

    // Guest sends ACK to complete handshake
    let mut tcp_ack = vec![0u8; 20];
    let mut tcp_builder =
        TcpBuilder::new(&mut tcp_ack, config.guest_ipv4, config.host_ipv4).unwrap();
    tcp_builder
        .source_port(guest_port)
        .dest_port(host_port)
        .sequence_num(guest_iss + 1)
        .ack_num(slirp_iss + 1)
        .flags(netsim_packets::TCP_FLAG_ACK)
        .window_size(8192);
    tcp_builder.build();

    let mut ip_ack = vec![0u8; 20 + tcp_ack.len()];
    let (ip_header_slice, ip_payload_slice) = ip_ack.split_at_mut(20);
    let mut ipv4_builder =
        Ipv4Builder::new(ip_header_slice, IP_P_TCP, config.guest_ipv4, config.host_ipv4).unwrap();
    ipv4_builder.payload_len(tcp_ack.len());
    ipv4_builder.build();
    ip_payload_slice.copy_from_slice(&tcp_ack);

    let mut eth_ack = vec![0u8; 14 + ip_ack.len()];
    let (eth_header_slice, eth_payload_slice) = eth_ack.split_at_mut(14);
    let eth_frame_ack = EthernetFrame::mut_from_bytes(eth_header_slice).unwrap();
    eth_frame_ack.dst_addr = config.gateway_mac;
    eth_frame_ack.src_addr = config.guest_mac;
    eth_frame_ack.ethertype = 0x0800.into();
    eth_payload_slice.copy_from_slice(&ip_ack);

    let responses_ack =
        slirp_a.handle_request(SlirpRequest::Packet(Bytes::copy_from_slice(&eth_ack)));
    assert_eq!(responses_ack.len(), 1);

    // 3. Populate UDP State (DNS Flow)
    let dns_query = b"dns_query_payload";
    let mut udp_dns = vec![0u8; 8 + dns_query.len()];
    let mut udp_builder = UdpPacketBuilder::new(&mut udp_dns, 12345, 53).unwrap();
    udp_builder.payload_mut()[..dns_query.len()].copy_from_slice(dns_query);
    udp_builder.build();

    let mut ip_dns = vec![0u8; 20 + udp_dns.len()];
    let (ip_header_slice, ip_payload_slice) = ip_dns.split_at_mut(20);
    let mut ipv4_builder =
        Ipv4Builder::new(ip_header_slice, IP_P_UDP, config.guest_ipv4, config.host_ipv4).unwrap();
    ipv4_builder.payload_len(udp_dns.len());
    ipv4_builder.build();
    ip_payload_slice.copy_from_slice(&udp_dns);

    let mut eth_dns = vec![0u8; 14 + ip_dns.len()];
    let (eth_header_slice, eth_payload_slice) = eth_dns.split_at_mut(14);
    let eth_frame_dns = EthernetFrame::mut_from_bytes(eth_header_slice).unwrap();
    eth_frame_dns.dst_addr = config.gateway_mac;
    eth_frame_dns.src_addr = config.guest_mac;
    eth_frame_dns.ethertype = 0x0800.into();
    eth_payload_slice.copy_from_slice(&ip_dns);

    let responses_dns =
        slirp_a.handle_request(SlirpRequest::Packet(Bytes::copy_from_slice(&eth_dns)));
    assert_eq!(responses_dns.len(), 2, "Should return EstablishConnection and WriteToConnection");
    let udp_conn_id = match &responses_dns[0] {
        SlirpResponse::EstablishConnection(id, _) => *id,
        _ => panic!("Expected EstablishConnection"),
    };

    // 4. Populate Incoming TCP State (Hostfwd Connection)
    let inbound_conn_id = 999;
    let guest_inbound_addr: SocketAddr = "10.0.2.15:8080".parse().unwrap();
    let responses_inbound = slirp_a.handle_request(SlirpRequest::AcceptIncoming {
        conn_id: inbound_conn_id,
        host_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 8080),
        guest_addr: guest_inbound_addr,
    });
    assert_eq!(responses_inbound.len(), 1, "Should send SYN to guest");

    // ----------------------------------------------------
    // SUSPEND: Save State
    // ----------------------------------------------------
    let state_bytes = slirp_a.save_state().unwrap();
    assert!(!state_bytes.is_empty());

    // ----------------------------------------------------
    // RESUME: Restore State into a fresh Instance B
    // ----------------------------------------------------
    let mut slirp_b = Slirp::new(config.clone());
    let responses_restore = slirp_b.restore_state(&state_bytes).unwrap();
    assert_eq!(
        responses_restore.len(),
        4,
        "Should return Reset and EstablishConnection for active TCP, incoming hostfwd TCP, and \
         UDP flows"
    );
    assert_eq!(responses_restore[0], SlirpResponse::Reset);

    let mut has_tcp = false;
    let mut has_inbound_tcp = false;
    let mut has_udp = false;

    for resp in responses_restore.into_iter().skip(1) {
        match resp {
            SlirpResponse::EstablishConnection(id, ConnectionArgs::Tcp(args)) if id == conn_id => {
                has_tcp = true;
                assert_eq!(
                    args.destination,
                    SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), host_port)
                );
                assert_eq!(args.guest_ip, IpAddr::V4(config.guest_ipv4));
                assert_eq!(args.guest_port, guest_port);
            }
            SlirpResponse::EstablishConnection(id, ConnectionArgs::Tcp(args))
                if id == inbound_conn_id =>
            {
                has_inbound_tcp = true;
                assert_eq!(args.destination, SocketAddr::new(IpAddr::V4(config.host_ipv4), 49152));
                assert_eq!(args.guest_ip, guest_inbound_addr.ip());
                assert_eq!(args.guest_port, guest_inbound_addr.port());
            }
            SlirpResponse::EstablishConnection(id, ConnectionArgs::Udp(args)) => {
                has_udp = true;
                assert_eq!(id, udp_conn_id);
                assert_eq!(args.destination, "8.8.8.8:53".parse().unwrap());
                assert_eq!(args.guest_ip, IpAddr::V4(config.guest_ipv4));
                assert_eq!(args.guest_port, 12345);
            }
            _ => panic!("Unexpected response: {resp:?}"),
        }
    }

    assert!(has_tcp, "Missing TCP restore");
    assert!(has_inbound_tcp, "Missing inbound TCP restore");
    assert!(has_udp, "Missing UDP restore");

    // ----------------------------------------------------
    // VERIFY RESTORED STATE
    // ----------------------------------------------------

    // A. Verify DHCP Lease is Restored
    let request_b = create_request_packet(config.guest_ipv4);
    let eth_request_b = wrap_dhcp_packet(&request_b, config.guest_mac);
    let responses_request_b = slirp_b.handle_request(SlirpRequest::Packet(eth_request_b));
    assert_eq!(responses_request_b.len(), 1, "Should reply with DHCPACK after restore");
    let ack_packet_b = match &responses_request_b[0] {
        SlirpResponse::Packet(p) => p,
        _ => panic!("Expected Packet response"),
    };
    let (_, offer_ip_b) = EthernetFrame::parse(ack_packet_b).unwrap();
    let (_, offer_udp_b) = Ipv4Header::parse(offer_ip_b).unwrap();
    let (_, offer_dhcp_payload_b) = UdpHeader::parse(offer_udp_b).unwrap();
    let (offer_dhcp_b, _) = DhcpPacket::parse(offer_dhcp_payload_b).unwrap();
    assert_eq!(offer_dhcp_b.op, 2); // BOOTREPLY
    let options_b = parse_dhcp_options(&offer_dhcp_b.options);
    assert_eq!(options_b.get(&53).unwrap(), &vec![DhcpMessageType::Ack as u8]);

    // B. Verify TCP Connection is Restored
    let guest_data = b"hello_post_restore";
    let mut tcp_data = vec![0u8; 20 + guest_data.len()];
    let mut tcp_builder =
        TcpBuilder::new(&mut tcp_data, config.guest_ipv4, config.host_ipv4).unwrap();
    tcp_builder
        .source_port(guest_port)
        .dest_port(host_port)
        .sequence_num(guest_iss + 1)
        .ack_num(slirp_iss + 1)
        .flags(netsim_packets::TCP_FLAG_ACK)
        .window_size(8192)
        .payload(guest_data);
    tcp_builder.build();

    let mut ip_data = vec![0u8; 20 + tcp_data.len()];
    let (ip_header_slice, ip_payload_slice) = ip_data.split_at_mut(20);
    let mut ipv4_builder =
        Ipv4Builder::new(ip_header_slice, IP_P_TCP, config.guest_ipv4, config.host_ipv4).unwrap();
    ipv4_builder.payload_len(tcp_data.len());
    ipv4_builder.build();
    ip_payload_slice.copy_from_slice(&tcp_data);

    let mut eth_data = vec![0u8; 14 + ip_data.len()];
    let (eth_header_slice, eth_payload_slice) = eth_data.split_at_mut(14);
    let eth_frame_data = EthernetFrame::mut_from_bytes(eth_header_slice).unwrap();
    eth_frame_data.dst_addr = config.gateway_mac;
    eth_frame_data.src_addr = config.guest_mac;
    eth_frame_data.ethertype = 0x0800.into();
    eth_payload_slice.copy_from_slice(&ip_data);

    let responses_guest_data =
        slirp_b.handle_request(SlirpRequest::Packet(Bytes::copy_from_slice(&eth_data)));

    // Verify Slirp forwards data to host and acks it
    assert_eq!(
        responses_guest_data.len(),
        2,
        "Should return WriteToConnection and ACK packet after restore"
    );
    let mut has_write = false;
    for resp in &responses_guest_data {
        match resp {
            SlirpResponse::WriteToConnection(id, data) => {
                assert_eq!(*id, conn_id);
                assert_eq!(data[..], guest_data[..]);
                has_write = true;
            }
            SlirpResponse::Packet(_) => {} // The ACK packet to guest
            _ => panic!("Unexpected response type"),
        }
    }
    assert!(has_write, "Should forward guest data to host driver after restore");

    // C. Verify UDP Flow (DNS) is Restored
    let dns_reply = b"dns_reply_post_restore";
    let reply_responses =
        slirp_b.handle_request(SlirpRequest::Data(udp_conn_id, Bytes::copy_from_slice(dns_reply)));
    assert_eq!(reply_responses.len(), 1, "Should return exactly one reply packet after restore");
    let response_packet = match &reply_responses[0] {
        SlirpResponse::Packet(p) => p,
        _ => panic!("Expected Packet response"),
    };
    let (_, eth_payload_reply) = EthernetFrame::parse(response_packet).unwrap();
    let (ip_header_reply, ip_payload_reply) = Ipv4Header::parse(eth_payload_reply).unwrap();
    assert_eq!(ip_header_reply.protocol, IP_P_UDP);
    assert_eq!(Ipv4Addr::from(ip_header_reply.source_addr), config.host_ipv4);
    assert_eq!(Ipv4Addr::from(ip_header_reply.dest_addr), config.guest_ipv4);
    let (udp_header_reply, udp_payload_reply) = UdpHeader::parse(ip_payload_reply).unwrap();
    assert_eq!(udp_header_reply.source_port.get(), 53);
    assert_eq!(udp_payload_reply, dns_reply);
}

#[test]
fn test_tcp_ipv6_integration() {
    let config = Config::default();
    let mut slirp = Slirp::new(config.clone());

    let guest_port = 12345;
    let host_port = 80;
    let guest_ip = config.guest_ipv6;
    let host_ip = config.host_ipv6;
    let conn_id = 1;

    // 1. Guest sends TCP SYN over IPv6
    let guest_iss = 1000;
    let mut tcp_syn = vec![0u8; 20];
    let mut tcp_builder = TcpBuilder::new_v6(&mut tcp_syn, guest_ip, host_ip).unwrap();
    tcp_builder
        .source_port(guest_port)
        .dest_port(host_port)
        .sequence_num(guest_iss)
        .flags(netsim_packets::TCP_FLAG_SYN)
        .window_size(8192);
    tcp_builder.build();

    let mut ip_syn = vec![0u8; 40 + tcp_syn.len()];
    let (ip_header_slice, ip_payload_slice) = ip_syn.split_at_mut(40);
    let mut ipv6_builder = Ipv6Builder::new(ip_header_slice, IP_P_TCP, guest_ip, host_ip).unwrap();
    ipv6_builder.payload_len(tcp_syn.len());
    ipv6_builder.build();
    ip_payload_slice.copy_from_slice(&tcp_syn);

    let mut eth_syn = vec![0u8; 14 + ip_syn.len()];
    let (eth_header_slice, eth_payload_slice) = eth_syn.split_at_mut(14);
    let eth_frame_syn = EthernetFrame::mut_from_bytes(eth_header_slice).unwrap();
    eth_frame_syn.dst_addr = config.gateway_mac;
    eth_frame_syn.src_addr = config.guest_mac;
    eth_frame_syn.ethertype = 0x86DD.into();
    eth_payload_slice.copy_from_slice(&ip_syn);

    let responses_syn =
        slirp.handle_request(SlirpRequest::Packet(Bytes::copy_from_slice(&eth_syn)));

    assert_eq!(responses_syn.len(), 2, "Should return EstablishConnection and SYN-ACK packet");
    let mut syn_ack_packet = None;
    for resp in &responses_syn {
        match resp {
            SlirpResponse::EstablishConnection(id, conn_args) => {
                assert_eq!(*id, conn_id);
                match conn_args {
                    ConnectionArgs::Tcp(args) => {
                        assert_eq!(
                            args.destination,
                            SocketAddr::new(IpAddr::V6(Ipv6Addr::LOCALHOST), host_port)
                        );
                        assert_eq!(args.guest_ip, IpAddr::V6(guest_ip));
                        assert_eq!(args.guest_port, guest_port);
                    }
                    _ => panic!("Expected TCP connection args"),
                }
            }
            SlirpResponse::Packet(p) => {
                syn_ack_packet = Some(p);
            }
            _ => panic!("Unexpected response type"),
        }
    }
    let syn_ack_packet = syn_ack_packet.expect("Should have sent SYN-ACK packet");

    let (_, eth_payload_syn_ack) = EthernetFrame::parse(syn_ack_packet).unwrap();
    let (ipv6_header_syn_ack, ip_payload_syn_ack) = Ipv6Header::parse(eth_payload_syn_ack).unwrap();
    assert_eq!(ipv6_header_syn_ack.next_header, IP_P_TCP);
    assert_eq!(Ipv6Addr::from(ipv6_header_syn_ack.source_addr), host_ip);
    assert_eq!(Ipv6Addr::from(ipv6_header_syn_ack.dest_addr), guest_ip);

    let (tcp_header_syn_ack, _) = TcpHeader::parse(ip_payload_syn_ack).unwrap();
    assert_eq!(tcp_header_syn_ack.source_port.get(), host_port);
    assert_eq!(tcp_header_syn_ack.dest_port.get(), guest_port);
    assert_eq!(tcp_header_syn_ack.ack_num.get(), guest_iss + 1);
    assert!(tcp_header_syn_ack.syn(), "SYN-ACK should have SYN flag set");
    assert!(tcp_header_syn_ack.ack(), "SYN-ACK should have ACK flag set");

    let slirp_iss = tcp_header_syn_ack.sequence_num.get();

    // 2. Guest sends TCP ACK over IPv6
    let mut tcp_ack = vec![0u8; 20];
    let mut tcp_builder = TcpBuilder::new_v6(&mut tcp_ack, guest_ip, host_ip).unwrap();
    tcp_builder
        .source_port(guest_port)
        .dest_port(host_port)
        .sequence_num(guest_iss + 1)
        .ack_num(slirp_iss + 1)
        .flags(netsim_packets::TCP_FLAG_ACK)
        .window_size(8192);
    tcp_builder.build();

    let mut ip_ack = vec![0u8; 40 + tcp_ack.len()];
    let (ip_header_slice, ip_payload_slice) = ip_ack.split_at_mut(40);
    let mut ipv6_builder = Ipv6Builder::new(ip_header_slice, IP_P_TCP, guest_ip, host_ip).unwrap();
    ipv6_builder.payload_len(tcp_ack.len());
    ipv6_builder.build();
    ip_payload_slice.copy_from_slice(&tcp_ack);

    let mut eth_ack = vec![0u8; 14 + ip_ack.len()];
    let (eth_header_slice, eth_payload_slice) = eth_ack.split_at_mut(14);
    let eth_frame_ack = EthernetFrame::mut_from_bytes(eth_header_slice).unwrap();
    eth_frame_ack.dst_addr = config.gateway_mac;
    eth_frame_ack.src_addr = config.guest_mac;
    eth_frame_ack.ethertype = 0x86DD.into();
    eth_payload_slice.copy_from_slice(&ip_ack);

    let responses_ack =
        slirp.handle_request(SlirpRequest::Packet(Bytes::copy_from_slice(&eth_ack)));
    assert_eq!(responses_ack.len(), 1);
    match &responses_ack[0] {
        SlirpResponse::ActivateFastPath { conn_id: id, guest_addr, host_addr } => {
            assert_eq!(*id, conn_id);
            assert_eq!(*guest_addr, SocketAddr::new(IpAddr::V6(guest_ip), guest_port));
            assert_eq!(*host_addr, SocketAddr::new(IpAddr::V6(host_ip), host_port));
        }
        _ => panic!("Expected ActivateFastPath"),
    }

    // 3. Send data from guest to host over IPv6
    let guest_data = b"hello_ipv6_tcp";
    let mut tcp_data = vec![0u8; 20 + guest_data.len()];
    let mut tcp_builder = TcpBuilder::new_v6(&mut tcp_data, guest_ip, host_ip).unwrap();
    tcp_builder
        .source_port(guest_port)
        .dest_port(host_port)
        .sequence_num(guest_iss + 1)
        .ack_num(slirp_iss + 1)
        .flags(netsim_packets::TCP_FLAG_ACK)
        .window_size(8192)
        .payload(guest_data);
    tcp_builder.build();

    let mut ip_data = vec![0u8; 40 + tcp_data.len()];
    let (ip_header_slice, ip_payload_slice) = ip_data.split_at_mut(40);
    let mut ipv6_builder = Ipv6Builder::new(ip_header_slice, IP_P_TCP, guest_ip, host_ip).unwrap();
    ipv6_builder.payload_len(tcp_data.len());
    ipv6_builder.build();
    ip_payload_slice.copy_from_slice(&tcp_data);

    let mut eth_data = vec![0u8; 14 + ip_data.len()];
    let (eth_header_slice, eth_payload_slice) = eth_data.split_at_mut(14);
    let eth_frame_data = EthernetFrame::mut_from_bytes(eth_header_slice).unwrap();
    eth_frame_data.dst_addr = config.gateway_mac;
    eth_frame_data.src_addr = config.guest_mac;
    eth_frame_data.ethertype = 0x86DD.into();
    eth_payload_slice.copy_from_slice(&ip_data);

    let responses_data =
        slirp.handle_request(SlirpRequest::Packet(Bytes::copy_from_slice(&eth_data)));

    assert_eq!(responses_data.len(), 2, "Should return WriteToConnection and ACK packet");
    let mut has_write = false;
    let mut ack_packet = None;
    for resp in &responses_data {
        match resp {
            SlirpResponse::WriteToConnection(id, data) => {
                assert_eq!(*id, conn_id);
                assert_eq!(data[..], guest_data[..]);
                has_write = true;
            }
            SlirpResponse::Packet(p) => {
                ack_packet = Some(p);
            }
            _ => panic!("Unexpected response type"),
        }
    }
    assert!(has_write, "Should forward guest data to host driver");
    let ack_packet = ack_packet.expect("Should have sent ACK packet");

    let (_, eth_payload_ack) = EthernetFrame::parse(ack_packet).unwrap();
    let (ipv6_header_ack, ip_payload_ack) = Ipv6Header::parse(eth_payload_ack).unwrap();
    assert_eq!(ipv6_header_ack.next_header, IP_P_TCP);
    assert_eq!(Ipv6Addr::from(ipv6_header_ack.source_addr), host_ip);
    assert_eq!(Ipv6Addr::from(ipv6_header_ack.dest_addr), guest_ip);

    let (tcp_header_ack, _) = TcpHeader::parse(ip_payload_ack).unwrap();
    assert_eq!(tcp_header_ack.source_port.get(), host_port);
    assert_eq!(tcp_header_ack.dest_port.get(), guest_port);
    assert_eq!(tcp_header_ack.ack_num.get(), guest_iss + 1 + guest_data.len() as u32);
    assert!(tcp_header_ack.ack(), "ACK packet should have ACK flag set");
}

#[test]
fn test_udp_ipv6_integration() {
    let config = Config::default();
    let mut slirp = Slirp::new(config.clone());

    let guest_port = 12345;
    let guest_ip = config.guest_ipv6;
    let host_ip = config.host_ipv6;

    // 1. Send DNS query over IPv6 (destined to gateway:53)
    let dns_query = b"dns_query_ipv6";
    let mut udp_dns = vec![0u8; 8 + dns_query.len()];
    let mut udp_builder = UdpPacketBuilder::new(&mut udp_dns, guest_port, 53).unwrap();
    udp_builder.payload_mut()[..dns_query.len()].copy_from_slice(dns_query);
    udp_builder.build();

    let mut ip_dns = vec![0u8; 40 + udp_dns.len()];
    let (ip_header_slice, ip_payload_slice) = ip_dns.split_at_mut(40);
    let mut ipv6_builder = Ipv6Builder::new(ip_header_slice, IP_P_UDP, guest_ip, host_ip).unwrap();
    ipv6_builder.payload_len(udp_dns.len());
    ipv6_builder.build();
    ip_payload_slice.copy_from_slice(&udp_dns);

    let mut eth_dns = vec![0u8; 14 + ip_dns.len()];
    let (eth_header_slice, eth_payload_slice) = eth_dns.split_at_mut(14);
    let eth_frame_dns = EthernetFrame::mut_from_bytes(eth_header_slice).unwrap();
    eth_frame_dns.dst_addr = config.gateway_mac;
    eth_frame_dns.src_addr = config.guest_mac;
    eth_frame_dns.ethertype = 0x86DD.into();
    eth_payload_slice.copy_from_slice(&ip_dns);

    let responses_dns =
        slirp.handle_request(SlirpRequest::Packet(Bytes::copy_from_slice(&eth_dns)));

    // Verify Slirp intercepts and returns EstablishConnection and WriteToConnection
    assert_eq!(responses_dns.len(), 2, "Should return EstablishConnection and WriteToConnection");
    let mut udp_conn_id = None;
    let mut has_write = false;
    for resp in &responses_dns {
        match resp {
            SlirpResponse::EstablishConnection(id, conn_args) => {
                udp_conn_id = Some(*id);
                match conn_args {
                    ConnectionArgs::Udp(args) => {
                        assert_eq!(
                            args.destination,
                            SocketAddr::new(
                                IpAddr::V6("2001:4860:4860::8888".parse().unwrap()),
                                53
                            )
                        );
                        assert_eq!(args.guest_ip, IpAddr::V6(guest_ip));
                        assert_eq!(args.guest_port, guest_port);
                    }
                    _ => panic!("Expected UDP connection args"),
                }
            }
            SlirpResponse::WriteToConnection(_, data) => {
                assert_eq!(data[..], dns_query[..]);
                has_write = true;
            }
            _ => panic!("Unexpected response type"),
        }
    }
    let udp_conn_id = udp_conn_id.expect("Should have returned connection ID");
    assert!(has_write, "Should forward DNS query to host driver");

    // 2. Send DNS reply from host back to guest
    let dns_reply = b"dns_reply_ipv6";
    let reply_responses =
        slirp.handle_request(SlirpRequest::Data(udp_conn_id, Bytes::copy_from_slice(dns_reply)));

    assert_eq!(reply_responses.len(), 1, "Should return exactly one reply packet");
    let response_packet = match &reply_responses[0] {
        SlirpResponse::Packet(p) => p,
        _ => panic!("Expected Packet response"),
    };

    // Parse reply packet to verify it is a valid IPv6 UDP packet
    let (_, eth_payload_reply) = EthernetFrame::parse(response_packet).unwrap();
    let (ipv6_header_reply, ip_payload_reply) = Ipv6Header::parse(eth_payload_reply).unwrap();
    assert_eq!(ipv6_header_reply.next_header, IP_P_UDP);
    assert_eq!(Ipv6Addr::from(ipv6_header_reply.source_addr), host_ip);
    assert_eq!(Ipv6Addr::from(ipv6_header_reply.dest_addr), guest_ip);

    let (udp_header_reply, udp_payload_reply) = UdpHeader::parse(ip_payload_reply).unwrap();
    assert_eq!(udp_header_reply.source_port.get(), 53);
    assert_eq!(udp_header_reply.dest_port.get(), guest_port);
    assert_eq!(udp_payload_reply, dns_reply);
}

#[test]
fn test_ndp_router_solicitation_integration() {
    let config = Config::default();
    let mut slirp = Slirp::new(config.clone());

    // --- CASE A: Solicited RS with Unicast Source IP ---
    let guest_link_local = Ipv6Addr::new(0xfe80, 0, 0, 0, 0, 0, 0, 1);
    let all_routers_multicast = Ipv6Addr::new(0xff02, 0, 0, 0, 0, 0, 0, 2);

    let mut rs_payload = vec![0u8; 4];
    let rs_builder = netsim_packets::RouterSolicitationBuilder::new(&mut rs_payload).unwrap();
    rs_builder.build();

    let mut ip_rs = vec![0u8; 40 + 8 + rs_payload.len()];
    let (ip_header_slice, ip_payload_slice) = ip_rs.split_at_mut(40);
    let mut ipv6_builder =
        Ipv6Builder::new(ip_header_slice, IP_P_ICMPV6, guest_link_local, all_routers_multicast)
            .unwrap();
    ipv6_builder.hop_limit(255);
    ipv6_builder.payload_len(8 + rs_payload.len());
    ipv6_builder.build();

    let (icmpv6_slice, icmpv6_payload_slice) = ip_payload_slice.split_at_mut(8);
    let icmpv6_header = Icmpv6Header::mut_from_bytes(icmpv6_slice).unwrap();
    icmpv6_header.icmpv6_type = 133;
    icmpv6_header.icmpv6_code = 0;
    icmpv6_header.icmpv6_checksum = 0.into();
    icmpv6_header.rest = [0u8; 4];
    icmpv6_payload_slice.copy_from_slice(&rs_payload);

    let checksum = netsim_packets::icmpv6_checksum(
        &ip_payload_slice[..12],
        guest_link_local,
        all_routers_multicast,
    );
    let mut_icmpv6_header = Icmpv6Header::mut_from_bytes(&mut ip_payload_slice[..8]).unwrap();
    mut_icmpv6_header.icmpv6_checksum.set(checksum);

    let mut eth_rs = vec![0u8; 14 + ip_rs.len()];
    let (eth_header_slice, eth_payload_slice) = eth_rs.split_at_mut(14);
    let eth_frame_rs = EthernetFrame::mut_from_bytes(eth_header_slice).unwrap();
    eth_frame_rs.dst_addr = MacAddr { bytes: [0x33, 0x33, 0x00, 0x00, 0x00, 0x02] };
    eth_frame_rs.src_addr = config.guest_mac;
    eth_frame_rs.ethertype = 0x86DD.into();
    eth_payload_slice.copy_from_slice(&ip_rs);

    let responses_unicast =
        slirp.handle_request(SlirpRequest::Packet(Bytes::copy_from_slice(&eth_rs)));

    assert_eq!(responses_unicast.len(), 1, "Should return exactly one reply packet");
    let response_packet = match &responses_unicast[0] {
        SlirpResponse::Packet(p) => p,
        _ => panic!("Expected Packet response"),
    };

    let (_, eth_payload_reply) = EthernetFrame::parse(response_packet).unwrap();
    let eth_frame_reply = EthernetFrame::parse(response_packet).unwrap().0;
    assert_eq!(eth_frame_reply.dst_addr, config.guest_mac);
    assert_eq!(eth_frame_reply.src_addr, config.gateway_mac);

    let (ipv6_header_reply, ip_payload_reply) = Ipv6Header::parse(eth_payload_reply).unwrap();
    assert_eq!(ipv6_header_reply.next_header, IP_P_ICMPV6);
    assert_eq!(Ipv6Addr::from(ipv6_header_reply.source_addr), config.host_ipv6);
    assert_eq!(Ipv6Addr::from(ipv6_header_reply.dest_addr), guest_link_local);

    let (icmpv6_header_reply, icmpv6_payload_reply) =
        Icmpv6Header::parse(ip_payload_reply).unwrap();
    assert_eq!(icmpv6_header_reply.icmpv6_type, 134);
    assert_eq!(icmpv6_header_reply.icmpv6_code, 0);

    let ra = netsim_packets::RouterAdvertisement::parse(icmpv6_payload_reply).unwrap();
    assert_eq!(ra.cur_hop_limit, 64);
    assert_eq!(ra.router_lifetime.get(), 1800);

    // Verify NDP Options
    let options_slice = &icmpv6_payload_reply[12..];

    // 1. Verify SLLA Option (offset 0..8)
    let slla = netsim_packets::SourceLinkLayerAddressOption::parse(&options_slice[0..8]).unwrap();
    assert_eq!(slla.option_type, 1);
    assert_eq!(slla.length, 1);
    assert_eq!(slla.addr, config.gateway_mac.bytes);

    // 2. Verify PIO Option (offset 8..40)
    let pio = netsim_packets::PrefixInformationOption::parse(&options_slice[8..40]).unwrap();
    assert_eq!(pio.option_type, 3);
    assert_eq!(pio.length, 4);
    assert_eq!(pio.prefix_length, 64);
    let mut expected_prefix = [0u8; 16];
    expected_prefix[..8].copy_from_slice(&config.guest_ipv6.octets()[..8]);
    assert_eq!(pio.prefix, expected_prefix);

    // 3. Verify RDNSS Option (offset 40..64)
    let rdnss = netsim_packets::RdnssOption::parse(&options_slice[40..64]).unwrap();
    assert_eq!(rdnss.option_type, 25);
    assert_eq!(rdnss.length, 3);
    assert_eq!(rdnss.lifetime.get(), 1800);
    assert_eq!(rdnss.dns_servers, [config.host_ipv6.octets()]);

    // --- CASE B: Solicited RS with Unspecified Source IP (::) ---
    let unspecified_ip = Ipv6Addr::UNSPECIFIED;
    let mut ip_rs_unspec = vec![0u8; 40 + 8 + rs_payload.len()];
    let (ip_header_slice, ip_payload_slice) = ip_rs_unspec.split_at_mut(40);
    let mut ipv6_builder =
        Ipv6Builder::new(ip_header_slice, IP_P_ICMPV6, unspecified_ip, all_routers_multicast)
            .unwrap();
    ipv6_builder.hop_limit(255);
    ipv6_builder.payload_len(8 + rs_payload.len());
    ipv6_builder.build();

    let (icmpv6_slice, icmpv6_payload_slice) = ip_payload_slice.split_at_mut(8);
    let icmpv6_header = Icmpv6Header::mut_from_bytes(icmpv6_slice).unwrap();
    icmpv6_header.icmpv6_type = 133;
    icmpv6_header.icmpv6_code = 0;
    icmpv6_header.icmpv6_checksum = 0.into();
    icmpv6_header.rest = [0u8; 4];
    icmpv6_payload_slice.copy_from_slice(&rs_payload);

    let checksum = netsim_packets::icmpv6_checksum(
        &ip_payload_slice[..12],
        unspecified_ip,
        all_routers_multicast,
    );
    let mut_icmpv6_header = Icmpv6Header::mut_from_bytes(&mut ip_payload_slice[..8]).unwrap();
    mut_icmpv6_header.icmpv6_checksum.set(checksum);

    let mut eth_rs_unspec = vec![0u8; 14 + ip_rs_unspec.len()];
    let (eth_header_slice, eth_payload_slice) = eth_rs_unspec.split_at_mut(14);
    let eth_frame_rs = EthernetFrame::mut_from_bytes(eth_header_slice).unwrap();
    eth_frame_rs.dst_addr = MacAddr { bytes: [0x33, 0x33, 0x00, 0x00, 0x00, 0x02] };
    eth_frame_rs.src_addr = config.guest_mac;
    eth_frame_rs.ethertype = 0x86DD.into();
    eth_payload_slice.copy_from_slice(&ip_rs_unspec);

    let responses_unspec =
        slirp.handle_request(SlirpRequest::Packet(Bytes::copy_from_slice(&eth_rs_unspec)));

    assert_eq!(responses_unspec.len(), 1, "Should return exactly one reply packet");
    let response_packet_unspec = match &responses_unspec[0] {
        SlirpResponse::Packet(p) => p,
        _ => panic!("Expected Packet response"),
    };

    let (_, eth_payload_reply_unspec) = EthernetFrame::parse(response_packet_unspec).unwrap();
    let eth_frame_reply_unspec = EthernetFrame::parse(response_packet_unspec).unwrap().0;
    assert_eq!(
        eth_frame_reply_unspec.dst_addr,
        MacAddr { bytes: [0x33, 0x33, 0x00, 0x00, 0x00, 0x01] }
    );
    assert_eq!(eth_frame_reply_unspec.src_addr, config.gateway_mac);

    let (ipv6_header_reply_unspec, _) = Ipv6Header::parse(eth_payload_reply_unspec).unwrap();
    assert_eq!(ipv6_header_reply_unspec.next_header, IP_P_ICMPV6);
    assert_eq!(Ipv6Addr::from(ipv6_header_reply_unspec.source_addr), config.host_ipv6);
    assert_eq!(
        Ipv6Addr::from(ipv6_header_reply_unspec.dest_addr),
        Ipv6Addr::new(0xff02, 0, 0, 0, 0, 0, 0, 1)
    );
}

#[test]
fn test_ndp_periodic_router_advertisement() {
    let config = Config::default();
    let clock = Arc::new(Mutex::new(MockClock::new()));
    let mut slirp = Slirp::new_with_clock(config.clone(), Box::new(clock.clone()));

    // 1. Initially, no events should have fired
    let responses = slirp.handle_request(SlirpRequest::Timer);
    assert_eq!(responses.len(), 1);
    let initial_timer = match &responses[0] {
        SlirpResponse::SetTimer(d) => {
            assert!(d.as_secs() <= 600);
            *d
        }
        _ => panic!("Expected SetTimer"),
    };

    // 2. Advance clock to just before initial timer - still no RA
    clock.lock().unwrap().advance(initial_timer - Duration::from_secs(1));
    let responses = slirp.handle_request(SlirpRequest::Timer);
    assert_eq!(responses.len(), 1);

    // 3. Advance clock by 1 more second - periodic RA fires!
    clock.lock().unwrap().advance(Duration::from_secs(1));
    let responses = slirp.handle_request(SlirpRequest::Timer);
    assert_eq!(responses.len(), 2, "Should return periodic RA packet and SetTimer for next event");

    let mut ra_packet = None;
    let mut next_timer = None;
    for resp in &responses {
        match resp {
            SlirpResponse::Packet(p) => {
                ra_packet = Some(p);
            }
            SlirpResponse::SetTimer(d) => {
                next_timer = Some(*d);
            }
            _ => panic!("Unexpected response type"),
        }
    }
    let ra_packet = ra_packet.expect("Should have sent periodic RA");
    let next_timer = next_timer.expect("Should have rescheduled timer");
    assert!(next_timer.as_secs() <= 600, "Next timer should be scheduled in 600s");

    let (_, eth_payload_reply) = EthernetFrame::parse(ra_packet).unwrap();
    let eth_frame_reply = EthernetFrame::parse(ra_packet).unwrap().0;
    assert_eq!(eth_frame_reply.dst_addr, MacAddr { bytes: [0x33, 0x33, 0x00, 0x00, 0x00, 0x01] });
    assert_eq!(eth_frame_reply.src_addr, config.gateway_mac);

    let (ipv6_header_reply, ip_payload_reply) = Ipv6Header::parse(eth_payload_reply).unwrap();
    assert_eq!(ipv6_header_reply.next_header, IP_P_ICMPV6);
    assert_eq!(Ipv6Addr::from(ipv6_header_reply.source_addr), config.host_ipv6);
    assert_eq!(
        Ipv6Addr::from(ipv6_header_reply.dest_addr),
        Ipv6Addr::new(0xff02, 0, 0, 0, 0, 0, 0, 1)
    );

    let (icmpv6_header_reply, icmpv6_payload_reply) =
        Icmpv6Header::parse(ip_payload_reply).unwrap();
    assert_eq!(icmpv6_header_reply.icmpv6_type, 134);

    let ra = netsim_packets::RouterAdvertisement::parse(icmpv6_payload_reply).unwrap();
    assert_eq!(ra.cur_hop_limit, 64);
    assert_eq!(ra.router_lifetime.get(), 1800);

    // Verify NDP Options
    let options_slice = &icmpv6_payload_reply[12..];

    // 1. Verify SLLA Option (offset 0..8)
    let slla = netsim_packets::SourceLinkLayerAddressOption::parse(&options_slice[0..8]).unwrap();
    assert_eq!(slla.option_type, 1);
    assert_eq!(slla.length, 1);
    assert_eq!(slla.addr, config.gateway_mac.bytes);

    // 2. Verify PIO Option (offset 8..40)
    let pio = netsim_packets::PrefixInformationOption::parse(&options_slice[8..40]).unwrap();
    assert_eq!(pio.option_type, 3);
    assert_eq!(pio.length, 4);
    assert_eq!(pio.prefix_length, 64);
    let mut expected_prefix = [0u8; 16];
    expected_prefix[..8].copy_from_slice(&config.guest_ipv6.octets()[..8]);
    assert_eq!(pio.prefix, expected_prefix);

    // 3. Verify RDNSS Option (offset 40..64)
    let rdnss = netsim_packets::RdnssOption::parse(&options_slice[40..64]).unwrap();
    assert_eq!(rdnss.option_type, 25);
    assert_eq!(rdnss.length, 3);
    assert_eq!(rdnss.lifetime.get(), 1800);
    assert_eq!(rdnss.dns_servers, [config.host_ipv6.octets()]);

    // 4. Advance clock by next_timer duration - second periodic RA fires!
    clock.lock().unwrap().advance(next_timer);
    let responses = slirp.handle_request(SlirpRequest::Timer);
    assert_eq!(responses.len(), 2, "Should fire second periodic RA");
}

#[test]
fn test_icmpv6_echo_integration() {
    let config = Config::default();
    let mut slirp = Slirp::new(config.clone());

    let payload = b"ping pong v6";
    let icmpv6_header_len = 8;
    let total_icmpv6_len = icmpv6_header_len + payload.len();
    let total_ipv6_len = 40 + total_icmpv6_len;
    let total_len = 14 + total_ipv6_len;

    let mut eth_packet = vec![0u8; total_len];

    // 1. Fill Ethernet Header
    let (eth_slice, eth_payload) = eth_packet.split_at_mut(14);
    let eth_frame = EthernetFrame::mut_from_bytes(eth_slice).unwrap();
    eth_frame.dst_addr = config.gateway_mac;
    eth_frame.src_addr = config.guest_mac;
    eth_frame.ethertype = 0x86DD.into(); // IPv6

    // 2. Build IPv6 Header
    let (ipv6_slice, ipv6_payload) = eth_payload.split_at_mut(40);
    let mut ipv6_builder =
        Ipv6Builder::new(ipv6_slice, IP_P_ICMPV6, config.guest_ipv6, config.host_ipv6).unwrap();
    ipv6_builder.hop_limit(64);

    // 3. Build ICMPv6 Header
    let (icmpv6_slice, icmpv6_payload_slice) = ipv6_payload.split_at_mut(icmpv6_header_len);
    let icmpv6_header = Icmpv6Header::mut_from_bytes(icmpv6_slice).unwrap();
    icmpv6_header.icmpv6_type = netsim_packets::Icmpv6Type::EchoRequest as u8;
    icmpv6_header.icmpv6_code = 0;
    icmpv6_header.icmpv6_checksum = 0.into();
    icmpv6_header.set_echo_fields(0xabcd, 0x1234);

    // Copy payload
    icmpv6_payload_slice[..payload.len()].copy_from_slice(payload);

    // 4. Calculate ICMPv6 Checksum
    let checksum = netsim_packets::icmpv6_checksum(
        &ipv6_payload[..total_icmpv6_len],
        config.guest_ipv6,
        config.host_ipv6,
    );
    let mut_icmpv6_header =
        Icmpv6Header::mut_from_bytes(&mut ipv6_payload[..icmpv6_header_len]).unwrap();
    mut_icmpv6_header.icmpv6_checksum.set(checksum);

    // 5. Finalize IPv6 Header
    ipv6_builder.payload_len(total_icmpv6_len);
    ipv6_builder.build();

    // Send the packet
    let responses = slirp.handle_request(SlirpRequest::Packet(Bytes::copy_from_slice(&eth_packet)));

    assert_eq!(responses.len(), 1, "Should return exactly one reply packet");
    let reply_packet = match &responses[0] {
        SlirpResponse::Packet(p) => p,
        _ => panic!("Expected Packet response"),
    };

    // Verify Reply
    let (_, eth_payload_reply) = EthernetFrame::parse(reply_packet).unwrap();
    let eth_frame_reply = EthernetFrame::parse(reply_packet).unwrap().0;
    assert_eq!(eth_frame_reply.dst_addr, config.guest_mac);
    assert_eq!(eth_frame_reply.src_addr, config.gateway_mac);

    let (ipv6_header_reply, ip_payload_reply) = Ipv6Header::parse(eth_payload_reply).unwrap();
    assert_eq!(ipv6_header_reply.next_header, IP_P_ICMPV6);
    assert_eq!(Ipv6Addr::from(ipv6_header_reply.source_addr), config.host_ipv6);
    assert_eq!(Ipv6Addr::from(ipv6_header_reply.dest_addr), config.guest_ipv6);

    let (icmpv6_header_reply, icmpv6_payload_reply) =
        Icmpv6Header::parse(ip_payload_reply).unwrap();
    assert_eq!(icmpv6_header_reply.icmpv6_type, netsim_packets::Icmpv6Type::EchoReply as u8);
    assert_eq!(icmpv6_header_reply.icmpv6_code, 0);
    assert_eq!(icmpv6_header_reply.echo_fields(), Some((0xabcd, 0x1234)));
    assert_eq!(icmpv6_payload_reply, payload);
}

#[test]
fn test_icmpv6_port_unreachable_integration() {
    let config = Config::default();
    let mut slirp = Slirp::new(config.clone());

    // Send a UDP packet to an unserved gateway port
    // (port 0 is reserved and never translated).
    let payload = b"hello closed port";
    let udp_header_len = 8;
    let total_udp_len = udp_header_len + payload.len();
    let total_ipv6_len = 40 + total_udp_len;
    let total_len = 14 + total_ipv6_len;

    let mut eth_packet = vec![0u8; total_len];

    // 1. Fill Ethernet Header
    let (eth_slice, eth_payload) = eth_packet.split_at_mut(14);
    let eth_frame = EthernetFrame::mut_from_bytes(eth_slice).unwrap();
    eth_frame.dst_addr = config.gateway_mac;
    eth_frame.src_addr = config.guest_mac;
    eth_frame.ethertype = 0x86DD.into(); // IPv6

    // 2. Build IPv6 Header
    let (ipv6_slice, ipv6_payload) = eth_payload.split_at_mut(40);
    let mut ipv6_builder =
        Ipv6Builder::new(ipv6_slice, IP_P_UDP, config.guest_ipv6, config.host_ipv6).unwrap();
    ipv6_builder.hop_limit(64);

    // 3. Build UDP Header
    let mut udp_builder = UdpPacketBuilder::new(
        &mut ipv6_payload[..total_udp_len],
        12345, // src port
        0,     // dst port (reserved!)
    )
    .unwrap();
    udp_builder.payload_mut().copy_from_slice(payload);
    udp_builder.build();

    // Finalize IPv6 Header
    ipv6_builder.payload_len(total_udp_len);
    ipv6_builder.build();

    // Send the packet
    let responses = slirp.handle_request(SlirpRequest::Packet(Bytes::copy_from_slice(&eth_packet)));

    assert_eq!(responses.len(), 1, "Should return exactly one reply packet");
    let reply_packet = match &responses[0] {
        SlirpResponse::Packet(p) => p,
        _ => panic!("Expected Packet response"),
    };

    // Verify Reply is ICMPv6 Port Unreachable
    let (_, eth_payload_reply) = EthernetFrame::parse(reply_packet).unwrap();
    let eth_frame_reply = EthernetFrame::parse(reply_packet).unwrap().0;
    assert_eq!(eth_frame_reply.dst_addr, config.guest_mac);
    // ICMPv6 error uses a generic QEMU/Slirp gateway MAC [0x52, 0x54, 0x00, 0x12,
    // 0x34, 0x56]
    assert_eq!(eth_frame_reply.src_addr, MacAddr { bytes: [0x52, 0x54, 0x00, 0x12, 0x34, 0x56] });

    let (ipv6_header_reply, ip_payload_reply) = Ipv6Header::parse(eth_payload_reply).unwrap();
    assert_eq!(ipv6_header_reply.next_header, IP_P_ICMPV6);
    assert_eq!(Ipv6Addr::from(ipv6_header_reply.source_addr), config.host_ipv6);
    assert_eq!(Ipv6Addr::from(ipv6_header_reply.dest_addr), config.guest_ipv6);

    let (icmpv6_header_reply, icmpv6_payload_reply) =
        Icmpv6Header::parse(ip_payload_reply).unwrap();
    assert_eq!(
        icmpv6_header_reply.icmpv6_type,
        netsim_packets::Icmpv6Type::DestinationUnreachable as u8
    );
    assert_eq!(
        icmpv6_header_reply.icmpv6_code,
        netsim_packets::Icmpv6UnreachableCode::PortUnreachable as u8
    );

    // Verify offending packet is copied back in payload (at least the IPv6 header!)
    let l3_offending_sent = &eth_packet[14..];
    assert_eq!(&icmpv6_payload_reply[..l3_offending_sent.len()], l3_offending_sent);
}

#[test]
fn test_dhcpv6_integration() {
    let config = Config {
        dns_search: Some(vec!["example.com".to_string(), "test.org".to_string()]),
        ..Default::default()
    };
    let mut slirp = Slirp::new(config.clone());

    // 1. Build a DHCPv6 Information-Request packet
    let transaction_id = [0x99, 0x88, 0x77];
    let client_duid = [0, 3, 0, 1, 0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc];

    let total_dhcpv6_len = 4 + 14; // Header (4) + Option Client ID (14)
    let mut dhcpv6_payload = vec![0u8; total_dhcpv6_len];

    let (hdr_slice, opt_slice) = dhcpv6_payload.split_at_mut(4);
    let hdr = netsim_packets::Dhcpv6Header::mut_from_bytes(hdr_slice).unwrap();
    hdr.msg_type = netsim_packets::MSG_INFORMATION_REQUEST;
    hdr.transaction_id = transaction_id;

    let opt_hdr = netsim_packets::Dhcpv6OptionHeader::mut_from_bytes(&mut opt_slice[..4]).unwrap();
    opt_hdr.code.set(netsim_packets::OPTION_CLIENTID);
    opt_hdr.len.set(10);
    opt_slice[4..14].copy_from_slice(&client_duid);

    // Build Ethernet + IPv6 + UDP wrapper
    let udp_header_len = 8;
    let total_udp_len = udp_header_len + total_dhcpv6_len;
    let total_ipv6_len = 40 + total_udp_len;
    let total_len = 14 + total_ipv6_len;
    let mut eth_packet = vec![0u8; total_len];

    // 1. Fill Ethernet Header
    let (eth_slice, eth_payload) = eth_packet.split_at_mut(14);
    let eth_frame = EthernetFrame::mut_from_bytes(eth_slice).unwrap();
    // DHCPv6 servers multicast to ff02::1:2
    eth_frame.dst_addr = MacAddr { bytes: [0x33, 0x33, 0x00, 0x01, 0x00, 0x02] };
    eth_frame.src_addr = config.guest_mac;
    eth_frame.ethertype = 0x86DD.into(); // IPv6

    // 2. Build IPv6 Header
    let (ipv6_slice, ipv6_payload) = eth_payload.split_at_mut(40);
    let mut ipv6_builder = Ipv6Builder::new(
        ipv6_slice,
        IP_P_UDP,
        config.guest_ipv6,
        Ipv6Addr::new(0xff02, 0, 0, 0, 0, 0, 1, 2), // All_DHCP_Relay_Agents_and_Servers
    )
    .unwrap();
    ipv6_builder.hop_limit(64);

    // 3. Build UDP Header
    let mut udp_builder = UdpPacketBuilder::new(
        &mut ipv6_payload[..total_udp_len],
        netsim_packets::DHCPV6_CLIENT_PORT,
        netsim_packets::DHCPV6_SERVER_PORT,
    )
    .unwrap();
    udp_builder.payload_mut().copy_from_slice(&dhcpv6_payload);
    udp_builder.build();

    ipv6_builder.payload_len(total_udp_len);
    ipv6_builder.build();

    // Send the packet
    let responses = slirp.handle_request(SlirpRequest::Packet(Bytes::copy_from_slice(&eth_packet)));

    assert_eq!(responses.len(), 1, "Should return exactly one reply packet");
    let reply_packet = match &responses[0] {
        SlirpResponse::Packet(p) => p,
        _ => panic!("Expected Packet response"),
    };

    // Parse Reply
    let (_, reply_eth_payload) = EthernetFrame::parse(reply_packet).unwrap();
    let reply_eth_frame = EthernetFrame::parse(reply_packet).unwrap().0;
    assert_eq!(reply_eth_frame.dst_addr, config.guest_mac);
    assert_eq!(reply_eth_frame.src_addr, config.gateway_mac);

    let (reply_ipv6, reply_ip_payload) = Ipv6Header::parse(reply_eth_payload).unwrap();
    assert_eq!(Ipv6Addr::from(reply_ipv6.source_addr), config.host_ipv6);
    assert_eq!(Ipv6Addr::from(reply_ipv6.dest_addr), config.guest_ipv6);

    let (_, reply_udp_payload) = UdpHeader::parse(reply_ip_payload).unwrap();
    let reply_udp = UdpHeader::parse(reply_ip_payload).unwrap().0;
    assert_eq!(reply_udp.source_port.get(), netsim_packets::DHCPV6_SERVER_PORT);
    assert_eq!(reply_udp.dest_port.get(), netsim_packets::DHCPV6_CLIENT_PORT);

    let (reply_dhcpv6, reply_options) =
        netsim_packets::Dhcpv6Header::parse(reply_udp_payload).unwrap();
    assert_eq!(reply_dhcpv6.msg_type, netsim_packets::MSG_REPLY);
    assert_eq!(reply_dhcpv6.transaction_id, transaction_id);

    // Parse Reply Options
    let mut has_server_id = false;
    let mut has_client_id = false;
    let mut has_dns_servers = false;
    let mut has_domain_list = false;

    let iter = netsim_packets::Dhcpv6OptionIterator::new(reply_options);
    for (code, val) in iter {
        match code {
            netsim_packets::OPTION_SERVERID => {
                has_server_id = true;
                assert_eq!(val.len(), 10);
                assert_eq!(val[..2], 3u16.to_be_bytes()); // Type DUID-LL (3)
                assert_eq!(val[2..4], 1u16.to_be_bytes()); // HW Type Ethernet (1)
                assert_eq!(val[4..10], config.gateway_mac.bytes);
            }
            netsim_packets::OPTION_CLIENTID => {
                has_client_id = true;
                assert_eq!(val, &client_duid[..]);
            }
            netsim_packets::OPTION_DNS_SERVERS => {
                has_dns_servers = true;
                assert_eq!(val.len(), 16);
                assert_eq!(val, config.host_ipv6.octets());
            }
            netsim_packets::OPTION_DOMAIN_LIST => {
                has_domain_list = true;
                let expected_bytes =
                    crate::dns::encode_dns_search_list(config.dns_search.as_ref().unwrap());
                assert_eq!(val, &expected_bytes[..]);
            }
            _ => panic!("Unexpected option code: {code}"),
        }
    }

    assert!(has_server_id, "Missing Server ID option");
    assert!(has_client_id, "Missing Client ID option");
    assert!(has_dns_servers, "Missing DNS Servers option");
    assert!(has_domain_list, "Missing Domain List option");
}

#[test]
fn test_tcp_guestfwd_integration() {
    let virtual_addr: SocketAddr = "10.0.2.100:80".parse().unwrap();
    let host_addr: SocketAddr = "127.0.0.1:8080".parse().unwrap();

    let config = Config {
        guestfwd: vec![crate::GuestFwdRule { virtual_addr, host_addr }],
        ..Config::default()
    };

    let mut slirp = Slirp::new(config.clone());

    // 1. Guest sends TCP SYN to virtual IP:port (10.0.2.100:80)
    let guest_port = 12345;
    let guest_iss = 1000;

    let mut tcp_syn = vec![0u8; 20];
    let mut tcp_builder = TcpBuilder::new(
        &mut tcp_syn,
        config.guest_ipv4,
        match virtual_addr.ip() {
            IpAddr::V4(ip) => ip,
            _ => panic!("Expected IPv4"),
        },
    )
    .unwrap();
    tcp_builder
        .source_port(guest_port)
        .dest_port(virtual_addr.port())
        .sequence_num(guest_iss)
        .flags(netsim_packets::TCP_FLAG_SYN)
        .window_size(8192);
    tcp_builder.build();

    let mut ip_syn = vec![0u8; 20 + tcp_syn.len()];
    let (ip_header_slice, ip_payload_slice) = ip_syn.split_at_mut(20);
    let mut ipv4_builder = Ipv4Builder::new(
        ip_header_slice,
        IP_P_TCP,
        config.guest_ipv4,
        match virtual_addr.ip() {
            IpAddr::V4(ip) => ip,
            _ => panic!("Expected IPv4"),
        },
    )
    .unwrap();
    ipv4_builder.payload_len(tcp_syn.len());
    ipv4_builder.build();
    ip_payload_slice.copy_from_slice(&tcp_syn);

    let mut eth_syn = vec![0u8; 14 + ip_syn.len()];
    let (eth_header_slice, eth_payload_slice) = eth_syn.split_at_mut(14);
    let eth_frame_syn = EthernetFrame::mut_from_bytes(eth_header_slice).unwrap();
    eth_frame_syn.dst_addr = config.gateway_mac;
    eth_frame_syn.src_addr = config.guest_mac;
    eth_frame_syn.ethertype = 0x0800.into();
    eth_payload_slice.copy_from_slice(&ip_syn);

    let responses = slirp.handle_request(SlirpRequest::Packet(Bytes::copy_from_slice(&eth_syn)));

    // Slirp replies with EstablishConnection (to host_addr!) and SYN-ACK packet
    assert_eq!(responses.len(), 2);

    let mut has_establish = false;
    let mut has_packet = false;

    for resp in responses {
        match resp {
            SlirpResponse::EstablishConnection(_, ConnectionArgs::Tcp(args)) => {
                has_establish = true;
                assert_eq!(args.destination, host_addr); // Must be host_addr!
                assert_eq!(args.guest_ip, IpAddr::V4(config.guest_ipv4));
                assert_eq!(args.guest_port, guest_port);
            }
            SlirpResponse::Packet(p) => {
                has_packet = true;
                // Verify SYN-ACK packet source IP is still the virtual IP!
                let (_, eth_payload) = EthernetFrame::parse(&p).unwrap();
                let (ip_header, _) = Ipv4Header::parse(eth_payload).unwrap();
                assert_eq!(
                    Ipv4Addr::from(ip_header.source_addr),
                    match virtual_addr.ip() {
                        IpAddr::V4(ip) => ip,
                        _ => panic!("Expected IPv4"),
                    }
                );
            }
            _ => panic!("Unexpected response: {resp:?}"),
        }
    }

    assert!(has_establish, "Missing EstablishConnection");
    assert!(has_packet, "Missing Packet");
}

#[test]
fn test_udp_guestfwd_integration() {
    let virtual_addr: SocketAddr = "10.0.2.100:53".parse().unwrap();
    let host_addr: SocketAddr = "127.0.0.1:1053".parse().unwrap();

    let config = Config {
        guestfwd: vec![crate::GuestFwdRule { virtual_addr, host_addr }],
        ..Config::default()
    };

    let mut slirp = Slirp::new(config.clone());

    // 1. Guest sends UDP packet to virtual IP:port (10.0.2.100:53)
    let guest_port = 12345;
    let payload = b"test_udp_payload";

    let mut udp_data = vec![0u8; 8 + payload.len()];
    let mut udp_builder =
        UdpPacketBuilder::new(&mut udp_data, guest_port, virtual_addr.port()).unwrap();
    udp_builder.payload_mut()[..payload.len()].copy_from_slice(payload);
    udp_builder.build();

    let mut ip_data = vec![0u8; 20 + udp_data.len()];
    let (ip_header_slice, ip_payload_slice) = ip_data.split_at_mut(20);
    let mut ipv4_builder = Ipv4Builder::new(
        ip_header_slice,
        IP_P_UDP,
        config.guest_ipv4,
        match virtual_addr.ip() {
            IpAddr::V4(ip) => ip,
            _ => panic!("Expected IPv4"),
        },
    )
    .unwrap();
    ipv4_builder.payload_len(udp_data.len());
    ipv4_builder.build();
    ip_payload_slice.copy_from_slice(&udp_data);

    let mut eth_data = vec![0u8; 14 + ip_data.len()];
    let (eth_header_slice, eth_payload_slice) = eth_data.split_at_mut(14);
    let eth_frame = EthernetFrame::mut_from_bytes(eth_header_slice).unwrap();
    eth_frame.dst_addr = config.gateway_mac;
    eth_frame.src_addr = config.guest_mac;
    eth_frame.ethertype = 0x0800.into();
    eth_payload_slice.copy_from_slice(&ip_data);

    let responses = slirp.handle_request(SlirpRequest::Packet(Bytes::copy_from_slice(&eth_data)));

    // Slirp replies with EstablishConnection (to host_addr!) and WriteToConnection
    assert_eq!(responses.len(), 2);

    let mut has_establish = false;
    let mut has_write = false;

    for resp in responses {
        match resp {
            SlirpResponse::EstablishConnection(_, ConnectionArgs::Udp(args)) => {
                has_establish = true;
                assert_eq!(args.destination, host_addr); // Must be host_addr!
                assert_eq!(args.guest_ip, IpAddr::V4(config.guest_ipv4));
                assert_eq!(args.guest_port, guest_port);
            }
            SlirpResponse::WriteToConnection(_, p) => {
                has_write = true;
                assert_eq!(p.as_ref(), payload);
            }
            _ => panic!("Unexpected response: {resp:?}"),
        }
    }

    assert!(has_establish, "Missing EstablishConnection");
    assert!(has_write, "Missing WriteToConnection");
}

#[test]
fn test_tcp_connect_failure_sends_icmp_unreachable() {
    let config = Config::default();
    let mut slirp = Slirp::new(config.clone());

    // 1. Guest sends TCP SYN to some remote IP (e.g. 93.184.216.34:80)
    let remote_ip = Ipv4Addr::new(93, 184, 216, 34);
    let remote_port = 80;
    let guest_port = 12345;

    // Create TCP SYN packet
    let mut tcp_data = vec![0u8; 20];
    let mut tcp_builder = TcpBuilder::new(&mut tcp_data, config.guest_ipv4, remote_ip).unwrap();
    tcp_builder
        .source_port(guest_port)
        .dest_port(remote_port)
        .sequence_num(1000)
        .flags(netsim_packets::TCP_FLAG_SYN)
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
    eth_frame.ethertype = 0x0800.into();
    eth_payload_slice.copy_from_slice(&ip_data);

    // Send SYN to Slirp
    let responses = slirp.handle_request(SlirpRequest::Packet(Bytes::copy_from_slice(&eth_data)));

    // Verify it returns EstablishConnection and a SYN-ACK packet
    assert_eq!(responses.len(), 2);
    let mut conn_id = None;
    for resp in &responses {
        if let SlirpResponse::EstablishConnection(id, ConnectionArgs::Tcp(_)) = resp {
            conn_id = Some(*id);
        }
    }
    let conn_id = conn_id.expect("Should have returned EstablishConnection");

    // 2. Simulate connection failure by sending ConnectionClosed from TokioHost
    let replies = slirp.handle_request(SlirpRequest::ConnectionClosed(conn_id));

    // Verify it returns an ICMP Host Unreachable packet!
    assert_eq!(replies.len(), 1);
    let reply_packet = match &replies[0] {
        SlirpResponse::Packet(p) => p,
        _ => panic!("Expected a packet response, got {replies:?}"),
    };

    // Parse reply packet
    let (eth_frame, ip_payload) = EthernetFrame::parse(reply_packet).unwrap();
    assert_eq!(eth_frame.src_addr, MacAddr { bytes: [0x52, 0x54, 0x00, 0x12, 0x34, 0x56] }); // Default ICMP gateway MAC
    assert_eq!(eth_frame.dst_addr, config.guest_mac);

    let (ip_header, icmp_payload) = Ipv4Header::parse(ip_payload).unwrap();
    assert_eq!(ip_header.protocol, 1); // ICMP
    assert_eq!(Ipv4Addr::from(ip_header.dest_addr), config.guest_ipv4);
    assert_eq!(Ipv4Addr::from(ip_header.source_addr), config.host_ipv4); // Gateway IP

    let (icmp_header, icmp_data) = IcmpHeader::parse(icmp_payload).unwrap();
    assert_eq!(icmp_header.icmp_type, netsim_packets::IcmpType::DestUnreachable as u8);
    assert_eq!(icmp_header.icmp_code, netsim_packets::UnreachableCode::HostUnreachable as u8);

    // Verify the offending packet is copied into the ICMP payload!
    // The payload of DestUnreachable contains 4 bytes unused, then the IP header +
    // 8 bytes TCP header of the offending packet!
    let offending_ip_header_slice = &icmp_data[..20];
    let offending_ip_header = Ipv4Header::parse(offending_ip_header_slice).unwrap().0;
    assert_eq!(Ipv4Addr::from(offending_ip_header.source_addr), config.guest_ipv4);
    assert_eq!(Ipv4Addr::from(offending_ip_header.dest_addr), remote_ip);

    let offending_tcp_header_slice = &icmp_data[20..28];
    let (offending_tcp_header, _) = UdpHeader::parse(offending_tcp_header_slice).unwrap(); // UdpHeader parse is identical to first 8 bytes of TCP (ports)
    assert_eq!(offending_tcp_header.source_port.get(), guest_port);
    assert_eq!(offending_tcp_header.dest_port.get(), remote_port);
}

#[test]
fn test_dns_over_tcp_redirection_integration() {
    let config = Config::default();
    let mut slirp = Slirp::new(config.clone());

    // 1. Guest sends TCP SYN to gateway IP on port 53 (DNS)
    let guest_port = 12345;
    let guest_iss = 1000;

    let mut tcp_syn = vec![0u8; 20];
    let mut tcp_builder =
        TcpBuilder::new(&mut tcp_syn, config.guest_ipv4, config.host_ipv4).unwrap();
    tcp_builder
        .source_port(guest_port)
        .dest_port(53)
        .sequence_num(guest_iss)
        .flags(netsim_packets::TCP_FLAG_SYN)
        .window_size(8192);
    tcp_builder.build();

    let mut ip_syn = vec![0u8; 20 + tcp_syn.len()];
    let (ip_header_slice, ip_payload_slice) = ip_syn.split_at_mut(20);
    let mut ipv4_builder =
        Ipv4Builder::new(ip_header_slice, IP_P_TCP, config.guest_ipv4, config.host_ipv4).unwrap();
    ipv4_builder.payload_len(tcp_syn.len());
    ipv4_builder.build();
    ip_payload_slice.copy_from_slice(&tcp_syn);

    let mut eth_syn = vec![0u8; 14 + ip_syn.len()];
    let (eth_header_slice, eth_payload_slice) = eth_syn.split_at_mut(14);
    let eth_frame_syn = EthernetFrame::mut_from_bytes(eth_header_slice).unwrap();
    eth_frame_syn.dst_addr = config.gateway_mac;
    eth_frame_syn.src_addr = config.guest_mac;
    eth_frame_syn.ethertype = 0x0800.into();
    eth_payload_slice.copy_from_slice(&ip_syn);

    let responses = slirp.handle_request(SlirpRequest::Packet(Bytes::copy_from_slice(&eth_syn)));

    // Slirp replies with EstablishConnection (redirected to 8.8.8.8:53!) and
    // SYN-ACK packet
    assert_eq!(responses.len(), 2);

    let mut has_establish = false;
    let mut has_packet = false;

    let expected_dns_host_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8)), 53);

    for resp in responses {
        match resp {
            SlirpResponse::EstablishConnection(_, ConnectionArgs::Tcp(args)) => {
                has_establish = true;
                assert_eq!(args.destination, expected_dns_host_addr); // Redirected to 8.8.8.8:53!
                assert_eq!(args.guest_ip, IpAddr::V4(config.guest_ipv4));
                assert_eq!(args.guest_port, guest_port);
            }
            SlirpResponse::Packet(p) => {
                has_packet = true;
                // Verify SYN-ACK packet source IP is still the gateway IP (so the guest is
                // fooled!)
                let (_, eth_payload) = EthernetFrame::parse(&p).unwrap();
                let (ip_header, _) = Ipv4Header::parse(eth_payload).unwrap();
                assert_eq!(Ipv4Addr::from(ip_header.source_addr), config.host_ipv4);
            }
            _ => panic!("Unexpected response: {resp:?}"),
        }
    }

    assert!(has_establish, "Missing EstablishConnection");
    assert!(has_packet, "Missing Packet");
}

fn build_ipv4_tcp_syn_frame(
    config: &Config,
    dst_ip: Ipv4Addr,
    src_port: u16,
    dst_port: u16,
    seq: u32,
) -> Bytes {
    let mut tcp_syn = vec![0u8; 20];
    let mut tcp_builder = TcpBuilder::new(&mut tcp_syn, config.guest_ipv4, dst_ip).unwrap();
    tcp_builder
        .source_port(src_port)
        .dest_port(dst_port)
        .sequence_num(seq)
        .ack_num(0)
        .flags(netsim_packets::TCP_FLAG_SYN)
        .window_size(8192);
    tcp_builder.build();

    let mut ip_syn = vec![0u8; 20 + tcp_syn.len()];
    let (ip_header_slice, ip_payload_slice) = ip_syn.split_at_mut(20);
    let mut ipv4_builder =
        Ipv4Builder::new(ip_header_slice, IP_P_TCP, config.guest_ipv4, dst_ip).unwrap();
    ipv4_builder.payload_len(tcp_syn.len());
    ipv4_builder.build();
    ip_payload_slice.copy_from_slice(&tcp_syn);

    let mut eth_syn = vec![0u8; 14 + ip_syn.len()];
    let (eth_header_slice, eth_payload_slice) = eth_syn.split_at_mut(14);
    let eth_frame_syn = EthernetFrame::mut_from_bytes(eth_header_slice).unwrap();
    eth_frame_syn.dst_addr = config.gateway_mac;
    eth_frame_syn.src_addr = config.guest_mac;
    eth_frame_syn.ethertype = ether_type::IPV4.into();
    eth_payload_slice.copy_from_slice(&ip_syn);

    Bytes::from(eth_syn)
}

const ETH_HEADER_LEN: usize = 14;
const IPV4_HEADER_LEN: usize = 20;
const IPV6_HEADER_LEN: usize = 40;
const TCP_HEADER_LEN: usize = 20;
const UDP_HEADER_LEN: usize = 8;

fn build_ipv6_tcp_syn_frame(
    config: &Config,
    dst_ip: Ipv6Addr,
    src_port: u16,
    dst_port: u16,
    seq: u32,
) -> Bytes {
    let total_ipv6_len = IPV6_HEADER_LEN + TCP_HEADER_LEN;
    let total_len = ETH_HEADER_LEN + total_ipv6_len;

    let mut eth_packet = vec![0u8; total_len];
    let (eth_slice, eth_payload) = eth_packet.split_at_mut(ETH_HEADER_LEN);
    let eth_frame = EthernetFrame::mut_from_bytes(eth_slice).unwrap();
    eth_frame.dst_addr = config.gateway_mac;
    eth_frame.src_addr = config.guest_mac;
    eth_frame.ethertype = ether_type::IPV6.into();

    let (ipv6_slice, tcp_slice) = eth_payload.split_at_mut(IPV6_HEADER_LEN);
    let mut ipv6_builder =
        Ipv6Builder::new(ipv6_slice, IP_P_TCP, config.guest_ipv6, dst_ip).unwrap();
    ipv6_builder.payload_len(TCP_HEADER_LEN);
    ipv6_builder.build();

    let mut tcp_builder = TcpBuilder::new_v6(tcp_slice, config.guest_ipv6, dst_ip).unwrap();
    tcp_builder
        .source_port(src_port)
        .dest_port(dst_port)
        .sequence_num(seq)
        .ack_num(0)
        .flags(netsim_packets::TCP_FLAG_SYN)
        .window_size(8192);
    tcp_builder.build();

    Bytes::from(eth_packet)
}

struct World {
    config: Config,
    slirp: Slirp,
    last_responses: Vec<SlirpResponse>,
}

impl World {
    fn new() -> Self {
        let config = Config::default();
        let slirp = Slirp::new(config.clone());
        Self { config, slirp, last_responses: Vec::new() }
    }

    fn given_guestfwd_rule(&mut self, virtual_addr: SocketAddr, host_addr: SocketAddr) {
        self.config.guestfwd = vec![crate::GuestFwdRule { virtual_addr, host_addr }];
        self.slirp = Slirp::new(self.config.clone());
    }

    fn given_no_dns_servers(&mut self) {
        self.config.dns_servers = Vec::new();
        self.slirp = Slirp::new(self.config.clone());
    }

    fn when_guest_sends_ipv4_tcp_syn(&mut self, src_port: u16, dst_port: u16) {
        self.when_guest_sends_ipv4_tcp_syn_to(self.config.host_ipv4, src_port, dst_port);
    }

    fn when_guest_sends_ipv4_tcp_syn_to(&mut self, dst_ip: Ipv4Addr, src_port: u16, dst_port: u16) {
        let eth_syn = build_ipv4_tcp_syn_frame(&self.config, dst_ip, src_port, dst_port, 1000);
        self.last_responses = self.slirp.handle_request(SlirpRequest::Packet(eth_syn));
    }

    fn when_guest_sends_ipv6_tcp_syn_to(&mut self, dst_ip: Ipv6Addr, src_port: u16, dst_port: u16) {
        let eth_syn = build_ipv6_tcp_syn_frame(&self.config, dst_ip, src_port, dst_port, 1000);
        self.last_responses = self.slirp.handle_request(SlirpRequest::Packet(eth_syn));
    }

    fn then_tcp_connection_established(
        &self,
        expected_dest: SocketAddr,
        expected_guest_port: u16,
    ) -> u64 {
        let mut conn_id = None;
        for resp in &self.last_responses {
            if let SlirpResponse::EstablishConnection(id, ConnectionArgs::Tcp(args)) = resp {
                assert_eq!(args.destination, expected_dest);
                match expected_dest.ip() {
                    IpAddr::V4(_) => {
                        assert_eq!(args.guest_ip, IpAddr::V4(self.config.guest_ipv4));
                    }
                    IpAddr::V6(_) => {
                        assert_eq!(args.guest_ip, IpAddr::V6(self.config.guest_ipv6));
                    }
                }
                assert_eq!(args.guest_port, expected_guest_port);
                conn_id = Some(*id);
            }
        }
        let conn_id =
            conn_id.unwrap_or_else(|| panic!("Expected EstablishConnection to {expected_dest}"));
        assert!(conn_id < (1 << 63), "TCP connection ID should have MSB clear");
        conn_id
    }

    fn then_guest_receives_ipv4_tcp_syn_ack(&self, expected_src_port: u16, expected_dst_port: u16) {
        self.then_guest_receives_ipv4_tcp_syn_ack_from(
            self.config.host_ipv4,
            expected_src_port,
            expected_dst_port,
        );
    }

    fn then_guest_receives_ipv4_tcp_syn_ack_from(
        &self,
        expected_src_ip: Ipv4Addr,
        expected_src_port: u16,
        expected_dst_port: u16,
    ) {
        let has_syn_ack = self.last_responses.iter().any(|resp| {
            if let SlirpResponse::Packet(p) = resp {
                let (_, eth_payload) = EthernetFrame::parse(p).unwrap();
                let (ip_header, ip_payload) = Ipv4Header::parse(eth_payload).unwrap();
                assert_eq!(Ipv4Addr::from(ip_header.source_addr), expected_src_ip);
                let (tcp_header, _) = TcpHeader::parse(ip_payload).unwrap();
                assert_eq!(tcp_header.source_port.get(), expected_src_port);
                assert_eq!(tcp_header.dest_port.get(), expected_dst_port);
                assert!(tcp_header.syn() && tcp_header.ack());
                true
            } else {
                false
            }
        });
        assert!(
            has_syn_ack,
            "Expected TCP SYN-ACK packet from {expected_src_ip}:{expected_src_port}"
        );
    }

    fn when_guest_sends_ipv4_udp(&mut self, src_port: u16, dst_port: u16, payload: &[u8]) {
        self.when_guest_sends_ipv4_udp_to(self.config.host_ipv4, src_port, dst_port, payload);
    }

    fn when_guest_sends_ipv4_udp_to(
        &mut self,
        dst_ip: Ipv4Addr,
        src_port: u16,
        dst_port: u16,
        payload: &[u8],
    ) {
        let mut udp_packet = vec![0u8; UDP_HEADER_LEN + payload.len()];
        let mut udp_builder = UdpPacketBuilder::new(&mut udp_packet, src_port, dst_port).unwrap();
        udp_builder.payload_mut().copy_from_slice(payload);
        udp_builder.build();

        let mut ipv4_packet = vec![0u8; IPV4_HEADER_LEN + udp_packet.len()];
        let (ipv4_header_slice, ipv4_payload_slice) = ipv4_packet.split_at_mut(IPV4_HEADER_LEN);
        let mut ipv4_builder =
            Ipv4Builder::new(ipv4_header_slice, IP_P_UDP, self.config.guest_ipv4, dst_ip).unwrap();
        ipv4_builder.payload_len(udp_packet.len());
        ipv4_builder.build();
        ipv4_payload_slice.copy_from_slice(&udp_packet);

        let mut eth_packet = vec![0u8; ETH_HEADER_LEN + ipv4_packet.len()];
        let (eth_header, eth_payload) = eth_packet.split_at_mut(ETH_HEADER_LEN);
        let eth_frame = EthernetFrame::mut_from_bytes(eth_header).unwrap();
        eth_frame.dst_addr = self.config.gateway_mac;
        eth_frame.src_addr = self.config.guest_mac;
        eth_frame.ethertype = ether_type::IPV4.into();
        eth_payload.copy_from_slice(&ipv4_packet);

        self.last_responses =
            self.slirp.handle_request(SlirpRequest::Packet(Bytes::copy_from_slice(&eth_packet)));
    }

    fn when_guest_sends_ipv6_udp(&mut self, src_port: u16, dst_port: u16, payload: &[u8]) {
        let total_udp_len = UDP_HEADER_LEN + payload.len();
        let total_ipv6_len = IPV6_HEADER_LEN + total_udp_len;
        let total_len = ETH_HEADER_LEN + total_ipv6_len;

        let mut eth_packet = vec![0u8; total_len];
        let (eth_slice, eth_payload) = eth_packet.split_at_mut(ETH_HEADER_LEN);
        let eth_frame = EthernetFrame::mut_from_bytes(eth_slice).unwrap();
        eth_frame.dst_addr = self.config.gateway_mac;
        eth_frame.src_addr = self.config.guest_mac;
        eth_frame.ethertype = ether_type::IPV6.into();

        let (ipv6_slice, ipv6_payload) = eth_payload.split_at_mut(IPV6_HEADER_LEN);
        let mut ipv6_builder =
            Ipv6Builder::new(ipv6_slice, IP_P_UDP, self.config.guest_ipv6, self.config.host_ipv6)
                .unwrap();
        ipv6_builder.hop_limit(64);

        let mut udp_builder =
            UdpPacketBuilder::new(&mut ipv6_payload[..total_udp_len], src_port, dst_port).unwrap();
        udp_builder.payload_mut().copy_from_slice(payload);
        udp_builder.build();

        ipv6_builder.payload_len(total_udp_len);
        ipv6_builder.build();

        self.last_responses =
            self.slirp.handle_request(SlirpRequest::Packet(Bytes::copy_from_slice(&eth_packet)));
    }

    fn then_udp_connection_established_and_written(
        &self,
        expected_dest: SocketAddr,
        expected_guest_port: u16,
        expected_payload: &[u8],
    ) -> u64 {
        assert_eq!(
            self.last_responses.len(),
            2,
            "Should return EstablishConnection and WriteToConnection"
        );
        let mut conn_id = None;
        for resp in &self.last_responses {
            if let SlirpResponse::EstablishConnection(id, ConnectionArgs::Udp(args)) = resp {
                assert_eq!(args.destination, expected_dest);
                match expected_dest.ip() {
                    IpAddr::V4(_) => {
                        assert_eq!(args.guest_ip, IpAddr::V4(self.config.guest_ipv4));
                    }
                    IpAddr::V6(_) => {
                        assert_eq!(args.guest_ip, IpAddr::V6(self.config.guest_ipv6));
                    }
                }
                assert_eq!(args.guest_port, expected_guest_port);
                conn_id = Some(*id);
            }
        }
        let conn_id = conn_id.expect("Expected EstablishConnection response");
        assert!(conn_id >= (1 << 63), "UDP connection ID should have MSB set");

        let has_write = self.last_responses.iter().any(|resp| {
            matches!(
                resp,
                SlirpResponse::WriteToConnection(id, data)
                    if *id == conn_id && data == expected_payload
            )
        });
        assert!(has_write, "Expected WriteToConnection with payload");
        conn_id
    }

    fn when_host_sends_data(&mut self, conn_id: u64, payload: &[u8]) {
        self.last_responses =
            self.slirp.handle_request(SlirpRequest::Data(conn_id, Bytes::copy_from_slice(payload)));
    }

    fn then_guest_receives_ipv4_udp_from_gateway(
        &self,
        expected_src_port: u16,
        expected_dst_port: u16,
        expected_payload: &[u8],
    ) {
        assert_eq!(self.last_responses.len(), 1, "Should return exactly one reply packet");
        let response_packet = match &self.last_responses[0] {
            SlirpResponse::Packet(p) => p,
            _ => panic!("Expected Packet response"),
        };
        let (eth_frame_reply, eth_payload_reply) = EthernetFrame::parse(response_packet).unwrap();
        assert_eq!(eth_frame_reply.dst_addr, self.config.guest_mac);
        assert_eq!(eth_frame_reply.src_addr, self.config.gateway_mac);
        let (ip_header_reply, ip_payload_reply) = Ipv4Header::parse(eth_payload_reply).unwrap();
        assert_eq!(ip_header_reply.protocol, IP_P_UDP);
        assert_eq!(Ipv4Addr::from(ip_header_reply.source_addr), self.config.host_ipv4);
        assert_eq!(Ipv4Addr::from(ip_header_reply.dest_addr), self.config.guest_ipv4);
        let (udp_header_reply, udp_payload_reply) = UdpHeader::parse(ip_payload_reply).unwrap();
        assert_eq!(udp_header_reply.source_port.get(), expected_src_port);
        assert_eq!(udp_header_reply.dest_port.get(), expected_dst_port);
        assert_eq!(udp_payload_reply, expected_payload);
    }

    fn then_guest_receives_ipv6_udp_from_gateway(
        &self,
        expected_src_port: u16,
        expected_dst_port: u16,
        expected_payload: &[u8],
    ) {
        assert_eq!(self.last_responses.len(), 1, "Should return exactly one reply packet");
        let response_packet = match &self.last_responses[0] {
            SlirpResponse::Packet(p) => p,
            _ => panic!("Expected Packet response"),
        };
        let (eth_frame_reply, eth_payload_reply) = EthernetFrame::parse(response_packet).unwrap();
        assert_eq!(eth_frame_reply.dst_addr, self.config.guest_mac);
        assert_eq!(eth_frame_reply.src_addr, self.config.gateway_mac);
        assert_eq!(eth_frame_reply.ethertype.get(), ether_type::IPV6);

        let (ip_header_reply, ip_payload_reply) = Ipv6Header::parse(eth_payload_reply).unwrap();
        assert_eq!(ip_header_reply.next_header, IP_P_UDP);
        assert_eq!(Ipv6Addr::from(ip_header_reply.source_addr), self.config.host_ipv6);
        assert_eq!(Ipv6Addr::from(ip_header_reply.dest_addr), self.config.guest_ipv6);

        let (udp_header_reply, udp_payload_reply) = UdpHeader::parse(ip_payload_reply).unwrap();
        assert_eq!(udp_header_reply.source_port.get(), expected_src_port);
        assert_eq!(udp_header_reply.dest_port.get(), expected_dst_port);
        assert_eq!(udp_payload_reply, expected_payload);
    }

    fn when_host_closes_connection(&mut self, conn_id: u64) {
        self.last_responses = self.slirp.handle_request(SlirpRequest::ConnectionClosed(conn_id));
    }

    fn when_host_refuses_connection(&mut self, conn_id: u64) {
        self.last_responses = self.slirp.handle_request(SlirpRequest::ConnectionRefused(conn_id));
    }

    fn then_no_responses(&self) {
        assert!(
            self.last_responses.is_empty(),
            "Expected no responses, got: {:?}",
            self.last_responses
        );
    }

    fn then_guest_receives_ipv4_icmp_port_unreachable_and_close(
        &self,
        conn_id: u64,
        expected_src_port: u16,
        expected_dst_port: u16,
    ) {
        self.then_guest_receives_port_unreachable_and_close(
            conn_id,
            IpAddr::V4(self.config.guest_ipv4),
            ether_type::IPV4,
            expected_src_port,
            expected_dst_port,
            |ip_payload| {
                let (ip_header, icmp_payload) = Ipv4Header::parse(ip_payload).unwrap();
                assert_eq!(ip_header.protocol, IP_P_ICMP);
                assert_eq!(Ipv4Addr::from(ip_header.source_addr), self.config.host_ipv4);
                assert_eq!(Ipv4Addr::from(ip_header.dest_addr), self.config.guest_ipv4);

                let (icmp_header, icmp_data) = IcmpHeader::parse(icmp_payload).unwrap();
                assert_eq!(icmp_header.icmp_type, netsim_packets::IcmpType::DestUnreachable as u8);
                assert_eq!(
                    icmp_header.icmp_code,
                    netsim_packets::UnreachableCode::PortUnreachable as u8
                );

                let offending_ip_header = Ipv4Header::parse(&icmp_data[..20]).unwrap().0;
                assert_eq!(offending_ip_header.protocol, IP_P_UDP);
                assert_eq!(Ipv4Addr::from(offending_ip_header.source_addr), self.config.guest_ipv4);
                assert_eq!(Ipv4Addr::from(offending_ip_header.dest_addr), self.config.host_ipv4);
                &icmp_data[20..28]
            },
        );
    }

    fn then_guest_receives_ipv6_icmpv6_port_unreachable_and_close(
        &self,
        conn_id: u64,
        expected_src_port: u16,
        expected_dst_port: u16,
    ) {
        self.then_guest_receives_port_unreachable_and_close(
            conn_id,
            IpAddr::V6(self.config.guest_ipv6),
            ether_type::IPV6,
            expected_src_port,
            expected_dst_port,
            |ip_payload| {
                let (ipv6_header, icmpv6_payload) = Ipv6Header::parse(ip_payload).unwrap();
                assert_eq!(ipv6_header.next_header, IP_P_ICMPV6);
                assert_eq!(Ipv6Addr::from(ipv6_header.source_addr), self.config.host_ipv6);
                assert_eq!(Ipv6Addr::from(ipv6_header.dest_addr), self.config.guest_ipv6);

                let (icmpv6_header, icmpv6_data) = Icmpv6Header::parse(icmpv6_payload).unwrap();
                assert_eq!(
                    icmpv6_header.icmpv6_type,
                    netsim_packets::Icmpv6Type::DestinationUnreachable as u8
                );
                assert_eq!(
                    icmpv6_header.icmpv6_code,
                    netsim_packets::Icmpv6UnreachableCode::PortUnreachable as u8
                );

                let offending_ipv6_header = Ipv6Header::parse(&icmpv6_data[..40]).unwrap().0;
                assert_eq!(offending_ipv6_header.next_header, IP_P_UDP);
                assert_eq!(
                    Ipv6Addr::from(offending_ipv6_header.source_addr),
                    self.config.guest_ipv6
                );
                assert_eq!(Ipv6Addr::from(offending_ipv6_header.dest_addr), self.config.host_ipv6);
                &icmpv6_data[40..48]
            },
        );
    }

    fn then_guest_receives_port_unreachable_and_close(
        &self,
        conn_id: u64,
        expected_guest_ip: IpAddr,
        expected_ethertype: u16,
        expected_src_port: u16,
        expected_dst_port: u16,
        assert_icmp_and_get_offending_l4: impl Fn(&[u8]) -> &[u8],
    ) {
        assert_eq!(
            self.last_responses.len(),
            2,
            "Expected ICMP/ICMPv6 packet and CloseConnection responses, got {:?}",
            self.last_responses
        );

        let mut has_close = false;
        let mut has_icmp = false;

        for resp in &self.last_responses {
            match resp {
                SlirpResponse::CloseConnection { conn_id: id, guest_addr } => {
                    assert_eq!(*id, conn_id);
                    assert_eq!(guest_addr.port(), expected_src_port);
                    assert_eq!(guest_addr.ip(), expected_guest_ip);
                    has_close = true;
                }
                SlirpResponse::Packet(p) => {
                    let (eth_frame, ip_payload) = EthernetFrame::parse(p).unwrap();
                    assert_eq!(eth_frame.dst_addr, self.config.guest_mac);
                    assert_eq!(eth_frame.ethertype.get(), expected_ethertype);

                    let offending_l4_header_slice = assert_icmp_and_get_offending_l4(ip_payload);
                    let (offending_udp_header, _) =
                        UdpHeader::parse(offending_l4_header_slice).unwrap();
                    assert_eq!(offending_udp_header.source_port.get(), expected_src_port);
                    assert_eq!(offending_udp_header.dest_port.get(), expected_dst_port);
                    has_icmp = true;
                }
                _ => panic!("Unexpected response: {resp:?}"),
            }
        }

        assert!(has_close, "Missing CloseConnection response");
        assert!(has_icmp, "Missing ICMP/ICMPv6 Port Unreachable packet response");
    }

    fn then_close_connection_without_icmp(
        &self,
        expected_conn_id: u64,
        expected_guest_addr: SocketAddr,
    ) {
        assert_eq!(
            self.last_responses.len(),
            1,
            "Expected exactly one response, got: {:?}",
            self.last_responses
        );
        match &self.last_responses[0] {
            SlirpResponse::CloseConnection { conn_id, guest_addr } => {
                assert_eq!(*conn_id, expected_conn_id);
                assert_eq!(*guest_addr, expected_guest_addr);
            }
            other => panic!("Expected CloseConnection response, got: {other:?}"),
        }
        assert_eq!(self.slirp.connection_info().count(), 0);
    }

    fn then_guest_receives_host_unreachable(
        &self,
        expected_ethertype: u16,
        expected_src_port: u16,
        expected_dst_port: u16,
        assert_icmp_and_get_quoted_transport: impl FnOnce(&[u8]) -> &[u8],
    ) {
        assert_eq!(
            self.last_responses.len(),
            1,
            "Expected exactly one response, got: {:?}",
            self.last_responses
        );
        let packet = match &self.last_responses[0] {
            SlirpResponse::Packet(p) => p,
            other => panic!("Expected Packet response, got: {other:?}"),
        };
        let (eth_frame, eth_payload) = EthernetFrame::parse(packet).unwrap();
        assert_eq!(eth_frame.dst_addr, self.config.guest_mac);
        assert_eq!(eth_frame.ethertype.get(), expected_ethertype);

        let quoted_l4 = assert_icmp_and_get_quoted_transport(eth_payload);
        assert!(quoted_l4.len() >= 8);
        // Only the first 8 bytes of the TCP header are quoted, so
        // TcpHeader::parse cannot be used.
        let quoted_src_port = u16::from_be_bytes([quoted_l4[0], quoted_l4[1]]);
        let quoted_dst_port = u16::from_be_bytes([quoted_l4[2], quoted_l4[3]]);
        assert_eq!(quoted_src_port, expected_src_port);
        assert_eq!(quoted_dst_port, expected_dst_port);

        assert_eq!(self.slirp.connection_info().count(), 0);
    }

    fn then_guest_receives_ipv4_icmp_host_unreachable(
        &self,
        expected_guest_port: u16,
        expected_dst_ip: Ipv4Addr,
        expected_dst_port: u16,
    ) {
        self.then_guest_receives_host_unreachable(
            ether_type::IPV4,
            expected_guest_port,
            expected_dst_port,
            |ip_payload| {
                let (ip_header, icmp_payload) = Ipv4Header::parse(ip_payload).unwrap();
                assert_eq!(ip_header.protocol, IP_P_ICMP);
                assert_eq!(Ipv4Addr::from(ip_header.source_addr), self.config.host_ipv4);
                assert_eq!(Ipv4Addr::from(ip_header.dest_addr), self.config.guest_ipv4);

                let (icmp_header, icmp_data) = IcmpHeader::parse(icmp_payload).unwrap();
                assert_eq!(icmp_header.icmp_type, netsim_packets::IcmpType::DestUnreachable as u8);
                assert_eq!(
                    icmp_header.icmp_code,
                    netsim_packets::UnreachableCode::HostUnreachable as u8
                );

                let (offending_ip_header, quoted_l4) = Ipv4Header::parse(icmp_data).unwrap();
                assert_eq!(offending_ip_header.protocol, IP_P_TCP);
                assert_eq!(Ipv4Addr::from(offending_ip_header.source_addr), self.config.guest_ipv4);
                assert_eq!(Ipv4Addr::from(offending_ip_header.dest_addr), expected_dst_ip);
                quoted_l4
            },
        );
    }

    fn then_guest_receives_ipv6_icmpv6_host_unreachable(
        &self,
        expected_guest_port: u16,
        expected_dst_ip: Ipv6Addr,
        expected_dst_port: u16,
    ) {
        self.then_guest_receives_host_unreachable(
            ether_type::IPV6,
            expected_guest_port,
            expected_dst_port,
            |ip_payload| {
                let (ipv6_header, icmpv6_payload) = Ipv6Header::parse(ip_payload).unwrap();
                assert_eq!(ipv6_header.next_header, IP_P_ICMPV6);
                assert_eq!(Ipv6Addr::from(ipv6_header.source_addr), self.config.host_ipv6);
                assert_eq!(Ipv6Addr::from(ipv6_header.dest_addr), self.config.guest_ipv6);

                let (icmpv6_header, icmpv6_data) = Icmpv6Header::parse(icmpv6_payload).unwrap();
                assert_eq!(
                    icmpv6_header.icmpv6_type,
                    netsim_packets::Icmpv6Type::DestinationUnreachable as u8
                );
                assert_eq!(
                    icmpv6_header.icmpv6_code,
                    netsim_packets::Icmpv6UnreachableCode::AddressUnreachable as u8
                );

                let (offending_ipv6_header, quoted_l4) = Ipv6Header::parse(icmpv6_data).unwrap();
                assert_eq!(offending_ipv6_header.next_header, IP_P_TCP);
                assert_eq!(
                    Ipv6Addr::from(offending_ipv6_header.source_addr),
                    self.config.guest_ipv6
                );
                assert_eq!(Ipv6Addr::from(offending_ipv6_header.dest_addr), expected_dst_ip);
                quoted_l4
            },
        );
    }
}

#[test]
fn test_tcp_gateway_loopback_nat_ipv4_integration() {
    // Scenario: Guest connects to host gateway port and gets translated to loopback
    // Given a running Slirp stack with default configuration
    let mut world = World::new();

    // When the guest sends a TCP SYN packet targeting the gateway
    let guest_port = 12345;
    let loopback_port = 8080;
    world.when_guest_sends_ipv4_tcp_syn(guest_port, loopback_port);

    // Then a TCP connection is established to container loopback
    world.then_tcp_connection_established(
        SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), loopback_port),
        guest_port,
    );
    // And the guest receives a TCP SYN-ACK packet from the gateway
    world.then_guest_receives_ipv4_tcp_syn_ack(loopback_port, guest_port);
}

#[test]
fn test_tcp_gateway_guestfwd_precedence_integration() {
    // Scenario: Explicit guestfwd rule takes precedence over default gateway loopback translation
    // Given a Slirp stack with an explicit guestfwd rule for gateway:9090 -> 192.168.1.100:9090
    let mut world = World::new();
    let guestfwd_port = 9090;
    let explicit_host_dest: SocketAddr = "192.168.1.100:9090".parse().unwrap();
    world.given_guestfwd_rule(
        SocketAddr::new(IpAddr::V4(Ipv4Addr::new(10, 0, 2, 2)), guestfwd_port),
        explicit_host_dest,
    );

    // When the guest sends a TCP SYN packet to gateway:9090
    let guest_port = 12346;
    world.when_guest_sends_ipv4_tcp_syn(guest_port, guestfwd_port);

    // Then a TCP connection is established to the explicit guestfwd target
    world.then_tcp_connection_established(explicit_host_dest, guest_port);
    // And the guest receives a TCP SYN-ACK packet from the gateway
    world.then_guest_receives_ipv4_tcp_syn_ack(guestfwd_port, guest_port);
}

#[test]
fn test_tcp_gateway_reserved_dns_port_not_rewritten_integration() {
    // Scenario: Gateway DNS port (53) without configured DNS servers is not redirected to loopback
    // Given a Slirp stack with no configured DNS servers
    let mut world = World::new();
    world.given_no_dns_servers();

    // When the guest sends a TCP SYN packet to gateway port 53
    let guest_port = 12347;
    world.when_guest_sends_ipv4_tcp_syn(guest_port, 53);

    // Then a TCP connection is established to the gateway's external host IPv4, not loopback
    world.then_tcp_connection_established(
        SocketAddr::new(IpAddr::V4(world.config.host_ipv4), 53),
        guest_port,
    );
    // And the guest receives a TCP SYN-ACK packet from the gateway
    world.then_guest_receives_ipv4_tcp_syn_ack(53, guest_port);
}

#[test]
fn test_udp_gateway_loopback_ipv4_integration() {
    // Scenario: Guest sends IPv4 UDP datagram to gateway and receives downlink
    // reply from gateway
    // Given a running Slirp stack with default configuration
    let mut world = World::new();

    // When the guest sends an IPv4 UDP packet targeting the gateway
    let src_port = 12345;
    let dst_port = 1234;
    let udp_payload = b"hello";
    world.when_guest_sends_ipv4_udp(src_port, dst_port, udp_payload);

    // Then a UDP connection is established to container loopback with payload
    // written
    let conn_id = world.then_udp_connection_established_and_written(
        SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), dst_port),
        src_port,
        udp_payload,
    );

    // When the host sends a reply datagram back to the guest
    let reply_payload = b"reply_payload";
    world.when_host_sends_data(conn_id, reply_payload);

    // Then the guest receives an IPv4 UDP packet from the gateway with the
    // reply payload
    world.then_guest_receives_ipv4_udp_from_gateway(dst_port, src_port, reply_payload);
}

#[test]
fn test_udp_gateway_loopback_ipv6_integration() {
    // Scenario: Guest sends IPv6 UDP datagram to gateway and receives downlink
    // reply from gateway
    // Given a running Slirp stack with default configuration
    let mut world = World::new();

    // When the guest sends an IPv6 UDP packet targeting the gateway
    let src_port = 12345;
    let dst_port = 12345;
    let payload = b"hello udp gateway loopback";
    world.when_guest_sends_ipv6_udp(src_port, dst_port, payload);

    // Then a UDP connection is established to container loopback with payload
    // written
    let conn_id = world.then_udp_connection_established_and_written(
        SocketAddr::new(IpAddr::V6(Ipv6Addr::LOCALHOST), dst_port),
        src_port,
        payload,
    );

    // When the host sends a reply datagram back to the guest
    let reply_payload = b"reply_payload_v6";
    world.when_host_sends_data(conn_id, reply_payload);

    // Then the guest receives an IPv6 UDP packet from the gateway with the
    // reply payload
    world.then_guest_receives_ipv6_udp_from_gateway(dst_port, src_port, reply_payload);
}

#[test]
fn test_tcp_non_gateway_destination_not_rewritten_integration() {
    // Scenario: Guest connects to non-gateway host IP and destination is
    // preserved
    // Given a running Slirp stack with default configuration
    let mut world = World::new();

    // When the guest sends a TCP SYN packet targeting an external non-gateway
    // IP
    let guest_port = 12348;
    let remote_port = 8080;
    let non_gateway_ip = Ipv4Addr::new(192, 168, 1, 50);
    world.when_guest_sends_ipv4_tcp_syn_to(non_gateway_ip, guest_port, remote_port);

    // Then a TCP connection is established directly to the non-gateway IP
    // (not loopback!)
    world.then_tcp_connection_established(
        SocketAddr::new(IpAddr::V4(non_gateway_ip), remote_port),
        guest_port,
    );
    // And the guest receives a TCP SYN-ACK packet from the non-gateway IP
    world.then_guest_receives_ipv4_tcp_syn_ack_from(non_gateway_ip, remote_port, guest_port);
}

#[test]
fn test_udp_non_gateway_destination_not_rewritten_integration() {
    // Scenario: Guest sends UDP datagram to non-gateway host IP and destination
    // is preserved
    // Given a running Slirp stack with default configuration
    let mut world = World::new();

    // When the guest sends an IPv4 UDP packet targeting an external
    // non-gateway IP
    let src_port = 12348;
    let dst_port = 5353;
    let non_gateway_ip = Ipv4Addr::new(192, 168, 1, 50);
    let payload = b"udp non gateway";
    world.when_guest_sends_ipv4_udp_to(non_gateway_ip, src_port, dst_port, payload);

    // Then a UDP connection is established directly to the non-gateway IP
    // (not loopback!)
    world.then_udp_connection_established_and_written(
        SocketAddr::new(IpAddr::V4(non_gateway_ip), dst_port),
        src_port,
        payload,
    );
}

#[test]
fn test_udp_gateway_loopback_port_unreachable_ipv4_integration() {
    // Scenario: Guest sends IPv4 UDP packet to closed gateway port and receives
    // ICMP Port Unreachable
    // Given a running Slirp stack with default configuration
    let mut world = World::new();

    // When the guest sends an IPv4 UDP packet targeting a port on the gateway
    let src_port = 12345;
    let dst_port = 1234;
    let udp_payload = b"test udp unreachable";
    world.when_guest_sends_ipv4_udp(src_port, dst_port, udp_payload);

    // Then a UDP connection is established to container loopback with payload
    // written
    let conn_id = world.then_udp_connection_established_and_written(
        SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), dst_port),
        src_port,
        udp_payload,
    );

    // When the host refuses the connection (e.g. host port is closed /
    // ICMP port unreachable on host)
    world.when_host_refuses_connection(conn_id);

    // Then the guest receives an IPv4 ICMP Port Unreachable packet and
    // CloseConnection
    world.then_guest_receives_ipv4_icmp_port_unreachable_and_close(conn_id, src_port, dst_port);

    // And when the host refuses the connection again, no further responses
    // are emitted
    world.when_host_refuses_connection(conn_id);
    world.then_no_responses();
}

#[test]
fn test_udp_gateway_loopback_port_unreachable_ipv6_integration() {
    // Scenario: Guest sends IPv6 UDP packet to closed gateway port and receives
    // ICMPv6 Port Unreachable
    // Given a running Slirp stack with default configuration
    let mut world = World::new();

    // When the guest sends an IPv6 UDP packet targeting a port on the gateway
    let src_port = 12345;
    let dst_port = 12345;
    let payload = b"test udp v6 unreachable";
    world.when_guest_sends_ipv6_udp(src_port, dst_port, payload);

    // Then a UDP connection is established to container loopback with payload
    // written
    let conn_id = world.then_udp_connection_established_and_written(
        SocketAddr::new(IpAddr::V6(Ipv6Addr::LOCALHOST), dst_port),
        src_port,
        payload,
    );

    // When the host refuses the connection (e.g. host port is closed /
    // ICMPv6 port unreachable on host)
    world.when_host_refuses_connection(conn_id);

    // Then the guest receives an IPv6 ICMPv6 Port Unreachable packet and
    // CloseConnection
    world.then_guest_receives_ipv6_icmpv6_port_unreachable_and_close(conn_id, src_port, dst_port);

    // And when the host refuses the connection again, no further responses
    // are emitted
    world.when_host_refuses_connection(conn_id);
    world.then_no_responses();
}

#[test]
fn test_udp_gateway_loopback_host_closed_without_icmp_ipv4_integration() {
    // Scenario: Host closes IPv4 UDP flow for a non-refusal reason (e.g.
    // ENETUNREACH); slirp tears down the flow without ICMP
    // Given a running Slirp stack with default configuration
    let mut world = World::new();

    // When the guest sends an IPv4 UDP packet targeting a port on the gateway
    let src_port = 12345;
    let dst_port = 1234;
    let udp_payload = b"test udp host closed without icmp";
    world.when_guest_sends_ipv4_udp(src_port, dst_port, udp_payload);

    // Then a UDP connection is established to container loopback with
    // payload written
    let conn_id = world.then_udp_connection_established_and_written(
        SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), dst_port),
        src_port,
        udp_payload,
    );

    // When the host closes the connection
    world.when_host_closes_connection(conn_id);

    // Then slirp emits CloseConnection without ICMP and connection count
    // drops to 0
    world.then_close_connection_without_icmp(
        conn_id,
        SocketAddr::new(IpAddr::V4(world.config.guest_ipv4), src_port),
    );

    // And when the host closes the connection again, no further responses
    // are emitted
    world.when_host_closes_connection(conn_id);
    world.then_no_responses();
}

#[test]
fn test_udp_gateway_loopback_host_closed_without_icmp_ipv6_integration() {
    // Scenario: Host closes IPv6 UDP flow for a non-refusal reason (e.g.
    // ENETUNREACH); slirp tears down the flow without ICMP
    // Given a running Slirp stack with default configuration
    let mut world = World::new();

    // When the guest sends an IPv6 UDP packet targeting a port on the gateway
    let src_port = 12345;
    let dst_port = 12345;
    let payload = b"test udp v6 host closed without icmp";
    world.when_guest_sends_ipv6_udp(src_port, dst_port, payload);

    // Then a UDP connection is established to container loopback with
    // payload written
    let conn_id = world.then_udp_connection_established_and_written(
        SocketAddr::new(IpAddr::V6(Ipv6Addr::LOCALHOST), dst_port),
        src_port,
        payload,
    );

    // When the host closes the connection
    world.when_host_closes_connection(conn_id);

    // Then slirp emits CloseConnection without ICMP and connection count
    // drops to 0
    world.then_close_connection_without_icmp(
        conn_id,
        SocketAddr::new(IpAddr::V6(world.config.guest_ipv6), src_port),
    );

    // And when the host closes the connection again, no further responses
    // are emitted
    world.when_host_closes_connection(conn_id);
    world.then_no_responses();
}

#[test]
fn test_tcp_connect_failure_sends_icmpv6_host_unreachable_integration() {
    // Scenario: IPv6 TCP connection failure in SynSent state emits ICMPv6
    // Address Unreachable quoting the pre-translation gateway destination
    // Given a running Slirp stack with default configuration
    let mut world = World::new();

    // When the guest sends an IPv6 TCP SYN targeting the gateway
    let guest_port = 12345;
    let target_port = 8080;
    let gateway_ipv6 = world.config.host_ipv6;
    world.when_guest_sends_ipv6_tcp_syn_to(gateway_ipv6, guest_port, target_port);

    // Then a TCP connection is established to container loopback in
    // SynSent state
    let conn_id = world.then_tcp_connection_established(
        SocketAddr::new(IpAddr::V6(Ipv6Addr::LOCALHOST), target_port),
        guest_port,
    );

    // When the host driver closes the connection (e.g. connect failed)
    world.when_host_closes_connection(conn_id);

    // Then the guest receives an ICMPv6 Address Unreachable packet quoting the
    // pre-translation gateway destination, and the connection is removed
    world.then_guest_receives_ipv6_icmpv6_host_unreachable(guest_port, gateway_ipv6, target_port);
}

#[test]
fn test_tcp_connection_refused_falls_back_to_host_unreachable_integration() {
    // Scenario: Host ConnectionRefused on a TCP connection ID falls back to
    // close_tcp_connection and emits ICMP Host Unreachable quoting the
    // pre-translation gateway destination
    // Given a running Slirp stack with default configuration
    let mut world = World::new();

    // When the guest sends an IPv4 TCP SYN targeting the gateway
    let guest_port = 12345;
    let target_port = 8080;
    let gateway_ipv4 = world.config.host_ipv4;
    world.when_guest_sends_ipv4_tcp_syn_to(gateway_ipv4, guest_port, target_port);

    // Then a TCP connection is established to container loopback in
    // SynSent state
    let conn_id = world.then_tcp_connection_established(
        SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), target_port),
        guest_port,
    );

    // When the host driver reports ConnectionRefused for the TCP connection
    world.when_host_refuses_connection(conn_id);

    // Then the guest receives an ICMP Host Unreachable packet quoting the
    // pre-translation gateway destination, and the connection is removed
    world.then_guest_receives_ipv4_icmp_host_unreachable(guest_port, gateway_ipv4, target_port);
}
