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

//! Integration Tests for Recent Slirp Fixes.
//!
//! Includes BDD test coverage for:
//! 1. ICMP Echo Request / Reply ID and checksum rewriting across NAPT
//!    translation.
//! 2. ICMPv6 Echo Request / Reply ID and checksum rewriting across NAPT
//!    translation.
//! 3. UDP datagrams with 0x0000 checksum (IPv4 spec RFC 768 allows 0 checksum).

use std::net::{Ipv4Addr, Ipv6Addr};

use netsim_packets::{
    EthernetFrame, IP_P_ICMP, IP_P_ICMPV6, IP_P_UDP, IcmpHeader, IcmpType, Icmpv6Header,
    Icmpv6Type, Ipv4Builder, Ipv4Header, Ipv6Builder, Ipv6Header, MacAddr, UdpHeader,
    UdpPacketBuilder, icmpv6_checksum, ipv4_checksum,
};
use slirp_actor::SlirpBackend;
use zerocopy::FromBytes;

use super::test_driver::SlirpTestDriver;

fn create_icmp_echo_request_packet(
    src_ip: Ipv4Addr,
    dst_ip: Ipv4Addr,
    id: u16,
    seq: u16,
    payload: &[u8],
) -> bytes::Bytes {
    let icmp_header_len = std::mem::size_of::<IcmpHeader>();
    let total_icmp_len = icmp_header_len + payload.len();

    let mut icmp_data = vec![0u8; total_icmp_len];
    let (hdr_slice, payload_slice) = icmp_data.split_at_mut(icmp_header_len);
    let icmp_header = IcmpHeader::mut_from_bytes(hdr_slice).unwrap();
    icmp_header.icmp_type = IcmpType::EchoRequest as u8;
    icmp_header.icmp_code = 0;
    icmp_header.icmp_checksum.set(0);
    icmp_header.rest[..2].copy_from_slice(&id.to_be_bytes());
    icmp_header.rest[2..].copy_from_slice(&seq.to_be_bytes());
    payload_slice.copy_from_slice(payload);

    let checksum = ipv4_checksum(&icmp_data);
    let icmp_hdr_mut = IcmpHeader::mut_from_bytes(&mut icmp_data[..icmp_header_len]).unwrap();
    icmp_hdr_mut.icmp_checksum.set(checksum);

    let mut ip_data = vec![0u8; 20 + total_icmp_len];
    let mut ipv4_builder = Ipv4Builder::new(&mut ip_data, IP_P_ICMP, src_ip, dst_ip).unwrap();
    ipv4_builder.payload(&icmp_data).unwrap();
    ipv4_builder.build().unwrap();

    let mut eth_data = vec![0u8; 14 + ip_data.len()];
    let (eth_header_slice, eth_payload_slice) = eth_data.split_at_mut(14);
    let eth_frame = EthernetFrame::mut_from_bytes(eth_header_slice).unwrap();
    eth_frame.dst_addr = MacAddr { bytes: [0x52, 0x54, 0x00, 0x12, 0x34, 0x56] };
    eth_frame.src_addr = MacAddr { bytes: [0x02, 0x15, 0xb2, 0x00, 0x00, 0x00] };
    eth_frame.ethertype = 0x0800.into();
    eth_payload_slice.copy_from_slice(&ip_data);

    bytes::Bytes::from(eth_data)
}

