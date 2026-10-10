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

use netsim_types::{Chip, ChipInfo, ChipKind, DeviceInfo};

#[test]
fn test_chip_kind_display_and_as_str() {
    assert_eq!(ChipKind::BLUETOOTH.as_str(), "BLUETOOTH");
    assert_eq!(format!("{}", ChipKind::BLUETOOTH), "BLUETOOTH");
    assert_eq!(format!("{}", ChipKind::WIFI), "WIFI");
    assert_eq!(format!("{}", ChipKind::UWB), "UWB");
    assert_eq!(format!("{}", ChipKind::NFC), "NFC");
    assert_eq!(format!("{}", ChipKind::CELLULAR), "CELLULAR");
    assert_eq!(format!("{}", ChipKind::CELLULAR_DATA), "CELLULAR_DATA");
    assert_eq!(format!("{}", ChipKind::ETHERNET), "ETHERNET");
    assert_eq!(format!("{}", ChipKind::UNSPECIFIED), "UNSPECIFIED");
}

#[test]
fn test_chip_kind_predicates() {
    assert!(ChipKind::BLUETOOTH.is_wireless());
    assert!(ChipKind::WIFI.is_wireless());
    assert!(ChipKind::UWB.is_wireless());
    assert!(ChipKind::NFC.is_wireless());
    assert!(!ChipKind::CELLULAR.is_wireless());
    assert!(!ChipKind::CELLULAR_DATA.is_wireless());
    assert!(!ChipKind::ETHERNET.is_wireless());

    assert!(ChipKind::CELLULAR.is_cellular());
    assert!(ChipKind::CELLULAR_DATA.is_cellular());
    assert!(!ChipKind::BLUETOOTH.is_cellular());
    assert!(!ChipKind::WIFI.is_cellular());
    assert!(!ChipKind::ETHERNET.is_cellular());
}

#[test]
fn test_chip_info_display_and_helpers() {
    let chip_info = ChipInfo::new("Pixel 9 Pro", ChipKind::BLUETOOTH);
    assert_eq!(chip_info.chip_kind(), ChipKind::BLUETOOTH);
    assert_eq!(chip_info.device_name(), "Pixel 9 Pro");
    assert_eq!(format!("{chip_info}"), "BLUETOOTH on device Pixel 9 Pro");

    let custom_info = ChipInfo {
        name: "uwb-main".to_string(),
        chip: Some(Chip::new(ChipKind::UWB, "uwb-main")),
        device_info: Some(DeviceInfo::new("TestDevice", "d1")),
    };
    assert_eq!(custom_info.chip_kind(), ChipKind::UWB);
    assert_eq!(custom_info.device_name(), "TestDevice");
    assert_eq!(format!("{custom_info}"), "UWB on device TestDevice");

    let no_chip = ChipInfo { name: "test-device".to_string(), chip: None, device_info: None };
    assert_eq!(no_chip.chip_kind(), ChipKind::UNSPECIFIED);
    assert_eq!(no_chip.device_name(), "Unknown");
    assert_eq!(format!("{no_chip}"), "UNSPECIFIED on device Unknown");
}
