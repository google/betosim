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

//! Integration tests for multi-client shared radio MAC delivery and ToDS
//! NullFunc BSSID learning (b/581470055).

use crate::world::{AP_BSSID, MDNS_MULTICAST_MAC, World};

const DEFAULT_OPEN_AP_BSSID: [u8; 6] = [0x02, 0x15, 0xb2, 0x00, 0x00, 0x01];
const HWSIM_RADIO_ADDR: [u8; 6] = [0x02, 0x00, 0x00, 0x00, 0x00, 0x00];
const GATEWAY_MAC: [u8; 6] = [0x02, 0x15, 0xb2, 0x00, 0x00, 0xfe];
const BROADCAST_MAC: [u8; 6] = [0xff; 6];
const IBSS_BSSID: [u8; 6] = [0x02, 0x15, 0xb2, 0x88, 0x88, 0x88];
const UNLEARNED_PEER_MAC: [u8; 6] = [0x02, 0x15, 0xb2, 0x99, 0x99, 0x99];
const STA_MAC_A: [u8; 6] = [0x02, 0x11, 0x22, 0x33, 0x44, 0x01];
const STA_MAC_B: [u8; 6] = [0x02, 0x11, 0x22, 0x33, 0x44, 0x02];
const STA_MAC_C: [u8; 6] = [0x02, 0x11, 0x22, 0x33, 0x44, 0x03];
const CHIP_1: u32 = 1;
const CHIP_2: u32 = 2;
const CHIP_3: u32 = 3;
const FREQ_2412: u32 = 2412;
const WITNESS_BEACON_COUNT: usize = 2;

// Scenario: ToDS NullFunc learns hosted AP BSSID for unicast downlink routing
//
// Given: A default open AP and a fresh station (Chip 1) on 2412 MHz.
// When: Chip 1 sends a ToDS NullFunc and the gateway injects a unicast downlink frame.
// Then: Chip 1 receives the unicast downlink frame immediately.
#[tokio::test]
async fn test_to_ds_nullfunc_learns_bssid_for_unicast_downlink() {
    let mut world = World::new_with_mock_gateway().await;
    world.given_default_open_ap().await;
    world.given_raw_chip(CHIP_1, STA_MAC_A).await;

    world
        .when_raw_chip_sends_nullfunc(CHIP_1, HWSIM_RADIO_ADDR, DEFAULT_OPEN_AP_BSSID, FREQ_2412)
        .await;
    world.when_gateway_injects_downlink(&GATEWAY_MAC, &STA_MAC_A, b"DownlinkAfterNullFunc");

    world
        .then_raw_chip_receives_downlink(
            CHIP_1,
            b"DownlinkAfterNullFunc",
            STA_MAC_A,
            HWSIM_RADIO_ADDR,
            Some(FREQ_2412),
        )
        .await;
}

// Scenario: Two guests sharing the same hwsim radio MAC both receive AP beacons
//
// Given: Two distinct guests (Chip 1 and Chip 2) on a default open AP.
// When: Both guests send a ToDS NullFunc using the shared hwsim radio MAC and drain initial RX.
// Then: Both guests receive AP beacons on 2412 MHz.
#[tokio::test]
async fn test_two_guests_sharing_hwsim_addr_both_receive_ap_beacons() {
    let mut world = World::new_with_mock_gateway().await;
    world.given_default_open_ap().await;
    world.given_raw_chip(CHIP_1, STA_MAC_A).await;
    world.given_raw_chip(CHIP_2, STA_MAC_B).await;

    world
        .when_raw_chip_sends_nullfunc(CHIP_1, HWSIM_RADIO_ADDR, DEFAULT_OPEN_AP_BSSID, FREQ_2412)
        .await;
    world
        .when_raw_chip_sends_nullfunc(CHIP_2, HWSIM_RADIO_ADDR, DEFAULT_OPEN_AP_BSSID, FREQ_2412)
        .await;
    world.when_raw_chip_rx_drained(CHIP_1);
    world.when_raw_chip_rx_drained(CHIP_2);

    world.then_raw_chip_receives_beacon(CHIP_1, DEFAULT_OPEN_AP_BSSID, HWSIM_RADIO_ADDR).await;
    world.then_raw_chip_receives_beacon(CHIP_2, DEFAULT_OPEN_AP_BSSID, HWSIM_RADIO_ADDR).await;
}

