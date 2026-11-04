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

use netsim_model::{ChipId, ChipKind};
use slirp_actor::SlirpBackend;

use super::world::{GATEWAY_IP, World};

const GUEST_A_MAC: [u8; 6] = [0x02, 0x15, 0xb2, 0x00, 0x00, 0x01];
const GUEST_A_IP: [u8; 4] = [10, 0, 2, 15];
const GUEST_B_MAC: [u8; 6] = [0x02, 0x15, 0xb2, 0x00, 0x00, 0x02];
const GUEST_B_IP: [u8; 4] = [10, 0, 2, 16];

// Scenario: An Ethernet guest chip reaches the Slirp gateway via ARP
#[tokio::test]
async fn test_ethernet_guest_reaches_slirp_gateway() {
    let mut world = World::new().await;
    // Given an active Ethernet guest chip connected to the Slirp switch
    world.given_guest_chip(21, ChipKind::ETHERNET, GUEST_A_MAC, GUEST_A_IP).await;
    // When the guest sends an ARP request for the Slirp gateway IP
    world.when_guest_sends_arp_request(21, GATEWAY_IP).await;
    // Then the guest receives an ARP reply from the gateway
    world.then_guest_receives_arp_reply(21, GATEWAY_IP).await;
}

// Scenario: A cellular data guest chip reaches the Slirp gateway via ARP
#[tokio::test]
async fn test_cellular_data_guest_reaches_slirp_gateway() {
    let mut world = World::new().await;
    // Given an active cellular data guest chip connected to the Slirp switch
    world.given_guest_chip(22, ChipKind::CELLULAR_DATA, GUEST_A_MAC, GUEST_A_IP).await;
    // When the guest sends an ARP request for the Slirp gateway IP
    world.when_guest_sends_arp_request(22, GATEWAY_IP).await;
    // Then the guest receives an ARP reply from the gateway
    world.then_guest_receives_arp_reply(22, GATEWAY_IP).await;
}

// Scenario: Ethernet guest chips reach the Slirp gateway across Native and C-FFI backends
#[tokio::test]
async fn test_slirp_gateway_reachability_across_backends() {
    // Given an Ethernet guest chip attached to a Native Slirp backend
    let mut world_native = World::with_slirp_backend(SlirpBackend::Native).await;
    world_native.given_guest_chip(23, ChipKind::ETHERNET, GUEST_A_MAC, GUEST_A_IP).await;
    // When the guest sends an ARP request for the gateway IP
    world_native.when_guest_sends_arp_request(23, GATEWAY_IP).await;
    // Then the guest receives an ARP reply from the Native backend gateway
    world_native.then_guest_receives_arp_reply(23, GATEWAY_IP).await;

    // Given an Ethernet guest chip attached to a C-FFI Slirp backend
    let mut world_cffi = World::with_slirp_backend(SlirpBackend::CFfi).await;
    world_cffi.given_guest_chip(24, ChipKind::ETHERNET, GUEST_B_MAC, GUEST_B_IP).await;
    // When the guest sends an ARP request for the gateway IP
    world_cffi.when_guest_sends_arp_request(24, GATEWAY_IP).await;
    // Then the guest receives an ARP reply from the C-FFI backend gateway
    world_cffi.then_guest_receives_arp_reply(24, GATEWAY_IP).await;
}

// Scenario: Cellular data guest chip is isolated from flooded non-cellular downlink traffic
#[tokio::test]
async fn test_cellular_data_isolation_from_non_cellular_traffic() {
    let mut world = World::new().await;
    // Given an Ethernet guest chip and an isolated cellular data guest chip
    world.given_guest_chip(25, ChipKind::ETHERNET, GUEST_A_MAC, GUEST_A_IP).await;
    world.given_guest_chip(26, ChipKind::CELLULAR_DATA, GUEST_B_MAC, GUEST_B_IP).await;

    // When an unlearned host sends an ARP request that triggers a flooded reply
    world.when_unlearned_host_arps_gateway().await;
    // Then the Ethernet guest receives the flooded ARP reply while the cellular data guest does not
    world.then_guest_receives_flooded_arp_reply(25, GATEWAY_IP).await;
    world.then_guest_receives_no_arp_reply(26, GATEWAY_IP).await;
}

