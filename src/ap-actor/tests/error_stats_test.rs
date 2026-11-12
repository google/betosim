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

use ap_actor::err::Code;

use crate::world::ApWorld;

const UNKNOWN_AP_ID: u32 = 9999;
const STATION_MAC: &str = "02:00:00:00:00:42";
const SHORT_RAW_FRAME: [u8; 2] = [0x01, 0x02];

// ============================================================================
// Feature: Coded AP error stats
// ============================================================================

// Scenario: Invalid MAC in force_disconnect records ForceDisconnectInvalidMac
// Given a registered AP with error stats tracking enabled
// When force_disconnect is requested for a malformed MAC string
// Then error stats record one ForceDisconnectInvalidMac event
#[tokio::test]
async fn test_force_disconnect_invalid_mac_records_error_stat() {
    // Given: A registered AP with error stats tracking enabled
    let mut world = ApWorld::with_error_stats().await;
    world.given_a_registered_ap("ErrStatsMacAP").await;

    // When: force_disconnect is requested for a malformed MAC string
    world.when_force_disconnect_is_requested_for("not-a-mac").await;

    // Then: Error stats record one ForceDisconnectInvalidMac event
    world.then_error_stats_are(&[(Code::ForceDisconnectInvalidMac as u32, 1)]).await;
}

// Scenario: Force disconnecting an unknown MAC records
//   ForceDisconnectUnknownMac
// Given a registered AP with no associated stations
// When force_disconnect is requested for an unassociated station MAC
// Then error stats record one ForceDisconnectUnknownMac event
#[tokio::test]
async fn test_force_disconnect_unknown_mac_records_error_stat() {
    // Given: A registered AP with error stats tracking and no stations
    let mut world = ApWorld::with_error_stats().await;
    world.given_a_registered_ap("ErrStatsForceUnknownAP").await;

    // When: force_disconnect is requested for an unassociated station MAC
    world.when_force_disconnect_is_requested_for(STATION_MAC).await;

    // Then: Error stats record one ForceDisconnectUnknownMac event
    world.then_error_stats_are(&[(Code::ForceDisconnectUnknownMac as u32, 1)]).await;
}

// Scenario: Deleting a non-existent AP ID records DeleteApNotFound
// Given a registered AP with error stats tracking enabled
// When deletion is requested for an unknown AP ID
// Then error stats record one DeleteApNotFound event
#[tokio::test]
async fn test_delete_non_existent_ap_records_error_stat() {
    // Given: A registered AP with error stats tracking enabled
    let mut world = ApWorld::with_error_stats().await;
    world.given_a_registered_ap("ErrStatsDeleteAP").await;

    // When: Deletion is requested for an unknown AP ID
    world.when_non_existent_ap_is_deleted(UNKNOWN_AP_ID).await;

    // Then: Error stats record one DeleteApNotFound event
    world.then_error_stats_are(&[(Code::DeleteApNotFound as u32, 1)]).await;
}

// Scenario: Disconnecting an unassociated MAC records DisconnectUnknownMac
// Given a registered AP with no associated stations
// When disconnect is requested for an unassociated station MAC
// Then error stats record one DisconnectUnknownMac event
#[tokio::test]
async fn test_disconnect_unknown_mac_records_error_stat() {
    // Given: A registered AP with error stats tracking and no stations
    let mut world = ApWorld::with_error_stats().await;
    world.given_a_registered_ap("ErrStatsDisconnectUnknownAP").await;

    // When: Disconnect is requested for an unassociated station MAC
    world.when_disconnect_is_requested_for(STATION_MAC).await;

    // Then: Error stats record one DisconnectUnknownMac event
    world.then_error_stats_are(&[(Code::DisconnectUnknownMac as u32, 1)]).await;
}

// Scenario: Deauth send over a closed output sink records DeauthSendFailed
// Given a registered AP with an associated station and beacons disabled
// When the output sink is closed and station disconnect is requested
// Then error stats record one DeauthSendFailed event
#[tokio::test]
async fn test_deauth_send_failed_records_error_stat() {
    // Given: A registered AP with an associated station and beacons disabled
    let mut world = ApWorld::with_error_stats().await;
    world.given_a_registered_ap("ErrStatsDeauthSinkAP").await;
    world.given_station_is_associated(STATION_MAC).await;
    world.given_ap_beacons_disabled().await;

    // When: The output sink is closed and station disconnect is requested
    world.when_sink_closed().await;
    world.when_disconnect_is_requested_for(STATION_MAC).await;

    // Then: Error stats record one DeauthSendFailed event
    world.then_error_stats_are(&[(Code::DeauthSendFailed as u32, 1)]).await;
}

