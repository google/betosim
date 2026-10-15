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
use netsim_model::WifiMode;
use netsim_packets::{
    AuthenticationFixedFields, FrameControl, Ieee80211, MacAddr, MacHeader3Addr, SequenceControl,
    management_subtype,
};
use zerocopy::{IntoBytes, U16};

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
    let mut world = ApWorld::new().await;

    // Config with SAE Enabled
    let config = ap_actor::ApConfig {
        ssid: "SaeAP".to_string(),
        bssid: "02:00:00:00:00:01".parse().unwrap(),
        channel: 36,
        hw_mode: WifiMode::Ax,
        wpa_passphrase: Some("password123".to_string()),
        beacon_interval: 100,
        country_code: None,
        dtim_period: 2,
        hidden_ssid: false,
        sae: true,
        wmm_enabled: true,
        enterprise_enabled: false,
        mac_acl_mode: 0,
        mac_acl_list: vec![],
        ftm_responder_enabled: true,
        position: netsim_model::Position::default(),
    };

    world.given_a_registered_ap_with_config(config.clone()).await;

    let client_mac: MacAddr = "02:00:00:00:00:02".try_into().unwrap();
    let ap_bssid = config.bssid;

    // Start Client-Side SAE State Machine to generate valid frames
    let mut client_sae = SaeStateMachine::new(&client_mac.bytes, &ap_bssid.bytes, b"password123");

    // 1. Client Sends Commit
    let commit_body = client_sae.build_commit().expect("Client Commit");

    // Construct Auth Frame (Seq 1)
    let mut auth_frame = Vec::new();
    let header = MacHeader3Addr::new(
        FrameControl::new(0x00B0), // Mgmt, Auth
        0,
        ap_bssid,
        client_mac,
        ap_bssid,
        SequenceControl::new(0),
    );
    auth_frame.extend_from_slice(header.as_bytes());

    let fixed = AuthenticationFixedFields {
        algorithm: U16::new(3), // SAE
        sequence: U16::new(1),
        status: U16::new(0),
    };
    auth_frame.extend_from_slice(fixed.as_bytes());
    auth_frame.extend_from_slice(&commit_body);

    let tx = world.tx_to_ap.as_ref().unwrap().clone();
    let src_id = netsim_model::ChipId(200);
    tx.send(bytes::Bytes::from(auth_frame)).unwrap();

    // 2. Expect AP Commit (Seq 1 or 2? SAE is 1, but response usually has same seq
    //    if strictly following Request/Response? No.
    // 2. Expect AP Commit (Seq 1 or 2? SAE is 1, but response usually has same seq
    //    if strictly following Request/Response? No.
    // 802.11-2016: SAE Commit is Seq 1. Confirm is Seq 2.
    // Both sides send Commit (Seq 1).
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

    // Check it is SAE
    // Skip Header(24) + Fixed(6)
    let body1 = &msg1[30..];
    // AP should send Commit (Seq 1)
    // Try to parse with Client SAE
    client_sae.parse_commit(body1).expect("Client failed to parse AP Commit");

    // 3. Client Sends Confirm (Seq 2)
    let confirm_body = client_sae.build_confirm().expect("Client Confirm");

    let mut confirm_frame = Vec::new();
    let header2 = MacHeader3Addr::new(
        FrameControl::new(0x00B0),
        0,
        ap_bssid,
        client_mac,
        ap_bssid,
        SequenceControl::new(0),
    );
    confirm_frame.extend_from_slice(header2.as_bytes());
    let fixed2 = AuthenticationFixedFields {
        algorithm: U16::new(3),
        sequence: U16::new(2),
        status: U16::new(0),
    };
    confirm_frame.extend_from_slice(fixed2.as_bytes());
    confirm_frame.extend_from_slice(&confirm_body);

    tx.send(bytes::Bytes::from(confirm_frame)).unwrap();

    // 4. Expect AP Confirm (Seq 2)
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

    println!("SAE Handshake Integration Test Passed");
}
