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

//! Tier 2 BDD scenarios for `SlirpActor` and `SlirpClient` (`b/577212628`).
//!
//! Exercises client registration/unregistration lifecycle, disconnect callbacks,
//! isolated vs LAN downlink routing, IPv6 Solicited-Node Neighbor Solicitation
//! static address matching, dynamic backend switching (`SwitchBackend` stack
//! teardown and reinitialization), and concurrent multi-stream TCP SYN/RST/FIN
//! teardown stress (`b/567662999`).

use netsim_model::ChipId;
use slirp_actor::SlirpBackend;

use crate::world::{ClientConfig, World};

#[tokio::test]
async fn test_bdd_client_register_unregister_and_disconnect_notification() {
    // Scenario: Client registration, explicit unregistration, and disconnect
    // callback on guest channel drop
    // Given a running SlirpActor in the BDD World with two registered clients (10 and 20)
    let mut world = World::new(SlirpBackend::Native).await;
    world.given_registered_client(10, ClientConfig::lan()).await;
    world.given_registered_client(20, ClientConfig::lan()).await;
    world.when_client_queries_gateway_arp(10).await;
    world.then_client_receives_gateway_arp_reply(10).await;
    world.when_client_queries_gateway_arp(20).await;
    world.then_client_receives_gateway_arp_reply(20).await;
    world.then_no_disconnect_notification_is_pending();

    // When client 10 explicitly unregisters via SlirpClient::unregister
    world.when_client_is_unregistered(10).await;
    world.when_downlink_arp_targets_client(10).await;

    // Then downlink packets targeting client 10's IPv4 are no longer delivered to client 10
    world.then_client_receives_no_frame(10).await;
    world.then_no_disconnect_notification_is_pending();

    // And client 20 continues to exchange ICMP echo traffic with the SLIRP gateway normally
    world.when_client_pings_gateway(20, 1).await;
    world.then_client_receives_gateway_icmp_reply(20, 1).await;

    // When client 20's guest packet sender channel drops and triggers stream EOF
    world.when_client_uplink_channel_drops(20).await;

    // Then the actor's notify_disconnected callback fires for ChipId(20) and the
    // actor shuts down cleanly
    world.then_disconnect_notification_fired_for(ChipId(20)).await;
    world.when_actor_is_shut_down().await;
}

#[tokio::test]
async fn test_bdd_client_isolation_blocks_unknown_unicast_and_broadcast_while_preserving_gateway() {
    // Scenario: Client isolation (is_isolated = true) blocks unknown unicast and
    // LAN broadcasts while preserving SLIRP gateway access
    // Given two isolated guests (clients 10 and 20) and one non-isolated LAN guest (client 30)
    let mut world = World::new(SlirpBackend::Native).await;
    world.given_registered_client(10, ClientConfig::isolated()).await;
    world.given_registered_client(20, ClientConfig::isolated()).await;
    world.given_registered_client(30, ClientConfig::lan()).await;

    // When an unknown-unicast frame arrives on the downlink (not matching any learned MAC)
    world.when_downlink_unknown_unicast_arrives().await;

    // Then only non-isolated client 30 receives the flooded frame while isolated
    // clients 10 and 20 do not
    world.then_client_receives_unknown_unicast_frame(30).await;
    world.then_client_receives_no_frame(10).await;
    world.then_client_receives_no_frame(20).await;

    // When a generic LAN broadcast frame arrives on the downlink
    world.when_downlink_lan_broadcast_arrives().await;

    // Then non-isolated client 30 receives the broadcast while isolated clients 10
    // and 20 are shielded
    world.then_client_receives_broadcast_frame(30).await;
    world.then_client_receives_no_frame(10).await;
    world.then_client_receives_no_frame(20).await;

    // When a targeted ARP downlink packet explicitly addresses isolated client 10's
    // static IPv4 address
    world.when_downlink_arp_targets_client(10).await;

    // Then isolated clients (10 and 20) receive the cellular ARP resolution while
    // non-isolated LAN client 30 does not
    world.then_client_receives_arp_for_client(10, 10).await;
    world.then_client_receives_arp_for_client(20, 10).await;
    world.then_client_receives_no_frame(30).await;

    // And both isolated clients 10 and 20 still have full ARP and ICMP Echo
    // connectivity to the SLIRP gateway
    world.then_client_has_gateway_connectivity(10, 11).await;
    world.then_client_has_gateway_connectivity(20, 22).await;
    world.when_actor_is_shut_down().await;
}

