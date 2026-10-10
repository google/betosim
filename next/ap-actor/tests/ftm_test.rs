// Copyright 2024 The Android Open Source Project
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

use netsim_model::WifiMode;
use netsim_packets::{Ieee80211, MacAddr, category, management_subtype, public_action};
use zerocopy::IntoBytes;

use crate::world;

// ============================================================================
// Feature: 802.11mc FTM Ranging
// ============================================================================

// Scenario: FTM Ranging Exchange
// Given a registered AP with FTM Responder enabled
// When a Station sends a Fine Timing Measurement (FTM) Request
// Then the AP responds with an FTM Initial Frame (Ack)
// And the AP follows up with an FTM Frame containing timestamps (TOD/TOA)
#[tokio::test]
async fn test_ftm_ranging_exchange() {
    let mut world = world::ApWorld::new().await;
    let config = ap_actor::ApConfig {
        ssid: "ftm_test_ap".to_string(),
        bssid: MacAddr::new([0x02, 0x00, 0x00, 0x00, 0x00, 0x01]),
        channel: 6,
        hw_mode: WifiMode::G,
        wpa_passphrase: None,
        beacon_interval: 100,
        country_code: None,
        dtim_period: 2,
        hidden_ssid: false,
        sae: false,
        wmm_enabled: true,
        enterprise_enabled: false,
        mac_acl_mode: 0,
        mac_acl_list: vec![],
        ftm_responder_enabled: true,
        position: netsim_model::Position::default(),
    };

    world.given_a_registered_ap_with_config(config.clone()).await;
    let ap_id = world.ap_id.expect("AP ID not set");
    let client_mac = world::generate_random_mac().into();

    // 1. Verify Beacon Advertisement (Extended Capabilities)
    world.then_beacon_is_received(&config.ssid).await;
    // Verify FTM Responder capabilities (Bit 70) in the Beacon.
    // TODO: Inspect the beacon via rx_from_ap manually if needed, or rely on
    // `then_beacon_is_received` for existence. For now, we proceed to FTM
    // exchange.

    // 2. Send FTM Request
    // Construct FTM Request Frame (Public Action 32)
    let mut req_frame = Vec::new();

    // Header
    let header = netsim_packets::MacHeader3Addr {
        frame_control: netsim_packets::FrameControl::new(0x00D0), // Action
        duration_id: zerocopy::U16::new(0),
        addr1: config.bssid,
        addr2: client_mac,
        addr3: config.bssid,
        sequence_control: netsim_packets::SequenceControl::new(0),
    };
    req_frame.extend_from_slice(header.as_bytes());

    // Body
    req_frame.push(category::PUBLIC);
    req_frame.push(public_action::FTM_REQUEST);
    req_frame.push(1); // Trigger = 1

    let src_id = netsim_model::ChipId(100);
    world.tx_to_ap.as_ref().unwrap().send(bytes::Bytes::from(req_frame)).unwrap();

    // 3. Verify Response(s)
    // Expect: FTM Initial Frame
    let ftm_1 = world
        .recv_frame(|frame, msg| {
            if frame.stype() == management_subtype::BEACON {
                return false;
            }
            frame.stype() == management_subtype::ACTION
                && msg.len() > 25
                && msg[24] == category::PUBLIC
                && msg[25] == public_action::FINE_TIMING_MEASUREMENT
        })
        .await;

    // Expect: FTM Follow Up Frame (with timestamps)
    // For simplicity, we just look for another FTM Action frame.
    // In a real scenario, we might want to check Dialog Token or other fields to
    // differentiate. But since recv_frame returns a copy, we can call it again.
    let ftm_2 = world
        .recv_frame(|frame, msg| {
            if frame.stype() == management_subtype::BEACON {
                return false;
            }
            frame.stype() == management_subtype::ACTION
                && msg.len() > 25
                && msg[24] == category::PUBLIC
                && msg[25] == public_action::FINE_TIMING_MEASUREMENT
                && msg != ftm_1 // Ensure it's a new frame (though strictly
            // recv_frame doesn't buffer past, it drains)
            // Actually recv_frame consumes from rx, so
            // calling it again yields the next one.
        })
        .await;

    let ftm_2_parsed = Ieee80211::decode(&ftm_2).unwrap();
    assert!(ftm_2_parsed.stype() == management_subtype::ACTION);
    assert_eq!(ftm_2[24], category::PUBLIC);
    assert_eq!(ftm_2[25], public_action::FINE_TIMING_MEASUREMENT);

    // Offset 26
    let _dialog_token = ftm_2[26];
    let _follow_up = ftm_2[27];
    let tod_bytes: [u8; 6] = ftm_2[28..34].try_into().unwrap();
    let toa_bytes: [u8; 6] = ftm_2[34..40].try_into().unwrap();

    // Verify they are non-zero (simulated)
    assert!(tod_bytes != [0; 6], "TOD should be populated");
    assert!(toa_bytes != [0; 6], "TOA should be populated");

    println!("FTM Success! TOD={:?} TOA={:?}", tod_bytes, toa_bytes);
}