// Scenario: A truncated Authentication frame records AuthFrameTooShort
// Given a registered AP with error stats tracking enabled
// When a station sends an Authentication frame truncated after Addr2
// Then error stats record one AuthFrameTooShort event
#[tokio::test]
async fn test_truncated_auth_frame_records_error_stat() {
    // Given: A registered AP with error stats tracking enabled
    let mut world = ApWorld::with_error_stats().await;
    world.given_a_registered_ap("ErrStatsAuthAP").await;

    // When: A station sends an Authentication frame truncated after Addr2
    world.when_station_sends_auth_frame_truncated_after_addr2(STATION_MAC).await;

    // Then: Error stats record one AuthFrameTooShort event
    world.then_error_stats_are(&[(Code::AuthFrameTooShort as u32, 1)]).await;
}

// Scenario: Truncated Auth fixed fields record AuthFixedFieldsInvalid
// Given a registered AP with error stats tracking enabled
// When a station sends an Auth frame with truncated fixed fields
// Then error stats record one AuthFixedFieldsInvalid event
#[tokio::test]
async fn test_auth_fixed_fields_invalid_records_error_stat() {
    // Given: A registered AP with error stats tracking enabled
    let mut world = ApWorld::with_error_stats().await;
    world.given_a_registered_ap("ErrStatsAuthFixedAP").await;

    // When: A station sends an Auth frame with truncated fixed fields
    world.when_station_sends_auth_frame_with_truncated_fixed_fields(STATION_MAC).await;

    // Then: Error stats record one AuthFixedFieldsInvalid event
    world.then_error_stats_are(&[(Code::AuthFixedFieldsInvalid as u32, 1)]).await;
}

// Scenario: SAE Auth when SAE is disabled records AuthSaeNotEnabled
// Given a registered AP without SAE enabled
// When a station sends an SAE Authentication Commit frame
// Then error stats record one AuthSaeNotEnabled event
#[tokio::test]
async fn test_auth_sae_not_enabled_records_error_stat() {
    // Given: A registered AP without SAE enabled and error stats tracking
    let mut world = ApWorld::with_error_stats().await;
    world.given_a_registered_ap("ErrStatsSaeDisabledAP").await;

    // When: A station sends an SAE Authentication Commit frame
    world
        .when_station_sends_sae_auth(STATION_MAC.parse().expect("valid station MAC"), 1, 0, &[])
        .await;

    // Then: Error stats record one AuthSaeNotEnabled event
    world.then_error_stats_are(&[(Code::AuthSaeNotEnabled as u32, 1)]).await;
}

// Scenario: EAPOL-Start with enterprise disabled records
//   EapolStartEnterpriseDisabled
// Given a registered AP with WPA-Enterprise disabled
// When a station sends an EAPOL-Start frame
// Then error stats record one EapolStartEnterpriseDisabled event
#[tokio::test]
async fn test_eapol_start_enterprise_disabled_records_error_stat() {
    // Given: A registered AP with WPA-Enterprise disabled
    let mut world = ApWorld::with_error_stats().await;
    world.given_a_registered_ap("ErrStatsEnterpriseDisabledAP").await;

    // When: A station sends an EAPOL-Start frame
    world.when_station_sends_eapol_start(STATION_MAC.parse().expect("valid station MAC")).await;

    // Then: Error stats record one EapolStartEnterpriseDisabled event
    world.then_error_stats_are(&[(Code::EapolStartEnterpriseDisabled as u32, 1)]).await;
}

// Scenario: Unparseable stream packet records StreamFrameParseFailed
// Given a registered AP with error stats tracking enabled
// When malformed raw packet bytes are sent on the input stream
// Then error stats record one StreamFrameParseFailed event
#[tokio::test]
async fn test_stream_frame_parse_failed_records_error_stat() {
    // Given: A registered AP with error stats tracking enabled
    let mut world = ApWorld::with_error_stats().await;
    world.given_a_registered_ap("ErrStatsStreamParseAP").await;

    // When: Malformed raw packet bytes are sent on the input stream
    world.when_malformed_raw_bytes_sent(&SHORT_RAW_FRAME).await;

    // Then: Error stats record one StreamFrameParseFailed event
    world.then_error_stats_are(&[(Code::StreamFrameParseFailed as u32, 1)]).await;
}