#[tokio::test]
async fn test_bdd_static_ipv6_matching_and_ndp_routing_between_guests() {
    // Scenario: Static IPv6 address matching routes Solicited-Node Neighbor
    // Solicitations between isolated and LAN scopes
    // Given isolated client 10 (with a static IPv6 lease) and LAN client 20
    let mut world = World::new(SlirpBackend::Native).await;
    world.given_registered_client(10, ClientConfig::isolated()).await;
    world.given_registered_client(20, ClientConfig::lan()).await;

    // When an IPv6 Solicited-Node multicast Neighbor Solicitation arrives targeting
    // isolated client 10's static IPv6 address
    world.when_downlink_ipv6_ns_targets_client(10).await;

    // Then isolated client 10 receives the Neighbor Solicitation via
    // matches_static_ipv6 while LAN client 20 does not
    world.then_client_receives_ipv6_ns_for_client(10, 10).await;
    world.then_client_receives_no_frame(20).await;

    // When an IPv6 Solicited-Node multicast Neighbor Solicitation arrives targeting
    // LAN client 20's IPv6 address
    world.when_downlink_ipv6_ns_targets_client(20).await;

    // Then LAN client 20 receives the Neighbor Solicitation while isolated client
    // 10 does not match and is shielded
    world.then_client_receives_ipv6_ns_for_client(20, 20).await;
    world.then_client_receives_no_frame(10).await;

    // When isolated client 10 releases its static lease and another Neighbor
    // Solicitation arrives for that former IPv6 address
    world.when_client_releases_lease(10).await;
    world.when_downlink_ipv6_ns_targets_client(10).await;

    // Then matches_static_ipv6 no longer matches, so LAN client 20 receives the
    // multicast frame and isolated client 10 is shielded
    world.then_client_receives_ipv6_ns_for_client(20, 10).await;
    world.then_client_receives_no_frame(10).await;
    world.when_actor_is_shut_down().await;
}

#[tokio::test]
async fn test_bdd_switch_backend_reinitializes_stack_and_preserves_registered_clients() {
    // Scenario: Dynamic backend switching tears down the active backend stack,
    // reinitializes it, and preserves registered clients
    // Given a SlirpActor running with SlirpBackend::Native and two registered clients (10 and 20)
    let mut world = World::new(SlirpBackend::Native).await;
    world.given_registered_client(10, ClientConfig::lan()).await;
    world.given_registered_client(20, ClientConfig::lan()).await;

    // And client 10 has an active TCP connection established on source port 24000
    // (which drops duplicate SYNs)
    world.when_client_establishes_loopback_tcp_flow(10, 24000, 1000).await;
    world.when_client_sends_loopback_tcp_syn(10, 24000, 2000).await;
    world.then_client_receives_no_frame(10).await;

    // When the actor switches backend via SlirpClient::switch_backend to CFfi and
    // back to Native
    world.when_backend_is_switched(SlirpBackend::CFfi).await;
    world.then_client_has_gateway_connectivity(10, 31).await;
    world.then_client_has_gateway_connectivity(20, 32).await;
    world.when_backend_is_switched(SlirpBackend::Native).await;
    world.when_client_sends_loopback_tcp_syn(10, 24000, 3000).await;
    world.then_client_receives_loopback_tcp_syn_ack(10, 24000, 3001).await;

    // Then both existing clients 10 and 20 continue to resolve gateway ARP and
    // exchange ICMP Echo replies across the reinitialized backend instance
    world.then_client_has_gateway_connectivity(10, 41).await;
    world.then_client_has_gateway_connectivity(20, 42).await;
    world.when_actor_is_shut_down().await;
}

#[tokio::test]
async fn test_bdd_concurrent_multi_stream_tcp_syn_rst_fin_teardown_stress() {
    // Scenario: Concurrent multi-stream TCP SYN/SYN-ACK/ACK establishment and
    // RST/FIN teardown under SlirpActor (b/567662999)
    // Given a running SlirpActor and a registered guest client
    let mut world = World::new(SlirpBackend::Native).await;
    world.given_registered_client(10, ClientConfig::lan()).await;
    world.when_client_queries_gateway_arp(10).await;
    world.then_client_receives_gateway_arp_reply(10).await;

    // When 16 concurrent TCP streams perform full 3-way handshakes and interleaved
    // RST / FIN-ACK teardowns
    world.when_concurrent_tcp_streams_stress(10, 16).await;

    // Then every RST-torn-down TCP 4-tuple has been removed from TcpManager and
    // accepts a fresh reuse SYN with SYN-ACK
    world.then_rst_torn_down_tcp_streams_accept_reuse_syn(10, 16).await;

    // And the SLIRP actor remains responsive for subsequent ICMP Echo requests and
    // shuts down deterministically
    world.when_client_pings_gateway(10, 99).await;
    world.then_client_receives_gateway_icmp_reply(10, 99).await;
    world.when_actor_is_shut_down().await;
}
