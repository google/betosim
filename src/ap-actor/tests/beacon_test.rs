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

use netsim_model::WifiMode;
use netsim_packets::{
    FrameControl, Ieee80211, MacAddr, MacHeader3Addr, SequenceControl, management_subtype,
};
use tracing::info;
use zerocopy::IntoBytes;

use crate::world::ApWorld;

// ============================================================================
// Feature: Wireless Network Visibility (Beacons)
//
// As a user, I want APs to broadcast beacons so that devices can discover
// the network and view its capabilities (e.g. WiFi 6 support).
// ============================================================================

// Scenario: Create an AP and verify beacons
// Given a new ApWorld
// When an AP is registered with SSID "TestAP"
// Then the World receives a Beacon for "TestAP"
#[tokio::test]
async fn test_create_ap_beacon_generation() {
    info!("Scenario: Create an AP and verify beacons");
    let mut world = ApWorld::new().await;

    // When
    world.given_a_registered_ap("TestAP").await;

    // Then
    world.then_beacon_is_received("TestAP").await;
}

// Scenario: Delete AP
// Given a registered AP "DeleteAP"
// When the AP is deleted
// Then no more beacons are received
#[tokio::test]
async fn test_delete_ap_stops_beacons() {
    info!("Scenario: Delete AP - stops beacons");
    let mut world = ApWorld::new().await;
    // Given (Implicit via world method logging)
    world.given_a_registered_ap("DeleteAP").await;

    // Ensure we receive at least one beacon first
    world.then_beacon_is_received("DeleteAP").await;

    // When
    world.when_ap_is_deleted().await;

    // Then
    world.then_no_beacons_are_received().await;
}

// Scenario: Create Request for WiFi 6 AP
// Given a new ApWorld
// When a WiFi 6 AP is registered
// Then the World receives a Beacon with HE Capabilities
#[tokio::test]
async fn test_wifi6_beacon() {
    info!("Scenario: Create Request for WiFi 6 AP");
    let mut world = ApWorld::new().await;

    // When
    world.given_a_wifi6_ap("WiFi6_AP").await;

    // Verify HE Element
    // Verify HE Element
    let rx = world.rx_from_ap.as_mut().expect("AP registered");
    let msg = tokio::time::timeout(std::time::Duration::from_secs(1), rx.recv())
        .await
        .expect("Timeout")
        .expect("Beacon");

    // Verify HE IE (ID 255, Ext 35)
    let mut offset = 36;
    let mut found_he = false;
    while offset < msg.len() {
        if offset + 1 >= msg.len() {
            break;
        }
        let id = msg[offset];
        let len = msg[offset + 1] as usize;
        if offset + 2 + len > msg.len() {
            break;
        }

        if id == 255 {
            if len > 0 && msg[offset + 2] == 35 {
                found_he = true;
                break;
            }
        }
        offset += 2 + len;
    }
    assert!(found_he, "Beacon should contain HE Capabilities IE");
}

// Scenario: Multiple APs
// Given a new ApWorld
// When two APs are registered
// Then the World receives beacons from both
#[tokio::test]
async fn test_create_two_aps() {
    info!("Scenario: Multiple APs");
    let mut world = ApWorld::new().await;

    // When
    world.given_a_registered_ap("AP1").await;
    world.given_a_registered_ap("AP2").await;

    // Then
    let rx = world.rx_from_ap.as_mut().expect("APs registered");
    let mut received_ssids = std::collections::HashSet::new();
    let start = std::time::Instant::now();

    while received_ssids.len() < 2 && start.elapsed() < std::time::Duration::from_secs(2) {
        if let Ok(Some(msg)) =
            tokio::time::timeout(std::time::Duration::from_millis(200), rx.recv()).await
        {
            if msg.len() > 38 {
                let len = msg[37] as usize;
                if msg.len() >= 38 + len {
                    let ssid = String::from_utf8_lossy(&msg[38..38 + len]).to_string();
                    received_ssids.insert(ssid);
                }
            }
        }
    }
    assert!(received_ssids.contains("AP1"), "Missing AP1 beacon");
    assert!(received_ssids.contains("AP2"), "Missing AP2 beacon");
}

