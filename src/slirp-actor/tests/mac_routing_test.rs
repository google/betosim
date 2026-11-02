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

//! Decoupled Integration Tests: Hardcoded Guest MAC Defect (b/560171262).
//!
//! Verifies Ethernet header destination MAC address handling in response
//! packets when packets originate from a non-default guest MAC address ([0x02,
//! 0x99, 0x88, 0x77, 0x66, 0x55]).

use std::{net::Ipv4Addr, time::Duration};

use netsim_packets::{EthernetFrame, Ipv4Header};
use slirp_actor::SlirpBackend;
use tokio::time::timeout;

use super::{
    packet_helpers::{build_dns_query_packet_with_dst, build_udp_packet_with_dst_mac},
    test_driver::SlirpTestDriver,
};

static TFTP_DIR_COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

struct TempDirGuard(std::path::PathBuf);

impl Drop for TempDirGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

const NON_DEFAULT_MAC: [u8; 6] = [0x02, 0x99, 0x88, 0x77, 0x66, 0x55];

/// Helper to await the next IPv4 UDP downlink frame within 2 seconds.
async fn await_udp_reply(driver: &mut SlirpTestDriver) -> bytes::Bytes {
    let is_udp = |frame: &bytes::Bytes| {
        EthernetFrame::parse(frame)
            .filter(|(eth, _)| eth.ethertype.get() == netsim_packets::ether_type::IPV4)
            .and_then(|(_, payload)| Ipv4Header::parse(payload))
            .is_some_and(|(ip, _)| ip.protocol == netsim_packets::IP_P_UDP)
    };
    timeout(Duration::from_secs(2), driver.recv_matching(is_udp))
        .await
        .expect("Timeout waiting for UDP reply")
        .expect("Stream closed")
}

// -----------------------------------------------------------------------------
// 1. UDP Non-Default MAC Reply Test
// -----------------------------------------------------------------------------

async fn run_udp_non_default_mac_reply_test(backend: SlirpBackend) {
    // Given a Slirp test driver running the specified backend and an echo socket
    let mut driver = SlirpTestDriver::new(backend, None).await;

    let echo_socket =
        tokio::net::UdpSocket::bind("127.0.0.1:0").await.expect("Failed to bind UDP socket");
    let echo_port = echo_socket.local_addr().expect("Failed to get local addr").port();

    tokio::spawn(async move {
        let mut buf = [0u8; 1024];
        let (len, peer) = echo_socket.recv_from(&mut buf).await.expect("Failed to recv_from");
        echo_socket.send_to(&buf[..len], peer).await.expect("Failed to send_to");
    });

    // And the guest has resolved the gateway MAC via ARP
    let gw_mac = driver.resolve_gateway_mac(NON_DEFAULT_MAC, Ipv4Addr::new(10, 0, 2, 15)).await;

    // When the guest sends a UDP packet from a non-default MAC to the echo server
    let payload = [0x12, 0x34, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];
    let udp_packet = build_udp_packet_with_dst_mac(
        &NON_DEFAULT_MAC,
        &gw_mac,
        &[10, 0, 2, 15],
        &[127, 0, 0, 1],
        54321,
        echo_port,
        &payload,
    );
    driver.send_packet(bytes::Bytes::from(udp_packet)).await.expect("Failed to send packet");

    // Then Slirp delivers the UDP downlink reply with destination MAC matching
    // the non-default guest MAC
    let reply_bytes = await_udp_reply(&mut driver).await;
    let (eth_frame, eth_payload) =
        EthernetFrame::parse(&reply_bytes).expect("Failed to parse Ethernet frame");
    assert_eq!(eth_frame.dst_addr.bytes, NON_DEFAULT_MAC);
    assert_eq!(eth_frame.src_addr.bytes, gw_mac);

    let (_, ipv4_payload) = Ipv4Header::parse(eth_payload).expect("Failed to parse IPv4 header");
    let (udp, udp_payload) =
        netsim_packets::UdpHeader::parse(ipv4_payload).expect("Failed to parse UDP header");
    assert_eq!(udp.source_port.get(), echo_port);
    assert_eq!(udp_payload, &payload);
}

// Scenario: Route UDP downlink reply to non-default guest MAC address
#[tokio::test]
async fn test_udp_non_default_mac_reply() {
    for backend in [SlirpBackend::CFfi, SlirpBackend::Native] {
        run_udp_non_default_mac_reply_test(backend).await;
    }
}

// -----------------------------------------------------------------------------
// 2. DNS-Formatted UDP Non-Default MAC Reply Test
//    (via loopback NAT; port-53 DNS proxy is hardcoded)
// -----------------------------------------------------------------------------

