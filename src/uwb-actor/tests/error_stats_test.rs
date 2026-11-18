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

use std::io::ErrorKind;

use netsim_model::ChipId;
use pica::packets::uci;
use uwb_actor::err::Code;

use crate::world::{NotifyBehavior, World};

// Feature: UWB Actor Coded Error Statistics
//
//   As a telemetry consumer
//   I want UWB actor transport and client failures recorded with stable codes
//   So that sink and device notification issues are visible without log noise

// Scenario: Open sink and clean chip deletion record no error stats
//
//   Given an initialized UWB chip with error stats enabled
//   When the chip is reset and deleted cleanly
//   Then notify_chip_removed completes and no error stats are recorded
#[tokio::test]
async fn test_open_sink_and_delete_records_no_error_stats() {
    // Given
    let mut world = World::with_error_stats().await;
    let chip_id = 1;
    world.given_a_chip(chip_id).await;
    world.then_status_notification_is_received(chip_id, uci::DeviceState::DeviceStateReady).await;

    // When
    world.when_chip_is_reset(chip_id).await;
    world.then_reset_response_is_received(chip_id, uci::Status::Ok).await;
    world.then_status_notification_is_received(chip_id, uci::DeviceState::DeviceStateReady).await;
    world.when_delete_chip(chip_id).await.unwrap();

    // Then
    world.then_device_notify_completed().await;
    world.then_error_stats_are(&[]).await;
}

// Scenario: Socket guest disconnect during ranging records no error stats
//
//   Given two UWB chips with an active FiRa ranging session
//   When the initiator guest disconnects with BrokenPipe on the next packet
//   Then the chip is deleted and no error stats are recorded
#[tokio::test]
async fn test_socket_guest_disconnect_during_ranging_records_no_error_stats() {
    // Given
    let mut world = World::with_error_stats().await;
    world
        .given_active_session_between_two_chips(
            uci::SessionType::FiraRangingSession,
            1,
            [0x00, 0x01],
            2,
            [0x00, 0x02],
            1,
        )
        .await;

    // When
    world.when_guest_disconnects_on_next_packet(ChipId(1), false);

    // Then
    world.then_chip_does_not_exist(1).await;
    world.then_device_notify_completed().await;
    world.then_error_stats_are(&[]).await;
}

// Scenario: gRPC guest disconnect during ranging records no error stats
//
//   Given two UWB chips with an active FiRa ranging session
//   When the initiator guest disconnects with a gRPC stream disconnect error
//   Then the chip is deleted and no error stats are recorded
#[tokio::test]
async fn test_grpc_guest_disconnect_during_ranging_records_no_error_stats() {
    // Given
    let mut world = World::with_error_stats().await;
    world
        .given_active_session_between_two_chips(
            uci::SessionType::FiraRangingSession,
            1,
            [0x00, 0x01],
            2,
            [0x00, 0x02],
            1,
        )
        .await;

    // When
    world.when_guest_disconnects_on_next_packet(ChipId(1), true);

    // Then
    world.then_chip_does_not_exist(1).await;
    world.then_device_notify_completed().await;
    world.then_error_stats_are(&[]).await;
}

// Scenario: Packet sink error during DeviceActor shutdown records no stats
//
//   Given an initialized UWB chip and a DeviceActor in normal shutdown
//   When the sink fails with ErrorKind::Other as the chip emits UCI packets
//   Then the chip is deleted and no error stats are recorded
#[tokio::test]
async fn test_sink_error_during_shutdown_records_no_error_stats() {
    // Given: an initialized UWB chip before shutdown begins
    let mut world = World::with_error_stats().await;
    let chip_id = 8;
    world.given_a_chip(chip_id).await;
    world.then_status_notification_is_received(chip_id, uci::DeviceState::DeviceStateReady).await;

    // When: DeviceActor shuts down and the sink fails on chip reset
    world.when_device_actor_shuts_down().await;
    world.given_packet_sink_write_error(ChipId(chip_id), ErrorKind::Other);
    world.when_chip_is_reset(ChipId(chip_id)).await;

    // Then: chip teardown completes without recording PacketSinkSendFailed
    world.then_chip_does_not_exist(chip_id).await;
    world.then_device_notify_completed().await;
    world.then_error_stats_are(&[]).await;
}

// Scenario: Unexpected packet sink error on open stream records error stat
//
//   Given an initialized UWB chip whose sink fails with InvalidData
//   When the chip is reset to emit UCI packets while the stream stays open
//   Then the chip is deleted and PacketSinkSendFailed is recorded once
#[tokio::test]
async fn test_unexpected_sink_error_records_error_stat() {
    // Given
    let mut world = World::with_error_stats().await;
    let chip_id = 2;
    world.given_a_chip(chip_id).await;
    world.then_status_notification_is_received(chip_id, uci::DeviceState::DeviceStateReady).await;
    world.given_packet_sink_write_error(ChipId(chip_id), ErrorKind::InvalidData);

    // When
    world.when_chip_is_reset(chip_id).await;

    // Then
    world.then_chip_does_not_exist(chip_id).await;
    world.then_device_notify_completed().await;
    world.then_error_stats_are(&[(Code::PacketSinkSendFailed as u32, 1)]).await;
}