// Scenario: Two guests sharing the same hwsim radio MAC both receive gateway broadcasts
//
// Given: Two distinct guests registered on 2412 MHz sharing the default hwsim radio MAC.
// When: The gateway injects a broadcast Ethernet frame.
// Then: Both guests receive the broadcast downlink frame.
#[tokio::test]
async fn test_two_guests_sharing_hwsim_addr_both_receive_gateway_broadcasts() {
    let mut world = World::new_with_mock_gateway().await;
    world.given_default_open_ap().await;
    world
        .given_raw_chip_registered_with_data(
            CHIP_1,
            STA_MAC_A,
            HWSIM_RADIO_ADDR,
            GATEWAY_MAC,
            DEFAULT_OPEN_AP_BSSID,
            FREQ_2412,
        )
        .await;
    world
        .given_raw_chip_registered_with_data(
            CHIP_2,
            STA_MAC_B,
            HWSIM_RADIO_ADDR,
            GATEWAY_MAC,
            DEFAULT_OPEN_AP_BSSID,
            FREQ_2412,
        )
        .await;

    world.when_gateway_injects_downlink(&GATEWAY_MAC, &BROADCAST_MAC, b"BroadcastToBothGuests");

    world
        .then_raw_chip_receives_downlink(
            CHIP_1,
            b"BroadcastToBothGuests",
            BROADCAST_MAC,
            HWSIM_RADIO_ADDR,
            Some(FREQ_2412),
        )
        .await;
    world
        .then_raw_chip_receives_downlink(
            CHIP_2,
            b"BroadcastToBothGuests",
            BROADCAST_MAC,
            HWSIM_RADIO_ADDR,
            Some(FREQ_2412),
        )
        .await;
}

// Scenario: Unknown-unicast flood reaches all peers sharing the same hwsim radio MAC
//
// Given: Three guests registered on 2412 MHz sharing the default hwsim radio MAC.
// When: Chip 1 sends a non-infrastructure IBSS data frame to an unlearned peer MAC.
// Then: Both peer guests (Chip 2 and Chip 3) receive the flooded IBSS frame.
#[tokio::test]
async fn test_unknown_unicast_flood_reaches_all_peers_sharing_hwsim_addr() {
    let mut world = World::new_with_mock_gateway().await;
    world.given_default_open_ap().await;
    world
        .given_raw_chip_registered_with_nullfunc(
            CHIP_1,
            STA_MAC_A,
            HWSIM_RADIO_ADDR,
            DEFAULT_OPEN_AP_BSSID,
            FREQ_2412,
        )
        .await;
    world
        .given_raw_chip_registered_with_nullfunc(
            CHIP_2,
            STA_MAC_B,
            HWSIM_RADIO_ADDR,
            DEFAULT_OPEN_AP_BSSID,
            FREQ_2412,
        )
        .await;
    world
        .given_raw_chip_registered_with_nullfunc(
            CHIP_3,
            STA_MAC_C,
            HWSIM_RADIO_ADDR,
            DEFAULT_OPEN_AP_BSSID,
            FREQ_2412,
        )
        .await;

    world
        .when_raw_chip_sends_ibss_data(
            CHIP_1,
            HWSIM_RADIO_ADDR,
            UNLEARNED_PEER_MAC,
            IBSS_BSSID,
            FREQ_2412,
            b"UnknownUnicastFlood",
        )
        .await;

    world
        .then_raw_chip_receives_ibss_data(
            CHIP_2,
            b"UnknownUnicastFlood",
            UNLEARNED_PEER_MAC,
            HWSIM_RADIO_ADDR,
            FREQ_2412,
        )
        .await;
    world
        .then_raw_chip_receives_ibss_data(
            CHIP_3,
            b"UnknownUnicastFlood",
            UNLEARNED_PEER_MAC,
            HWSIM_RADIO_ADDR,
            FREQ_2412,
        )
        .await;
}

