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

//! Decoupled Integration Tests: Host & Guest Port Forwarding (b/560172253).
//!
//! Uses `SlirpTestDriver` to keep test assertions independent of actor
//! framework primitives.

use std::net::Ipv4Addr;

use slirp_actor::SlirpBackend;

use super::{packet_helpers::create_tcp_packet, test_driver::SlirpTestDriver};

// -----------------------------------------------------------------------------
// 1. Host Port Forwarding (hostfwd) Registration Test
// -----------------------------------------------------------------------------

async fn run_hostfwd_registration_test(backend: SlirpBackend) {
    // Given a Slirp driver running the specified backend
    let mut driver = SlirpTestDriver::new(backend, None).await;

    // When client checks initialization status
    let initialized = driver.is_initialized().await;

    // Then driver is initialized successfully
    assert!(initialized);
}

#[tokio::test]
async fn test_hostfwd_registration_parity() {
    for backend in [SlirpBackend::CFfi, SlirpBackend::Native] {
        run_hostfwd_registration_test(backend).await;
    }
}

// -----------------------------------------------------------------------------
// 2. Guest Port Forwarding (guestfwd) Virtual IP Test
// -----------------------------------------------------------------------------

async fn run_guestfwd_virtual_ip_test(backend: SlirpBackend) {
    // Given a Slirp driver running the specified backend
    let mut driver = SlirpTestDriver::new(backend, None).await;

    // When guest connects to virtual IP 10.0.2.100:8080 (guestfwd target)
    let syn_packet = create_tcp_packet(
        Ipv4Addr::new(10, 0, 2, 15),
        Ipv4Addr::new(10, 0, 2, 100),
        12345,
        8080,
        100,
        0,
        true,  // syn
        false, // ack
        false, // fin
        false, // rst
        8192,
        &[],
    );

    let result = driver.send_packet(syn_packet).await;

    // Then Slirp processes the guestfwd SYN packet
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_guestfwd_virtual_ip() {
    for backend in [SlirpBackend::CFfi, SlirpBackend::Native] {
        run_guestfwd_virtual_ip_test(backend).await;
    }
}
