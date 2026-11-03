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

use crate::world::{MockGateway, World};

#[tokio::test]
async fn test_gateway_routing() {
    // 1. Setup World with MockGateway
    let mock_gateway = MockGateway::new();
    let outgoing_packets = mock_gateway.outgoing_packets.clone();
    let mut world = World::new_with_gateway(Box::new(mock_gateway)).await;

    // 2. Create AP
    world.given_an_ap().await;

    // 3. Create Chip
    let chip1_id = world.given_a_chip(1).await;
    outgoing_packets.lock().unwrap().clear();

    // 3. Chip transmits data destined for Internet (ToDS)
    // This helper sends a ToDS frame destined to a dummy "Gateway" MAC.
    // The WifiActor should route this to the GatewayTrait implementation
    // (MockGateway).
    world.when_chip_transmits_data_to_slirp(0).await;

    // 4. Verify Gateway received the packet
    // Give it a moment to process
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let packets = outgoing_packets.lock().unwrap();
    assert_eq!(packets.len(), 1, "Gateway should have received 1 packet");

    let (cid, _frame) = &packets[0];
    assert_eq!(cid.0, chip1_id, "Packet should be from Chip 1");

    // We can also verify contents of _frame if needed, but World helper sent
    // known payload.
}

#[tokio::test]
async fn test_gateway_routing_blocked_when_disabled() {
    let mock_gateway = MockGateway::new();
    let outgoing_packets = mock_gateway.outgoing_packets.clone();
    let mut world = World::new_with_gateway(Box::new(mock_gateway)).await;

    world.given_an_ap().await;
    world.given_a_chip(1).await;
    world.given_chip_is_disabled(0).await;
    outgoing_packets.lock().unwrap().clear();

    world.when_chip_transmits_data_to_slirp(0).await;
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    assert!(outgoing_packets.lock().unwrap().is_empty());

    world.when_chip_transmits_infra_mdns(0, "\x01\x02\x03\x04").await;
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    assert_eq!(outgoing_packets.lock().unwrap().len(), 1);
}

#[cfg(target_os = "linux")]
mod tap_downlink_tests {
    use super::*;

    const ETHERNET_HEADER_LEN: usize = 14;
    const STANDARD_ETHERNET_MTU: usize = 1500;
    const FULL_ETHERNET_FRAME_LEN: usize = ETHERNET_HEADER_LEN + STANDARD_ETHERNET_MTU;
    const GATEWAY_MAC: [u8; 6] = [0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff];
    const IPV6_ALL_NODES_MULTICAST_MAC: [u8; 6] = [0x33, 0x33, 0x00, 0x00, 0x00, 0x01];
    const ETHERTYPE_IPV4: [u8; 2] = [0x08, 0x00];
    const ETHERTYPE_IPV6: [u8; 2] = [0x86, 0xdd];

    fn build_1514_byte_ethernet_frame(
        dst_mac: [u8; 6],
        src_mac: [u8; 6],
        ethertype: [u8; 2],
    ) -> Vec<u8> {
        let mut frame = Vec::with_capacity(FULL_ETHERNET_FRAME_LEN);
        frame.extend_from_slice(&dst_mac);
        frame.extend_from_slice(&src_mac);
        frame.extend_from_slice(&ethertype);
        let payload: Vec<u8> = (0..STANDARD_ETHERNET_MTU).map(|i| (i % 251) as u8).collect();
        frame.extend_from_slice(&payload);
        assert_eq!(frame.len(), FULL_ETHERNET_FRAME_LEN);
        frame
    }

    // Scenario: TapGateway delivers full-MTU 1514-byte unicast frame intact and records download throughput
    // Given a World with TapGateway, an active AP, and an associated Chip 1 at t=1000ms
    // When a 1514-byte unicast Ethernet II frame arrives on the TAP downlink
    // Then Chip 1 receives all 1514 bytes intact and WifiStats records the 1500-byte L3 payload
    #[tokio::test]
    async fn test_tap_gateway_downlink_delivers_1514_byte_unicast_frame_intact() {
        let mut world = World::new_with_tap_gateway().await;
        world.given_an_ap().await;
        world.given_an_associated_chip(1).await;
        world.given_mock_clock_millis(1000);

        let sta_mac = world.chips[0].mac;
        let expected_eth_frame =
            build_1514_byte_ethernet_frame(sta_mac, GATEWAY_MAC, ETHERTYPE_IPV4);

        world.when_gateway_sends_downlink_frame(&expected_eth_frame);
        world.then_chip_receives_ethernet_frame(0, &expected_eth_frame).await;

        // Flush the 5-second download throughput window to verify 1500 L3 payload bytes were recorded
        world.given_mock_clock_millis(6000);
        world.when_gateway_sends_downlink_frame(&expected_eth_frame);
        world.then_chip_receives_ethernet_frame(0, &expected_eth_frame).await;

        // 1500 payload bytes over 5.0s = 300 B/s = 0.0024 Mbps
        let expected_mbps = ((STANDARD_ETHERNET_MTU as f64 / 5.0) * 8.0 / 1_000_000.0) as f32;
        world.then_max_download_throughput_is(expected_mbps).await;
    }

    // Scenario: TapGateway delivers full-MTU 1514-byte IPv6 multicast frame intact
    // Given a World with TapGateway, an active AP, and an associated Chip 1
    // When a 1514-byte IPv6 multicast frame arrives on the TAP downlink
    // Then TapGateway fans out the frame across active BSSIDs and Chip 1 receives all 1514 bytes intact
    #[tokio::test]
    async fn test_tap_gateway_downlink_delivers_1514_byte_ipv6_multicast_frame_intact() {
        let mut world = World::new_with_tap_gateway().await;
        world.given_an_ap().await;
        world.given_an_associated_chip(1).await;

        let expected_eth_frame = build_1514_byte_ethernet_frame(
            IPV6_ALL_NODES_MULTICAST_MAC,
            GATEWAY_MAC,
            ETHERTYPE_IPV6,
        );

        world.when_gateway_sends_downlink_frame(&expected_eth_frame);
        world.then_chip_receives_ethernet_frame(0, &expected_eth_frame).await;
    }
}