async fn run_dns_non_default_mac_reply_test(backend: SlirpBackend) {
    // Given a Slirp test driver running the specified backend and a mock DNS socket
    let mut driver = SlirpTestDriver::new(backend, None).await;

    let dns_socket =
        tokio::net::UdpSocket::bind("127.0.0.1:0").await.expect("Failed to bind UDP socket");
    let dns_port = dns_socket.local_addr().expect("Failed to get local addr").port();

    tokio::spawn(async move {
        let mut buf = [0u8; 1024];
        let (len, peer) = dns_socket.recv_from(&mut buf).await.expect("Failed to recv_from");
        buf[2] = 0x81;
        buf[3] = 0x80;
        dns_socket.send_to(&buf[..len], peer).await.expect("Failed to send_to");
    });

    // And the guest has resolved the gateway MAC via ARP
    let gw_mac = driver.resolve_gateway_mac(NON_DEFAULT_MAC, Ipv4Addr::new(10, 0, 2, 15)).await;

    // When the guest sends a DNS-formatted UDP packet from a non-default MAC to the DNS socket
    let dns_packet = build_dns_query_packet_with_dst(
        &NON_DEFAULT_MAC,
        &gw_mac,
        &[10, 0, 2, 15],
        &[127, 0, 0, 1],
        54321,
        dns_port,
        "example.com",
    );
    driver.send_packet(bytes::Bytes::from(dns_packet)).await.expect("Failed to send packet");

    // Then Slirp delivers the DNS-formatted UDP downlink reply with destination MAC
    // matching the non-default guest MAC
    let reply_bytes = await_udp_reply(&mut driver).await;
    let (eth_frame, eth_payload) =
        EthernetFrame::parse(&reply_bytes).expect("Failed to parse Ethernet frame");
    assert_eq!(eth_frame.dst_addr.bytes, NON_DEFAULT_MAC);
    assert_eq!(eth_frame.src_addr.bytes, gw_mac);

    let (_, ipv4_payload) = Ipv4Header::parse(eth_payload).expect("Failed to parse IPv4 header");
    let (udp, udp_payload) =
        netsim_packets::UdpHeader::parse(ipv4_payload).expect("Failed to parse UDP header");
    assert_eq!(udp.source_port.get(), dns_port);
    assert_eq!(&udp_payload[..4], &[0x12, 0x34, 0x81, 0x80]);
}

// Scenario: Route DNS-formatted UDP downlink reply to non-default guest MAC address
#[tokio::test]
async fn test_dns_non_default_mac_reply() {
    for backend in [SlirpBackend::CFfi, SlirpBackend::Native] {
        run_dns_non_default_mac_reply_test(backend).await;
    }
}

// -----------------------------------------------------------------------------
// 3. TFTP RRQ Non-Default MAC Reply Test
// -----------------------------------------------------------------------------

async fn run_tftp_non_default_mac_reply_test(backend: SlirpBackend) {
    // Given a Slirp test driver running with a local TFTP root directory
    let count = TFTP_DIR_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let temp_dir_path = std::env::temp_dir().join(format!(
        "netsim_tftp_{}_{:?}_{}",
        std::process::id(),
        backend,
        count
    ));
    std::fs::create_dir_all(&temp_dir_path).expect("Failed to create temp dir");
    let temp_dir = TempDirGuard(temp_dir_path);
    std::fs::write(temp_dir.0.join("pxelinux.0"), b"fake_pxelinux")
        .expect("Failed to write TFTP payload");

    let mut driver = SlirpTestDriver::new_with_tftp_dir(backend, temp_dir.0.clone()).await;
    // And the guest has resolved the gateway MAC via ARP
    let gw_mac = driver.resolve_gateway_mac(NON_DEFAULT_MAC, Ipv4Addr::new(10, 0, 2, 15)).await;

    // When the guest sends a TFTP RRQ packet from a non-default MAC
    let mut tftp_rrq = vec![0u8, 1];
    tftp_rrq.extend_from_slice(b"pxelinux.0\0octet\0");

    let rrq_packet = build_udp_packet_with_dst_mac(
        &NON_DEFAULT_MAC,
        &gw_mac,
        &[10, 0, 2, 15],
        &[10, 0, 2, 2],
        55555,
        69,
        &tftp_rrq,
    );
    driver.send_packet(bytes::Bytes::from(rrq_packet)).await.expect("Failed to send TFTP RRQ");

    // Then Slirp delivers TFTP DATA block 1 with destination MAC matching the non-default guest MAC
    let reply_bytes = await_udp_reply(&mut driver).await;
    let (eth_frame, eth_payload) =
        EthernetFrame::parse(&reply_bytes).expect("Failed to parse Ethernet frame");
    assert_eq!(eth_frame.dst_addr.bytes, NON_DEFAULT_MAC);
    assert_eq!(eth_frame.src_addr.bytes, gw_mac);

    let (_, ipv4_payload) = Ipv4Header::parse(eth_payload).expect("Failed to parse IPv4 header");
    let (_, udp_payload) =
        netsim_packets::UdpHeader::parse(ipv4_payload).expect("Failed to parse UDP header");

    assert_eq!(
        &udp_payload[..4],
        &[0x00, 0x03, 0x00, 0x01],
        "Expected TFTP DATA (opcode=3) block 1"
    );
    assert_eq!(&udp_payload[4..], b"fake_pxelinux", "Expected TFTP payload to match");
}

// Scenario: Route TFTP DATA reply to non-default guest MAC address
#[tokio::test]
async fn test_tftp_non_default_mac_reply() {
    for backend in [SlirpBackend::CFfi, SlirpBackend::Native] {
        run_tftp_non_default_mac_reply_test(backend).await;
    }
}
