// Copyright 2025-2026 The Android Open Source Project
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

use std::time::{Duration, Instant};

use ap_actor::SaeStateMachine;
use netsim_packets::{MacAddr, management_subtype};
use tracing::info;

use crate::world::ApWorld;

// ============================================================================
// Feature: Station Deauthentication and Session Cleanup
// ============================================================================

// Scenario: Station Deauthentication and Re-association
// Given a registered AP with WPA2 enabled
// And a Station is associated
// When the Station sends a Deauthentication frame
// Then the AP removes the station state and allows the station to re-associate cleanly
#[tokio::test]
async fn test_station_deauth() {
    info!("Scenario: Station Deauthentication");
    // Given
    let mut world = ApWorld::new().await;
    world.given_a_registered_ap_with_wpa("WpaAP", "CorrectPassword").await;

    let station_mac = "02:00:00:00:00:99";
    let station_mac_addr: MacAddr = station_mac.try_into().unwrap();

    world.when_station_sends_assoc_req(station_mac).await;
    world.then_station_receives_assoc_resp(station_mac).await;
    let ap_id = world.ap_id.expect("AP ID not set");
    let ap_before = world.client.get_ap(ap_id).await.expect("Get AP before deauth").unwrap();
    assert_eq!(ap_before.associations.len(), 1);

    // When: The Station sends a Deauthentication frame
    info!("When the Station sends a Deauthentication frame");
    world.when_station_sends_deauth(station_mac_addr, 3).await;

    // Then: The AP removes the station from associations and allows it to re-associate cleanly
    let deadline = Instant::now() + Duration::from_secs(1);
    loop {
        let ap_after = world.client.get_ap(ap_id).await.expect("Get AP after deauth").unwrap();
        if ap_after.associations.is_empty() {
            break;
        }
        assert!(Instant::now() < deadline, "Expected associations to be cleared on deauth");
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    world.when_station_sends_assoc_req(station_mac).await;
    world.then_station_receives_assoc_resp(station_mac).await;
}

// Scenario: Auth Sequence 1 restart clears stale SAE session state via implicit deauthentication
// Given a registered WPA3-SAE AP and a Station that has completed SAE Commit (step 1)
// When the Station initiates a fresh Auth Sequence 1 (triggering clear_station_state) and then
//   sends SAE Confirm (step 2) from the prior session
// Then the AP rejects the stale SAE Confirm because the prior SAE session was cleared
#[tokio::test]
async fn test_auth_seq1_restart_clears_stale_sae_session() {
    // Given: A WPA3-SAE AP and a station that completed SAE Commit
    let mut world = ApWorld::new().await;
    let bssid = world.given_a_sae_ap("SaeDeauthAP", "password123").await;

    let client_mac: MacAddr = "02:00:00:00:00:88".try_into().unwrap();
    let mut client_sae = SaeStateMachine::new(&client_mac.bytes, &bssid.bytes, b"password123");

    let commit_body = client_sae.build_commit().expect("Client Commit");
    world.when_station_sends_sae_auth(client_mac, 1, 0, &commit_body).await;

    let msg1 = world
        .recv_frame(|frame, _| {
            if frame.stype() == management_subtype::BEACON {
                return false;
            }
            frame.stype() == management_subtype::AUTHENTICATION
        })
        .await;
    client_sae.parse_commit(&msg1[30..]).expect("Parse AP Commit");
    let confirm_body = client_sae.build_confirm().expect("Client Confirm");

    // When: Station sends a fresh Auth Seq 1 (triggering implicit deauth/clear_station_state)
    // and then attempts to use the old SAE session's Confirm (Seq 2)
    world.when_station_sends_sae_auth(client_mac, 1, 0, &[0x00, 0x01]).await;
    world.when_station_sends_sae_auth(client_mac, 2, 0, &confirm_body).await;

    // Then: AP ignores the stale SAE Confirm because clear_station_state removed the old session
    world.then_no_eap_or_auth_frame_is_received().await;
}
