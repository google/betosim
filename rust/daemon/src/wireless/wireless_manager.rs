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

use crate::{
    devices::chip::ChipIdentifier,
    wireless::WirelessChipImpl,
    wireless::{ble_beacon, mocked},
};

#[cfg(not(test))]
use crate::wireless::{bluetooth, uwb, wifi_chip, wifi_manager};

/// Parameter for each constructor of Emulated Chips
#[allow(clippy::large_enum_variant, dead_code)]
pub enum CreateParam {
    BleBeacon(ble_beacon::CreateParams),
    #[cfg(not(test))]
    Bluetooth(bluetooth::CreateParams),
    #[cfg(not(test))]
    Wifi(wifi_chip::CreateParams),
    #[cfg(not(test))]
    Uwb(uwb::CreateParams),
    Mock(mocked::CreateParams),
}

/// This is called when the transport module receives a new packet stream
/// connection from a virtual device.
pub fn add_chip(create_param: &CreateParam, chip_id: ChipIdentifier) -> WirelessChipImpl {
    // Based on create_param, construct WirelessChip.
    match create_param {
        CreateParam::BleBeacon(params) => ble_beacon::add_chip(params, chip_id),
        #[cfg(not(test))]
        CreateParam::Bluetooth(params) => bluetooth::add_chip(params, chip_id),
        #[cfg(not(test))]
        CreateParam::Wifi(params) => wifi_manager::add_chip(params, chip_id),
        #[cfg(not(test))]
        CreateParam::Uwb(params) => uwb::add_chip(params, chip_id),
        CreateParam::Mock(params) => mocked::add_chip(params, chip_id),
    }
}
