// Copyright 2025 The Android Open Source Project
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

//! Integration tests verifying Beacon RSSI updates and transmit power levels.

use netsim_model::{AdvertiseTxPower, Position, TxPower};

use crate::world::World;

// Scenario: Scanner receives decreasing RSSI as beacon distance increases
//   Given a world with a scanner and a beacon at 1.0 m distance
//   When the beacon moves to 10.0 m
//   Then the scanner receives an advertising report with lower RSSI
//   When the beacon moves to 20.0 m
//   Then the scanner receives an advertising report with lower RSSI
#[tokio::test]
async fn test_rssi_updates() {
    let mut world = World::new();

    // Given a world with a scanner and a beacon at 1.0 m distance
    world.given_scanner("scanner").await;
    world.given_beacon_at("beacon", Position { x: 1.0, y: 0.0, z: 0.0 }).await;
    let rssi_1m =
        world.then_scanner_sees_adv_with_rssi_lower_than("scanner", "beacon", i8::MAX).await;

    // When the beacon moves to 10.0 m
    world.when_beacon_moves_to("beacon", Position { x: 10.0, y: 0.0, z: 0.0 }).await;
    // Then the scanner receives an advertising report with lower RSSI
    let rssi_10m =
        world.then_scanner_sees_adv_with_rssi_lower_than("scanner", "beacon", rssi_1m).await;

    // When the beacon moves to 20.0 m
    world.when_beacon_moves_to("beacon", Position { x: 20.0, y: 0.0, z: 0.0 }).await;
    // Then the scanner receives an advertising report with lower RSSI
    world.then_scanner_sees_adv_with_rssi_lower_than("scanner", "beacon", rssi_10m).await;
}

// Scenario: Scanner receives updated RSSI and AD payload across
// AdvertiseTxPower levels
//   Given a world with a scanner and a beacon at 1.0 m distance
//   When the beacon tx_power is reconfigured across UltraLow, Low, Medium, High
//   Then the scanner receives matching RSSI and TX Power Level AD element
#[tokio::test]
async fn test_beacon_advertise_tx_power_levels() {
    let mut world = World::new();

    // Given a world with a scanner and a beacon at 1.0 m distance
    world.given_scanner("scanner").await;
    world.given_beacon_at("beacon", Position { x: 1.0, y: 0.0, z: 0.0 }).await;

    // When beacon tx_power is reconfigured to UltraLow (-21 dBm)
    world
        .when_update_beacon_tx_power("beacon", TxPower::TxPowerLevel(AdvertiseTxPower::UltraLow))
        .await;
    // Then scanner receives ADV_SCAN_IND at -21 dBm RSSI with AD tx_power
    // -21 dBm
    world.then_scanner_sees_adv_with_tx_power_and_rssi("scanner", "beacon", -21, -21).await;

    // When beacon tx_power is reconfigured to Low (-15 dBm)
    world.when_update_beacon_tx_power("beacon", TxPower::TxPowerLevel(AdvertiseTxPower::Low)).await;
    // Then scanner receives ADV_SCAN_IND at -15 dBm RSSI with AD tx_power
    // -15 dBm
    world.then_scanner_sees_adv_with_tx_power_and_rssi("scanner", "beacon", -15, -15).await;

    // When beacon tx_power is reconfigured to Medium (-7 dBm)
    world
        .when_update_beacon_tx_power("beacon", TxPower::TxPowerLevel(AdvertiseTxPower::Medium))
        .await;
    // Then scanner receives ADV_SCAN_IND at -7 dBm RSSI with AD tx_power -7 dBm
    world.then_scanner_sees_adv_with_tx_power_and_rssi("scanner", "beacon", -7, -7).await;

    // When beacon tx_power is reconfigured to High (+1 dBm)
    world
        .when_update_beacon_tx_power("beacon", TxPower::TxPowerLevel(AdvertiseTxPower::High))
        .await;
    // Then scanner receives ADV_SCAN_IND at 1 dBm RSSI with AD tx_power 1 dBm
    world.then_scanner_sees_adv_with_tx_power_and_rssi("scanner", "beacon", 1, 1).await;
}

