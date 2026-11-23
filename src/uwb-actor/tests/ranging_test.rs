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

use netsim_model::{Chip, ChipClient, ChipCreate, ChipError, ChipVariant, Position, UwbMode};
use netsim_testing::mock_stream;
use pica::packets::uci;

use crate::world::World;

// Feature: UWB ranging between chips and anchors

const SESSION_ID: u32 = 1;
const CHIP_A: u32 = 1;
const CHIP_A_MAC: [u8; 2] = [0x00, 0x00];
const ANCHOR_ID: u32 = 2;
const ANCHOR_CHIP: netsim_model::ChipId = netsim_model::ChipId(ANCHOR_ID);
const OTHER_ANCHOR_ID: u32 = 3;
const ANCHOR_ADDR: &str = "00:01";
const ANCHOR_MAC: [u8; 2] = [0x00, 0x01];
const ORIGIN: Position = Position { x: 0.0, y: 0.0, z: 0.0 };
const ANCHOR_POS: Position = Position { x: 3.0, y: 4.0, z: 0.0 };

async fn setup_chip_at(world: &mut World, chip_id: u32, pos: Position) {
    world.given_a_chip_at(chip_id, pos).await;
    world.then_status_notification_is_received(chip_id, uci::DeviceState::DeviceStateReady).await;
    world.when_uci_is_reset(chip_id).await;
}

async fn setup_anchor_session(
    world: &mut World,
    chip_pos: Position,
    anchor_pos: Position,
    anchor_addr: Option<&str>,
    anchor_mac: [u8; 2],
) {
    setup_chip_at(world, CHIP_A, chip_pos).await;
    world.given_an_anchor_at(ANCHOR_ID, anchor_addr, anchor_pos).await;
    world
        .when_uci_session_is_established(
            CHIP_A,
            SESSION_ID,
            uci::DeviceType::Controller,
            uci::DeviceRole::Initiator,
            CHIP_A_MAC,
            anchor_mac,
        )
        .await;
}

fn q9_7_deg(raw: u16) -> i16 {
    (raw as i16) >> 7
}

async fn expect_create_anchor_invalid_arg(world: &mut World, chip_id: u32, addr: &str) {
    let Err(err) = world.when_create_anchor(chip_id, Some(addr), Position::default()).await else {
        panic!("create_anchor({addr}) unexpectedly succeeded");
    };
    let e = err.as_chip_error();
    assert!(matches!(e, Some(ChipError::InvalidArguments(_))), "{addr}: {err:?}");
}

// Scenario: Two chips measure 10m range and update p2p telemetry
#[tokio::test]
async fn test_ranging_between_two_chips() {
    let mut world = World::new().await;
    let (chip_b, chip_b_mac) = (2, [0, 1]);
    // Given two reset UWB chips 10m apart
    setup_chip_at(&mut world, CHIP_A, ORIGIN).await;
    setup_chip_at(&mut world, chip_b, Position { x: 10.0, y: 0.0, z: 0.0 }).await;
    // When a controller/controlee session starts ranging
    world
        .when_uci_session_is_established(
            CHIP_A,
            SESSION_ID,
            uci::DeviceType::Controller,
            uci::DeviceRole::Initiator,
            CHIP_A_MAC,
            chip_b_mac,
        )
        .await;
    world
        .when_uci_session_is_established(
            chip_b,
            SESSION_ID,
            uci::DeviceType::Controlee,
            uci::DeviceRole::Responder,
            chip_b_mac,
            CHIP_A_MAC,
        )
        .await;
    world.when_ranging_is_started(chip_b, SESSION_ID).await;
    world.when_ranging_is_started(CHIP_A, SESSION_ID).await;
    // Then the initiator receives 1000cm and, after stop, only its p2p counters increment
    world.then_ranging_measurement_is_received(CHIP_A, 1000).await;
    let extra = world.when_ranging_is_stopped(CHIP_A, SESSION_ID).await;
    let stats = world.client.read_statistics().await.expect("Failed to read statistics");
    let stats_a = stats.iter().find(|s| s.id == CHIP_A).unwrap();
    let stats_b = stats.iter().find(|s| s.id == chip_b).unwrap();
    assert_eq!((stats_a.p2p_tx_count, stats_a.p2p_rx_count), (1 + extra, 1 + extra));
    assert_eq!((stats_b.p2p_tx_count, stats_b.p2p_rx_count), (0, 0));
}

// Scenario: Creating an anchor without an address allocates the default short MAC
#[tokio::test]
async fn test_anchor_creation_allocates_short_mac() {
    let mut world = World::new().await;
    // When an anchor is created without an explicit address
    world.given_an_anchor_at(ANCHOR_ID, None, ORIGIN).await;
    // Then the anchor gets FF:FE in Anchor mode and appears once in count and stats
    let anchor_chip = world.when_get_chip(ANCHOR_ID).await.unwrap();
    let Some(ChipVariant::Uwb(uwb)) = anchor_chip.variant else {
        panic!("expected UWB variant");
    };
    assert_eq!(uwb.mode, UwbMode::Anchor);
    assert_eq!(uwb.address, "FF:FE");
    assert_eq!(world.client.read_count_for_testing().await.unwrap(), 1);
    let stats = world.client.read_statistics().await.unwrap();
    assert_eq!(stats.len(), 1);
    assert_eq!(stats[0].id, ANCHOR_ID);
}