// Scenario: Unexpected opaque packet sink error on open stream records stat
//
//   Given an initialized UWB chip whose sink fails with ErrorKind::Other
//   When the chip is reset to emit UCI packets while the stream stays open
//   Then the chip is deleted and PacketSinkSendFailed is recorded once
#[tokio::test]
async fn test_unexpected_other_sink_error_on_open_stream_records_error_stat() {
    // Given: an initialized chip whose sink fails with ErrorKind::Other
    let mut world = World::with_error_stats().await;
    let chip_id = 7;
    world.given_a_chip(chip_id).await;
    world.then_status_notification_is_received(chip_id, uci::DeviceState::DeviceStateReady).await;
    world.given_packet_sink_write_error(ChipId(chip_id), ErrorKind::Other);

    // When: the chip is reset while the inbound stream remains open
    world.when_chip_is_reset(ChipId(chip_id)).await;

    // Then: the chip is deleted and PacketSinkSendFailed is recorded once
    world.then_chip_does_not_exist(chip_id).await;
    world.then_device_notify_completed().await;
    world.then_error_stats_are(&[(Code::PacketSinkSendFailed as u32, 1)]).await;
}

// Scenario: DeviceNotFound during notify_chip_removed is ignored
//
//   Given a UWB chip and DeviceActor configured to return DeviceNotFound
//   When the chip is deleted
//   Then notify_chip_removed completes and no error stats are recorded
#[tokio::test]
async fn test_notify_chip_removed_device_not_found_is_ignored() {
    // Given
    let mut world = World::with_error_stats().await;
    let chip_id = 3;
    world.given_a_chip(chip_id).await;
    world.given_device_notify_behavior(NotifyBehavior::DeviceNotFound);

    // When
    world.when_delete_chip(chip_id).await.unwrap();

    // Then
    world.then_device_notify_completed().await;
    world.then_error_stats_are(&[]).await;
}

// Scenario: ActorClosed during normal DeviceActor shutdown is ignored
//
//   Given a UWB chip and a DeviceActor that has initiated normal shutdown
//   When the chip is deleted after DeviceActor shuts down
//   Then notify_chip_removed completes and no error stats are recorded
#[tokio::test]
async fn test_notify_chip_removed_during_shutdown_records_no_error_stats() {
    // Given
    let mut world = World::with_error_stats().await;
    let chip_id = 4;
    world.given_a_chip(chip_id).await;

    // When
    world.when_device_actor_shuts_down().await;
    world.when_delete_chip(chip_id).await.unwrap();

    // Then
    world.then_device_notify_completed().await;
    world.then_error_stats_are(&[]).await;
}

// Scenario: Unexpected ActorClosed in notify_chip_removed records error stat
//
//   Given a UWB chip and DeviceActor returning ActorClosed without shutdown
//   When the chip is deleted
//   Then notify_chip_removed completes and DeviceNotifyChipRemovedFailed logs
#[tokio::test]
async fn test_notify_chip_removed_unexpected_actor_closed_records_error_stat() {
    // Given
    let mut world = World::with_error_stats().await;
    let chip_id = 5;
    world.given_a_chip(chip_id).await;
    world.given_device_notify_behavior(NotifyBehavior::ActorClosed);

    // When
    world.when_delete_chip(chip_id).await.unwrap();

    // Then
    world.then_device_notify_completed().await;
    world.then_error_stats_are(&[(Code::DeviceNotifyChipRemovedFailed as u32, 1)]).await;
}

// Scenario: Unexpected ActorDropped in notify_chip_removed records error stat
//
//   Given a UWB chip and DeviceActor dropping its reply channel unexpectedly
//   When the chip is deleted
//   Then notify_chip_removed completes and DeviceNotifyChipRemovedFailed logs
#[tokio::test]
async fn test_notify_chip_removed_unexpected_actor_dropped_records_error_stat() {
    // Given
    let mut world = World::with_error_stats().await;
    let chip_id = 6;
    world.given_a_chip(chip_id).await;
    world.given_device_notify_behavior(NotifyBehavior::ActorDropped);

    // When
    world.when_delete_chip(chip_id).await.unwrap();

    // Then
    world.then_device_notify_completed().await;
    world.then_error_stats_are(&[(Code::DeviceNotifyChipRemovedFailed as u32, 1)]).await;
}