// Scenario: Active Discovery (Probe Response)
// Given a registered AP
// When a Station sends a Probe Request
// Then the AP responds with a Probe Response
#[tokio::test]
async fn test_probe_response() {
    info!("Scenario: Active Discovery (Probe Response) - Generic/Wildcard");
    let mut world = ApWorld::new().await;
    info!("Given a registered AP 'ProbeAP'");
    world.given_a_registered_ap("ProbeAP").await;

    // Station sends Probe Req
    let station_mac: MacAddr = "02:00:00:00:00:99".try_into().unwrap();

    // When (Manual send, not helper)
    info!("When a Station sends a Wildcard Probe Request");
    let tx = world.tx_to_ap.as_mut().expect("AP registered");
    // FC: Mgmt(00), Probe Req(0100=4) -> 0x40
    let header = MacHeader3Addr::new(
        FrameControl::new(0x0040),
        0,
        MacAddr::BROADCAST, // DA
        station_mac,        // SA
        MacAddr::BROADCAST, // BSSID
        SequenceControl::new(0),
    );

    let mut frame = Vec::new();
    frame.extend_from_slice(header.as_bytes());
    // Body can be empty for our lax parser (Wildcard behavior)
    let src_id = netsim_model::ChipId(123);
    tx.send(bytes::Bytes::from(frame)).expect("Send Probe Req");

    // Verify Response
    info!("Then the AP responds with a Probe Response");
    let msg =
        world.recv_frame(|frame, _| frame.stype() == management_subtype::PROBE_RESPONSE).await;

    // Check if it is Probe Response (0x50)
    // 0x50 = Mgmt(00) + Subtype(0101) = 5.
    assert_eq!(msg[0], 0x50);
    // Dest should be Station
    assert_eq!(&msg[4..10], &station_mac.bytes);
    // Source should be AP (BSSID matches default ...01 from given_a_registered_ap)
    assert_eq!(&msg[10..16], &[0x02, 0x00, 0x00, 0x00, 0x00, 0x01]);
}

#[tokio::test]
async fn test_probe_response_ssid_match() {
    info!("Scenario: Active Discovery (Probe Response) - Specific SSID Match");
    let mut world = ApWorld::new().await;
    info!("Given a registered AP 'MatchAP'");
    world.given_a_registered_ap("MatchAP").await;

    let station_mac: MacAddr = "02:00:00:00:00:99".try_into().unwrap();
    let tx = world.tx_to_ap.as_mut().expect("AP registered");

    info!("When a Station sends a Probe Request for 'MatchAP'");
    // FC: Mgmt(00), Probe Req(0100=4) -> 0x40
    let header = MacHeader3Addr::new(
        FrameControl::new(0x0040),
        0,
        MacAddr::BROADCAST, // DA
        station_mac,        // SA
        MacAddr::BROADCAST, // BSSID
        SequenceControl::new(0),
    );

    let mut frame = Vec::new();
    frame.extend_from_slice(header.as_bytes());

    // SSID IE: Tag 0, Len 7, "MatchAP"
    frame.push(0);
    frame.push(7);
    frame.extend_from_slice(b"MatchAP");

    let src_id = netsim_model::ChipId(123);
    tx.send(bytes::Bytes::from(frame)).expect("Send Probe Req");

    info!("Then the AP responds with a Probe Response");
    let msg =
        world.recv_frame(|frame, _| frame.stype() == management_subtype::PROBE_RESPONSE).await;
    assert_eq!(msg[0], 0x50);
}

