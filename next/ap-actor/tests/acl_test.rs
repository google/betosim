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
    ieee80211::{AuthenticationFixedFields, Ieee80211},
};
use zerocopy::FromBytes;

mod world;
use ap_actor::netsim_model::WifiMode;
use world::ApWorld;

const DENY_MODE: u8 = 1;
const ALLOW_MODE: u8 = 2;

#[tokio::test]
async fn test_acl_deny_mode() {
    let mut world = ApWorld::new().await;

    // Client to be denied
    let denied_mac: MacAddr = "02:00:00:00:11:11".parse().unwrap();

    let config = ap_actor::ApConfig {
        ssid: "DenyAP".to_string(),
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
        enterprise_enabled: false,
        mac_acl_mode: DENY_MODE,
        mac_acl_list: vec![denied_mac],
        ftm_responder_enabled: true,
        position: ap_actor::Position::default(),
    };

    world.given_a_registered_ap_with_config(config).await;

    // Send Auth
    world.send_auth(denied_mac).await;

    // Expect Auth Response with Failure (Status 1)
    let msg = world
        .recv_frame(|frame, _| {
            if frame.stype() == netsim_packets::management_subtype::BEACON {
                return false;
            }
            frame.stype() == netsim_packets::management_subtype::AUTHENTICATION
        })
        .await;

    let frame = Ieee80211::decode(&msg).expect("Decode");
    // Verify Status is Not Success (0)
    // 24 Header + 6 Fixed
    let payload = &msg[24..];
    let fixed = AuthenticationFixedFields::read_from_prefix(payload).unwrap();
    assert_ne!(fixed.status.get(), 0, "Should be rejected");
    assert_eq!(fixed.status.get(), 1, "Status 1 expected");
}

#[tokio::test]
async fn test_acl_allow_mode_reject() {
    let mut world = ApWorld::new().await;

    // Client NOT in list
    let unknown_mac: MacAddr = "02:00:00:00:22:22".parse().unwrap();
    let allowed_mac: MacAddr = "02:00:00:00:33:33".parse().unwrap();

    let config = ap_actor::ApConfig {
        ssid: "AllowAP".to_string(),
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
        enterprise_enabled: false,
        mac_acl_mode: ALLOW_MODE,
        mac_acl_list: vec![allowed_mac],
        ftm_responder_enabled: true,
        position: ap_actor::Position::default(),
    };

    world.given_a_registered_ap_with_config(config).await;

    // Send Auth
    world.send_auth(unknown_mac).await;

    // Expect Auth Response with Failure
    let msg = world
        .recv_frame(|frame, _| {
            if frame.stype() == netsim_packets::management_subtype::BEACON {
                return false;
            }
            frame.stype() == netsim_packets::management_subtype::AUTHENTICATION
        })
        .await;

    let frame = Ieee80211::decode(&msg).expect("Decode");
    let payload = &msg[24..];
    let fixed = AuthenticationFixedFields::read_from_prefix(payload).unwrap();
    assert_ne!(fixed.status.get(), 0, "Should be rejected");
}

#[tokio::test]
async fn test_acl_allow_mode_accept() {
    let mut world = ApWorld::new().await;

    // Client IN list
    let allowed_mac: MacAddr = "02:00:00:00:33:33".parse().unwrap();

    let config = ap_actor::ApConfig {
        ssid: "AllowAP2".to_string(),
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
        enterprise_enabled: false,
        mac_acl_mode: ALLOW_MODE,
        mac_acl_list: vec![allowed_mac],
        ftm_responder_enabled: true,
        position: ap_actor::Position::default(),
    };

    world.given_a_registered_ap_with_config(config).await;

    // Send Auth
    world.send_auth(allowed_mac).await;

    // Expect Auth Response with Failure
    let msg = world
        .recv_frame(|frame, _| {
            if frame.stype() == netsim_packets::management_subtype::BEACON {
                return false;
            }
            frame.stype() == netsim_packets::management_subtype::AUTHENTICATION
        })
        .await;

    let payload = &msg[24..];
    let fixed = AuthenticationFixedFields::read_from_prefix(payload).unwrap();
    assert_eq!(fixed.status.get(), 0, "Should be accepted");
}
