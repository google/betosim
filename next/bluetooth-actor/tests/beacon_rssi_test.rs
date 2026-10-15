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

use std::time::Duration;

use bluetooth_actor::{REPORT_ADDR_OFFSET, REPORT_NUM_REPORTS_OFFSET, is_le_advertising_report};
use netsim_model::Position;
use netsim_packets::{
    Enable, LeScanType, LeScanningFilterPolicy, LeSetEventMask, LeSetScanEnable,
    LeSetScanParameters, OwnAddressType, Reset, SetEventMask,
};
use tokio;

use crate::world::World;

/// Scenario: Scanner receives RSSI updates from Beacon
///   Given a world with a scanner and a beacon
///   When the scanner enables scanning
///   Then the scanner receives advertising reports with decreasing RSSI as
/// distance increases
#[tokio::test]
async fn test_rssi_updates() -> Result<(), Box<dyn std::error::Error>> {
    let _ = tracing_subscriber::fmt().with_test_writer().try_init();

    // GIVEN a world with a scanner and a beacon
    let mut world = World::new();

    // Create Scanner Device using World helper
    // Name "scanner" will be used to reference it.
    world.given_device("scanner").await;

    // Create Beacon Device at initial position (1, 0, 0)
    let beacon_address = "00:00:00:00:00:02";
    world.given_beacon_with_address("beacon", beacon_address).await;

    let beacon_id = *world.chips.get("beacon").expect("Beacon should exist");

    // Move beacon to initial position
    {
        let mut update = netsim_model::ChipUpdate::default();
        update.pose.position = Some(Position { x: 1.0, y: 0.0, z: 0.0 });
        update.id = Some(beacon_id);
        world.client.0.update(beacon_id, update).await?;
    }

    // WHEN the scanner enables scanning
    world.when_command_sent("scanner", Reset {}).await;
    world.when_command_sent("scanner", SetEventMask { event_mask: u64::MAX.into() }).await;
    world
        .when_command_sent("scanner", LeSetEventMask { le_event_mask: (u8::MAX as u64).into() })
        .await;
    world
        .when_command_sent(
            "scanner",
            LeSetScanParameters {
                le_scan_type: LeScanType::PASSIVE,
                le_scan_interval: 0x0010.into(),
                le_scan_window: 0x0010.into(),
                own_address_type: OwnAddressType::PUBLIC_DEVICE_ADDRESS,
                scanning_filter_policy: LeScanningFilterPolicy::ACCEPT_ALL,
            },
        )
        .await;
    world
        .when_command_sent(
            "scanner",
            LeSetScanEnable {
                le_scan_enable: Enable::ENABLED,
                filter_duplicates: Enable::DISABLED,
            },
        )
        .await;

    // THEN the scanner receives advertising reports with decreasing RSSI as
    // distance increases
    let distances = [1.0, 10.0, 20.0];
    let mut last_rssi = i8::MAX;

    // Derive expected address byte dynamically
    let last_byte_str =
        beacon_address.split(':').last().ok_or("Invalid beacon address format: missing bytes")?;
    let expected_addr_byte = u8::from_str_radix(last_byte_str, 16)?;

    for distance in distances {
        // Move beacon
        {
            let mut update = netsim_model::ChipUpdate::default();
            update.pose.position = Some(Position { x: distance, y: 0.0, z: 0.0 });
            update.id = Some(beacon_id);
            world.client.0.update(beacon_id, update).await?;
        }

        // Wait for pose to propagate and old packets to settle
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Drain stale packets
        if let Some(rx) = world.sinks.get_mut("scanner") {
            while rx.try_recv().is_ok() {}
        }

        // Wait up to 2 seconds for a valid packet
        let start = std::time::Instant::now();
        let mut found_rssi = None;

        while start.elapsed() < Duration::from_secs(2) {
            let packet = world.receive_packet("scanner").await;

            if is_le_advertising_report(&packet) {
                // Strict validation: Check Num Reports
                if packet[REPORT_NUM_REPORTS_OFFSET] < 1 {
                    continue;
                }

                // Check address.
                if packet.len() > REPORT_ADDR_OFFSET
                    && packet[REPORT_ADDR_OFFSET] == expected_addr_byte
                {
                    // Found our beacon!
                    // RSSI is the last byte
                    if let Some(&rssi_byte) = packet.last() {
                        let rssi = rssi_byte as i8;
                        found_rssi = Some(rssi);
                        break;
                    }
                }
            }
        }

        let rssi = found_rssi.ok_or(format!("Failed to find RSSI for distance {}", distance))?;

        assert!(
            rssi < last_rssi,
            "RSSI {} should be less than last RSSI {} at distance {}",
            rssi,
            last_rssi,
            distance
        );
        last_rssi = rssi;
    }
    Ok(())
}