// Scenario: Unicast downlink reply reaches only the requesting guest chip
#[tokio::test]
async fn test_downlink_reply_reaches_only_the_requesting_chip() {
    let mut world = World::new().await;
    // Given two active Ethernet guest chips on the same switch
    world.given_guest_chip(27, ChipKind::ETHERNET, GUEST_A_MAC, GUEST_A_IP).await;
    world.given_guest_chip(28, ChipKind::ETHERNET, GUEST_B_MAC, GUEST_B_IP).await;
    // When the first guest chip sends an ARP request for the gateway
    world.when_guest_sends_arp_request(27, GATEWAY_IP).await;
    // Then only the first guest chip receives the unicast ARP reply
    world.then_guest_receives_arp_reply(27, GATEWAY_IP).await;
    world.then_guest_receives_no_arp_reply(28, GATEWAY_IP).await;
}

// Scenario: Deleting an active guest chip disconnects it from Slirp packet forwarding
#[tokio::test]
async fn test_deleted_chip_is_disconnected_from_slirp() {
    let mut world = World::new().await;
    // Given an active Ethernet guest chip that has verified gateway connectivity
    world.given_guest_chip(29, ChipKind::ETHERNET, GUEST_A_MAC, GUEST_A_IP).await;
    world.when_guest_sends_arp_request(29, GATEWAY_IP).await;
    world.then_guest_receives_arp_reply(29, GATEWAY_IP).await;

    // When the chip is deleted and the device is notified
    world.when_delete_chip(29).await;
    world.then_device_notified_chip_removed(29).await;

    // Then subsequent ARP requests from the deleted chip receive no reply
    world.when_guest_sends_arp_request(29, GATEWAY_IP).await;
    world.then_guest_receives_no_arp_reply(29, GATEWAY_IP).await;
}

// Scenario: Guest stream disconnection removes the chip and notifies the device actor
#[tokio::test]
async fn test_guest_disconnect_removes_chip_and_notifies_device() {
    let mut world = World::new().await;
    // Given an active Ethernet guest chip
    world.given_guest_chip(30, ChipKind::ETHERNET, GUEST_A_MAC, GUEST_A_IP).await;
    world.then_chip_count_is(1).await;

    // When the guest closes its packet transport channels
    world.when_guest_disconnects(30).await;
    // Then the device actor is notified and the chip is removed from the actor
    world.then_device_notified_chip_removed(30).await;
    world.then_chip_count_is(0).await;
    world.then_chip_not_found(30).await;
}

// Scenario: Disabling a chip or its technology radio drops uplink and downlink frames until re-enabled
#[tokio::test]
async fn test_disabled_chip_or_radio_drops_uplink_and_downlink_until_reenabled() {
    let mut world = World::new().await;
    // Given two active Ethernet guest chips and one active cellular data guest chip
    world.given_guest_chip(31, ChipKind::ETHERNET, GUEST_A_MAC, GUEST_A_IP).await;
    world.given_guest_chip(32, ChipKind::ETHERNET, GUEST_B_MAC, GUEST_B_IP).await;
    world
        .given_guest_chip(
            33,
            ChipKind::CELLULAR_DATA,
            [0x02, 0x15, 0xb2, 0x00, 0x00, 0x03],
            [10, 0, 2, 17],
        )
        .await;

    // When chip 31 is disabled via chip.enabled = false
    world.when_update_chip(31, None, Some(false), None).await;
    world.when_guest_sends_arp_request(31, GATEWAY_IP).await;
    world.when_unlearned_host_arps_gateway().await;
    // Then both uplink ARP requests and downlink flooded ARP replies are dropped for chip 31 while enabled chip 32 receives flooded frames
    world.then_guest_receives_no_arp_reply(31, GATEWAY_IP).await;
    world.then_guest_receives_flooded_arp_reply(32, GATEWAY_IP).await;

    // When chip 31 is re-enabled and then disabled via radio.state = Some(false)
    world.when_update_chip(31, None, Some(true), Some(false)).await;
    world.when_guest_sends_arp_request(31, GATEWAY_IP).await;
    world.when_unlearned_host_arps_gateway().await;
    // Then uplink and downlink frames remain dropped for chip 31 while enabled chip 32 still receives frames
    world.then_guest_receives_no_arp_reply(31, GATEWAY_IP).await;
    world.then_guest_receives_flooded_arp_reply(32, GATEWAY_IP).await;
    // And Slirp has not learned GUEST_A_MAC from the gated uplink frames, so a reply to GUEST_A_MAC floods to chip 32
    world.when_slirp_receives_arp_from_mac(GUEST_A_MAC, GUEST_A_IP).await;
    world.then_guest_receives_arp_reply_for_mac(32, GUEST_A_MAC, GATEWAY_IP).await;

    // When both chip.enabled and radio.state are re-enabled on chip 31
    world.when_update_chip(31, None, Some(true), Some(true)).await;
    world.when_guest_sends_arp_request(31, GATEWAY_IP).await;
    // Then ARP request and reply forwarding resumes on chip 31 and Slirp learns GUEST_A_MAC (no longer flooding to chip 32)
    world.then_guest_receives_arp_reply(31, GATEWAY_IP).await;
    world.then_guest_receives_no_arp_reply(32, GATEWAY_IP).await;

    // When cellular data chip 33 has its radio state disabled and later re-enabled
    world.when_update_chip(33, None, None, Some(false)).await;
    world.when_guest_sends_arp_request(33, GATEWAY_IP).await;
    // Then uplink ARP traffic is dropped while disabled and resumes once re-enabled
    world.then_guest_receives_no_arp_reply(33, GATEWAY_IP).await;
    world.when_update_chip(33, None, None, Some(true)).await;
    world.when_guest_sends_arp_request(33, GATEWAY_IP).await;
    world.then_guest_receives_arp_reply(33, GATEWAY_IP).await;
}