fn create_icmpv6_echo_request_packet(
    src_ip: Ipv6Addr,
    dst_ip: Ipv6Addr,
    id: u16,
    seq: u16,
    payload: &[u8],
) -> bytes::Bytes {
    let icmp_header_len = std::mem::size_of::<Icmpv6Header>();
    let total_icmp_len = icmp_header_len + payload.len();

    let mut icmp_data = vec![0u8; total_icmp_len];
    let (hdr_slice, payload_slice) = icmp_data.split_at_mut(icmp_header_len);
    let icmp_header = Icmpv6Header::mut_from_bytes(hdr_slice).unwrap();
    icmp_header.icmpv6_type = Icmpv6Type::EchoRequest as u8;
    icmp_header.icmpv6_code = 0;
    icmp_header.icmpv6_checksum.set(0);
    icmp_header.set_echo_fields(id, seq);
    payload_slice.copy_from_slice(payload);

    let checksum = icmpv6_checksum(&icmp_data, src_ip, dst_ip);
    let icmp_hdr_mut = Icmpv6Header::mut_from_bytes(&mut icmp_data[..icmp_header_len]).unwrap();
    icmp_hdr_mut.icmpv6_checksum.set(checksum);

    let mut ip_data = vec![0u8; 40 + total_icmp_len];
    let mut ipv6_builder = Ipv6Builder::new(&mut ip_data, IP_P_ICMPV6, src_ip, dst_ip).unwrap();
    ipv6_builder.payload(&icmp_data).unwrap();
    ipv6_builder.build().unwrap();

    let mut eth_data = vec![0u8; 14 + ip_data.len()];
    let (eth_header_slice, eth_payload_slice) = eth_data.split_at_mut(14);
    let eth_frame = EthernetFrame::mut_from_bytes(eth_header_slice).unwrap();
    eth_frame.dst_addr = MacAddr { bytes: [0x33, 0x33, 0x00, 0x00, 0x00, 0x01] };
    eth_frame.src_addr = MacAddr { bytes: [0x02, 0x15, 0xb2, 0x00, 0x00, 0x00] };
    eth_frame.ethertype = 0x86DD.into();
    eth_payload_slice.copy_from_slice(&ip_data);

    bytes::Bytes::from(eth_data)
}

// -----------------------------------------------------------------------------
// 1. ICMP Echo Request / Reply ID and Checksum Rewriting Test
// -----------------------------------------------------------------------------

async fn run_icmp_echo_id_rewriting_fix_test(backend: SlirpBackend) {
    // Given a Slirp test driver running the specified backend
    let mut driver = SlirpTestDriver::new(backend, None).await;

    // And an ICMP Echo Request with a specific identifier, sequence number, and
    // payload
    let guest_ip = Ipv4Addr::new(10, 0, 2, 15);
    let host_ip = Ipv4Addr::new(10, 0, 2, 2);
    let test_id: u16 = 0x1234;
    let test_seq: u16 = 0x0042;
    let payload = b"ping_napt_id_rewrite_test";

    let echo_req_packet =
        create_icmp_echo_request_packet(guest_ip, host_ip, test_id, test_seq, payload);

    // When the guest sends the ICMP Echo Request packet
    let send_result = driver.send_packet(echo_req_packet).await;
    assert!(send_result.is_ok());

    // Then Slirp processes the request and responds with a valid ICMP Echo Reply
    // (if network permits). Slirp may ARP for the guest first, so skip ahead to
    // the IPv4/ICMP frame.
    let is_icmp = |frame: &bytes::Bytes| {
        EthernetFrame::parse(frame)
            .filter(|(eth, _)| eth.ethertype.get() == 0x0800)
            .and_then(|(_, payload)| Ipv4Header::parse(payload))
            .is_some_and(|(ip, _)| ip.protocol == IP_P_ICMP)
    };
    if let Ok(Some(reply_bytes)) =
        tokio::time::timeout(std::time::Duration::from_millis(500), driver.recv_matching(is_icmp))
            .await
    {
        let (eth_frame, eth_payload) =
            EthernetFrame::parse(&reply_bytes).expect("Failed to parse Ethernet frame");
        assert_eq!(eth_frame.ethertype.get(), 0x0800);

        let (ipv4_hdr, ipv4_payload) =
            Ipv4Header::parse(eth_payload).expect("Failed to parse IPv4 header");
        assert_eq!(ipv4_hdr.protocol, IP_P_ICMP);
        assert_eq!(Ipv4Addr::from(ipv4_hdr.source_addr), host_ip);
        assert_eq!(Ipv4Addr::from(ipv4_hdr.dest_addr), guest_ip);

        let (icmp_hdr, icmp_payload) =
            IcmpHeader::parse(ipv4_payload).expect("Failed to parse ICMP header");
        assert_eq!(icmp_hdr.icmp_type, IcmpType::EchoReply as u8);
        assert_eq!(icmp_hdr.icmp_code, 0);

        let reply_id = u16::from_be_bytes([icmp_hdr.rest[0], icmp_hdr.rest[1]]);
        let reply_seq = u16::from_be_bytes([icmp_hdr.rest[2], icmp_hdr.rest[3]]);
        assert_eq!(reply_id, test_id);
        assert_eq!(reply_seq, test_seq);
        assert_eq!(icmp_payload, payload);

        let computed_checksum = ipv4_checksum(ipv4_payload);
        assert_eq!(computed_checksum, 0, "ICMP Echo Reply checksum must be valid");
    }
}

