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

//! Integration Tests: TCP Options & Advanced Window Handling (b/560172253).

use std::net::Ipv4Addr;

use slirp_actor::SlirpBackend;

use super::test_driver::{SlirpTestDriver, create_tcp_packet};

// -----------------------------------------------------------------------------
// 1. TCP MSS Option Handling Test
// -----------------------------------------------------------------------------

async fn run_tcp_mss_option_test(backend: SlirpBackend) {
    // Given a Slirp driver running the specified backend
    let mut driver = SlirpTestDriver::new(backend, None).await;

    // When guest sends TCP SYN packet with TCP Option 2 (MSS = 1460 bytes)
    let mss_option = [2u8, 4, 0x05, 0xb4];
    let syn_mss_packet = create_tcp_packet(
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
        &mss_option,
        &[], // payload
    );
    let result = driver.send_packet(syn_mss_packet).await;

    // Then Slirp accepts the TCP SYN packet with MSS option
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_tcp_mss_option() {
    for backend in [SlirpBackend::CFfi, SlirpBackend::Native] {
        run_tcp_mss_option_test(backend).await;
    }
}

// -----------------------------------------------------------------------------
// 2. TCP Zero Window Probe Test
// -----------------------------------------------------------------------------

async fn run_tcp_zero_window_probe_test(backend: SlirpBackend) {
    // Given a Slirp driver running the specified backend
    let mut driver = SlirpTestDriver::new(backend, None).await;

    // When guest sends a TCP packet announcing Window Size = 0
    let zero_win_packet = create_tcp_packet(
        Ipv4Addr::new(10, 0, 2, 15),
        Ipv4Addr::new(93, 184, 216, 34),
        12345,
        80,
        1000,  // seq
        2000,  // ack
        false, // syn
        true,  // ack_flag
        false, // fin
        false, // rst
        0,     // window_size = 0
        &[],   // options
        &[],   // payload
    );
    let result1 = driver.send_packet(zero_win_packet).await;

    // And guest sends a 1-byte TCP window probe packet
    let probe_packet = create_tcp_packet(
        Ipv4Addr::new(10, 0, 2, 15),
        Ipv4Addr::new(93, 184, 216, 34),
        12345,
        80,
        1000,  // seq
        2000,  // ack
        false, // syn
        true,  // ack_flag
        false, // fin
        false, // rst
        0,     // window_size = 0
        &[],   // options
        b"X",  // 1-byte window probe payload
    );
    let result2 = driver.send_packet(probe_packet).await;

    // Then Slirp handles the zero-window announcement and window probe cleanly
    assert!(result1.is_ok());
    assert!(result2.is_ok());
}

#[tokio::test]
async fn test_tcp_zero_window_probe() {
    for backend in [SlirpBackend::CFfi, SlirpBackend::Native] {
        run_tcp_zero_window_probe_test(backend).await;
    }
}

// -----------------------------------------------------------------------------
// 3. TCP Out-of-Order Segments Test
// -----------------------------------------------------------------------------

async fn run_tcp_out_of_order_segments_test(backend: SlirpBackend) {
    // Given a Slirp driver running the specified backend
    let mut driver = SlirpTestDriver::new(backend, None).await;

    // When guest sends an out-of-order TCP data segment (seq 2000..3000)
    let payload_2000_3000 = vec![0xAA; 1000];
    let segment_2000 = create_tcp_packet(
        Ipv4Addr::new(10, 0, 2, 15),
        Ipv4Addr::new(93, 184, 216, 34),
        12345,
        80,
        2000,  // seq 2000 (covers 2000..3000)
        100,   // ack
        false, // syn
        true,  // ack_flag
        false, // fin
        false, // rst
        8192,  // window_size
        &[],   // options
        &payload_2000_3000,
    );
    let result1 = driver.send_packet(segment_2000).await;

    // And guest sends the preceding missing TCP data segment (seq 1000..2000)
    let payload_1000_2000 = vec![0xBB; 1000];
    let segment_1000 = create_tcp_packet(
        Ipv4Addr::new(10, 0, 2, 15),
        Ipv4Addr::new(93, 184, 216, 34),
        12345,
        80,
        1000,  // seq 1000 (covers 1000..2000)
        100,   // ack
        false, // syn
        true,  // ack_flag
        false, // fin
        false, // rst
        8192,  // window_size
        &[],   // options
        &payload_1000_2000,
    );
    let result2 = driver.send_packet(segment_1000).await;

    // Then Slirp processes the out-of-order TCP segments cleanly
    assert!(result1.is_ok());
    assert!(result2.is_ok());
}

#[tokio::test]
async fn test_tcp_out_of_order_segments() {
    for backend in [SlirpBackend::CFfi, SlirpBackend::Native] {
        run_tcp_out_of_order_segments_test(backend).await;
    }
}