// Scenario: Chip ranging with a stationary anchor reports distance and AoA
#[tokio::test]
async fn test_ranging_between_chip_and_anchor() {
    let mut world = World::new().await;
    // Given a chip at origin and an auto-addressed anchor at (3, 4, 0) in an established session
    setup_anchor_session(&mut world, ORIGIN, ANCHOR_POS, None, [0xFF, 0xFE]).await;
    // When ranging starts
    world.when_ranging_is_started(CHIP_A, SESSION_ID).await;
    // Then the measurement reports 5m distance and expected AoA bounds
    let m = world.then_single_ranging_measurement_is_received(CHIP_A).await;
    assert!((480..=520).contains(&m.distance));
    assert!((87..=93).contains(&q9_7_deg(m.aoa_azimuth)));
    assert!((50..=56).contains(&q9_7_deg(m.aoa_elevation)));
    assert!((-93..=-87).contains(&q9_7_deg(m.aoa_destination_azimuth)));
    assert!((-56..=-50).contains(&q9_7_deg(m.aoa_destination_elevation)));
}

// Scenario: Moving an anchor updates ranging distance and AoA after stop and restart
#[tokio::test]
async fn test_ranging_updates_after_anchor_move() {
    let mut world = World::new().await;
    // Given an established chip-anchor session that measured 5m and stopped
    setup_anchor_session(&mut world, ORIGIN, ANCHOR_POS, Some(ANCHOR_ADDR), ANCHOR_MAC).await;
    world.when_ranging_is_started(CHIP_A, SESSION_ID).await;
    let before = world.then_single_ranging_measurement_is_received(CHIP_A).await;
    assert!((480..=520).contains(&before.distance));
    world.when_ranging_is_stopped(CHIP_A, SESSION_ID).await;
    // When the anchor moves to (0, 0, 10) and ranging restarts
    world.when_chip_moves_to(ANCHOR_CHIP, Position { x: 0.0, y: 0.0, z: 10.0 }).await;
    world.when_ranging_is_triggered(CHIP_A, SESSION_ID).await;
    // Then the measurement reflects 10m vertical distance and updated AoA
    let m = world.then_single_ranging_measurement_is_received(CHIP_A).await;
    assert!((980..=1020).contains(&m.distance));
    assert!((-3..=3).contains(&q9_7_deg(m.aoa_azimuth)));
    assert!((-3..=3).contains(&q9_7_deg(m.aoa_elevation)));
    assert!(q9_7_deg(m.aoa_destination_azimuth).abs() >= 177);
    assert!((-3..=3).contains(&q9_7_deg(m.aoa_destination_elevation)));
}

// Scenario: Ranging times out after the target anchor is deleted
#[tokio::test]
async fn test_ranging_times_out_after_anchor_deleted() {
    let mut world = World::new().await;
    // Given an established chip-anchor ranging session
    setup_anchor_session(&mut world, ORIGIN, ANCHOR_POS, Some(ANCHOR_ADDR), ANCHOR_MAC).await;
    // When the anchor is deleted and ranging is triggered
    world.when_delete_chip(ANCHOR_ID).await.unwrap();
    world.when_ranging_is_triggered(CHIP_A, SESSION_ID).await;
    // Then only the device chip remains, ranging times out, and the session stops
    assert_eq!(world.client.read_count_for_testing().await.unwrap(), 1);
    let m = world.then_single_ranging_measurement_is_received(CHIP_A).await;
    assert_eq!(m.status, uci::Status::RangingRxTimeout);
    world.when_ranging_is_stopped(CHIP_A, SESSION_ID).await;
}

// Scenario: Creating an anchor rejects duplicate, broadcast, extended, and malformed addresses
#[tokio::test]
async fn test_create_anchor_rejects_invalid_addresses() {
    let mut world = World::new().await;
    // Given an existing anchor with a valid short address
    world.given_an_anchor_at(ANCHOR_ID, Some(ANCHOR_ADDR), Position::default()).await;
    // When creating another anchor with duplicate, broadcast, extended, or malformed address
    // Then each attempt fails with InvalidArguments
    expect_create_anchor_invalid_arg(&mut world, OTHER_ANCHOR_ID, ANCHOR_ADDR).await;
    expect_create_anchor_invalid_arg(&mut world, OTHER_ANCHOR_ID, "FF:FF").await;
    expect_create_anchor_invalid_arg(&mut world, OTHER_ANCHOR_ID, "00:11:22:33:44:55:66:77").await;
    expect_create_anchor_invalid_arg(&mut world, OTHER_ANCHOR_ID, "invalid").await;
}

// Scenario: An anchor rejects start_ranging and stop_ranging commands
#[tokio::test]
async fn test_anchor_rejects_ranging_commands() {
    let mut world = World::new().await;
    // Given an anchor
    world.given_an_anchor_at(ANCHOR_ID, Some(ANCHOR_ADDR), Position::default()).await;
    // When start_ranging and stop_ranging are invoked on the anchor
    let c = &world.client;
    let start_err = c.start_ranging(ANCHOR_CHIP, SESSION_ID).await.expect_err("start fails");
    let stop_err = c.stop_ranging(ANCHOR_CHIP, SESSION_ID).await.expect_err("stop fails");
    // Then both fail with Unsupported
    assert!(matches!(start_err.as_chip_error(), Some(ChipError::Unsupported)));
    assert!(matches!(stop_err.as_chip_error(), Some(ChipError::Unsupported)));
}

// Scenario: Creating an anchor with a packet stream fails with InvalidArguments
#[tokio::test]
async fn test_anchor_rejects_packet_stream() {
    let world = World::new().await;
    // Given anchor create params carrying a packet stream
    let (stream, _tx) = mock_stream();
    let chip = Chip::new_test_uwb_anchor("uwb_anchor".to_string(), None);
    let params = ChipCreate { packet_stream: Some(stream), packet_sink: None, chip };
    // When creating the anchor
    let err = world.client.create(ANCHOR_CHIP, params).await.expect_err("stream fails");
    // Then creation fails with InvalidArguments
    assert!(matches!(err.as_chip_error(), Some(ChipError::InvalidArguments(_))));
}