#[tokio::test]
async fn test_probe_response_ssid_mismatch() {
    info!("Scenario: Active Discovery - SSID Mismatch");
    let mut world = ApWorld::new().await;
    info!("Given a registered AP 'MyAP'");
    world.given_a_registered_ap("MyAP").await;

    let station_mac: MacAddr = "02:00:00:00:00:88".try_into().unwrap();
    let tx = world.tx_to_ap.as_mut().expect("AP registered");

    info!("When a Station sends a Probe Request for 'OtherAP'");
    // FC: Mgmt(00), Probe Req(0100=4) -> 0x40
    let header = MacHeader3Addr::new(
        FrameControl::new(0x0040),
        0,
        MacAddr::BROADCAST, // DA
        station_mac,        // SA
        MacAddr::BROADCAST, // BSSID
        SequenceControl::new(0),
    );

    let mut frame = Vec::new();
    frame.extend_from_slice(header.as_bytes());

    // SSID IE: Tag 0, Len 7, "OtherAP"
    frame.push(0);
    frame.push(7);
    frame.extend_from_slice(b"OtherAP");

    let src_id = netsim_model::ChipId(123);
    tx.send(bytes::Bytes::from(frame)).expect("Send Probe Req");

    info!("Then the AP does NOT respond (ignores request)"); // No helper call
    let rx = world.rx_from_ap.as_mut().expect("AP registered");

    // Logic: We might receive Beacons!
    // We need to filter out Beacons (0x80) and ensure NO Probe Resp (0x50) is
    // received.
    let start = std::time::Instant::now();
    while start.elapsed() < std::time::Duration::from_secs(1) {
        if let Ok(Some(msg)) =
            tokio::time::timeout(std::time::Duration::from_millis(100), rx.recv()).await
        {
            if let Ok(f) = Ieee80211::decode(&msg) {
                if f.stype() == management_subtype::BEACON {
                    continue;
                }
                if f.stype() == management_subtype::PROBE_RESPONSE {
                    panic!("Received Probe Response for mismatched SSID!");
                }
            }
        }
    }
}