#[tokio::test]
async fn test_icmp_echo_id_rewriting_fix() {
    for backend in [SlirpBackend::CFfi, SlirpBackend::Native] {
        run_icmp_echo_id_rewriting_fix_test(backend).await;
    }
}

// -----------------------------------------------------------------------------
// 2. ICMPv6 Echo Request / Reply ID and Checksum Rewriting Test
// -----------------------------------------------------------------------------

async fn run_icmpv6_echo_id_rewriting_fix_test(backend: SlirpBackend) {
    // Given a Slirp test driver running the specified backend
    let mut driver = SlirpTestDriver::new(backend, None).await;

    // And an ICMPv6 Echo Request with a specific identifier, sequence number, and
    // payload
    let guest_ipv6: Ipv6Addr = "fec0::1".parse().unwrap();
    let host_ipv6: Ipv6Addr = "fec0::2".parse().unwrap();
    let test_id: u16 = 0x5678;
    let test_seq: u16 = 0x0084;
    let payload = b"pingv6_napt_id_rewrite_test";

    let echo_req_packet =
        create_icmpv6_echo_request_packet(guest_ipv6, host_ipv6, test_id, test_seq, payload);

    // When the guest sends the ICMPv6 Echo Request packet
    let send_result = driver.send_packet(echo_req_packet).await;
    assert!(send_result.is_ok());

    // Then Slirp processes the request and responds with a valid ICMPv6 Echo
    // Reply (if network permits). Slirp solicits the guest's link-layer address
    // first, so skip past NDP to the echo reply.
    let is_echo_reply = |frame: &bytes::Bytes| {
        EthernetFrame::parse(frame)
            .filter(|(eth, _)| eth.ethertype.get() == 0x86DD)
            .and_then(|(_, payload)| Ipv6Header::parse(payload))
            .filter(|(ip, _)| ip.next_header == IP_P_ICMPV6)
            .and_then(|(_, payload)| Icmpv6Header::parse(payload))
            .is_some_and(|(icmpv6, _)| icmpv6.icmpv6_type == Icmpv6Type::EchoReply as u8)
    };
    if let Ok(Some(reply_bytes)) = tokio::time::timeout(
        std::time::Duration::from_millis(500),
        driver.recv_matching(is_echo_reply),
    )
    .await
    {
        let (eth_frame, eth_payload) =
            EthernetFrame::parse(&reply_bytes).expect("Failed to parse Ethernet frame");
        assert_eq!(eth_frame.ethertype.get(), 0x86DD);

        let (ipv6_hdr, ipv6_payload) =
            Ipv6Header::parse(eth_payload).expect("Failed to parse IPv6 header");
        assert_eq!(ipv6_hdr.next_header, IP_P_ICMPV6);
        assert_eq!(Ipv6Addr::from(ipv6_hdr.source_addr), host_ipv6);
        assert_eq!(Ipv6Addr::from(ipv6_hdr.dest_addr), guest_ipv6);

        let (icmpv6_hdr, icmpv6_payload) =
            Icmpv6Header::parse(ipv6_payload).expect("Failed to parse ICMPv6 header");
        assert_eq!(icmpv6_hdr.icmpv6_type, Icmpv6Type::EchoReply as u8);
        assert_eq!(icmpv6_hdr.icmpv6_code, 0);

        let (reply_id, reply_seq) =
            icmpv6_hdr.echo_fields().expect("Failed to extract echo fields");
        assert_eq!(reply_id, test_id);
        assert_eq!(reply_seq, test_seq);
        assert_eq!(icmpv6_payload, payload);

        let computed_checksum = icmpv6_checksum(ipv6_payload, host_ipv6, guest_ipv6);
        assert_eq!(computed_checksum, 0, "ICMPv6 Echo Reply checksum must be valid");
    }
}

