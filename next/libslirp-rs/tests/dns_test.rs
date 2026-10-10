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

// Feature: Virtual DNS Name Resolution
//
//   As a virtual guest network stack
//   I want to issue DNS queries through LibSlirp to configured host DNS servers
//   So that name resolution succeeds across IPv4, IPv6, custom ports, and
// multi-server setups

// Scenario: IPv4 guest DNS queries translated to a host IPv6 DNS server
//   Given a running IPv6 DNS server on the host
//   And a LibSlirp stack configured with the host IPv6 DNS server
//   When the guest sends an IPv4 DNS query to the virtual nameserver
//   Then the guest receives the DNS reply on the guest query port
#[test]
fn test_udp_cross_family_dns() {
    // Given
    let mut world = World::new();
    let server_idx = world.given_ipv6_dns_server();
    world.given_slirp_with_dns(&[server_idx]);

    // When
    let guest_port = world.next_guest_port();
    world.when_send_ipv4_udp(World::VNAMESERVER_IPV4_1, guest_port, World::DNS_PORT, PAYLOAD);
    world.when_receive_reply(Duration::from_secs(2));

    // Then
    world.then_received_reply_matches(
        World::VNAMESERVER_IPV4_1,
        World::GUEST_IPV4,
        World::DNS_PORT,
        guest_port,
        PAYLOAD_PONG,
    );
}

// Scenario: IPv4 guest DNS queries to a host DNS server on a custom port
//   Given a running IPv4 DNS server on the host on a custom port
//   And a LibSlirp stack configured with the custom port DNS server
//   When the guest sends an IPv4 DNS query to the virtual nameserver
//   Then the guest receives the DNS reply with port translation
#[test]
fn test_udp_ipv4_dns_custom_port() {
    // Given
    let mut world = World::new();
    let server_idx = world.given_ipv4_dns_server();
    world.given_slirp_with_dns(&[server_idx]);

    // When
    let guest_port = world.next_guest_port();
    world.when_send_ipv4_udp(World::VNAMESERVER_IPV4_1, guest_port, World::DNS_PORT, PAYLOAD);
    world.when_receive_reply(Duration::from_secs(2));

    // Then
    world.then_received_reply_matches(
        World::VNAMESERVER_IPV4_1,
        World::GUEST_IPV4,
        World::DNS_PORT,
        guest_port,
        PAYLOAD_PONG,
    );
}

// Scenario: IPv6 guest DNS queries to a host IPv6 DNS server on a custom port
//   Given a running IPv6 DNS server on the host on a custom port
//   And an IPv6-only LibSlirp stack configured with the host DNS server
//   When the guest sends an IPv6 DNS query to the IPv6 virtual nameserver
//   Then the guest receives the DNS reply on the query port
#[test]
fn test_udp_ipv6_dns_custom_port() {
    // Given
    let mut world = World::new();
    let server_idx = world.given_ipv6_dns_server();
    world.given_slirp(SlirpConfig {
        in_enabled: false,
        in6_enabled: true,
        host_dns: vec![world.server_addr(server_idx)],
        ..Default::default()
    });

    // When
    let guest_port = world.next_guest_port();
    world.when_send_ipv6_udp(World::VNAMESERVER_IPV6, guest_port, World::DNS_PORT, PAYLOAD);
    world.when_receive_reply(Duration::from_secs(2));

    // Then
    world.then_received_reply_matches(
        World::VNAMESERVER_IPV6,
        World::GUEST_IPV6,
        World::DNS_PORT,
        guest_port,
        PAYLOAD_PONG,
    );
}

// Scenario: Reverse cross-family DNS routing IPv6 guest queries to host IPv4
// server   Given a running IPv4 DNS server on the host
//   And a dual-stack LibSlirp stack configured with the host IPv4 DNS server
//   When the guest sends an IPv6 DNS query to the IPv6 virtual nameserver
//   Then the guest receives the translated DNS reply
#[test]
fn test_udp_reverse_cross_family_dns() {
    // Given
    let mut world = World::new();
    let server_idx = world.given_ipv4_dns_server();
    world.given_slirp(SlirpConfig {
        in_enabled: true,
        in6_enabled: true,
        host_dns: vec![world.server_addr(server_idx)],
        ..Default::default()
    });

    // When
    let guest_port = world.next_guest_port();
    world.when_send_ipv6_udp(World::VNAMESERVER_IPV6, guest_port, World::DNS_PORT, PAYLOAD);
    world.when_receive_reply(Duration::from_secs(2));

    // Then
    world.then_received_reply_matches(
        World::VNAMESERVER_IPV6,
        World::GUEST_IPV6,
        World::DNS_PORT,
        guest_port,
        PAYLOAD_PONG,
    );
}

