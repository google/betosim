// Copyright 2024 The Android Open Source Project
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

use std::time::Duration;

use libslirp_rs::SlirpConfig;

use crate::world::{PAYLOAD, PAYLOAD_PONG, World};

// Feature: UDP Echo Translation
//
//   As a virtual guest network driver
//   I want to transmit UDP packets through LibSlirp
//   So that host echo services correctly process virtual traffic

// Scenario: IPv4 UDP echo translation
//   Given a running IPv4 UDP echo server on the host
//   And a running LibSlirp stack with default configuration
//   When the guest sends an IPv4 UDP packet to the virtual host
//   Then the guest receives the echo reply from the virtual host
#[test]
fn test_udp_echo() {
    // Given
    let mut world = World::new();
    let server_idx = world.given_ipv4_echo_server();
    world.given_default_slirp();

    // When
    let guest_port = world.next_guest_port();
    let server_port = world.server_port(server_idx);
    world.when_send_ipv4_udp(World::VHOST_IPV4, guest_port, server_port, PAYLOAD);
    world.when_receive_reply(Duration::from_secs(2));

    // Then
    world.then_received_reply_matches(
        World::VHOST_IPV4,
        World::GUEST_IPV4,
        server_port,
        guest_port,
        PAYLOAD_PONG,
    );
}

// Scenario: IPv6 UDP echo packet translation with port preservation
//   Given a running IPv6 UDP echo server on the host
//   And an IPv6-only LibSlirp stack
//   When the guest sends an IPv6 UDP packet to the virtual host
//   Then the guest receives the echo reply from the IPv6 virtual host
#[test]
fn test_udp_ipv6_echo() {
    // Given
    let mut world = World::new();
    let server_idx = world.given_ipv6_echo_server();
    world.given_slirp(SlirpConfig { in_enabled: false, in6_enabled: true, ..Default::default() });

    // When
    let guest_port = world.next_guest_port();
    let server_port = world.server_port(server_idx);
    world.when_send_ipv6_udp(World::VHOST_IPV6, guest_port, server_port, PAYLOAD);
    world.when_receive_reply(Duration::from_secs(2));

    // Then
    world.then_received_reply_matches(
        World::VHOST_IPV6,
        World::GUEST_IPV6,
        server_port,
        guest_port,
        PAYLOAD_PONG,
    );
}
