// Copyright 2023-2025 The Android Open Source Project
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

use crate::world;

// Feature: Beacon Support
//
//   As a client
//   I want to create beacon chips
//   So that I can simulate BLE beacons

// Scenario: Successfully create a beacon chip
//
//   Given the bluetooth actor is running
//   When request to create a beacon chip
//   Then the chip is created successfully
#[tokio::test]
async fn test_add_chip() {
    let mut world = world::World::new();

    // Given a Bluetooth beacon
    world.given_beacon("Beacon").await;

    // Then the chip count should be 1
    world.then_chip_count_is(1).await;
}

// Scenario: Beacon created without a name gets Beacon-{ID}
#[tokio::test]
async fn test_beacon_unique_default_naming() {
    let mut world = world::World::new();

    // When a beacon is created with defaults (empty name)
    let id = world.when_create_beacon_with_defaults().await;

    // Then it should have the name Beacon-{ID}
    world.then_chip_name_is(id, &format!("Beacon-{}", id.0)).await;
}

// Scenario: Multiple beacons created with defaults get unique names
#[tokio::test]
async fn test_multiple_beacons_unique_naming() {
    let mut world = world::World::new();

    // When two beacons are created with defaults
    let id1 = world.when_create_beacon_with_defaults().await;
    let id2 = world.when_create_beacon_with_defaults().await;

    // Then they should have unique names
    world.then_chip_name_is(id1, &format!("Beacon-{}", id1.0)).await;
    world.then_chip_name_is(id2, &format!("Beacon-{}", id2.0)).await;
}

// Scenario: Beacon created with explicit name preserves it
#[tokio::test]
async fn test_beacon_explicit_name() {
    let mut world = world::World::new();

    // When a beacon is created with an explicit name "MyBeacon"
    world.given_beacon("MyBeacon").await;

    // Then it should keep that name
    let id = *world.chips.get("MyBeacon").unwrap();
    world.then_chip_name_is(id, "MyBeacon").await;
}

// Scenario: Beacon respects configured interval (default vs override)
#[tokio::test]
async fn test_beacon_interval() {
    let mut world = world::World::new();

    // Given a scanner
    world.given_scanner("Scanner").await;

    // And a beacon with default settings
    world.given_beacon("Beacon-Default").await;

    // And a beacon configured with BALANCED mode (250ms)
    world
        .given_beacon_with_interval(
            "Beacon-250ms",
            netsim_model::Interval::AdvertiseMode(netsim_model::AdvertiseMode::Balanced.into()),
        )
        .await;

    // Then scanner should see advertisements from "Beacon-Default" at ~1000ms rate
    world.then_scanner_measures_interval_from("Scanner", "Beacon-Default", 1000).await;

    // And scanner should see advertisements from "Beacon-250ms" at ~250ms rate
    world.then_scanner_measures_interval_from("Scanner", "Beacon-250ms", 250).await;
}

// Scenario: Beacon is created with classic disabled by default
#[tokio::test]
async fn test_beacon_classic_disabled() {
    let mut world = world::World::new();

    // When a beacon is created
    world.given_beacon("Beacon").await;

    // Then its classic radio state should be disabled (false)
    let id = *world.chips.get("Beacon").unwrap();
    world.then_classic_state_is(id, Some(false)).await;
}

// Scenario: Beacon advertises ADV_SCAN_IND (0x02) by default
#[tokio::test]
async fn test_beacon_advertises_adv_scan_ind() {
    let mut world = world::World::new();

    world.given_beacon("Beacon").await;
    world.given_scanner("Scanner").await;

    world.then_scanner_sees_adv_type("Scanner", 0x02).await;
}

// Scenario: Beacon advertisement includes standard BLE Flags
// (BR/EDR Not Supported, 0x04)
#[tokio::test]
async fn test_beacon_advertises_standard_ble_flags() {
    let mut world = world::World::new();

    world.given_beacon("Beacon").await;
    world.given_scanner("Scanner").await;

    let reports = world.receive_scan_report("Scanner").await;
    assert!(!reports.is_empty(), "Expected at least one scan report");
    assert_eq!(reports[0].payload, vec![0x02, 0x01, 0x04]);
}
