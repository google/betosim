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

use netsim_model::{
    Chip, ChipClient, ChipCreate, ChipError, ChipId, ChipUpdate, ChipVariant, DeviceId, PoseUpdate,
    Position, Uwb, UwbMode,
};
use netsim_testing::mock_stream;
use pica::packets::uci;

use crate::world::World;

// Feature: UWB Ranging between chips
//
//   As a client
//   I want to place two UWB chips at different positions
//   And establish a ranging session between them
//   So that I can verify that pica computes the correct range

// Scenario: Ranging between two chips at 10 meters distance
//
//   Given a chip 100 at (0, 0, 0)
//   And a chip 200 at (10, 0, 0)
//   When UCI session 1 is established on chip 100 as CONTROLLER with peer mac
// [0, 1]   And UCI session 1 is established on chip 200 as CONTROLEE with peer
// mac [0, 0]   And ranging is started on chip 100 for session 1
//   Then a ranging measurement of 1000 cm is received on chip 100
#[tokio::test]
async fn test_ranging_between_two_chips() {
    let mut world = World::new().await;

    let chip_a = 1;
    let chip_a_mac = [0, 0];
    let chip_b = 2;
    let chip_b_mac = [0, 1];

    // Given
    world.given_a_chip_at(chip_a, Position { x: 0.0, y: 0.0, z: 0.0 }).await;
    world.given_a_chip_at(chip_b, Position { x: 10.0, y: 0.0, z: 0.0 }).await;

    // Clear initial READY notifications and RESET
    world.then_status_notification_is_received(chip_a, uci::DeviceState::DeviceStateReady).await;
    world.when_uci_is_reset(chip_a).await;

    world.then_status_notification_is_received(chip_b, uci::DeviceState::DeviceStateReady).await;
    world.when_uci_is_reset(chip_b).await;

    // When
    world
        .when_uci_session_is_established(
            chip_a,
            1,
            uci::DeviceType::Controller,
            uci::DeviceRole::Initiator,
            chip_a_mac,
            chip_b_mac,
        )
        .await;

    world
        .when_uci_session_is_established(
            chip_b,
            1,
            uci::DeviceType::Controlee,
            uci::DeviceRole::Responder,
            chip_b_mac,
            chip_a_mac,
        )
        .await;

    world.when_ranging_is_started(chip_a, 1).await;
    world.when_ranging_is_started(chip_b, 1).await;

    // Trigger one ranging round manually
    world.when_ranging_is_triggered(chip_a, 1).await;

    // Then
    // distance = 1m = 1000cm
    world.then_ranging_measurement_is_received(chip_a, 1000).await;

    // Verify telemetry
    let stats = world.client.read_statistics().await.expect("Failed to read statistics");
    let stats_a = stats.iter().find(|s| s.id == chip_a).unwrap();
    let stats_b = stats.iter().find(|s| s.id == chip_b).unwrap();
    assert_eq!(stats_a.p2p_tx_count, 1);
    assert_eq!(stats_a.p2p_rx_count, 1);
    assert_eq!(stats_b.p2p_tx_count, 0);
    assert_eq!(stats_b.p2p_rx_count, 0);
}

