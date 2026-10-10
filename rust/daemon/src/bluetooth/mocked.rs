// Copyright 2023 The Android Open Source Project
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

use crate::bluetooth::{BeaconChip, get_beacon_chips};
use crate::devices::chip::{ChipIdentifier, FacadeIdentifier};
use crate::devices::device::{AddChipResult, DeviceIdentifier};
use ::protobuf::MessageField;
use log::info;
use netsim_proto::config::Bluetooth as BluetoothConfig;
use netsim_proto::configuration::Controller as RootcanalController;
use netsim_proto::model::chip::{BleBeacon, Bluetooth};
use netsim_proto::model::chip_create::Chip as Builtin;
use netsim_proto::model::{ChipCreate, DeviceCreate};
use std::sync::Mutex;
use std::sync::RwLock;
use std::sync::atomic::{AtomicU32, Ordering};
use std::{collections::HashMap, ptr::null};

static IDS: AtomicU32 = AtomicU32::new(0);

fn next_id() -> FacadeIdentifier {
    FacadeIdentifier(IDS.fetch_add(1, Ordering::SeqCst))
}

// Avoid crossing cxx boundary in tests
pub fn ble_beacon_add(
    device_name: String,
    chip_id: ChipIdentifier,
    chip_proto: &ChipCreate,
) -> Result<FacadeIdentifier, String> {
    let beacon_proto = match &chip_proto.chip {
        Some(Builtin::BleBeacon(beacon_proto)) => beacon_proto,
        _ => return Err(String::from("failed to create ble beacon: unexpected chip type")),
    };

    let beacon_chip = BeaconChip::from_proto(device_name, chip_id, beacon_proto)?;
    if get_beacon_chips().write().unwrap().insert(chip_id, Mutex::new(beacon_chip)).is_some() {
        return Err(format!(
            "failed to create a bluetooth beacon chip with id {chip_id}: chip id already exists.",
        ));
    }

    let facade_id = next_id();

    info!("ble_beacon_add successful with chip_id: {chip_id}");
    Ok(facade_id)
}

pub fn ble_beacon_remove(
    chip_id: ChipIdentifier,
    facade_id: FacadeIdentifier,
) -> Result<(), String> {
    info!("{:?}", get_beacon_chips().read().unwrap().keys());
    if get_beacon_chips().write().unwrap().remove(&chip_id).is_none() {
        Err(format!("failed to delete ble beacon chip: chip with id {chip_id} does not exist"))
    } else {
        Ok(())
    }
}
