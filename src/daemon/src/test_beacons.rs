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

use device_actor::DeviceClient;
use device_api::{ChipCreateVariant, DeviceChipCreate, DeviceCreate};
use netsim_model::{AdvertiseData, AdvertiseSettings, BleBeacon, Interval, Pose};
use tracing::warn;

const DEFAULT_BEACON_INTERVAL_MS: u64 = 1000;

/// Creates test beacons for dev mode.
pub(crate) async fn create_test_beacons(device_client: &DeviceClient) {
    spawn_beacon(1, DEFAULT_BEACON_INTERVAL_MS, device_client).await;
    spawn_beacon(2, DEFAULT_BEACON_INTERVAL_MS, device_client).await;
}

async fn spawn_beacon(idx: u32, interval: u64, device_client: &DeviceClient) {
    let beacon = BleBeacon {
        address: format!("be:ac:01:be:ef:{idx:02x}"),
        settings: Some(AdvertiseSettings {
            interval: Some(Interval::Milliseconds(interval)),
            scannable: true,
            ..Default::default()
        }),
        adv_data: Some(AdvertiseData { include_device_name: true, ..Default::default() }),
        scan_response: Some(AdvertiseData {
            manufacturer_data: vec![1u8, 2, 3, 4],
            ..Default::default()
        }),
    };

    let device_config = device_api::DeviceConfig {
        name: format!("gDevice-beacon-{idx}"),
        visible: true,
        pose: Pose::default(),
        builtin: true,
        device_info: None,
    };

    let device_create = DeviceCreate {
        device_config,
        chip: DeviceChipCreate {
            name: format!("gDevice-bt-beacon-chip-{idx}"),
            manufacturer: "Netsim".to_string(),
            product_name: "Netsim".to_string(),
            chip: ChipCreateVariant::Beacon(beacon),
        },
    };

    if let Err(error) = device_client.create_device(device_create).await {
        warn!(%error, "Failed to create beacon device {idx}");
    }
}
