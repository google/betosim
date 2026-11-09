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

use crate::world::World;

// ============================================================================
// Feature: Gateway (Slirp) Uplink Routing and RF State Gating
// ============================================================================

// Scenario: An enabled station routes ToDS data frames destined for the Internet to the Gateway
// Given a Wifi World configured with a MockGateway, an active AP, and an enabled station chip
// When the station transmits a ToDS data frame addressed to the Internet gateway
// Then the MockGateway receives the 802.11 frame tagged with the station's ChipId
#[tokio::test]
async fn test_gateway_routing() {
    // Given: Setup World with MockGateway, AP, and Chip
    let (mut world, mock_gateway, chip1_id) = World::given_mock_gateway_world().await;

    // When: Chip transmits data destined for Internet (ToDS)
    world.when_chip_transmits_data_to_slirp(0).await;

    // Then: Verify Gateway received the packet
    mock_gateway.wait_for_packet_count(1).await;

    let packets = mock_gateway.outgoing_packets.lock().unwrap();
    let (cid, _frame) = &packets[0];
    assert_eq!(cid.0, chip1_id, "Packet should be from Chip 1");
}

// Scenario: A disabled station drops unicast ToDS gateway traffic while allowing mDNS multicast
// Given a Wifi World with a MockGateway, an active AP, and a chip disabled via ChipClient::update
// When the chip transmits a unicast ToDS data frame to Slirp and an infrastructure mDNS multicast
//   frame
// Then the unicast ToDS frame is dropped while the mDNS multicast frame reaches the gateway
#[tokio::test]
async fn test_gateway_routing_blocked_when_disabled() {
    // Given
    let (mut world, mock_gateway, _chip1_id) = World::given_mock_gateway_world().await;
    world.given_chip_is_disabled(0).await;

    // When: Unicast ToDS frame to Slirp while disabled
    world.when_chip_transmits_data_to_slirp(0).await;

    // Then: Unicast ToDS frame is blocked
    mock_gateway.assert_no_packets_within(50).await;

    // When: Infrastructure mDNS multicast frame
    world.when_chip_transmits_infra_mdns(0, "\x01\x02\x03\x04").await;

    // Then: mDNS frame reaches the gateway
    mock_gateway.wait_for_packet_count(1).await;
}

// Scenario: Disabling RF state via WifiClient::set_rf_state drops unicast ToDS SLIRP traffic
//   (ag/43192696)
// Given a Wifi World with a MockGateway, an active AP, and a provisioned station chip
// When WifiClient::set_rf_state(false) disables the chip and the chip transmits a unicast ToDS
//   data frame to Slirp
// Then the MockGateway receives zero packets until WifiClient::set_rf_state(true) re-enables the
//   chip
#[tokio::test]
async fn test_set_rf_state_disabled_drops_unicast_tods_slirp_traffic() {
    // Given
    let (mut world, mock_gateway, chip1_id) = World::given_mock_gateway_world().await;

    // When: Disable RF state via WifiClient::set_rf_state and transmit unicast ToDS data to Slirp
    world.when_rf_state_is_set(chip1_id, false).await;
    world.when_chip_transmits_data_to_slirp(0).await;

    // Then: Unicast ToDS SLIRP traffic is dropped
    mock_gateway.assert_no_packets_within(50).await;

    // When: Re-enable RF state via WifiClient::set_rf_state and retransmit
    world.when_rf_state_is_set(chip1_id, true).await;
    world.when_chip_transmits_data_to_slirp(0).await;

    // Then: Unicast ToDS SLIRP traffic is forwarded once RF state is re-enabled
    mock_gateway.wait_for_packet_count(1).await;
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
