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

use netsim_packets::{AuthenticationFixedFields, Ieee80211, MacAddr};
use zerocopy::FromBytes;

use crate::world::ApWorld;

const DENY_MODE: u8 = 1;
const ALLOW_MODE: u8 = 2;

// ============================================================================
// Feature: MAC Address Access Control List (ACL) Filtering
// ============================================================================

// Scenario: Deny mode rejects authentication from a station listed in mac_acl_list
// Given a registered AP configured with DENY_MODE (1) and a denied station MAC in mac_acl_list
// When the denied station sends an Open System Authentication request
// Then the AP replies with an Authentication response with non-zero failure status (1)
#[tokio::test]
async fn test_acl_deny_mode() {
    // Given
    let mut world = ApWorld::new().await;
    let denied_mac: MacAddr = "02:00:00:00:11:11".parse().unwrap();
    world.given_an_ap_with_acl("DenyAP", DENY_MODE, vec![denied_mac]).await;

    // When: Send Auth
    world.when_station_sends_open_auth(denied_mac).await;

    // Then: Expect Auth Response with Failure (Status 1)
    let msg = world
        .recv_frame(|frame, _| {
            if frame.stype() == netsim_packets::management_subtype::BEACON {
                return false;
            }
            frame.stype() == netsim_packets::management_subtype::AUTHENTICATION
        })
        .await;

    let _frame = Ieee80211::decode(&msg).expect("Decode");
    let payload = &msg[24..];
    let (fixed, _) = AuthenticationFixedFields::read_from_prefix(payload).unwrap();
    assert_ne!(fixed.status.get(), 0, "Should be rejected");
    assert_eq!(fixed.status.get(), 1, "Status 1 expected");
}

// Scenario: Allow mode rejects authentication from a station absent from mac_acl_list
// Given a registered AP configured with ALLOW_MODE (2) and an allowlist excluding the station MAC
// When the unlisted station sends an Open System Authentication request
// Then the AP replies with an Authentication response with a non-zero failure status
#[tokio::test]
async fn test_acl_allow_mode_reject() {
    // Given
    let mut world = ApWorld::new().await;
    let unknown_mac: MacAddr = "02:00:00:00:22:22".parse().unwrap();
    let allowed_mac: MacAddr = "02:00:00:00:33:33".parse().unwrap();
    world.given_an_ap_with_acl("AllowAP", ALLOW_MODE, vec![allowed_mac]).await;

    // When: Send Auth
    world.when_station_sends_open_auth(unknown_mac).await;

    // Then: Expect Auth Response with Failure
    let msg = world
        .recv_frame(|frame, _| {
            if frame.stype() == netsim_packets::management_subtype::BEACON {
                return false;
            }
            frame.stype() == netsim_packets::management_subtype::AUTHENTICATION
        })
        .await;

    let _frame = Ieee80211::decode(&msg).expect("Decode");
    let payload = &msg[24..];
    let (fixed, _) = AuthenticationFixedFields::read_from_prefix(payload).unwrap();
    assert_ne!(fixed.status.get(), 0, "Should be rejected");
}

// Scenario: Allow mode accepts authentication from a station present in mac_acl_list
// Given a registered AP configured with ALLOW_MODE (2) and an allowlist containing the station MAC
// When the allowlisted station sends an Open System Authentication request
// Then the AP replies with an Authentication response with status 0 (Success)
#[tokio::test]
async fn test_acl_allow_mode_accept() {
    // Given
    let mut world = ApWorld::new().await;
    let allowed_mac: MacAddr = "02:00:00:00:33:33".parse().unwrap();
    world.given_an_ap_with_acl("AllowAP2", ALLOW_MODE, vec![allowed_mac]).await;

    // When: Send Auth
    world.when_station_sends_open_auth(allowed_mac).await;

    // Then: Expect Auth Response with Success (Status 0)
    let msg = world
        .recv_frame(|frame, _| {
            if frame.stype() == netsim_packets::management_subtype::BEACON {
                return false;
            }
            frame.stype() == netsim_packets::management_subtype::AUTHENTICATION
        })
        .await;

    let payload = &msg[24..];
    let (fixed, _) = AuthenticationFixedFields::read_from_prefix(payload).unwrap();
    assert_eq!(fixed.status.get(), 0, "Should be accepted");
}