// Scenario: Resetting a disabled chip restores enabled state, clears disabled radio state, and restores packet forwarding
#[tokio::test]
async fn test_reset_disabled_chip_restores_enabled_radio_and_forwarding() {
    let mut world = World::new().await;
    // Given an active Ethernet guest chip and an active cellular data guest chip that are disabled
    world.given_guest_chip(34, ChipKind::ETHERNET, GUEST_A_MAC, GUEST_A_IP).await;
    world.given_guest_chip(35, ChipKind::CELLULAR_DATA, GUEST_B_MAC, GUEST_B_IP).await;
    world.when_update_chip(34, Some("eth-disabled"), Some(false), Some(false)).await;
    world.when_update_chip(35, Some("cell-disabled"), Some(false), Some(false)).await;
    world.when_guest_sends_arp_request(34, GATEWAY_IP).await;
    world.then_guest_receives_no_arp_reply(34, GATEWAY_IP).await;
    world.when_guest_sends_arp_request(35, GATEWAY_IP).await;
    world.then_guest_receives_no_arp_reply(35, GATEWAY_IP).await;

    // When both disabled chips are reset
    world.when_reset_chip(34).await;
    world.then_last_reset_chip_matches(34, ChipKind::ETHERNET, "eth-disabled", true);
    world.then_chip_matches(34, ChipKind::ETHERNET, "eth-disabled", true, None).await;
    world.when_reset_chip(35).await;
    world.then_last_reset_chip_matches(35, ChipKind::CELLULAR_DATA, "cell-disabled", true);
    world.then_chip_matches(35, ChipKind::CELLULAR_DATA, "cell-disabled", true, None).await;

    // Then ARP request and reply forwarding works again for both chips
    world.when_guest_sends_arp_request(34, GATEWAY_IP).await;
    world.then_guest_receives_arp_reply(34, GATEWAY_IP).await;
    world.when_guest_sends_arp_request(35, GATEWAY_IP).await;
    world.then_guest_receives_arp_reply(35, GATEWAY_IP).await;
}

// Scenario: Guest chip created in a disabled state drops uplink and downlink frames until reset
#[tokio::test]
async fn test_initially_disabled_guest_chip_drops_traffic_until_reset() {
    let mut world = World::new().await;
    // Given an initially disabled guest chip and an active peer guest chip
    world.given_disabled_guest_chip(ChipId(36), GUEST_A_MAC, GUEST_A_IP).await;
    world.given_guest_chip(37, ChipKind::ETHERNET, GUEST_B_MAC, GUEST_B_IP).await;

    // When the disabled guest sends an ARP request and an unlearned host ARPs the gateway
    world.when_guest_sends_arp_request(36, GATEWAY_IP).await;
    world.when_unlearned_host_arps_gateway().await;

    // Then only the active peer receives flooded ARP replies while the disabled guest drops frames
    world.then_guest_receives_flooded_arp_reply(37, GATEWAY_IP).await;
    world.then_guest_receives_no_arp_reply(36, GATEWAY_IP).await;
    // And Slirp has not learned GUEST_A_MAC from the gated uplink ARP, so a reply to GUEST_A_MAC floods to chip 37
    world.when_slirp_receives_arp_from_mac(GUEST_A_MAC, GUEST_A_IP).await;
    world.then_guest_receives_arp_reply_for_mac(37, GUEST_A_MAC, GATEWAY_IP).await;
    world.then_guest_receives_no_arp_reply(36, GATEWAY_IP).await;

    // When the disabled chip is reset
    world.when_reset_chip(36).await;

    // Then the chip is restored to active Ethernet state and forwards ARP traffic
    world.then_last_reset_chip_matches(36, ChipKind::ETHERNET, "eth0", true);
    world.when_guest_sends_arp_request(36, GATEWAY_IP).await;
    world.then_guest_receives_arp_reply(36, GATEWAY_IP).await;
}
