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

use std::{collections::HashSet, time::Duration};

use bytes::Bytes;

use crate::world::World;

#[tokio::test]
async fn beacon_scan_stress_test() {
    let mut world = World::new();
    let scanner_name = "Scanner";
    world.given_scanner(scanner_name).await;

    let num_beacons = 20;

    // Add beacons to the world
    for i in 0..num_beacons {
        world.given_beacon(&format!("Beacon-{}", i)).await;
        // Small delay to prevent overwhelming the initial setup
        tokio::time::sleep(Duration::from_millis(1)).await;
    }

    let mut seen_beacons = HashSet::new();
    let timeout = Duration::from_secs(20);
    let start = std::time::Instant::now();

    // Collect advertisements until all beacons are seen or timeout occurs
    while seen_beacons.len() < num_beacons && start.elapsed() < timeout {
        let reports = world.receive_scan_report(scanner_name).await;
        for report in reports {
            // Beacons created by World have address 00:00:00:00:HIGH:LOW.
            let id = ((report.mac[4] as u32) << 8) | (report.mac[5] as u32);
            seen_beacons.insert(id);
        }
    }

    let final_count = seen_beacons.len();
    println!("Seen beacons count: {}", final_count);

    // Verify all beacons were seen
    assert_eq!(
        final_count, num_beacons,
        "Lost BLE scan results! Seen {}/{}",
        final_count, num_beacons
    );
}

// Scenario: Handle heavy load without drops
//
//   Given a bluetooth chip in device mode
//   When a burst of 100 commands is sent rapidly
//   Then all 100 responses are received (no silent drops in internal channels)
#[tokio::test]
async fn test_actor_heavy_load_no_drops() {
    let mut world = World::new();

    // 1. Create a virtual device chip with a slow sink (10ms delay per packet).
    world.given_slow_device("A", Duration::from_millis(10)).await;

    // 2. Send 100 HCI Reset commands rapidly.
    // Since we don't read them immediately, this fills up the internal buffer (size
    // 100).
    let hci_reset_cmd = Bytes::from(vec![0x01, 0x03, 0x0c, 0x00]);
    for _ in 0..100 {
        world.when_packet_sent("A", hci_reset_cmd.clone()).await;
    }

    // 3. Verify that all 100 responses are received.
    // If the buffer size was 10, many of these would have been dropped!
    for _ in 0..100 {
        let _packet = world.receive_packet("A").await;
    }
}