#[tokio::test]
async fn test_probe_response_bssid_mismatch() {
    info!("Scenario: Active Discovery - BSSID Mismatch");
    let mut world = ApWorld::new().await;
    info!("Given a registered AP 'SpecificAP'");
    world.given_a_registered_ap("SpecificAP").await;

    let station_mac: MacAddr = "02:00:00:00:00:77".try_into().unwrap();
    let tx = world.tx_to_ap.as_mut().expect("AP registered");

    // Send Probe Req with Correct SSID but Wrong BSSID (Unicast)
    info!("When a Station sends a Probe Request for 'SpecificAP' to wrong BSSID");

    let wrong_bssid: MacAddr = "02:00:00:00:00:99".try_into().unwrap();

    // Probe Req: Type=Management(0), Subtype=ProbeReq(4) -> 0x0040 (LE: 0x40 00)
    let header = MacHeader3Addr::new(
        FrameControl::new(0x0040),
        0,
        wrong_bssid, // DA
        station_mac, // SA
        wrong_bssid, // BSSID
        SequenceControl::new(0),
    );

    let mut frame = Vec::new();
    frame.extend_from_slice(header.as_bytes());

    // SSID IE: Match
    frame.push(0);
    frame.push(10);
    frame.extend_from_slice(b"SpecificAP");

    let src_id = netsim_model::ChipId(123);
    tx.send(bytes::Bytes::from(frame)).expect("Send Probe Req");

    info!("Then the AP does NOT respond");
    let rx = world.rx_from_ap.as_mut().expect("AP registered");

    let start = std::time::Instant::now();
    while start.elapsed() < std::time::Duration::from_secs(1) {
        if let Ok(Some(msg)) =
            tokio::time::timeout(std::time::Duration::from_millis(100), rx.recv()).await
        {
            if let Ok(f) = Ieee80211::decode(&msg) {
                if f.stype() == management_subtype::BEACON {
                    continue;
                }
                if f.stype() == management_subtype::PROBE_RESPONSE {
                    panic!("Received Probe Response for mismatched BSSID!");
                }
            }
        }
    }
}
// Scenario: Create AP with Country Code and Verify TIM
// Given an AP configured with Country Code "US" and DTIM Period 3
// When a beacon is received
// Then the beacon contains a Country IE for "US"
// And the beacon contains a TIM IE with DTIM Count 0 and DTIM Period 3
#[tokio::test]
async fn test_create_ap_with_country_and_tim() {
    info!("Scenario: Create AP with Country and TIM");
    let mut world = ApWorld::new().await;

    let config = ap_actor::ApConfig {
        ssid: "CountryAP".to_string(),
        bssid: "02:00:00:00:01:00".parse().unwrap(),
        channel: 6,
        hw_mode: WifiMode::G,
        wpa_passphrase: None,
        beacon_interval: 100,
        country_code: Some("US".to_string()),
        dtim_period: 3,
        hidden_ssid: false,
        sae: false,
        wmm_enabled: true,
        enterprise_enabled: false,
        mac_acl_mode: 0,
        mac_acl_list: vec![],
        ftm_responder_enabled: true,
        position: netsim_model::Position::default(),
    };

    world.given_a_registered_ap_with_config(config).await;

    // Verify Beacons
    let rx = world.rx_from_ap.as_mut().expect("client registered");
    let msg = tokio::time::timeout(std::time::Duration::from_secs(1), rx.recv())
        .await
        .expect("Timeout")
        .expect("Beacon");

    // Check for Country IE (Tag 7) and TIM IE (Tag 5)
    let mut offset = 36; // Skip Header (24) + Fixed Params (12)
    let mut found_country = false;
    let mut found_tim = false;

    while offset < msg.len() {
        if offset + 1 >= msg.len() {
            break;
        }
        let id = msg[offset];
        let len = msg[offset + 1] as usize;
        if offset + 2 + len > msg.len() {
            break;
        }
        let body = &msg[offset + 2..offset + 2 + len];

        if id == 7 {
            // Country
            found_country = true;
            assert!(body.starts_with(b"US"));
            assert_eq!(body[2], 1); // First Channel
            assert_eq!(body[3], 13); // Num Channels
            assert_eq!(body[4], 20); // Max Power
        } else if id == 5 {
            // TIM
            found_tim = true;
            assert_eq!(body[0], 0); // DTIM Count
            assert_eq!(body[1], 3); // DTIM Period
            assert_eq!(body[2], 0); // Bitmap Ctrl
            assert_eq!(body[3], 0); // Partial Virtual Bitmap
        }

        offset += 2 + len;
    }
    assert!(found_country, "Beacon missing Country IE");
    assert!(found_tim, "Beacon missing TIM IE");
}