// Scenario: Silent client on shared hwsim radio MAC receives no beacons before first uplink
//
// Given: Registered witness Chip 1 and newly created silent Chip 2 on a default open AP.
// When: Witness Chip 1 observes two consecutive AP beacons.
// Then: Silent Chip 2 has received zero frames.
#[tokio::test]
async fn test_silent_client_on_shared_hwsim_addr_receives_no_beacons_before_first_uplink() {
    let mut world = World::new_with_mock_gateway().await;
    world.given_default_open_ap().await;
    world
        .given_raw_chip_registered_with_nullfunc(
            CHIP_1,
            STA_MAC_A,
            HWSIM_RADIO_ADDR,
            DEFAULT_OPEN_AP_BSSID,
            FREQ_2412,
        )
        .await;
    world.given_raw_chip(CHIP_2, STA_MAC_B).await;

    world
        .when_witness_chip_observes_beacons(
            CHIP_1,
            DEFAULT_OPEN_AP_BSSID,
            HWSIM_RADIO_ADDR,
            WITNESS_BEACON_COUNT,
        )
        .await;

    world.then_raw_chip_receives_no_frames(CHIP_2).await;
}

// ============================================================================
// Feature: Colliding HWSIM Radio Address Dedup Protection (b/581470055)
// ============================================================================

const SECURE_AP_SHARED_HWSIM_ADDR: [u8; 6] = [0x02, 0x00, 0x00, 0x00, 0x01, 0x00];

// Scenario: mDNS multicast from station delivers to all non-sender stations sharing hwsim_addr
//
// Given: Three stations registered with distinct station MACs but the same HWSIM radio address on
// a default open AP.
// When: Station 1 transmits an mDNS multicast packet to the hosted default open AP BSSID.
// Then: Station 2 and Station 3 each receive an M2U unicast copy addressed to their station MAC.
#[tokio::test]
async fn test_colliding_hwsim_mdns_multicast_reaches_all_non_sender_stations() {
    let mut world = World::new().await;
    world.given_default_open_ap().await;

    world
        .given_raw_chip_registered_with_nullfunc(
            CHIP_1,
            STA_MAC_A,
            HWSIM_RADIO_ADDR,
            DEFAULT_OPEN_AP_BSSID,
            FREQ_2412,
        )
        .await;
    world
        .given_raw_chip_registered_with_nullfunc(
            CHIP_2,
            STA_MAC_B,
            HWSIM_RADIO_ADDR,
            DEFAULT_OPEN_AP_BSSID,
            FREQ_2412,
        )
        .await;
    world
        .given_raw_chip_registered_with_nullfunc(
            CHIP_3,
            STA_MAC_C,
            HWSIM_RADIO_ADDR,
            DEFAULT_OPEN_AP_BSSID,
            FREQ_2412,
        )
        .await;

    world
        .when_raw_chip_sends_to_ds_data(
            CHIP_1,
            HWSIM_RADIO_ADDR,
            MDNS_MULTICAST_MAC,
            DEFAULT_OPEN_AP_BSSID,
            FREQ_2412,
            b"MdnsDiscoveryPayload",
        )
        .await;

    world
        .then_raw_chip_receives_downlink(
            CHIP_2,
            b"MdnsDiscoveryPayload",
            STA_MAC_B,
            HWSIM_RADIO_ADDR,
            Some(FREQ_2412),
        )
        .await;
    world
        .then_raw_chip_receives_downlink(
            CHIP_3,
            b"MdnsDiscoveryPayload",
            STA_MAC_C,
            HWSIM_RADIO_ADDR,
            Some(FREQ_2412),
        )
        .await;
}