#[tokio::test]
async fn test_icmpv6_echo_id_rewriting_fix() {
    for backend in [SlirpBackend::CFfi, SlirpBackend::Native] {
        run_icmpv6_echo_id_rewriting_fix_test(backend).await;
    }
}

// -----------------------------------------------------------------------------
// 3. UDP Zero Checksum (0x0000) Test
// -----------------------------------------------------------------------------

async fn run_udp_zero_checksum_fix_test(backend: SlirpBackend) {
    // Given a Slirp test driver running the specified backend
    let mut driver = SlirpTestDriver::new(backend, None).await;

    // And a UDP datagram constructed with a zero checksum (0x0000) per IPv4 spec
    // RFC 768
    let src_ip = Ipv4Addr::new(10, 0, 2, 15);
    let dst_ip = Ipv4Addr::new(8, 8, 8, 8);
    let src_port = 54321;
    let dst_port = 53;
    let payload = b"udp_zero_checksum_test_data";

    let mut udp_data = vec![0u8; 8 + payload.len()];
    let mut udp_builder = UdpPacketBuilder::new(&mut udp_data, src_port, dst_port).unwrap();
    udp_builder.payload_mut()[..payload.len()].copy_from_slice(payload);
    udp_builder.payload_len(payload.len());
    udp_builder.build(); // UdpPacketBuilder leaves checksum as 0x0000

    // Verify the constructed UDP header has checksum 0x0000
    let (udp_hdr, _) = UdpHeader::parse(&udp_data).unwrap();
    assert_eq!(udp_hdr.checksum.get(), 0, "UDP checksum should be 0x0000");

    let mut ip_data = vec![0u8; 20 + udp_data.len()];
    let mut ipv4_builder = Ipv4Builder::new(&mut ip_data, IP_P_UDP, src_ip, dst_ip).unwrap();
    ipv4_builder.payload(&udp_data).unwrap();
    ipv4_builder.build().unwrap();

    let mut eth_data = vec![0u8; 14 + ip_data.len()];
    let (eth_header_slice, eth_payload_slice) = eth_data.split_at_mut(14);
    let eth_frame = EthernetFrame::mut_from_bytes(eth_header_slice).unwrap();
    eth_frame.dst_addr = MacAddr { bytes: [0x52, 0x54, 0x00, 0x12, 0x34, 0x56] };
    eth_frame.src_addr = MacAddr { bytes: [0x02, 0x15, 0xb2, 0x00, 0x00, 0x00] };
    eth_frame.ethertype = 0x0800.into();
    eth_payload_slice.copy_from_slice(&ip_data);

    let zero_checksum_pkt = bytes::Bytes::from(eth_data);

    // When the guest sends the zero-checksum UDP packet to Slirp
    let send_result = driver.send_packet(zero_checksum_pkt).await;

    // Then Slirp accepts and processes the UDP packet without error
    assert!(send_result.is_ok(), "Slirp must accept UDP datagrams with 0x0000 checksum");
}

#[tokio::test]
async fn test_udp_zero_checksum_fix() {
    for backend in [SlirpBackend::CFfi, SlirpBackend::Native] {
        run_udp_zero_checksum_fix_test(backend).await;
    }
}
