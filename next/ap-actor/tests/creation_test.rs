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

use netsim_model::WifiMode;

use crate::world::ApWorld;

// ============================================================================
// Feature: AP Creation and Configuration
//
// As a user, I want to create Access Points with various security and radio
// configurations so that I can simulate different network environments.
// ============================================================================

// Scenario: Create Open AP
// Given a running Actor
// When I create an Open AP with SSID "OpenNet"
// Then the AP is listed with correct configuration
#[tokio::test]
async fn test_create_open_ap() {
    let mut world = ApWorld::new().await;

    // When
    world.given_a_registered_ap("OpenNet").await;
    let id = world.ap_id.expect("AP ID missing");

    // Then
    let ap_state = world.client.get_ap(id).await.expect("Get failed").expect("AP not found");
    assert_eq!(ap_state.config.ssid, "OpenNet");
    assert!(ap_state.config.wpa_passphrase.is_none());
    assert!(!ap_state.config.hidden_ssid);
}

// Scenario: Create WPA2 AP
// Given a running Actor
// When I create a WPA2 AP with SSID "SecureNet"
// Then the AP is listed and has WPA enabled
#[tokio::test]
async fn test_create_wpa2_ap() {
    let mut world = ApWorld::new().await;

    // When
    world.given_a_registered_ap_with_wpa("SecureNet", "password123").await;
    let id = world.ap_id.expect("AP ID missing");

    // Then
    let ap_state = world.client.get_ap(id).await.expect("Get failed").expect("AP not found");
    assert_eq!(ap_state.config.ssid, "SecureNet");
    assert_eq!(ap_state.config.wpa_passphrase.as_deref(), Some("password123"));
}

// Scenario: Create Hidden SSID AP
// Given a running Actor
// When I create a Hidden AP
// Then the AP is listed with `hidden_ssid` set to true
#[tokio::test]
async fn test_create_hidden_ap() {
    let mut world = ApWorld::new().await;

    // When
    let config = ap_actor::ApConfig {
        ssid: "HiddenNet".to_string(),
        bssid: "02:00:00:00:00:99".try_into().unwrap(),
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
    let id = world.ap_id.expect("AP ID missing");

    // Then
    let ap_state = world.client.get_ap(id).await.expect("Get failed").expect("AP not found");
    assert!(ap_state.config.hidden_ssid);
}

// Scenario: Create 6GHz (HE) AP
// Given a running Actor
// When I create a WiFi 6 (ax) AP on channel 36
// Then the AP is listed with `hw_mode` "ax"
#[tokio::test]
async fn test_create_wifi6_ap() {
    let mut world = ApWorld::new().await;

    // When
    world.given_a_wifi6_ap("WiFi6Net").await;
    let id = world.ap_id.expect("AP ID missing");

    // Then
    let ap_state = world.client.get_ap(id).await.expect("Get failed").expect("AP not found");
    assert_eq!(ap_state.config.hw_mode, WifiMode::Ax);
    assert_eq!(ap_state.config.channel, 36);
}

// Scenario: Create Default AP
// Given a running Actor
// When I create an AP using the default configuration
// Then the AP should have a valid, non-zero BSSID
#[tokio::test]
async fn test_create_default_ap_has_valid_bssid() {
    let mut world = ApWorld::new().await;

    // When
    let config = ap_actor::ApConfig::default();
    world.given_a_registered_ap_with_config(config).await;
    let id = world.ap_id.expect("AP ID missing");

    // Then
    let ap_state = world.client.get_ap(id).await.expect("Get failed").expect("AP not found");
    let zero_bssid = netsim_packets::MacAddr::new([0; 6]);
    assert_ne!(
        ap_state.config.bssid, zero_bssid,
        "AP created with default config must not have an all-zero BSSID"
    );
}