// Scenario: Broadcast DHCP discovery and request deliver distinct IP leases across colliding
// hwsim_addr
//
// Given: An AP and three stations sharing the same HWSIM radio address.
// When: Each station executes DHCP Discover/Offer/Request/ACK lease acquisition.
// Then: All three stations receive broadcast replies and obtain valid, distinct SLiRP leases.
#[tokio::test]
async fn test_colliding_hwsim_broadcast_dhcp_reaches_stations_with_distinct_chaddr() {
    let mut world = World::new().await;
    world.given_default_open_ap().await;

    world
        .given_raw_chip_registered_with_nullfunc(
            CHIP_1,
            STA_MAC_A,
            HWSIM_RADIO_ADDR,
            DEFAULT_OPEN_AP_BSSID,
            FREQ_2412,
        )
        .await;
    world
        .given_raw_chip_registered_with_nullfunc(
            CHIP_2,
            STA_MAC_B,
            HWSIM_RADIO_ADDR,
            DEFAULT_OPEN_AP_BSSID,
            FREQ_2412,
        )
        .await;
    world
        .given_raw_chip_registered_with_nullfunc(
            CHIP_3,
            STA_MAC_C,
            HWSIM_RADIO_ADDR,
            DEFAULT_OPEN_AP_BSSID,
            FREQ_2412,
        )
        .await;

    let ip1 = world
        .when_raw_chip_acquires_dhcp_lease(
            CHIP_1,
            HWSIM_RADIO_ADDR,
            DEFAULT_OPEN_AP_BSSID,
            FREQ_2412,
        )
        .await;
    let ip2 = world
        .when_raw_chip_acquires_dhcp_lease(
            CHIP_2,
            HWSIM_RADIO_ADDR,
            DEFAULT_OPEN_AP_BSSID,
            FREQ_2412,
        )
        .await;
    let ip3 = world
        .when_raw_chip_acquires_dhcp_lease(
            CHIP_3,
            HWSIM_RADIO_ADDR,
            DEFAULT_OPEN_AP_BSSID,
            FREQ_2412,
        )
        .await;

    world.then_leases_are_valid_and_distinct(&[ip1, ip2, ip3]);
}

// Scenario: Three stations sharing hwsim_addr receive distinct CCMP-encrypted M2U frames
//
// Given: A secure AP and three stations sharing the same HWSIM radio address, each with distinct
// PTKs.
// When: The AP transmits an mDNS multicast packet converted to per-station encrypted M2U frames.
// Then: All three stations receive and successfully decrypt their CCMP-encrypted M2U frame with
// their PTK.
#[tokio::test]
async fn test_colliding_hwsim_stations_receive_distinct_encrypted_m2u_frames() {
    let mut world = World::new().await;
    world.given_a_secure_ap().await;

    world
        .given_raw_chip_registered_with_nullfunc(
            CHIP_1,
            STA_MAC_A,
            SECURE_AP_SHARED_HWSIM_ADDR,
            AP_BSSID,
            FREQ_2412,
        )
        .await;
    world
        .given_raw_chip_registered_with_nullfunc(
            CHIP_2,
            STA_MAC_B,
            SECURE_AP_SHARED_HWSIM_ADDR,
            AP_BSSID,
            FREQ_2412,
        )
        .await;
    world
        .given_raw_chip_registered_with_nullfunc(
            CHIP_3,
            STA_MAC_C,
            SECURE_AP_SHARED_HWSIM_ADDR,
            AP_BSSID,
            FREQ_2412,
        )
        .await;

    let tk1: [u8; 16] = [0x11; 16];
    let tk2: [u8; 16] = [0x22; 16];
    let tk3: [u8; 16] = [0x33; 16];

    world.given_station_ptk(STA_MAC_A, &tk1);
    world.given_station_ptk(STA_MAC_B, &tk2);
    world.given_station_ptk(STA_MAC_C, &tk3);

    world.when_infra_transmits_mdns("SecureMdnsPayload").await;

    world.then_raw_chip_receives_encrypted_m2u_frame(CHIP_1, &tk1, "SecureMdnsPayload").await;
    world.then_raw_chip_receives_encrypted_m2u_frame(CHIP_2, &tk2, "SecureMdnsPayload").await;
    world.then_raw_chip_receives_encrypted_m2u_frame(CHIP_3, &tk3, "SecureMdnsPayload").await;
}
