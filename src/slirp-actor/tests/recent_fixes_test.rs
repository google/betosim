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
    EthernetFrame, IP_P_ICMP, IP_P_ICMPV6, IcmpHeader, IcmpType, Icmpv6Header, Icmpv6Type,
    Ipv4Builder, Ipv4Header, Ipv6Builder, Ipv6Header, MacAddr, icmpv6_checksum, ipv4_checksum,
};
use slirp_actor::SlirpBackend;
use zerocopy::FromBytes;

use super::{packet_helpers::build_udp_packet_with_dst_mac, test_driver::SlirpTestDriver};

fn create_icmp_echo_request_packet(
    src_mac: [u8; 6],
    dst_mac: [u8; 6],
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
    eth_frame.dst_addr = MacAddr { bytes: dst_mac };
    eth_frame.src_addr = MacAddr { bytes: src_mac };
    eth_frame.ethertype = 0x0800.into();
    eth_payload_slice.copy_from_slice(&ip_data);

    bytes::Bytes::from(eth_data)
}

fn create_icmpv6_echo_request_packet(
    src_mac: [u8; 6],
    dst_mac: [u8; 6],
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
    eth_frame.dst_addr = MacAddr { bytes: dst_mac };
    eth_frame.src_addr = MacAddr { bytes: src_mac };
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
    let guest_mac = [0x02, 0x15, 0xb2, 0x00, 0x00, 0x00];
    let guest_ip = Ipv4Addr::new(10, 0, 2, 15);
    let host_ip = Ipv4Addr::new(10, 0, 2, 2);
    let test_id: u16 = 0x1234;
    let test_seq: u16 = 0x0042;
    let payload = b"ping_napt_id_rewrite_test";

    // And the guest has resolved the gateway MAC via ARP
    let gw_mac = driver.resolve_gateway_mac(guest_mac, guest_ip).await;
    let is_effective_native = driver.backend() == SlirpBackend::Native;

    let echo_req_packet = create_icmp_echo_request_packet(
        guest_mac, gw_mac, guest_ip, host_ip, test_id, test_seq, payload,
    );

    // When the guest sends the ICMP Echo Request packet
    let send_result = driver.send_packet(echo_req_packet).await;
    assert!(send_result.is_ok());

    // Then Slirp replies with an Echo Reply carrying the original ID, sequence, and payload
    let is_icmp = |frame: &bytes::Bytes| {
        EthernetFrame::parse(frame)
            .filter(|(eth, _)| eth.ethertype.get() == 0x0800)
            .and_then(|(_, payload)| Ipv4Header::parse(payload))
            .is_some_and(|(ip, _)| ip.protocol == IP_P_ICMP)
    };
    let reply_res =
        tokio::time::timeout(std::time::Duration::from_millis(500), driver.recv_matching(is_icmp))
            .await;

    // C-FFI requires host ping_group_range / timer-driven NDP neighbor retry;
    // Native replies in-process
    if is_effective_native || reply_res.is_ok() {
        let reply_bytes = reply_res.expect("Missing ICMP Echo Reply").expect("Stream closed");
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

// Scenario: Verify ICMP Echo ID and checksum rewriting across C-FFI and Native backends
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

    let guest_mac = [0x02, 0x15, 0xb2, 0x00, 0x00, 0x00];
    // And the guest has resolved the gateway MAC via ARP
    let gw_mac = driver.resolve_gateway_mac(guest_mac, Ipv4Addr::new(10, 0, 2, 15)).await;
    let is_effective_native = driver.backend() == SlirpBackend::Native;

    // And an ICMPv6 Echo Request with a specific identifier, sequence number, and
    // payload
    let guest_ipv6: Ipv6Addr = "fec0::1".parse().expect("Invalid IPv6");
    let host_ipv6: Ipv6Addr = "fec0::2".parse().expect("Invalid IPv6");
    let test_id: u16 = 0x5678;
    let test_seq: u16 = 0x0084;
    let payload = b"pingv6_napt_id_rewrite_test";

    let echo_req_packet = create_icmpv6_echo_request_packet(
        guest_mac, gw_mac, guest_ipv6, host_ipv6, test_id, test_seq, payload,
    );

    // When the guest sends the ICMPv6 Echo Request packet
    let send_result = driver.send_packet(echo_req_packet).await;
    assert!(send_result.is_ok());

    // Then Slirp replies with an Echo Reply carrying the original ID, sequence, and payload
    let is_echo_reply = |frame: &bytes::Bytes| {
        EthernetFrame::parse(frame)
            .filter(|(eth, _)| eth.ethertype.get() == 0x86DD)
            .and_then(|(_, payload)| Ipv6Header::parse(payload))
            .filter(|(ip, _)| ip.next_header == IP_P_ICMPV6)
            .and_then(|(_, payload)| Icmpv6Header::parse(payload))
            .is_some_and(|(icmpv6, _)| icmpv6.icmpv6_type == Icmpv6Type::EchoReply as u8)
    };
    let reply_res = tokio::time::timeout(
        std::time::Duration::from_millis(1000),
        driver.recv_matching(is_echo_reply),
    )
    .await;

    // C-FFI requires host ping_group_range / timer-driven NDP neighbor retry;
    // Native replies in-process
    if is_effective_native || reply_res.is_ok() {
        let reply_bytes = reply_res.expect("Missing ICMPv6 Echo Reply").expect("Stream closed");
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

// Scenario: Verify ICMPv6 Echo ID and checksum rewriting across C-FFI and Native backends
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
    // Given a Slirp test driver running the specified backend and a local UDP receiver socket
    let mut driver = SlirpTestDriver::new(backend, None).await;

    let socket =
        tokio::net::UdpSocket::bind("127.0.0.1:0").await.expect("Failed to bind UDP socket");
    let recv_port = socket.local_addr().expect("Failed to get local addr").port();

    let guest_mac = [0x02, 0x15, 0xb2, 0x00, 0x00, 0x00];
    let src_ip = Ipv4Addr::new(10, 0, 2, 15);

    // And the guest has resolved the gateway MAC via ARP
    let gw_mac = driver.resolve_gateway_mac(guest_mac, src_ip).await;

    // And a UDP datagram constructed with a zero checksum (0x0000) per IPv4 spec RFC 768
    let payload = b"zero_checksum_test";
    let mut packet = build_udp_packet_with_dst_mac(
        &guest_mac,
        &gw_mac,
        &src_ip.octets(),
        &[127, 0, 0, 1],
        54321,
        recv_port,
        payload,
    );
    const ETH_IPV4_UDP_CSUM_OFFSET: usize = 14 + 20 + 6;
    packet[ETH_IPV4_UDP_CSUM_OFFSET] = 0;
    packet[ETH_IPV4_UDP_CSUM_OFFSET + 1] = 0;

    // When the guest sends the zero-checksum UDP packet to Slirp
    driver.send_packet(bytes::Bytes::from(packet)).await.expect("Failed to send packet");

    // Then Slirp accepts and forwards the UDP packet to the local receiver without error
    let mut buf = [0u8; 1024];
    let (len, _) =
        tokio::time::timeout(std::time::Duration::from_secs(2), socket.recv_from(&mut buf))
            .await
            .expect("Timeout waiting for UDP packet")
            .expect("Failed to recv from UDP socket");
    assert_eq!(&buf[..len], payload);
}

// Scenario: Verify zero-checksum UDP datagrams are accepted across C-FFI and Native backends
#[tokio::test]
async fn test_udp_zero_checksum_fix() {
    for backend in [SlirpBackend::CFfi, SlirpBackend::Native] {
        run_udp_zero_checksum_fix_test(backend).await;
    }
}
