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

use crate::world::{AP_BSSID, World};

// ============================================================================
// Feature: SLIRP Dual MAC Mapping (Randomized 802.11 SA vs HWSIM Transmitter MAC)
// ============================================================================

// Scenario: SlirpGateway delivers downlink traffic addressed to a station's HWSIM MAC when using a
//   randomized 802.11 MAC
// Given a Wifi World with an active AP and a station whose 802.11 SA (00:00:00:00:00:02) differs
//   from its HWSIM MAC (00:00:00:00:00:03)
// When the station transmits an uplink frame establishing the dual mapping and SlirpGateway
//   receives a downlink Ethernet frame addressed to the HWSIM MAC
// Then SharedKeyStore maps both MACs to the AP BSSID and SlirpGateway::handle_incoming resolves
//   the BSSID and forwards the frame to the station without dropping it
#[tokio::test]
async fn test_slirp_does_not_drop_with_dual_mapping() {
    // Given: A World with an active AP and a station using a randomized 802.11 MAC
    // distinct from its HWSIM hardware MAC.
    let mut world = World::new().await;
    world.given_an_ap().await;

    let randomized_mac = [0x00, 0x00, 0x00, 0x00, 0x00, 0x02];
    let hw_mac = [0x00, 0x00, 0x00, 0x00, 0x00, 0x03];

    // When: The station transmits an uplink frame establishing the dual mapping in SharedKeyStore.
    world.given_a_chip_with_randomized_mac(1, randomized_mac, hw_mac).await;

    // Then: Both the randomized MAC and hardware MAC are mapped to the AP BSSID in SharedKeyStore,
    // and SlirpGateway::handle_incoming resolves hw_mac via SharedKeyStore::get_station_bssid and
    // delivers the downlink payload to the station without dropping.
    world.then_shared_keys_maps_mac_to_bssid(randomized_mac, AP_BSSID);
    world.then_shared_keys_maps_mac_to_bssid(hw_mac, AP_BSSID);
    world.then_slirp_gateway_delivers_downlink_to_mac(0, hw_mac, "Hello Dual Mapping");
}
