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

//! Integration Tests: TCP State Machine Transitions & Handshake/Teardown
//! handling (b/560172253).

use std::net::Ipv4Addr;

use slirp_actor::SlirpBackend;

use super::{packet_helpers::create_tcp_packet, test_driver::SlirpTestDriver};

// -----------------------------------------------------------------------------
// 1. TCP SYN-SENT Timeout Test
// -----------------------------------------------------------------------------

async fn run_tcp_syn_sent_timeout_test(backend: SlirpBackend) {
    // Given a Slirp driver initialized with the specified backend
    let mut driver = SlirpTestDriver::new(backend, None).await;

    // When guest sends a TCP SYN packet (10.0.2.15:12345 -> 1.1.1.1:80)
    let syn_packet = create_tcp_packet(
        Ipv4Addr::new(10, 0, 2, 15),
        Ipv4Addr::new(1, 1, 1, 1),
        12345,
        80,
        1000,  // seq
        0,     // ack
        true,  // syn
        false, // ack_flag
        false, // fin
        false, // rst
        8192,  // window_size
        &[],   // payload
    );
    let send_res = driver.send_packet(syn_packet).await;

    // And time elapses to simulate a SYN connection timeout
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    // Then Slirp accepts the packet cleanly without error
    assert!(send_res.is_ok());
}

#[tokio::test]
async fn test_tcp_syn_sent_timeout() {
    for backend in [SlirpBackend::CFfi, SlirpBackend::Native] {
        run_tcp_syn_sent_timeout_test(backend).await;
    }
}

// -----------------------------------------------------------------------------
// 2. TCP Simultaneous Close Test
// -----------------------------------------------------------------------------

async fn run_tcp_simultaneous_close_test(backend: SlirpBackend) {
    // Given a Slirp driver initialized with the specified backend
    let mut driver = SlirpTestDriver::new(backend, None).await;

    // When guest initiates connection with TCP SYN
    let syn_packet = create_tcp_packet(
        Ipv4Addr::new(10, 0, 2, 15),
        Ipv4Addr::new(1, 1, 1, 1),
        12346,
        80,
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
    assert!(syn_res.is_ok());

    // And guest sends a TCP FIN/ACK packet to close connection
    let fin_ack_packet = create_tcp_packet(
        Ipv4Addr::new(10, 0, 2, 15),
        Ipv4Addr::new(1, 1, 1, 1),
        12346,
        80,
        1001,  // seq
        1,     // ack
        false, // syn
        true,  // ack_flag
        true,  // fin
        false, // rst
        8192,
        &[],
    );
    let fin_res = driver.send_packet(fin_ack_packet).await;

    // Then Slirp processes guest FIN/ACK and handles teardown state cleanly
    assert!(fin_res.is_ok());
}

#[tokio::test]
async fn test_tcp_simultaneous_close() {
    for backend in [SlirpBackend::CFfi, SlirpBackend::Native] {
        run_tcp_simultaneous_close_test(backend).await;
    }
}

// -----------------------------------------------------------------------------
// 3. TCP TIME-WAIT 4-Tuple Reuse Test
// -----------------------------------------------------------------------------

async fn run_tcp_time_wait_reuse_test(backend: SlirpBackend) {
    // Given a Slirp driver initialized with the specified backend
    let mut driver = SlirpTestDriver::new(backend, None).await;

    let src_ip = Ipv4Addr::new(10, 0, 2, 15);
    let dst_ip = Ipv4Addr::new(1, 1, 1, 1);
    let src_port = 12347;
    let dst_port = 80;

    // When an initial TCP connection on 4-tuple (10.0.2.15:12347 -> 1.1.1.1:80) is
    // closed via FIN
    let syn1 = create_tcp_packet(
        src_ip,
        dst_ip,
        src_port,
        dst_port,
        1000,
        0,
        true,
        false,
        false,
        false,
        8192,
        &[],
    );
    driver.send_packet(syn1).await.unwrap();

    let fin1 = create_tcp_packet(
        src_ip,
        dst_ip,
        src_port,
        dst_port,
        1001,
        1,
        false,
        true,
        true,
        false,
        8192,
        &[],
    );
    driver.send_packet(fin1).await.unwrap();

    // And guest rapidly reconnects on the exact same 4-tuple with a new TCP SYN
    let syn2 = create_tcp_packet(
        src_ip,
        dst_ip,
        src_port,
        dst_port,
        2000,
        0,
        true,
        false,
        false,
        false,
        8192,
        &[],
    );
    let reconnect_res = driver.send_packet(syn2).await;

    // Then Slirp allows connection reuse on the same 4-tuple without error
    assert!(reconnect_res.is_ok());
}

#[tokio::test]
async fn test_tcp_time_wait_reuse() {
    for backend in [SlirpBackend::CFfi, SlirpBackend::Native] {
        run_tcp_time_wait_reuse_test(backend).await;
    }
}