// Scenario: Multiple custom DNS server targets queried concurrently
//   Given two running IPv4 DNS servers on the host
//   And a LibSlirp stack configured with both DNS servers
//   When the guest sends distinct queries to both virtual nameservers
//   Then the guest receives corresponding replies for both queries
#[test]
fn test_udp_multiple_dns_servers() {
    // Given
    let mut world = World::new();
    let server1_idx = world.given_ipv4_dns_server();
    let server2_idx = world.given_ipv4_dns_server();
    world.given_slirp_with_dns(&[server1_idx, server2_idx]);

    // When
    let guest_port1 = world.next_guest_port();
    let guest_port2 = world.next_guest_port();
    world.when_send_ipv4_udp(World::VNAMESERVER_IPV4_1, guest_port1, World::DNS_PORT, PAYLOAD);
    world.when_send_ipv4_udp(World::VNAMESERVER_IPV4_2, guest_port2, World::DNS_PORT, PAYLOAD);
    world.when_collect_replies(2, Duration::from_secs(4));

    // Then
    world.then_received_reply_on_port_matches(
        guest_port1,
        World::VNAMESERVER_IPV4_1,
        World::GUEST_IPV4,
        World::DNS_PORT,
        PAYLOAD_PONG,
    );
    world.then_received_reply_on_port_matches(
        guest_port2,
        World::VNAMESERVER_IPV4_2,
        World::GUEST_IPV4,
        World::DNS_PORT,
        PAYLOAD_PONG,
    );
}

// Scenario: Multiple IPv4 host DNS servers queried via IPv6 guest queries
//   Given two running IPv4 DNS servers on the host
//   And a dual-stack LibSlirp stack configured with both DNS servers
//   When the guest sends distinct IPv6 DNS queries to both IPv6 virtual
// nameservers   Then the guest receives corresponding replies for both queries
#[test]
fn test_udp_reverse_cross_family_multiple_dns_servers() {
    // Given
    let mut world = World::new();
    let server1_idx = world.given_ipv4_dns_server();
    let server2_idx = world.given_ipv4_dns_server();
    world.given_slirp_with_dns(&[server1_idx, server2_idx]);

    // When
    let guest_port1 = world.next_guest_port();
    let guest_port2 = world.next_guest_port();
    world.when_send_ipv6_udp(World::VNAMESERVER_IPV6_1, guest_port1, World::DNS_PORT, PAYLOAD);
    world.when_send_ipv6_udp(World::VNAMESERVER_IPV6_2, guest_port2, World::DNS_PORT, PAYLOAD);
    world.when_collect_replies(2, Duration::from_secs(4));

    // Then
    world.then_received_reply_on_port_matches(
        guest_port1,
        World::VNAMESERVER_IPV6_1,
        World::GUEST_IPV6,
        World::DNS_PORT,
        PAYLOAD_PONG,
    );
    world.then_received_reply_on_port_matches(
        guest_port2,
        World::VNAMESERVER_IPV6_2,
        World::GUEST_IPV6,
        World::DNS_PORT,
        PAYLOAD_PONG,
    );
}

// Scenario: Secondary IPv6 DNS server queried in a mixed IPv4/IPv6
// configuration   Given an IPv4 DNS server and an IPv6 DNS server configured on
// the host   And a dual-stack LibSlirp stack configured with [IPv4, IPv6] host
// DNS   When the guest sends an IPv6 query to the secondary nameserver
// (VNAMESERVER_IPV6_2)   Then the guest receives the DNS reply matching
// VNAMESERVER_IPV6_2
#[test]
fn test_udp_ipv6_multi_server_mixed_family_dns() {
    // Given
    let mut world = World::new();
    let server1_ipv4 = world.given_ipv4_dns_server();
    let server2_ipv6 = world.given_ipv6_dns_server();
    world.given_slirp_with_dns(&[server1_ipv4, server2_ipv6]);

    // When: Guest queries secondary nameserver (VNAMESERVER_IPV6_2 -> server2_ipv6)
    let guest_port = world.next_guest_port();
    world.when_send_ipv6_udp(World::VNAMESERVER_IPV6_2, guest_port, World::DNS_PORT, PAYLOAD);
    world.when_receive_reply(Duration::from_secs(2));

    // Then: Response is received from VNAMESERVER_IPV6_2 with port 53
    world.then_received_reply_matches(
        World::VNAMESERVER_IPV6_2,
        World::GUEST_IPV6,
        World::DNS_PORT,
        guest_port,
        PAYLOAD_PONG,
    );
}

// Scenario: Non-DNS UDP traffic to virtual nameserver address is not
// intercepted   Given a running echo server on the host
//   And a LibSlirp stack with configured DNS
//   When the guest sends non-DNS UDP traffic (port 8080) to the virtual
// nameserver IP   Then the packet is not rewritten to the host DNS server
#[test]
fn test_udp_ipv6_non_dns_to_vnameserver_not_intercepted() {
    // Given
    let mut world = World::new();
    let dns_idx = world.given_ipv6_dns_server();
    world.given_slirp_with_dns(&[dns_idx]);

    // When: Guest sends UDP to port 8080 (non-DNS)
    let guest_port = world.next_guest_port();
    world.when_send_ipv6_udp(World::VNAMESERVER_IPV6_1, guest_port, 8080, PAYLOAD);
    world.when_collect_replies(1, Duration::from_millis(500));

    // Then: No DNS reply received (packet was rejected/not forwarded as DNS)
    world.then_no_replies_received();
}
