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

use netsim_packets::{MacAddr, management_subtype};
use tracing::info;

use crate::world::ApWorld;

// ============================================================================
// Feature: Hostapd CLI Command Support
// ============================================================================

// Scenario: Update Beacon Channel
#[tokio::test]
async fn test_update_channel() {
    let mut world = ApWorld::new().await;
    world.given_a_registered_ap("ChannelAP").await;
    let id = world.ap_id.expect("AP ID");

    // 1. Verify Initial Channel (6) via Beacon
    // (Skipped for brevity/limitations of helper, assuming default)

    // 2. Update Channel to 1
    info!("When the AP channel is updated to 1");
    world.client.update_ap_config(id, None, Some(1), None, None).await.expect("Update failed");

    // 3. Verify Beacon has Channel 1
    // We filter for a beacon that has the DS Param Set tag (3) with channel 1.
    let _msg = world
        .recv_frame(|frame, msg| {
            if frame.stype() != management_subtype::BEACON {
                return false;
            }
            // Parse Tags
            // DS Parameter Set is Tag 3, Len 1, Channel(u8)
            let payload = &msg[36..]; // Skip Header(24) + Fixed(12)
            let mut offset = 0;
            while offset + 2 <= payload.len() {
                let id = payload[offset];
                let len = payload[offset + 1] as usize;
                if offset + 2 + len > payload.len() {
                    break;
                }
                if id == 3 && len == 1 {
                    let channel = payload[offset + 2];
                    if channel == 1 {
                        return true;
                    }
                }
                offset += 2 + len;
            }
            false
        })
        .await;
}

// Scenario: Force Disconnect (Deauthenticate)
#[tokio::test]
async fn test_force_disconnect() {
    let mut world = ApWorld::new().await;
    world.given_a_registered_ap("DeauthAP").await;
    let id = world.ap_id.expect("AP ID");

    let station_mac_str = "02:00:00:00:00:99";
    let station_mac: MacAddr = station_mac_str.try_into().unwrap();

    // 1. Associate
    world.when_station_sends_assoc_req(station_mac_str).await;
    world.then_station_receives_assoc_resp(station_mac_str).await;

    // 2. Force Disconnect
    info!("When the AP forces disconnect for {}", station_mac);
    // In `world.rs`, the station uses `ChipId(1234)`.
    world
        .client
        .update_ap_config(id, None, None, Some(vec![station_mac_str.to_string()]), None)
        .await
        .expect("Force disconnect failed");

    // 3. Verify Deauth Frame Received
    let _msg = world
        .recv_frame(|frame, _| {
            if frame.stype() == management_subtype::BEACON {
                return false;
            }
            if frame.stype() == management_subtype::DEAUTHENTICATION {
                if frame.get_addr1() == station_mac {
                    return true;
                }
            }
            false
        })
        .await;
}

// Scenario: Enable/Disable AP
#[tokio::test]
async fn test_enable_disable() {
    let mut world = ApWorld::new().await;
    world.given_a_registered_ap("ToggleAP").await;
    let id = world.ap_id.expect("AP ID");

    // 1. Verify Beacons running
    world.then_beacon_is_received("ToggleAP").await;

    // 2. Disable
    info!("When the AP is disabled");
    world.client.update_ap_config(id, None, None, None, Some(false)).await.expect("Disable failed");

    // 3. Verify No Beacons
    world.then_no_beacons_are_received().await;

    // 4. Enable
    info!("When the AP is enabled");
    world.client.update_ap_config(id, None, None, None, Some(true)).await.expect("Enable failed");

    // 5. Verify Beacons return
    world.then_beacon_is_received("ToggleAP").await;
}