// Scenario: Truncated EAPOL-Key on WPA2 AP records WpaHandleEapolFailed
// Given a registered WPA2 AP with an associated station
// When the station sends a truncated EAPOL-Key frame
// Then error stats record one WpaHandleEapolFailed event
#[tokio::test]
async fn test_wpa_handle_eapol_failed_records_error_stat() {
    // Given: A registered WPA2 AP with an associated station
    let mut world = ApWorld::with_error_stats().await;
    world.given_a_registered_ap_with_wpa("ErrStatsWpaEapolAP", "secretpassword").await;
    world.given_station_is_associated(STATION_MAC).await;

    // When: The station sends a truncated EAPOL-Key frame
    world
        .when_station_sends_truncated_eapol_key(STATION_MAC.parse().expect("valid station MAC"))
        .await;

    // Then: Error stats record one WpaHandleEapolFailed event
    world.then_error_stats_are(&[(Code::WpaHandleEapolFailed as u32, 1)]).await;
}

// Scenario: Closed output sink on beacon tick records BeaconSinkClosed
// Given a registered AP actively broadcasting beacons
// When the output sink is closed before the next beacon tick
// Then the actor shuts down and records one BeaconSinkClosed event
#[tokio::test]
async fn test_beacon_sink_closed_records_error_stat() {
    // Given: A registered AP actively broadcasting beacons
    let mut world = ApWorld::with_error_stats().await;
    world.given_a_registered_ap("ErrStatsBeaconSinkAP").await;

    // When: The output sink is closed before the next beacon tick
    world.when_sink_closed().await;

    // Then: The actor shuts down and records one BeaconSinkClosed event
    world.then_actor_shuts_down().await;
    world.then_error_stats_are(&[(Code::BeaconSinkClosed as u32, 1)]).await;
}

// Scenario: Unicast Auth over closed sink records UnicastResponseSinkClosed
// Given a registered AP with periodic beacons disabled
// When the output sink is closed and a station sends an Open Auth request
// Then the actor shuts down and records one UnicastResponseSinkClosed event
#[tokio::test]
async fn test_unicast_response_sink_closed_records_error_stat() {
    // Given: A registered AP with periodic beacons disabled
    let mut world = ApWorld::with_error_stats().await;
    world.given_a_registered_ap("ErrStatsUnicastSinkAP").await;
    world.given_ap_beacons_disabled().await;

    // When: The output sink is closed and a station sends an Open Auth request
    world.when_sink_closed().await;
    world.when_station_sends_open_auth(STATION_MAC.parse().expect("valid station MAC")).await;

    // Then: The actor shuts down and records one UnicastResponseSinkClosed
    world.then_actor_shuts_down().await;
    world.then_error_stats_are(&[(Code::UnicastResponseSinkClosed as u32, 1)]).await;
}

// Scenario: Closed sink on delayed M1 records DelayedFrameSinkClosed
// Given a WPA2 AP with beacons disabled and an associated station queuing M1
// When the output sink is closed and the AP processes the ready delayed M1
// Then the actor shuts down and records one DelayedFrameSinkClosed event
#[tokio::test]
async fn test_delayed_frame_sink_closed_records_error_stat() {
    // Given: A WPA2 AP with beacons disabled and an associated station
    let mut world = ApWorld::with_error_stats().await;
    world.given_a_registered_ap_with_wpa("TestWPA", "password123").await;
    world.given_ap_beacons_disabled().await;
    world.given_station_is_associated(STATION_MAC).await;

    // When: The output sink is closed and the delayed M1 frame fires
    world.when_sink_closed().await;
    world.when_ap_enabled_after_delayed_frame_ready().await;

    // Then: The actor shuts down and records one DelayedFrameSinkClosed event
    world.then_actor_shuts_down().await;
    world.then_error_stats_are(&[(Code::DelayedFrameSinkClosed as u32, 1)]).await;
}