#[tokio::test]
async fn test_hidden_ssid() {
    info!("Scenario: Hidden SSID");
    let mut world = ApWorld::new().await;

    let config = ap_actor::ApConfig {
        ssid: "HiddenAP".to_string(),
        bssid: "02:00:00:00:00:99".parse().unwrap(),
        channel: 6,
        hw_mode: WifiMode::G,
        wpa_passphrase: None,
        beacon_interval: 100,
        country_code: None,
        dtim_period: 2,
        hidden_ssid: true,
        sae: false,
        wmm_enabled: true,
        enterprise_enabled: false,
        mac_acl_mode: 0,
        mac_acl_list: vec![],
        ftm_responder_enabled: true,
        position: netsim_model::Position::default(),
    };

    world.given_a_registered_ap_with_config(config).await;

    // 1. Verify Beacon has Empty SSID
    let rx = world.rx_from_ap.as_mut().expect("client registered");
    let msg = tokio::time::timeout(std::time::Duration::from_secs(1), rx.recv())
        .await
        .expect("Timeout")
        .expect("Beacon");

    // SSID IE is Tag 0.
    // Parse to find Tag 0 and check len.
    let mut offset = 36;
    let mut found_ssid = false;
    while offset < msg.len() {
        if offset + 1 >= msg.len() {
            break;
        }
        let id = msg[offset];
        let len = msg[offset + 1] as usize;
        if offset + 2 + len > msg.len() {
            break;
        }

        if id == 0 {
            found_ssid = true;
            assert_eq!(len, 0, "Hidden network beacon should have empty SSID");
        }
        offset += 2 + len;
    }
    assert!(found_ssid, "Beacon missing SSID IE");

    // 2. Wildcard Probe Request -> Should be IGNORED
    let tx = world.tx_to_ap.as_mut().expect("AP registered");
    let station_mac: MacAddr = "02:00:00:00:11:11".try_into().unwrap();

    let header = MacHeader3Addr::new(
        FrameControl::new(0x0040), // Probe Req
        0,
        MacAddr::BROADCAST,
        station_mac,
        MacAddr::BROADCAST,
        SequenceControl::new(0),
    );
    let mut frame = Vec::new();
    frame.extend_from_slice(header.as_bytes());
    // Wildcard SSID (Len 0)
    frame.push(0);
    frame.push(0);

    let src_id = netsim_model::ChipId(123);
    tx.send(bytes::Bytes::from(frame)).expect("Send Wildcard Probe");

    // Drain rx for a moment to ensure NO Probe Resp (0x50)
    let start = std::time::Instant::now();
    while start.elapsed() < std::time::Duration::from_millis(500) {
        if let Ok(Some(msg)) =
            tokio::time::timeout(std::time::Duration::from_millis(100), rx.recv()).await
        {
            if let Ok(f) = Ieee80211::decode(&msg) {
                if f.stype() == management_subtype::BEACON {
                    continue;
                }
                if f.stype() == management_subtype::PROBE_RESPONSE {
                    panic!("Received Probe Response for Wildcard Probe on Hidden Network!");
                }
            }
        }
    }

    // 3. Specific Probe Request -> Should be ANSWERED
    let mut frame2 = Vec::new();
    frame2.extend_from_slice(header.as_bytes());
    // SSID "HiddenAP"
    frame2.push(0);
    frame2.push(8);
    frame2.extend_from_slice(b"HiddenAP");

    tx.send(bytes::Bytes::from(frame2)).expect("Send Specific Probe");

    let resp =
        world.recv_frame(|frame, _| frame.stype() == management_subtype::PROBE_RESPONSE).await;

    assert_eq!(resp[0], 0x50, "Expected Probe Response");
}

