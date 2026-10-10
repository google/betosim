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

//! Integration Tests: Layer 4 NAPT (TCP, UDP, & ICMP) Forwarding (b/560172253).

use std::net::Ipv4Addr;

use slirp_actor::SlirpBackend;

use super::{
    packet_helpers::{create_tcp_packet, create_udp_packet},
    test_driver::SlirpTestDriver,
};

// -----------------------------------------------------------------------------
// 1. TCP Outbound Handshake Test
// -----------------------------------------------------------------------------

async fn run_tcp_handshake_test(backend: SlirpBackend) {
    // Given a Slirp driver running the specified backend
    let mut driver = SlirpTestDriver::new(backend, None).await;

    // When guest sends TCP SYN (10.0.2.15:12345 -> 93.184.216.34:80)
    let syn_packet = create_tcp_packet(
        Ipv4Addr::new(10, 0, 2, 15),
        Ipv4Addr::new(93, 184, 216, 34),
        12345,
        80,
        1000,
        0,
        true,  // syn
        false, // ack
        false, // fin
        false, // rst
        8192,
        &[],
    );
    let result = driver.send_packet(syn_packet).await;

    // Then Slirp processes the SYN packet
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_tcp_handshake() {
    for backend in [SlirpBackend::CFfi, SlirpBackend::Native] {
        run_tcp_handshake_test(backend).await;
    }
}

// -----------------------------------------------------------------------------
// 2. TCP Graceful Teardown (FIN/ACK) Test
// -----------------------------------------------------------------------------

async fn run_tcp_fin_teardown_test(backend: SlirpBackend) {
    // Given a Slirp driver running the specified backend
    let mut driver = SlirpTestDriver::new(backend, None).await;

    // When guest sends TCP FIN/ACK packet
    let fin_packet = create_tcp_packet(
        Ipv4Addr::new(10, 0, 2, 15),
        Ipv4Addr::new(93, 184, 216, 34),
        12345,
        80,
        1001,
        2001,
        false, // syn
        true,  // ack
        true,  // fin
        false, // rst
        8192,
        &[],
    );
    let result = driver.send_packet(fin_packet).await;

    // Then Slirp accepts FIN teardown packet
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_tcp_fin_teardown() {
    for backend in [SlirpBackend::CFfi, SlirpBackend::Native] {
        run_tcp_fin_teardown_test(backend).await;
    }
}

// -----------------------------------------------------------------------------
// 3. TCP RST Abort Test
// -----------------------------------------------------------------------------

async fn run_tcp_rst_abort_test(backend: SlirpBackend) {
    // Given a Slirp driver running the specified backend
    let mut driver = SlirpTestDriver::new(backend, None).await;

    // When guest sends TCP RST packet
    let rst_packet = create_tcp_packet(
        Ipv4Addr::new(10, 0, 2, 15),
        Ipv4Addr::new(93, 184, 216, 34),
        12345,
        80,
        1002,
        0,
        false, // syn
        false, // ack
        false, // fin
        true,  // rst
        8192,
        &[],
    );
    let result = driver.send_packet(rst_packet).await;

    // Then Slirp processes TCP RST reset
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_tcp_rst_abort() {
    for backend in [SlirpBackend::CFfi, SlirpBackend::Native] {
        run_tcp_rst_abort_test(backend).await;
    }
}

// -----------------------------------------------------------------------------
// 4. UDP Datagram Outbound Test
// -----------------------------------------------------------------------------

async fn run_udp_outbound_test(backend: SlirpBackend) {
    // Given a Slirp driver running the specified backend
    let mut driver = SlirpTestDriver::new(backend, None).await;

    // When guest sends UDP datagram (10.0.2.15:54321 -> 8.8.8.8:53)
    let udp_packet = create_udp_packet(
        Ipv4Addr::new(10, 0, 2, 15),
        Ipv4Addr::new(8, 8, 8, 8),
        54321,
        53,
        b"hello_udp_dns",
    );
    let result = driver.send_packet(udp_packet).await;

    // Then Slirp processes the UDP datagram
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_udp_outbound() {
    for backend in [SlirpBackend::CFfi, SlirpBackend::Native] {
        run_udp_outbound_test(backend).await;
    }
}

// -----------------------------------------------------------------------------
// 5. UDP Concurrent Ports Test
// -----------------------------------------------------------------------------

async fn run_udp_concurrent_ports_test(backend: SlirpBackend) {
    // Given a Slirp driver running the specified backend
    let mut driver = SlirpTestDriver::new(backend, None).await;

    // When guest sends UDP datagrams across 3 distinct ephemeral ports
    for port in [10001, 10002, 10003] {
        let packet = create_udp_packet(
            Ipv4Addr::new(10, 0, 2, 15),
            Ipv4Addr::new(8, 8, 8, 8),
            port,
            53,
            b"concurrent_udp_flow",
        );
        let result = driver.send_packet(packet).await;
        assert!(result.is_ok());
    }
}

#[tokio::test]
async fn test_udp_concurrent_ports() {
    for backend in [SlirpBackend::CFfi, SlirpBackend::Native] {
        run_udp_concurrent_ports_test(backend).await;
    }
}

// -----------------------------------------------------------------------------
// 6. UDP Broadcast Test
// -----------------------------------------------------------------------------

async fn run_udp_broadcast_test(backend: SlirpBackend) {
    // Given a Slirp driver running the specified backend
    let mut driver = SlirpTestDriver::new(backend, None).await;

    // When guest sends UDP broadcast to 255.255.255.255:67
    let broadcast_packet = create_udp_packet(
        Ipv4Addr::new(0, 0, 0, 0),
        Ipv4Addr::new(255, 255, 255, 255),
        68,
        67,
        b"dhcp_discover_broadcast",
    );
    let result = driver.send_packet(broadcast_packet).await;

    // Then Slirp processes the broadcast packet
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_udp_broadcast() {
    for backend in [SlirpBackend::CFfi, SlirpBackend::Native] {
        run_udp_broadcast_test(backend).await;
    }
}