// Scenario: Beacon tx_power honors custom dBm and clamps out-of-range values
//   Given a world with a scanner and a beacon at 1.0 m distance
//   When the beacon tx_power is set to -128 dBm (below hardware floor)
//   Then the scanner receives ADV_SCAN_IND clamped to -120 dBm RSSI with
//     -127 dBm AD payload
//   When the beacon tx_power is set to +10 dBm
//   Then the scanner receives ADV_SCAN_IND at 10 dBm RSSI with
//     10 dBm AD payload
//   When the beacon tx_power is set to +25 dBm (above hardware ceiling)
//   Then the scanner receives ADV_SCAN_IND clamped to 20 dBm RSSI with
//     20 dBm AD payload
#[tokio::test]
async fn test_beacon_custom_and_clamped_tx_power() {
    let mut world = World::new();

    // Given a world with a scanner and a beacon at 1.0 m distance
    world.given_scanner("scanner").await;
    world.given_beacon_at("beacon", Position { x: 1.0, y: 0.0, z: 0.0 }).await;

    // When beacon tx_power is set to -128 dBm (below hardware floor)
    world.when_update_beacon_tx_power("beacon", TxPower::Dbm(-128)).await;
    // Then scanner receives ADV_SCAN_IND clamped to -120 dBm RSSI with
    // -127 dBm AD payload
    world.then_scanner_sees_adv_with_tx_power_and_rssi("scanner", "beacon", -127, -120).await;

    // When beacon tx_power is set to +10 dBm
    world.when_update_beacon_tx_power("beacon", TxPower::Dbm(10)).await;
    // Then scanner receives ADV_SCAN_IND at 10 dBm RSSI with 10 dBm AD payload
    world.then_scanner_sees_adv_with_tx_power_and_rssi("scanner", "beacon", 10, 10).await;

    // When beacon tx_power is set to +25 dBm (above hardware ceiling)
    world.when_update_beacon_tx_power("beacon", TxPower::Dbm(25)).await;
    // Then scanner receives ADV_SCAN_IND clamped to 20 dBm RSSI with
    // 20 dBm AD payload
    world.then_scanner_sees_adv_with_tx_power_and_rssi("scanner", "beacon", 20, 20).await;
}

// Scenario: Non-scannable beacon updates tx_power and transitions to scannable
//   Given a world with an active scanner and a non-scannable beacon at 1.0 m
//     distance
//   Then initial report is ADV_NONCONN_IND (0x03) with Low power (-15 dBm)
//   When updating tx_power to 5 dBm
//   Then emits ADV_NONCONN_IND (0x03) at 5 dBm RSSI
//   When transitioning to scannable with scan response and 9 dBm tx_power
//   Then emits ADV_SCAN_IND (0x02) at 9 dBm RSSI
//   And active scanner receives SCAN_RSP (0x04) at 9 dBm RSSI
#[tokio::test]
async fn test_non_scannable_beacon_tx_power_and_scannable_transition() {
    let mut world = World::new();

    // Given a world with an active scanner and a non-scannable beacon at
    // 1.0 m distance
    world.given_active_scanner("scanner").await;
    world
        .given_non_scannable_beacon_at(
            "nonconn_beacon",
            Position { x: 1.0, y: 0.0, z: 0.0 },
            TxPower::TxPowerLevel(AdvertiseTxPower::Low),
        )
        .await;

    // Then initial report is ADV_NONCONN_IND (0x03) with Low power (-15 dBm)
    world.then_scanner_sees_adv_type_and_rssi("scanner", "nonconn_beacon", 0x03, -15).await;

    // When updating tx_power to 5 dBm
    world.when_update_non_scannable_beacon_tx_power("nonconn_beacon", TxPower::Dbm(5)).await;

    // Then emits ADV_NONCONN_IND (0x03) at 5 dBm RSSI
    world.then_scanner_sees_adv_type_and_rssi("scanner", "nonconn_beacon", 0x03, 5).await;

    // When transitioning to scannable with scan response and 9 dBm tx_power
    world
        .when_transition_beacon_to_scannable("nonconn_beacon", TxPower::Dbm(9), vec![0x01, 0x02])
        .await;

    // Then emits ADV_SCAN_IND (0x02) at 9 dBm RSSI
    world.then_scanner_sees_adv_type_and_rssi("scanner", "nonconn_beacon", 0x02, 9).await;

    // And active scanner receives SCAN_RSP (0x04) at 9 dBm RSSI
    world.then_scanner_sees_scan_response_with_rssi("scanner", "nonconn_beacon", 9).await;
}
