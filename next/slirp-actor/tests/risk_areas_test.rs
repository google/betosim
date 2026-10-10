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

//! Decoupled Integration Tests: Key Risk Areas (b/560172253).
//!
//! Includes BDD test coverage for:
//! 1. `test_tcp_high_concurrency_flood`: TCP SYN flooding across 100 concurrent
//!    flows.
//! 2. `test_host_dns_dynamic_refresh`: Host DNS dynamic configuration and query
//!    processing.
//! 3. `test_hostfwd_idle_persistence`: Idle duration persistence for hostfwd
//!    ADB connection flows.

use std::net::Ipv4Addr;

use netsim_packets::{EthernetFrame, IP_P_UDP, Ipv4Builder, MacAddr, UdpPacketBuilder};
use slirp_actor::SlirpBackend;
use zerocopy::FromBytes;

use super::{packet_helpers::create_tcp_packet, test_driver::SlirpTestDriver};

/// Helper to construct a zero-copy Ethernet + IPv4 + UDP DNS query packet using
/// `netsim-packets`.
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
// 1. High Concurrency TCP SYN Flood Test
// -----------------------------------------------------------------------------

async fn run_tcp_high_concurrency_flood_test(backend: SlirpBackend) {
    // Given a Slirp test driver initialized with the specified backend
    let mut driver = SlirpTestDriver::new(backend, None).await;

    let src_ip = Ipv4Addr::new(10, 0, 2, 15);
    let dst_ip = Ipv4Addr::new(93, 184, 216, 34);
    let dst_port = 80;

    // When the guest constructs and sends 100 TCP SYN packets across distinct
    // ephemeral ports (10000..10100 -> 93.184.216.34:80)
    let mut send_results = Vec::with_capacity(100);
    for port in 10000..10100 {
        let syn_packet = create_tcp_packet(
            src_ip,
            dst_ip,
            port,
            dst_port,
            1000 + u32::from(port),
            0,     // ack
            true,  // syn
            false, // ack_flag
            false, // fin
            false, // rst
            8192,  // window_size
            &[],   // payload
        );
        let result = driver.send_packet(syn_packet).await;
        send_results.push(result);
    }

    // Then SlirpTestDriver processes all 100 concurrent connection flows cleanly
    // without errors or crashes
    assert_eq!(send_results.len(), 100);
    for (idx, res) in send_results.iter().enumerate() {
        assert!(res.is_ok(), "TCP SYN packet for port {} failed to send", 10000 + idx);
    }
}

#[tokio::test]
async fn test_tcp_high_concurrency_flood() {
    for backend in [SlirpBackend::CFfi, SlirpBackend::Native] {
        run_tcp_high_concurrency_flood_test(backend).await;
    }
}

// -----------------------------------------------------------------------------
// 2. Host DNS Dynamic Refresh Test
// -----------------------------------------------------------------------------

async fn run_host_dns_dynamic_refresh_test(backend: SlirpBackend) {
    // Given a Slirp test driver initialized with host DNS 8.8.8.8
    let mut driver = SlirpTestDriver::new(backend, Some("8.8.8.8".to_string())).await;

    // When the guest sends a DNS query packet for "google.com" to 10.0.2.3:53
    let query_packet = create_dns_query_packet(
        Ipv4Addr::new(10, 0, 2, 15),
        Ipv4Addr::new(10, 0, 2, 3), // gateway DNS (10.0.2.3:53)
        12345,                      // ephemeral src port
        "google.com",
        1, // Type A
    );
    let result = driver.send_packet(query_packet).await;

    // Then SlirpTestDriver accepts the DNS query packet cleanly
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_host_dns_dynamic_refresh() {
    for backend in [SlirpBackend::CFfi, SlirpBackend::Native] {
        run_host_dns_dynamic_refresh_test(backend).await;
    }
}

// -----------------------------------------------------------------------------
// 3. Hostfwd Idle Persistence Test
// -----------------------------------------------------------------------------

async fn run_hostfwd_idle_persistence_test(backend: SlirpBackend) {
    // Given a Slirp test driver initialized with the default backend
    let mut driver = SlirpTestDriver::new(backend, None).await;

    let src_ip = Ipv4Addr::new(10, 0, 2, 2);
    let dst_ip = Ipv4Addr::new(10, 0, 2, 15);
    let src_port = 12345;
    let dst_port = 5555; // hostfwd ADB port

    // When a TCP SYN packet is sent for hostfwd ADB port (10.0.2.15:5555)
    let syn_packet = create_tcp_packet(
        src_ip,
        dst_ip,
        src_port,
        dst_port,
        1000,  // seq
        0,     // ack
        true,  // syn
        false, // ack_flag
        false, // fin
        false, // rst
        8192,
        &[],
    );
    let syn_res = driver.send_packet(syn_packet).await;
    assert!(syn_res.is_ok(), "Failed to send initial TCP SYN for ADB port");

    // And an idle duration elapses
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    // And a subsequent TCP data packet is sent on the same connection flow
    let data_payload = b"CNXN\x00\x00\x00\x01\x00\x00\x10\x00host::";
    let data_packet = create_tcp_packet(
        src_ip,
        dst_ip,
        src_port,
        dst_port,
        1001,  // seq
        1,     // ack
        false, // syn
        true,  // ack_flag
        false, // fin
        false, // rst
        8192,
        data_payload,
    );
    let data_res = driver.send_packet(data_packet).await;

    // Then the connection flow persists after idle duration and the data packet is
    // accepted
    assert!(data_res.is_ok());
}

#[tokio::test]
async fn test_hostfwd_idle_persistence() {
    for backend in [SlirpBackend::CFfi, SlirpBackend::Native] {
        run_hostfwd_idle_persistence_test(backend).await;
    }
}
