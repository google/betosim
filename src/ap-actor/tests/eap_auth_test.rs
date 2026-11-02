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

use netsim_packets::{
    ethernet::MacAddr,
    ieee80211::{
        FrameControl, Ieee80211, MacHeader3Addr, SequenceControl,
        eapol::{
            EAP_CODE_REQUEST, EAP_CODE_RESPONSE, EAP_CODE_SUCCESS, EAP_TYPE_IDENTITY,
            EAPOL_TYPE_PACKET, EAPOL_TYPE_START, EapHeader, EapolHeader,
        },
    },
    llc::{LlcSnapHeader, control_field, sap},
};
use zerocopy::{IntoBytes, U16};

mod world;
use ap_actor::netsim_model::WifiMode;
use world::ApWorld;

fn build_eapol_frame(
    dest: MacAddr,
    bssid: MacAddr,
    version: u8,
    packet_type: u8,
    body: &[u8],
) -> Vec<u8> {
    let mut frame = Vec::new();
    // 802.11 Header (Data)
    let header = MacHeader3Addr::new(
        FrameControl::new(0x0008), // Data
        0,
        bssid, // DA (AP)
        dest,  // SA (Client)
        bssid, // BSSID
        SequenceControl::new(0),
    );
    frame.extend_from_slice(header.as_bytes());

    // LLC
    let llc = LlcSnapHeader::new(
        sap::SNAP,
        sap::SNAP,
        control_field::UI,
        [0x00, 0x00, 0x00],
        netsim_packets::ether_type::EAPOL,
    );
    frame.extend_from_slice(llc.as_bytes());

    // EAPOL Header
    let eapol = EapolHeader::new(version, packet_type, body.len() as u16);
    frame.extend_from_slice(eapol.as_bytes());
    frame.extend_from_slice(body);

    frame
}

// ============================================================================
// Feature: WPA2-Enterprise (802.1X) Authentication
// ============================================================================

// Scenario: EAP Mock Authentication Success
// Given a registered AP with Enterprise (802.1X) enabled
// When a Station initiates EAPOL (Start)
// And responds to Identity Request
// Then the AP sends EAP-Success
#[tokio::test]
async fn test_eap_mock_authentication_success() {
    let mut world = ApWorld::new().await;

    // Config with Enterprise Enabled
    let config = ap_actor::ApConfig {
        ssid: "EntAP".to_string(),
        bssid: "02:00:00:00:00:10".parse().unwrap(),
        channel: 36,
        hw_mode: WifiMode::Ax,
        wpa_passphrase: None,
        beacon_interval: 100,
        country_code: None,
        dtim_period: 2,
        hidden_ssid: false,
        sae: false,
        wmm_enabled: true,
        enterprise_enabled: true,
        mac_acl_mode: 0,
        mac_acl_list: vec![],
        ftm_responder_enabled: true,
        position: ap_actor::Position::default(),
    };

    world.given_a_registered_ap_with_config(config).await;

    let client_mac: MacAddr = "02:00:00:00:11:11".try_into().unwrap();
    let bssid: MacAddr = "02:00:00:00:00:10".try_into().unwrap();

    let tx = world.tx_to_ap.as_mut().expect("Registered");
    let rx = world.rx_from_ap.as_mut().expect("Registered");

    // 1. Send EAPOL-Start
    let start_frame = build_eapol_frame(client_mac, bssid, 1, EAPOL_TYPE_START, &[]);
    tx.send(bytes::Bytes::from(start_frame)).expect("Send Start");

    // 2. Expect EAP-Request/Identity
    let msg = world
        .recv_frame(|frame, msg| {
            // EAPOL Packet (0x888E)
            if frame.stype() == netsim_packets::management_subtype::BEACON {
                return false;
            }
            msg.len() > 32 && msg[30] == 0x88 && msg[31] == 0x8E
        })
        .await;

    // Decode
    let frame = Ieee80211::decode(&msg).expect("Decode 802.11");
    // Verify it's EAPOL
    // Skip 32 bytes (24 header + 8 LLC)
    // Payload: EAPOL Header(4) + EAP Header(4) + Type(1)
    let payload = &msg[32..];
    let eapol_header = zerocopy::Ref::<&[u8], EapolHeader>::from_prefix(payload).unwrap().0;
    assert_eq!(eapol_header.packet_type, EAPOL_TYPE_PACKET);

    let eap_packet = &payload[4..];
    let eap_header = zerocopy::Ref::<&[u8], EapHeader>::from_prefix(eap_packet).unwrap().0;
    assert_eq!(eap_header.code, EAP_CODE_REQUEST);

    let eap_body = &eap_packet[4..];
    assert_eq!(eap_body[0], EAP_TYPE_IDENTITY);

    let id = eap_header.id;
    // 3. Send EAP-Response/Identity
    let mut response_body = Vec::new();
    let resp_header = EapHeader::new(EAP_CODE_RESPONSE, id, 5 + 4); // Type(1) + "User"
    response_body.extend_from_slice(resp_header.as_bytes());
    response_body.push(EAP_TYPE_IDENTITY);
    response_body.extend_from_slice(b"User");

    let resp_frame = build_eapol_frame(client_mac, bssid, 1, EAPOL_TYPE_PACKET, &response_body);
    tx.send(bytes::Bytes::from(resp_frame)).expect("Send Response");

    // 4. Expect EAP-Success
    let msg2 = world
        .recv_frame(|frame, msg| {
            // EAPOL Packet (0x888E)
            if frame.stype() == netsim_packets::management_subtype::BEACON {
                return false;
            }
            msg.len() > 32 && msg[30] == 0x88 && msg[31] == 0x8E
        })
        .await;

    let payload2 = &msg2[32..];
    let eap_packet2 = &payload2[4..];
    let eap_header2 = zerocopy::Ref::<&[u8], EapHeader>::from_prefix(eap_packet2).unwrap().0;

    assert_eq!(eap_header2.code, EAP_CODE_SUCCESS);
    assert_eq!(eap_header2.id, id.wrapping_add(1));
}
