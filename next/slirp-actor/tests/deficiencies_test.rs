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

//! Decoupled Integration Tests: TCP Window Scale/SACK & UDP Flow Deficiencies
//! (b/560171940, b/560171321).
//!
//! Includes BDD test coverage for:
//! 1. `test_tcp_window_scale_ignored`: TCP SYN packet containing TCP Window
//!    Scale Option (Kind 3, Length 3, Shift 7).
//! 2. `test_tcp_sack_permitted_ignored`: TCP SYN packet containing TCP SACK
//!    Permitted Option (Kind 4, Length 2).
//! 3. `test_udp_flow_churn`: Rapid opening of 1100 distinct UDP flows from
//!    sequential ephemeral ports (20000..21100).

use std::net::Ipv4Addr;

use slirp_actor::SlirpBackend;

use super::{
    packet_helpers::{create_tcp_packet_with_options as create_tcp_packet, create_udp_packet},
    test_driver::SlirpTestDriver,
};

// -----------------------------------------------------------------------------
// 1. TCP Window Scale Option Ignored Test (b/560171940)
// -----------------------------------------------------------------------------

async fn run_tcp_window_scale_ignored_test(backend: SlirpBackend) {
    // Given a Slirp driver initialized with the specified backend
    let mut driver = SlirpTestDriver::new(backend, None).await;

    // When guest sends a TCP SYN packet containing TCP Window Scale Option (Kind 3,
    // Length 3, Shift 7)
    let window_scale_option = [3u8, 3, 7, 1];
    let syn_win_scale_packet = create_tcp_packet(
        Ipv4Addr::new(10, 0, 2, 15),
        Ipv4Addr::new(93, 184, 216, 34),
        12345,
        80,
        1000,  // seq
        0,     // ack
        true,  // syn
        false, // ack_flag
        false, // fin
        false, // rst
        8192,  // window_size
        &window_scale_option,
        &[], // payload
    );
    let result = driver.send_packet(syn_win_scale_packet).await;

    // Then Slirp handles the TCP SYN packet while driver maintains fixed 8192-byte
    // window handling
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_tcp_window_scale_ignored() {
    for backend in [SlirpBackend::CFfi, SlirpBackend::Native] {
        run_tcp_window_scale_ignored_test(backend).await;
    }
}

// -----------------------------------------------------------------------------
// 2. TCP SACK Permitted Option Ignored Test (b/560171940)
// -----------------------------------------------------------------------------

async fn run_tcp_sack_permitted_ignored_test(backend: SlirpBackend) {
    // Given a Slirp driver initialized with the specified backend
    let mut driver = SlirpTestDriver::new(backend, None).await;

    // When guest sends a TCP SYN packet containing TCP SACK Permitted Option (Kind
    // 4, Length 2)
    let sack_permitted_option = [4u8, 2, 1, 1];
    let syn_sack_packet = create_tcp_packet(
        Ipv4Addr::new(10, 0, 2, 15),
        Ipv4Addr::new(93, 184, 216, 34),
        12345,
        80,
        1000,  // seq
        0,     // ack
        true,  // syn
        false, // ack_flag
        false, // fin
        false, // rst
        8192,  // window_size
        &sack_permitted_option,
        &[], // payload
    );
    let result = driver.send_packet(syn_sack_packet).await;

    // Then Slirp handles the TCP SYN packet with SACK Permitted option cleanly
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_tcp_sack_permitted_ignored() {
    for backend in [SlirpBackend::CFfi, SlirpBackend::Native] {
        run_tcp_sack_permitted_ignored_test(backend).await;
    }
}

// -----------------------------------------------------------------------------
// 3. UDP Flow Churn Test (b/560171321)
// -----------------------------------------------------------------------------

async fn run_udp_flow_churn_test(backend: SlirpBackend) {
    // Given a Slirp driver initialized with the specified backend
    let mut driver = SlirpTestDriver::new(backend, None).await;

    // When guest rapidly opens 1100 distinct UDP flows from sequential ephemeral
    // ports (20000..21100) to exceed MAX_UDP_FLOWS (1024) and trigger eviction
    let src_ip = Ipv4Addr::new(10, 0, 2, 15);
    let dst_ip = Ipv4Addr::new(93, 184, 216, 34);
    let dst_port = 5353;
    let payload = b"UDP flow churn datagram payload";

    for port in 20000..21100 {
        let udp_packet = create_udp_packet(src_ip, dst_ip, port, dst_port, payload);
        let result = driver.send_packet(udp_packet).await;

        // Then datagrams are sent through driver to test UDP flow tracking
        assert!(result.is_ok());
    }
}

#[tokio::test]
async fn test_udp_flow_churn() {
    for backend in [SlirpBackend::CFfi, SlirpBackend::Native] {
        run_udp_flow_churn_test(backend).await;
    }
}
