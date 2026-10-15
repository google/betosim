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

use netsim_model::{ChipUpdate, Interval};

use crate::world::World;

// Feature: Scanner Support
//
//   As a client
//   I want to attach a scanner
//   So that I can capture BLE advertisements

// Scenario: Scanner receives advertisements
//
//   Given a beacon chip
//   And a scanner chip attached
//   When the beacon advertises
//   Then the scanner receives the advertisement packet
#[tokio::test]
async fn test_scanner_receives_advertisement() {
    let mut world = World::new();

    // 1. Create a beacon on a DIFFERENT device to allow loopback/air traffic.
    world.given_beacon("Beacon").await;

    // 2. Create a scanner with the mock packet sink (managed by World).
    world.given_scanner("Scanner").await;

    // 3. Wait for the advertisement packet.
    world.then_scanner_sees_any_adv("Scanner").await;
}

// Scenario: Scanner receives Tx Power and RSSI
//
//   Given a beacon chip with Tx Power enabled
//   And a scanner chip attached
//   When the beacon advertises
//   Then the scanner receives the advertisement packet with RSSI and Tx Power
#[tokio::test]
async fn test_scanner_receives_tx_power_and_rssi() {
    let mut world = World::new();

    // 1. Create a beacon with Tx Power enabled
    world.given_beacon_with_tx_power("Beacon", "High").await;

    // 2. Create a scanner
    world.given_scanner("Scanner").await;

    // 3. Verify fields
    world.then_scanner_sees_adv_with_rssi("Scanner", "High").await;
}

// Scenario: Sink closure triggers device removal
//
//   Given a scanner chip
//   And a beacon chip (to generate traffic)
//   When the packet sink is dropped/closed
//   Then the scanner chip is removed
#[tokio::test]
async fn test_sink_closure_removes_chip() {
    let mut world = World::new();

    // 1. Create a scanner (managed by World).
    world.given_scanner("Scanner").await;

    // 2. Create a beacon (to generate traffic).
    world.given_beacon("Beacon").await;

    // 3. Drop the sink (simulate disconnection).
    world.when_sink_dropped("Scanner");

    // 4. Wait for chip removal.
    world.then_chip_eventually_removed("Scanner").await;
}

// Scenario: Scanner captures duplicates (sniffer mode)
//
//   Given a beacon chip
//   And a scanner chip attached
//   When the beacon continues to advertise
//   Then the scanner receives multiple reports for the same beacon
#[tokio::test]
async fn test_scanner_captures_duplicates() {
    let mut world = World::new();

    // 1. Create a beacon.
    world.given_beacon("Beacon").await;

    // 2. Create a scanner.
    world.given_scanner("Scanner").await;

    // 3. Verify we receive at least 3 reports.
    for _ in 0..3 {
        world.receive_scan_report("Scanner").await;
    }
}

// Scenario: Scanner sees multiple beacons
//
//   Given two beacon chips
//   And a scanner chip attached
//   When both beacons advertise
//   Then the scanner receives reports from both
#[tokio::test]
async fn test_scanner_sees_multiple_beacons() {
    let mut world = World::new();

    // 1. Create two beacons with different names/addresses managed by World
    //    (auto-assigned).
    world.given_beacon("Beacon1").await;
    world.given_beacon("Beacon2").await;

    // 2. Create a scanner.
    world.given_scanner("Scanner").await;

    // 3. Verify scanner sees both beacons.
    world.then_scanner_sees_adv_from("Scanner", "Beacon1").await;
    world.then_scanner_sees_adv_from("Scanner", "Beacon2").await;
}

// Scenario: Active scanner does not deadlock the simulation
//
//   Given a beacon chip
//   And an active scanner chip
//   When the beacon advertises
//   Then the active scanner receives the advertisement and sends scan request
//   And the simulation does not deadlock
#[tokio::test]
async fn test_active_scanner_no_deadlock() {
    let mut world = World::new();

    // 1. Create a beacon.
    world.given_beacon("Beacon").await;

    // 2. Create an active scanner.
    world.given_active_scanner("ActiveScanner").await;

    // 3. Verify it receives the advertisement (if it deadlocks, this will hang).
    world.then_scanner_sees_any_adv("ActiveScanner").await;
}

// Scenario: Update under load does not deadlock
//
//   Given a beacon chip
//   And a scanner chip
//   When we update the beacon preset repeatedly
//   Then the simulation does not deadlock
#[tokio::test]
async fn test_update_deadlock_under_load() {
    let mut world = World::new();

    // 1. Create a beacon with fast advertising interval.
    world.given_beacon_with_interval("Beacon", Interval::Milliseconds(20)).await;

    // 2. Create a scanner.
    world.given_scanner("Scanner").await;

    let id = *world.chips.get("Beacon").unwrap();
    let client = world.client.clone();

    // 3. Spawn a task to update preset repeatedly to trigger handle_update ->
    //    set_properties.
    let handle = tokio::spawn(async move {
        let start = std::time::Instant::now();
        let mut i = 0;
        while start.elapsed() < std::time::Duration::from_millis(250) {
            let preset = if i % 2 == 0 { "default" } else { "laird_bl654" };
            let update = ChipUpdate {
                variant: Some(netsim_model::ChipVariantUpdate::Bluetooth(
                    netsim_model::BluetoothUpdate {
                        preset: Some(preset.to_string()),
                        ..Default::default()
                    },
                )),
                ..Default::default()
            };
            let _ = client.0.update(id, update).await;
            i += 1;
            tokio::task::yield_now().await;
        }
    });

    // Wait for the update task to finish (or hang if it deadlocks).
    tokio::time::timeout(std::time::Duration::from_secs(10), handle)
        .await
        .expect("Test timed out (deadlock detected)")
        .unwrap();
}