#[tokio::test]
async fn test_ranging_between_chip_and_anchor() {
    let mut world = World::new().await;

    let chip_a = 1;
    let chip_a_mac = [0, 0];
    let anchor_id = 2;
    let anchor_mac = [0xFF, 0xFE];

    world.given_a_chip_at(chip_a, Position { x: 0.0, y: 0.0, z: 0.0 }).await;
    world.given_an_anchor_at(anchor_id, None, Position { x: 3.0, y: 4.0, z: 0.0 }).await;

    let anchor_chip = world.when_get_chip(anchor_id).await.unwrap();
    assert_eq!(
        anchor_chip.variant,
        Some(ChipVariant::Uwb(Uwb {
            radio: Default::default(),
            address: "FF:FE".to_string(),
            mode: UwbMode::Anchor,
        }))
    );

    assert_eq!(world.client.read_count_for_testing().await.unwrap(), 2);
    assert_eq!(world.client.read_statistics().await.unwrap().len(), 2);

    world.then_status_notification_is_received(chip_a, uci::DeviceState::DeviceStateReady).await;
    world.when_uci_is_reset(chip_a).await;

    world
        .when_uci_session_is_established(
            chip_a,
            1,
            uci::DeviceType::Controller,
            uci::DeviceRole::Initiator,
            chip_a_mac,
            anchor_mac,
        )
        .await;

    world.when_ranging_is_started(chip_a, 1).await;
    world.when_ranging_is_triggered(chip_a, 1).await;

    let m = world.then_single_ranging_measurement_is_received(chip_a).await;
    assert_eq!(m.distance, 500);
    assert_eq!(m.aoa_azimuth, (90i16 << 7) as u16);
    assert_eq!(m.aoa_elevation, (53i16 << 7) as u16);
    assert_eq!(m.aoa_destination_azimuth, (-90i16 << 7) as u16);
    assert_eq!(m.aoa_destination_elevation, (-53i16 << 7) as u16);

    world
        .client
        .update(
            ChipId(anchor_id),
            ChipUpdate {
                pose: PoseUpdate {
                    position: Some(Position { x: 0.0, y: 0.0, z: 10.0 }),
                    orientation: None,
                },
                ..Default::default()
            },
        )
        .await
        .unwrap();

    world.when_ranging_is_triggered(chip_a, 1).await;

    let m2 = world.then_single_ranging_measurement_is_received(chip_a).await;
    assert_eq!(m2.distance, 1000);
    assert_eq!(m2.aoa_azimuth, 0);
    assert_eq!(m2.aoa_elevation, 0);
    assert_eq!(m2.aoa_destination_azimuth, (180i16 << 7) as u16);
    assert_eq!(m2.aoa_destination_elevation, 0);

    world.client.delete(ChipId(anchor_id)).await.unwrap();
    assert_eq!(world.client.read_count_for_testing().await.unwrap(), 1);

    world.when_ranging_is_triggered(chip_a, 1).await;
    let m3 = world.then_single_ranging_measurement_is_received(chip_a).await;
    assert_eq!(m3.status, uci::Status::RangingRxTimeout);
}

#[tokio::test]
async fn test_anchor_crud_errors() {
    let mut world = World::new().await;

    world.given_an_anchor_at(1, Some("00:03"), Position::default()).await;

    let err = world
        .when_create_anchor(2, Some("00:03"), Position::default())
        .await
        .expect_err("duplicate anchor creation fails");
    assert!(matches!(err.as_chip_error(), Some(ChipError::InvalidArguments(_))));

    let err = world
        .when_create_anchor(2, Some("FF:FF"), Position::default())
        .await
        .expect_err("broadcast anchor creation fails");
    assert!(matches!(err.as_chip_error(), Some(ChipError::InvalidArguments(_))));

    let err = world
        .when_create_anchor(3, Some("00:11:22:33:44:55:66:77"), Position::default())
        .await
        .expect_err("extended anchor creation fails");
    assert!(matches!(err.as_chip_error(), Some(ChipError::InvalidArguments(_))));

    let err = world
        .when_create_anchor(4, Some("invalid"), Position::default())
        .await
        .expect_err("malformed anchor address fails");
    assert!(matches!(err.as_chip_error(), Some(ChipError::InvalidArguments(_))));

    let err = world.client.start_ranging(ChipId(1), 1).await.expect_err("start ranging fails");
    assert!(matches!(err.as_chip_error(), Some(ChipError::Unsupported)));

    let err = world.client.stop_ranging(ChipId(1), 1).await.expect_err("stop ranging fails");
    assert!(matches!(err.as_chip_error(), Some(ChipError::Unsupported)));

    let (stream, _tx) = mock_stream();
    let mut chip = Chip::new_test_uwb_anchor("uwb_anchor_5".to_string(), None);
    chip.id = 5;
    chip.device_id = DeviceId(1);
    let params = ChipCreate { packet_stream: Some(stream), packet_sink: None, chip };
    let err = world.client.create(ChipId(5), params).await.expect_err("anchor with stream fails");
    assert!(matches!(err.as_chip_error(), Some(ChipError::InvalidArguments(_))));
}