#[tokio::test]
async fn test_wmm_ie_presence() {
    info!("Scenario: WMM IE Presence");
    let mut world = ApWorld::new().await;

    let config = ap_actor::ApConfig {
        ssid: "WmmAP".to_string(),
        bssid: "02:00:00:00:00:10".parse().unwrap(),
        channel: 36,
        hw_mode: WifiMode::Ax, // WiFi 6 implies WMM
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

    world.given_a_registered_ap_with_config(config).await;

    // Verify Beacon
    let rx = world.rx_from_ap.as_mut().expect("client registered");
    let msg = tokio::time::timeout(std::time::Duration::from_secs(1), rx.recv())
        .await
        .expect("Timeout")
        .expect("Beacon");

    // Check for WMM Vendor Specific IE (OUI 00:50:f2, Type 2)
    let mut offset = 36;
    let mut found_wmm = false;

    while offset < msg.len() {
        if offset + 1 >= msg.len() {
            break;
        }
        let id = msg[offset];
        let len = msg[offset + 1] as usize;
        if offset + 2 + len > msg.len() {
            break;
        }
        let body = &msg[offset + 2..offset + 2 + len];

        if id == 221 {
            // Vendor Specific
            if body.len() >= 6
                && body[0] == 0x00
                && body[1] == 0x50
                && body[2] == 0xf2
                && body[3] == 2
            {
                found_wmm = true;
                // Verify Subtype and Version
                assert_eq!(body[4], 1, "WMM Subtype should be 1"); // OUI Subtype
                assert_eq!(body[5], 1, "WMM Version should be 1"); // Version
            }
        }
        offset += 2 + len;
    }
    assert!(found_wmm, "Beacon missing WMM IE");
}

#[tokio::test]
async fn test_beacon_and_probe_response_frame_structure_parity() {
    info!("Scenario: Verify structural parity between Beacon and Probe Response frames");
    let mut world = ApWorld::new().await;
    let bssid: MacAddr = "02:00:00:00:00:01".parse().unwrap();
    let station_mac: MacAddr = "02:00:00:00:00:77".parse().unwrap();
    world.given_a_registered_ap_with_wpa("ParityAP", "SecretKey123").await;

    // 1. Receive Beacon frame
    let rx = world.rx_from_ap.as_mut().expect("client registered");
    let beacon_msg = tokio::time::timeout(std::time::Duration::from_secs(1), rx.recv())
        .await
        .expect("Timeout receiving beacon")
        .expect("Beacon channel closed");

    // 2. Send Probe Request to elicit Probe Response
    let tx = world.tx_to_ap.as_mut().expect("AP registered");
    let header = MacHeader3Addr::new(
        FrameControl::new(0x0040), // Mgmt(00), Probe Req(0100=4) -> 0x40
        0,
        MacAddr::BROADCAST,
        station_mac,
        MacAddr::BROADCAST,
        SequenceControl::new(0),
    );
    let mut probe_req = header.as_bytes().to_vec();
    probe_req.push(0); // Tag 0 (SSID)
    probe_req.push(8); // Len
    probe_req.extend_from_slice(b"ParityAP");
    tx.send(bytes::Bytes::from(probe_req)).expect("Send Probe Req");

    let probe_resp_msg =
        world.recv_frame(|frame, _| frame.stype() == management_subtype::PROBE_RESPONSE).await;

    // Verify Beacon Header & Addressing
    let beacon_frame = Ieee80211::decode(&beacon_msg).expect("Decode Beacon");
    assert_eq!(beacon_frame.stype(), management_subtype::BEACON);
    assert_eq!(beacon_frame.get_addr1(), MacAddr::BROADCAST);
    assert_eq!(beacon_frame.get_addr2(), bssid);
    assert_eq!(beacon_frame.get_addr3(), bssid);

    // Verify Probe Response Header & Addressing
    let probe_resp_frame = Ieee80211::decode(&probe_resp_msg).expect("Decode Probe Response");
    assert_eq!(probe_resp_frame.stype(), management_subtype::PROBE_RESPONSE);
    assert_eq!(probe_resp_frame.get_addr1(), station_mac);
    assert_eq!(probe_resp_frame.get_addr2(), bssid);
    assert_eq!(probe_resp_frame.get_addr3(), bssid);

    // Verify Fixed Fields: Capabilities at offset 34..36 match across both frames (ESS | Privacy)
    assert!(beacon_msg.len() >= 36);
    assert!(probe_resp_msg.len() >= 36);
    let beacon_caps = u16::from_le_bytes([beacon_msg[34], beacon_msg[35]]);
    let probe_resp_caps = u16::from_le_bytes([probe_resp_msg[34], probe_resp_msg[35]]);
    assert_eq!(beacon_caps, probe_resp_caps, "Capabilities mismatch");
    assert_eq!(beacon_caps & 0x0010, 0x0010, "Privacy bit must be set for WPA network");
    assert_eq!(beacon_caps & 0x0401, 0x0401, "ESS and Short Slot Time must be set");

    // Verify IEs payload (offset 36 onwards) is identical between Beacon and Probe Response
    assert_eq!(
        &beacon_msg[36..],
        &probe_resp_msg[36..],
        "IEs payload mismatch between Beacon and Probe Response"
    );
}
