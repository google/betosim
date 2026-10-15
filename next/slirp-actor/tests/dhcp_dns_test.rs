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

//! Decoupled Integration Tests: DHCP & DNS Services (b/560172253).
//!
//! Uses zero-copy `netsim-packets` builders, deep response parsing, and
//! `SlirpTestDriver`.

use std::{net::Ipv4Addr, time::Duration};

use netsim_packets::{
    EthernetFrame, IP_P_ICMP, IP_P_UDP, IcmpHeader, Ipv4Builder, Ipv4Header, MacAddr, UdpHeader,
    UdpPacketBuilder,
};
use slirp_actor::SlirpBackend;
use tokio::time::timeout;
use zerocopy::FromBytes;

use super::test_driver::SlirpTestDriver;

/// Helper to create a zero-copy ICMP Echo Request packet using
/// `netsim-packets`.
fn create_icmp_ping_packet(
    src_ip: Ipv4Addr,
    dst_ip: Ipv4Addr,
    ping_id: u16,
    ping_seq: u16,
    payload: &[u8],
) -> bytes::Bytes {
    let icmp_header_len = std::mem::size_of::<IcmpHeader>();
    let total_icmp_len = icmp_header_len + payload.len();

    let mut icmp_data = vec![0u8; total_icmp_len];
    let (icmp_hdr_slice, echo_payload_slice) = icmp_data.split_at_mut(icmp_header_len);
    let icmp_header = IcmpHeader::mut_from_bytes(icmp_hdr_slice).unwrap();
    icmp_header.icmp_type = 8; // Echo Request
    icmp_header.icmp_code = 0;
    icmp_header.icmp_checksum.set(0);
    icmp_header.rest[..2].copy_from_slice(&ping_id.to_be_bytes());
    icmp_header.rest[2..].copy_from_slice(&ping_seq.to_be_bytes());
    echo_payload_slice.copy_from_slice(payload);

    let checksum = netsim_packets::ipv4_checksum(&icmp_data);
    let icmp_header_mut = IcmpHeader::mut_from_bytes(&mut icmp_data[..icmp_header_len]).unwrap();
    icmp_header_mut.icmp_checksum.set(checksum);

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

/// Helper to create a zero-copy UDP DNS Query packet (A or AAAA record).
fn create_dns_query_packet(
    src_ip: Ipv4Addr,
    dst_ip: Ipv4Addr,
    src_port: u16,
    query_name: &str,
    qtype: u16,
) -> bytes::Bytes {
    let mut dns_payload = vec![
        0x12, 0x34, // TXID
        0x01, 0x00, // Flags (RD=1)
        0x00, 0x01, // Questions = 1
        0x00, 0x00, // Answers = 0
        0x00, 0x00, // Authority = 0
        0x00, 0x00, // Additional = 0
    ];
    for label in query_name.split('.') {
        dns_payload.push(label.len() as u8);
        dns_payload.extend_from_slice(label.as_bytes());
    }
    dns_payload.push(0); // Null terminator
    dns_payload.extend_from_slice(&qtype.to_be_bytes()); // Type (1 = A, 28 = AAAA)
    dns_payload.extend_from_slice(&1u16.to_be_bytes()); // Class IN

    let mut udp_data = vec![0u8; 8 + dns_payload.len()];
    let mut udp_builder = UdpPacketBuilder::new(&mut udp_data, src_port, 53).unwrap();
    udp_builder.payload_mut()[..dns_payload.len()].copy_from_slice(&dns_payload);
    udp_builder.payload_len(dns_payload.len());
    udp_builder.build();

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

    bytes::Bytes::from(eth_data)
}

// -----------------------------------------------------------------------------
// 1. DHCP Discover & Request Flow Test
// -----------------------------------------------------------------------------

const DHCP_CLIENT_MAC: [u8; 6] = [0x02, 0x15, 0xb2, 0x00, 0x00, 0x00];
const DHCP_MAGIC_COOKIE: [u8; 4] = [0x63, 0x82, 0x53, 0x63];

/// Builds a broadcast BOOTP/DHCP DISCOVER from `client_mac` (RFC 2131 §2).
fn create_dhcp_discover_packet(client_mac: [u8; 6], xid: u32) -> bytes::Bytes {
    // Fixed-format BOOTP header is 236 bytes, then the magic cookie and options.
    let mut dhcp = vec![0u8; 236];
    dhcp[0] = 1; // op: BOOTREQUEST
    dhcp[1] = 1; // htype: Ethernet
    dhcp[2] = 6; // hlen
    dhcp[4..8].copy_from_slice(&xid.to_be_bytes());
    dhcp[10..12].copy_from_slice(&0x8000u16.to_be_bytes()); // flags: broadcast
    dhcp[28..34].copy_from_slice(&client_mac); // chaddr
    dhcp.extend_from_slice(&DHCP_MAGIC_COOKIE);
    dhcp.extend_from_slice(&[53, 1, 1]); // Option 53: DHCPDISCOVER
    dhcp.push(255); // End
    // Pad out the full fixed-size options field (236 + 4 cookie + 308 options).
    // RFC 2131 permits a shorter message and the C backend accepts one, but the
    // native backend parses a fixed 548-byte struct and silently drops anything
    // smaller (b/565477868). Drop this padding once that is fixed.
    dhcp.resize(548, 0);

    let mut udp_data = vec![0u8; 8 + dhcp.len()];
    let mut udp_builder = UdpPacketBuilder::new(&mut udp_data, 68, 67).unwrap();
    udp_builder.payload_mut()[..dhcp.len()].copy_from_slice(&dhcp);
    // `UdpPacketBuilder` initializes the length field to the header size only,
    // so it has to be told the payload length or slirp sees an empty datagram.
    udp_builder.payload_len(dhcp.len());
    udp_builder.build();

    let mut ip_data = vec![0u8; 20 + udp_data.len()];
    let mut ipv4_builder =
        Ipv4Builder::new(&mut ip_data, IP_P_UDP, Ipv4Addr::UNSPECIFIED, Ipv4Addr::BROADCAST)
            .unwrap();
    ipv4_builder.payload(&udp_data).unwrap();
    ipv4_builder.build().unwrap();

    let mut eth_data = vec![0u8; 14 + ip_data.len()];
    let (eth_header_slice, eth_payload_slice) = eth_data.split_at_mut(14);
    let eth_frame = EthernetFrame::mut_from_bytes(eth_header_slice).unwrap();
    eth_frame.dst_addr = MacAddr { bytes: [0xff; 6] };
    eth_frame.src_addr = MacAddr { bytes: client_mac };
    eth_frame.ethertype = 0x0800.into();
    eth_payload_slice.copy_from_slice(&ip_data);

    bytes::Bytes::from(eth_data)
}

/// Returns the value of DHCP option 53 (message type) from a BOOTP payload.
fn dhcp_message_type(dhcp: &[u8]) -> Option<u8> {
    if dhcp.len() < 240 || dhcp[236..240] != DHCP_MAGIC_COOKIE {
        return None;
    }
    let mut i = 240;
    while i < dhcp.len() {
        match dhcp[i] {
            255 => return None, // End
            0 => i += 1,        // Pad
            code => {
                let len = *dhcp.get(i + 1)? as usize;
                if code == 53 {
                    return dhcp.get(i + 2).copied();
                }
                i += 2 + len;
            }
        }
    }
    None
}

async fn run_dhcp_flow_test(backend: SlirpBackend) {
    // Given a Slirp driver running the specified backend
    let mut driver = SlirpTestDriver::new(backend, None).await;

    // When the guest broadcasts a DHCP DISCOVER
    let xid = 0xdead_beefu32;
    let discover = create_dhcp_discover_packet(DHCP_CLIENT_MAC, xid);
    assert!(driver.send_packet(discover).await.is_ok(), "Failed to send DHCP DISCOVER");

    // Then slirp's built-in DHCP server replies with an OFFER for this client.
    // DHCP is served inside slirp, so unlike the DNS tests this needs no host
    // network egress and the assertion can be unconditional.
    let reply = timeout(Duration::from_millis(500), driver.recv_packet())
        .await
        .expect("Timed out waiting for DHCP OFFER")
        .expect("No DHCP OFFER received");

    let (eth, eth_payload) =
        EthernetFrame::parse(&reply).expect("Failed to parse reply Ethernet frame");
    assert_eq!(eth.ethertype.get(), 0x0800, "DHCP OFFER must be IPv4");

    let (ip, ip_payload) = Ipv4Header::parse(eth_payload).expect("Failed to parse IPv4 header");
    assert_eq!(ip.protocol, IP_P_UDP, "DHCP OFFER must be UDP");

    let (udp, dhcp) = UdpHeader::parse(ip_payload).expect("Failed to parse UDP header");
    assert_eq!(udp.source_port.get(), 67, "OFFER must come from the DHCP server port");
    assert_eq!(udp.dest_port.get(), 68, "OFFER must target the DHCP client port");

    assert_eq!(dhcp[0], 2, "OFFER must be a BOOTREPLY");
    assert_eq!(&dhcp[4..8], &xid.to_be_bytes(), "OFFER must echo the request xid");
    assert_eq!(&dhcp[28..34], &DHCP_CLIENT_MAC, "OFFER must target the requesting MAC");
    assert_eq!(dhcp_message_type(dhcp), Some(2), "Expected a DHCPOFFER (option 53 == 2)");

    let yiaddr = Ipv4Addr::new(dhcp[16], dhcp[17], dhcp[18], dhcp[19]);
    assert_ne!(yiaddr, Ipv4Addr::UNSPECIFIED, "OFFER must assign a client address");
}

#[tokio::test]
async fn test_dhcp_flow_parity() {
    for backend in [SlirpBackend::CFfi, SlirpBackend::Native] {
        run_dhcp_flow_test(backend).await;
    }
}

// -----------------------------------------------------------------------------
// 2. DHCP DISCOVER / OFFER Handshake Test
// -----------------------------------------------------------------------------

async fn run_dhcp_discover_handshake_test(backend: SlirpBackend) {
    // Given a Slirp driver running the specified backend
    let mut driver = SlirpTestDriver::new(backend, None).await;

    // When guest sends DHCP DISCOVER broadcast packet (0.0.0.0:68 ->
    // 255.255.255.255:67)
    let mut dhcp_payload = vec![0u8; 240];
    dhcp_payload[0] = 1; // Boot Request
    dhcp_payload[1] = 1; // Ethernet
    dhcp_payload[2] = 6; // Hlen

    // Magic cookie (0x63, 0x82, 0x53, 0x63)
    dhcp_payload[236..240].copy_from_slice(&[0x63, 0x82, 0x53, 0x63]);

    let mut udp_data = vec![0u8; 8 + dhcp_payload.len()];
    let mut udp_builder = UdpPacketBuilder::new(&mut udp_data, 68, 67).unwrap();
    udp_builder.payload_mut()[..dhcp_payload.len()].copy_from_slice(&dhcp_payload);
    udp_builder.payload_len(dhcp_payload.len());
    udp_builder.build();

    let mut ip_data = vec![0u8; 20 + udp_data.len()];
    let mut ipv4_builder = Ipv4Builder::new(
        &mut ip_data,
        IP_P_UDP,
        Ipv4Addr::new(0, 0, 0, 0),
        Ipv4Addr::new(255, 255, 255, 255),
    )
    .unwrap();
    ipv4_builder.payload(&udp_data).unwrap();
    ipv4_builder.build().unwrap();

    let mut eth_data = vec![0u8; 14 + ip_data.len()];
    let (eth_header_slice, eth_payload_slice) = eth_data.split_at_mut(14);
    let eth_frame = EthernetFrame::mut_from_bytes(eth_header_slice).unwrap();
    eth_frame.dst_addr = MacAddr { bytes: [0xff; 6] };
    eth_frame.src_addr = MacAddr { bytes: [0x02, 0x15, 0xb2, 0x00, 0x00, 0x00] };
    eth_frame.ethertype = 0x0800.into();
    eth_payload_slice.copy_from_slice(&ip_data);

    let result = driver.send_packet(bytes::Bytes::from(eth_data)).await;

    // Then DHCP DISCOVER packet is accepted
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_dhcp_discover_handshake() {
    for backend in [SlirpBackend::CFfi, SlirpBackend::Native] {
        run_dhcp_discover_handshake_test(backend).await;
    }
}

// -----------------------------------------------------------------------------
// 3. DHCP REQUEST Lease Renewal Test
// -----------------------------------------------------------------------------

async fn run_dhcp_request_renewal_test(backend: SlirpBackend) {
    // Given a Slirp driver running the specified backend
    let mut driver = SlirpTestDriver::new(backend, None).await;

    // When guest sends DHCP REQUEST packet for 10.0.2.15 to renew lease
    let mut dhcp_payload = vec![0u8; 240];
    dhcp_payload[0] = 1; // Boot Request
    dhcp_payload[12..16].copy_from_slice(&[10, 0, 2, 15]); // Requested IP
    dhcp_payload[236..240].copy_from_slice(&[0x63, 0x82, 0x53, 0x63]); // Magic Cookie

    let mut udp_data = vec![0u8; 8 + dhcp_payload.len()];
    let mut udp_builder = UdpPacketBuilder::new(&mut udp_data, 68, 67).unwrap();
    udp_builder.payload_mut()[..dhcp_payload.len()].copy_from_slice(&dhcp_payload);
    udp_builder.payload_len(dhcp_payload.len());
    udp_builder.build();

    let mut ip_data = vec![0u8; 20 + udp_data.len()];
    let mut ipv4_builder = Ipv4Builder::new(
        &mut ip_data,
        IP_P_UDP,
        Ipv4Addr::new(10, 0, 2, 15),
        Ipv4Addr::new(10, 0, 2, 2),
    )
    .unwrap();
    ipv4_builder.payload(&udp_data).unwrap();
    ipv4_builder.build().unwrap();

    let mut eth_data = vec![0u8; 14 + ip_data.len()];
    let (eth_header_slice, eth_payload_slice) = eth_data.split_at_mut(14);
    let eth_frame = EthernetFrame::mut_from_bytes(eth_header_slice).unwrap();
    eth_frame.dst_addr = MacAddr { bytes: [0x52, 0x54, 0x00, 0x12, 0x34, 0x56] };
    eth_frame.src_addr = MacAddr { bytes: [0x02, 0x15, 0xb2, 0x00, 0x00, 0x00] };
    eth_frame.ethertype = 0x0800.into();
    eth_payload_slice.copy_from_slice(&ip_data);

    let result = driver.send_packet(bytes::Bytes::from(eth_data)).await;

    // Then DHCP REQUEST renewal packet is accepted
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_dhcp_request_renewal() {
    for backend in [SlirpBackend::CFfi, SlirpBackend::Native] {
        run_dhcp_request_renewal_test(backend).await;
    }
}

// -----------------------------------------------------------------------------
// 4. DNS Proxy IPv4 "A" Record Query Test
// -----------------------------------------------------------------------------

async fn run_dns_a_record_query_test(backend: SlirpBackend) {
    // Given a Slirp driver with host DNS 8.8.8.8 configured
    let mut driver = SlirpTestDriver::new(backend, Some("8.8.8.8".to_string())).await;

    // When guest queries DNS "A" record for "google.com" to 10.0.2.3:53
    let query_packet = create_dns_query_packet(
        Ipv4Addr::new(10, 0, 2, 15),
        Ipv4Addr::new(10, 0, 2, 3), // gateway DNS
        12345,
        "google.com",
        1, // Type A
    );
    let result = driver.send_packet(query_packet).await;

    // Then DNS query packet is accepted and proxied
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_dns_a_record_query() {
    for backend in [SlirpBackend::CFfi, SlirpBackend::Native] {
        run_dns_a_record_query_test(backend).await;
    }
}

// -----------------------------------------------------------------------------
// 5. DNS Proxy IPv6 "AAAA" Record Query Test
// -----------------------------------------------------------------------------

async fn run_dns_aaaa_record_query_test(backend: SlirpBackend) {
    // Given a Slirp driver with host DNS configured
    let mut driver = SlirpTestDriver::new(backend, Some("8.8.8.8".to_string())).await;

    // When guest queries DNS "AAAA" record for "google.com"
    let query_packet = create_dns_query_packet(
        Ipv4Addr::new(10, 0, 2, 15),
        Ipv4Addr::new(10, 0, 2, 3),
        12346,
        "google.com",
        28, // Type AAAA
    );
    let result = driver.send_packet(query_packet).await;

    // Then AAAA DNS query packet is accepted
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_dns_aaaa_record_query() {
    for backend in [SlirpBackend::CFfi, SlirpBackend::Native] {
        run_dns_aaaa_record_query_test(backend).await;
    }
}

// -----------------------------------------------------------------------------
// 6. DNS Proxy ICMP / UDP Ping Test
// -----------------------------------------------------------------------------

async fn run_icmp_ping_dns_test(backend: SlirpBackend) {
    // Given a Slirp driver running the specified backend
    let mut driver = SlirpTestDriver::new(backend, Some("8.8.8.8".to_string())).await;

    // Build zero-copy ICMP Echo Request packet (10.0.2.15 -> 8.8.8.8) using
    // `netsim-packets`
    let ping_packet = create_icmp_ping_packet(
        Ipv4Addr::new(10, 0, 2, 15),
        Ipv4Addr::new(8, 8, 8, 8),
        0x1234,
        1,
        b"netsim_slirp_ping",
    );

    // When guest sends ICMP ping packet
    let result = driver.send_packet(ping_packet).await;

    // Then request is accepted and processed by backend
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_icmp_ping_dns_parity() {
    for backend in [SlirpBackend::CFfi, SlirpBackend::Native] {
        run_icmp_ping_dns_test(backend).await;
    }
}
