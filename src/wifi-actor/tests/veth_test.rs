// Copyright 2025 The Android Open Source Project
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

use tokio::net::UdpSocket;

use crate::world::World;

// ============================================================================
// Feature: Guest-to-Host UDP Roundtrip via Slirp Gateway
// ============================================================================

// Scenario: A Guest Wi-Fi station exchanges UDP packets with a Host UDP socket via Slirp
// Given a Wifi World with an active AP, a provisioned station chip, and a Host UDP listener
// When the Guest station sends a UDP datagram to the host via the Slirp gateway (10.0.2.2)
//   and the Host replies
// Then the Host receives the Guest UDP payload and the Guest station receives the Host UDP reply
//   (resolving ARP)
#[tokio::test]
async fn test_udp_guest_to_host() {
    // Given: A World with an AP, a station chip, and a bound Host UDP socket.
    let mut world = World::new().await;
    world.given_an_ap().await;
    world.given_a_chip(1).await;

    let host_socket = UdpSocket::bind("127.0.0.1:0").await.expect("Failed to bind host socket");
    let host_addr = host_socket.local_addr().expect("Failed to read local_addr");

    // When: The Guest station transmits a UDP packet to the Host socket via the Slirp gateway.
    let payload = b"Hello Host from Slirp!";
    world.when_chip_sends_udp_to_host(0, host_addr.port(), payload).await;

    // Then: The Host socket receives the Guest UDP packet.
    let mut buf = [0u8; 1024];
    let (amt, src) = tokio::time::timeout(Duration::from_secs(2), host_socket.recv_from(&mut buf))
        .await
        .expect("Host did not receive UDP packet before timeout")
        .expect("recv_from failed");
    assert_eq!(&buf[..amt], payload);

    // When: The Host replies to the Guest over UDP.
    let reply_payload = b"Hello Guest from Host!";
    host_socket.send_to(reply_payload, src).await.expect("Failed to send UDP reply from host");

    // Then: The Guest station receives the UDP reply after completing ARP resolution with Slirp.
    world.then_chip_receives_udp_reply_handling_arp(0, reply_payload).await;
}
