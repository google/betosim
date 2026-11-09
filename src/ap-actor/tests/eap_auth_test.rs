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

use netsim_packets::{EAP_CODE_RESPONSE, MacAddr};

use crate::world::ApWorld;

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
    // Given: A registered AP with Enterprise (802.1X) enabled
    let mut world = ApWorld::new().await;
    world.given_an_enterprise_ap("EntAP").await;

    let client_mac: MacAddr = "02:00:00:00:11:11".try_into().unwrap();

    // When: Station sends EAPOL-Start
    world.when_station_sends_eapol_start(client_mac).await;

    // Then: Station receives EAP-Request/Identity
    let req_id = world.then_station_receives_eap_request_identity().await;

    // When: Station responds with EAP-Response/Identity
    world.when_station_sends_eap_response_identity(client_mac, req_id, b"User").await;

    // Then: Station receives EAP-Success with incremented identifier
    world.then_station_receives_eap_success(req_id.wrapping_add(1)).await;
}

// Scenario: EAPOL-Start is ignored when Enterprise (802.1X) is disabled on the AP
// Given a registered AP with enterprise_enabled set to false
// When a Station sends an EAPOL-Start frame
// Then the AP ignores the frame and sends no EAP-Request
#[tokio::test]
async fn test_eap_ignored_when_enterprise_disabled() {
    // Given
    let mut world = ApWorld::new().await;
    let config = ApWorld::base_config("NonEntAP", "02:00:00:00:00:10");
    world.given_a_registered_ap_with_config(config).await;

    let client_mac: MacAddr = "02:00:00:00:11:22".try_into().unwrap();

    // When
    world.when_station_sends_eapol_start(client_mac).await;

    // Then
    world.then_no_eap_or_auth_frame_is_received().await;
}

// Scenario: Unsolicited EAP-Response without a prior EAPOL-Start session is rejected
// Given a registered AP with Enterprise (802.1X) enabled and no active EAP session for the station
// When the Station sends an EAP-Response/Identity without first initiating EAPOL-Start
// Then the AP drops the packet and does not emit EAP-Success
#[tokio::test]
async fn test_eap_response_without_active_session_is_dropped() {
    // Given
    let mut world = ApWorld::new().await;
    world.given_an_enterprise_ap("EntAPNoSession").await;

    let client_mac: MacAddr = "02:00:00:00:11:33".try_into().unwrap();

    // When: Send EAP-Response/Identity without EAPOL-Start
    world.when_station_sends_eap_response_identity(client_mac, 1, b"User").await;

    // Then: No EAP-Success is sent
    world.then_no_eap_or_auth_frame_is_received().await;
}

// Scenario: Unexpected non-Identity EAP-Response type (e.g. EAP-Nak) does not produce EAP-Success
// Given a registered AP with Enterprise (802.1X) enabled and an active EAP-Request/Identity
//   challenge
// When the Station replies with an unexpected EAP type (Type 3 / Legacy Nak) instead of Identity
// Then the AP does not complete authentication or send EAP-Success
#[tokio::test]
async fn test_eap_unexpected_response_type_does_not_succeed() {
    // Given
    let mut world = ApWorld::new().await;
    world.given_an_enterprise_ap("EntAPBadType").await;

    let client_mac: MacAddr = "02:00:00:00:11:44".try_into().unwrap();
    world.when_station_sends_eapol_start(client_mac).await;
    let req_id = world.then_station_receives_eap_request_identity().await;

    // When: Station sends EAP-Response with Type 3 (Legacy Nak) instead of EAP_TYPE_IDENTITY (1)
    world.when_station_sends_eap_packet(client_mac, EAP_CODE_RESPONSE, req_id, &[3, 0]).await;

    // Then: AP does not emit EAP-Success
    world.then_no_eap_or_auth_frame_is_received().await;
}

// Scenario: Malformed/truncated EAPOL-Packet shorter than EapHeader is safely dropped and session
//   remains intact
// Given a registered AP with Enterprise (802.1X) enabled and an active EAP session
// When the Station transmits a truncated EAPOL-Packet body (2 bytes, shorter than EapHeader)
//   followed by a valid EAP-Response/Identity
// Then the AP drops the malformed packet without panicking and completes EAP-Success on the
//   subsequent valid response
#[tokio::test]
async fn test_eap_truncated_packet_is_dropped() {
    // Given
    let mut world = ApWorld::new().await;
    world.given_an_enterprise_ap("EntAPTruncated").await;

    let client_mac: MacAddr = "02:00:00:00:11:55".try_into().unwrap();
    world.when_station_sends_eapol_start(client_mac).await;
    let req_id = world.then_station_receives_eap_request_identity().await;

    // When: Station sends a truncated 2-byte EAPOL-Packet payload
    world.when_station_sends_raw_eapol_packet(client_mac, &[EAP_CODE_RESPONSE, 1]).await;

    // Then: No response frame is emitted for the truncated packet
    world.then_no_eap_or_auth_frame_is_received().await;

    // And: The active EAP session remains intact for a subsequent valid Identity response
    world.when_station_sends_eap_response_identity(client_mac, req_id, b"User").await;
    world.then_station_receives_eap_success(req_id.wrapping_add(1)).await;
}
