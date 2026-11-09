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

use ap_actor::SaeStateMachine;
use netsim_packets::{Ieee80211, MacAddr, management_subtype};

use crate::world::ApWorld;

// ============================================================================
// Feature: WPA3-SAE (Simultaneous Authentication of Equals)
// ============================================================================

// Scenario: SAE Handshake Success
// Given a registered AP with WPA3-SAE enabled
// When a Station initiates SAE Authentication (Commit)
// And responds to the AP's Commit with Confirm
// Then the AP completes the handshake (sends Confirm)
#[tokio::test]
async fn test_sae_handshake_success() {
    // Given
    let mut world = ApWorld::new().await;
    let ap_bssid = world.given_a_sae_ap("SaeAP", "password123").await;

    let client_mac: MacAddr = "02:00:00:00:00:02".try_into().unwrap();
    let mut client_sae = SaeStateMachine::new(&client_mac.bytes, &ap_bssid.bytes, b"password123");

    // When: Client sends SAE Commit (Seq 1)
    let commit_body = client_sae.build_commit().expect("Client Commit");
    world.when_station_sends_sae_auth(client_mac, 1, 0, &commit_body).await;

    // Then: AP replies with SAE Commit (Seq 1)
    let msg1 = world
        .recv_frame(|frame, _| {
            if frame.stype() == management_subtype::BEACON {
                return false;
            }
            frame.stype() == management_subtype::AUTHENTICATION
        })
        .await;

    let frame1 = Ieee80211::decode(&msg1).unwrap();
    assert_eq!(frame1.stype(), management_subtype::AUTHENTICATION);

    let body1 = &msg1[30..];
    client_sae.parse_commit(body1).expect("Client failed to parse AP Commit");

    // When: Client sends SAE Confirm (Seq 2)
    let confirm_body = client_sae.build_confirm().expect("Client Confirm");
    world.when_station_sends_sae_auth(client_mac, 2, 0, &confirm_body).await;

    // Then: AP replies with SAE Confirm (Seq 2)
    let msg2 = world
        .recv_frame(|frame, _| {
            if frame.stype() == management_subtype::BEACON {
                return false;
            }
            frame.stype() == management_subtype::AUTHENTICATION
        })
        .await;

    let frame2 = Ieee80211::decode(&msg2).unwrap();
    assert_eq!(frame2.stype(), management_subtype::AUTHENTICATION);

    let body2 = &msg2[30..];
    client_sae.parse_confirm(body2).expect("Client failed to parse AP Confirm");
}

// Scenario: SAE Confirm without a prior SAE Commit session is rejected
// Given a registered AP with WPA3-SAE enabled and no active SAE session for the station
// When a Station sends an out-of-order SAE Confirm (sequence 2) without a prior Commit
// Then the AP ignores the frame and does not send an Authentication response
#[tokio::test]
async fn test_sae_confirm_without_prior_commit_is_rejected() {
    // Given
    let mut world = ApWorld::new().await;
    world.given_a_sae_ap("SaeAPNoSession", "password123").await;

    let client_mac: MacAddr = "02:00:00:00:00:03".try_into().unwrap();

    // When: Send SAE Confirm (seq 2) with dummy confirm payload without prior Commit
    world.when_station_sends_sae_auth(client_mac, 2, 0, &[0u8; 34]).await;

    // Then: AP does not emit an SAE Confirm
    world.then_no_eap_or_auth_frame_is_received().await;
}

// Scenario: SAE Commit with malformed/truncated payload is rejected
// Given a registered AP with WPA3-SAE enabled
// When a Station sends an SAE Commit (sequence 1) with a truncated/invalid body
// Then the AP rejects the malformed Commit and sends no Authentication response
#[tokio::test]
async fn test_sae_commit_with_invalid_payload_is_rejected() {
    // Given
    let mut world = ApWorld::new().await;
    world.given_a_sae_ap("SaeAPMalformed", "password123").await;

    let client_mac: MacAddr = "02:00:00:00:00:04".try_into().unwrap();

    // When: Send SAE Commit (seq 1) with truncated 4-byte payload
    world.when_station_sends_sae_auth(client_mac, 1, 0, &[0x13, 0x00, 0xAA, 0xBB]).await;

    // Then: AP does not emit an SAE Commit response
    world.then_no_eap_or_auth_frame_is_received().await;
}
