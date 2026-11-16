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

use netsim_model::{ChipId, DeviceId};
use nfc_actor::err::Code;

use crate::world::NfcClientWorld;

const ORIGIN_METERS: f32 = 0.0;
const TRUNCATED_DATA_PACKET: [u8; 2] = [0x00, 0x00];
const UNDEFINED_GID_COMMAND_PACKET: [u8; 3] = [0x25, 0x00, 0x00];

// ============================================================================
// Feature: nfc-actor coded error statistics
// ============================================================================

// Scenario: Baseline NCI core reset and init records no error stats
// Given an initialized NFC chip with error stats tracking enabled
// When NCI core reset and core init complete normally
// Then no error stats are recorded
#[tokio::test]
async fn test_baseline_core_reset_records_no_error_stats() {
    // Given: An NFC chip with error stats tracking enabled
    let mut world = NfcClientWorld::with_error_stats().await;
    world.given_chip_at(ChipId(1), DeviceId(1), ORIGIN_METERS, ORIGIN_METERS, ORIGIN_METERS).await;

    // When: NCI core reset and core init complete normally
    world.when_nci_core_reset_completed(ChipId(1)).await;
    world.when_nci_core_init_completed(ChipId(1)).await;

    // Then: No error stats are recorded
    world.then_error_stats_are(&[]).await;
    world.shutdown().await;
}

// Scenario: Malformed data packet records DataPacketDecodeFailed
// Given an active NFC chip with error stats tracking enabled
// When a truncated NCI data packet shorter than 3 bytes is sent
// Then error stats record one DataPacketDecodeFailed event
#[tokio::test]
async fn test_malformed_data_packet_records_error_stat() {
    // Given: An active NFC chip with error stats tracking enabled
    let mut world = NfcClientWorld::with_error_stats().await;
    world.given_chip_at(ChipId(1), DeviceId(1), ORIGIN_METERS, ORIGIN_METERS, ORIGIN_METERS).await;

    // When: A truncated NCI data packet shorter than 3 bytes is sent
    world.when_raw_nci_packet_sent(ChipId(1), &TRUNCATED_DATA_PACKET).await;

    // Then: Error stats record one DataPacketDecodeFailed event
    world.then_error_stats_are(&[(Code::DataPacketDecodeFailed as u32, 1)]).await;
    world.shutdown().await;
}

// Scenario: Malformed control packet records ControlPacketDecodeFailed
// Given an active NFC chip with error stats tracking enabled
// When an NCI command packet with an undefined GID is sent
// Then error stats record one ControlPacketDecodeFailed event
#[tokio::test]
async fn test_malformed_control_packet_records_error_stat() {
    // Given: An active NFC chip with error stats tracking enabled
    let mut world = NfcClientWorld::with_error_stats().await;
    world.given_chip_at(ChipId(1), DeviceId(1), ORIGIN_METERS, ORIGIN_METERS, ORIGIN_METERS).await;

    // When: An NCI command packet with an undefined GID is sent
    world.when_raw_nci_packet_sent(ChipId(1), &UNDEFINED_GID_COMMAND_PACKET).await;

    // Then: Error stats record one ControlPacketDecodeFailed event
    world.then_error_stats_are(&[(Code::ControlPacketDecodeFailed as u32, 1)]).await;
    world.shutdown().await;
}

// Scenario: Closed guest packet sink records GuestPacketSendFailed
// Given an active NFC chip backed by a closable guest sink
// When the guest packet receiver is closed and CoreInit is sent
// Then error stats record one GuestPacketSendFailed event
#[tokio::test]
async fn test_closed_guest_sink_records_error_stat() {
    // Given: An active NFC chip backed by a closable guest sink
    let mut world = NfcClientWorld::with_error_stats().await;
    world.given_chip_with_closable_sink(ChipId(1), DeviceId(1)).await;

    // When: The guest packet receiver is closed and CoreInit is sent
    world.when_guest_packet_receiver_closed(ChipId(1));
    world.when_nci_core_init_sent(ChipId(1)).await;

    // Then: Error stats record one GuestPacketSendFailed event
    world.then_error_stats_are(&[(Code::GuestPacketSendFailed as u32, 1)]).await;
    world.shutdown().await;
}

// Scenario: Exceeding max Casimir devices records AddRfDeviceFailed
// Given all Casimir scene device slots are occupied
// When creating an additional RF control channel fails
// Then error stats record one AddRfDeviceFailed event
#[tokio::test]
async fn test_exceeding_max_casimir_devices_records_error_stat() {
    // Given: All Casimir scene device slots are occupied
    let mut world = NfcClientWorld::with_error_stats().await;
    world.given_max_casimir_devices_occupied().await;

    // When: Creating an additional RF control channel fails
    world.when_control_channel_create_fails().await;

    // Then: Error stats record one AddRfDeviceFailed event
    world.then_control_channel_create_failed();
    world.then_error_stats_are(&[(Code::AddRfDeviceFailed as u32, 1)]).await;
    world.shutdown().await;
}
