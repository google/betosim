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

// Device.rs

use crate::devices::chip;
use crate::devices::chip::Chip;
use crate::devices::chip::ChipIdentifier;
use crate::devices::devices_handler::PoseManager;
use crate::wireless::WirelessChipImpl;
use netsim_proto::common::ChipKind as ProtoChipKind;
use netsim_proto::frontend::patch_device_request::PatchDeviceFields as ProtoPatchDeviceFields;
use netsim_proto::model::Device as ProtoDevice;
use netsim_proto::stats::NetsimRadioStats as ProtoRadioStats;
use std::collections::BTreeMap;
use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};

#[derive(Clone, Copy, Default, Eq, Hash, Ord, PartialOrd, PartialEq)]
pub struct DeviceIdentifier(pub u32);

impl fmt::Display for DeviceIdentifier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl fmt::Debug for DeviceIdentifier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

pub struct Device {
    pub id: DeviceIdentifier,
    pub guid: String,
    pub name: String,
    pub visible: AtomicBool,
    pub chips: RwLock<BTreeMap<ChipIdentifier, Arc<Chip>>>,
    pub builtin: bool,
}
impl Device {
    pub fn new(
        id: DeviceIdentifier,
        guid: impl Into<String>,
        name: impl Into<String>,
        builtin: bool,
    ) -> Self {
        Device {
            id,
            guid: guid.into(),
            name: name.into(),
            visible: AtomicBool::new(true),
            chips: RwLock::new(BTreeMap::new()),
            builtin,
        }
    }
}

#[derive(Debug, Clone)]
pub struct AddChipResult {
    pub device_id: DeviceIdentifier,
    pub chip_id: ChipIdentifier,
}

impl Device {
    pub fn get(&self, pose_manager: Arc<PoseManager>) -> Result<ProtoDevice, String> {
        let mut device = ProtoDevice::new();
        device.id = self.id.0;
        device.name.clone_from(&self.name);
        device.visible = Some(self.visible.load(Ordering::SeqCst));
        device.position = protobuf::MessageField::from(pose_manager.get_position(&self.id));
        device.orientation = protobuf::MessageField::from(pose_manager.get_orientation(&self.id));
        for chip in self.chips.read().unwrap().values() {
            device.chips.push(chip.get()?);
        }
        Ok(device)
    }

    /// Patch a device and its chips.
    pub fn patch(
        &self,
        patch: &ProtoPatchDeviceFields,
        pose_manager: Arc<PoseManager>,
    ) -> Result<(), String> {
        if let Some(visible) = patch.visible {
            self.visible.store(visible, Ordering::SeqCst);
        }
        if patch.position.is_some() {
            pose_manager.set_position(self.id, &patch.position);
        }
        if patch.orientation.is_some() {
            pose_manager.set_orientation(self.id, &patch.orientation);
        }
        // iterate over patched ProtoChip entries and patch matching chip
        for patch_chip in patch.chips.iter() {
            let mut patch_chip_kind = patch_chip.kind.enum_value_or_default();
            // Check if chip is given when kind is not given.
            // TODO: Fix patch device request body in CLI to include ChipKind, and remove if block below.
            if patch_chip_kind == ProtoChipKind::UNSPECIFIED {
                if patch_chip.has_bt() {
                    patch_chip_kind = ProtoChipKind::BLUETOOTH;
                } else if patch_chip.has_ble_beacon() {
                    patch_chip_kind = ProtoChipKind::BLUETOOTH_BEACON;
                } else if patch_chip.has_wifi() {
                    patch_chip_kind = ProtoChipKind::WIFI;
                } else if patch_chip.has_uwb() {
                    patch_chip_kind = ProtoChipKind::UWB;
                } else {
                    break;
                }
            }
            let patch_chip_name = &patch_chip.name;
            // Find the matching chip and patch the proto chip
            let target = self.match_target_chip(patch_chip_kind, patch_chip_name)?;
            match target {
                Some(chip) => chip.patch(patch_chip)?,
                None => {
                    return Err(format!(
                        "Chip {} not found in device {}",
                        patch_chip_name, self.name
                    ));
                }
            }
        }
        Ok(())
    }

    fn match_target_chip(
        &self,
        patch_chip_kind: ProtoChipKind,
        patch_chip_name: &str,
    ) -> Result<Option<Arc<Chip>>, String> {
        let mut multiple_matches = false;
        let mut target: Option<Arc<Chip>> = None;
        for chip in self.chips.read().unwrap().values() {
            // Check for specified chip kind and matching chip name
            if chip.kind == patch_chip_kind && chip.name.contains(patch_chip_name) {
                // Check for exact match
                if chip.name == patch_chip_name {
                    multiple_matches = false;
                    target = Some(Arc::clone(chip));
                    break;
                }
                // Check for ambiguous match
                if target.is_none() {
                    target = Some(Arc::clone(chip));
                } else {
                    // Return if no chip name is supplied but multiple chips of specified kind exist
                    if patch_chip_name.is_empty() {
                        return Err(format!(
                            "No chip name is supplied but multiple chips of chip kind {:?} exist.",
                            chip.kind
                        ));
                    }
                    // Multiple matches were found - continue to look for possible exact match
                    multiple_matches = true;
                }
            }
        }
        if multiple_matches {
            return Err(format!(
                "Multiple ambiguous matches were found with chip name {patch_chip_name}"
            ));
        }
        Ok(target)
    }

    /// Remove a chip from a device.
    pub fn remove_chip(&self, chip_id: &ChipIdentifier) -> Result<Vec<ProtoRadioStats>, String> {
        let radio_stats = self
            .chips
            .read()
            .unwrap()
            .get(chip_id)
            .ok_or(format!("RemoveChip chip id {chip_id} not found"))?
            .get_stats();
        // Chip and emulated chip will be dropped
        self.chips.write().unwrap().remove(chip_id);
        chip::remove_chip(chip_id);
        Ok(radio_stats)
    }

    pub fn add_chip(
        &mut self,
        chip_create_params: &chip::CreateParams,
        chip_id: ChipIdentifier,
        wireless_chip: WirelessChipImpl,
    ) -> Result<(DeviceIdentifier, ChipIdentifier), String> {
        for chip in self.chips.read().unwrap().values() {
            if chip.kind == chip_create_params.kind
                && chip_create_params.name.clone().is_some_and(|name| name == chip.name)
            {
                return Err(format!("Device::AddChip - duplicate at id {}, skipping.", chip.id));
            }
        }
        let device_id = self.id;
        let chip = chip::new(chip_id, device_id, &self.name, chip_create_params, wireless_chip)?;
        self.chips.write().unwrap().insert(chip_id, chip);

        Ok((device_id, chip_id))
    }

    /// Reset a device to its default state.
    pub fn reset(&self) -> Result<(), String> {
        self.visible.store(true, Ordering::SeqCst);
        for chip in self.chips.read().unwrap().values() {
            chip.reset()?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wireless::mocked;
    use std::sync::atomic::{AtomicU32, Ordering};
    static PATCH_CHIP_KIND: ProtoChipKind = ProtoChipKind::BLUETOOTH;
    static TEST_DEVICE_NAME: &str = "test_device";
    static TEST_CHIP_NAME_1: &str = "test-bt-chip-1";
    static TEST_CHIP_NAME_2: &str = "test-bt-chip-2";
    static IDS: AtomicU32 = AtomicU32::new(1000);

    fn create_test_device() -> Result<Device, String> {
        let mut device = Device::new(DeviceIdentifier(0), "0", TEST_DEVICE_NAME, false);
        let chip_id_1 = ChipIdentifier(IDS.fetch_add(1, Ordering::SeqCst));
        let chip_id_2 = ChipIdentifier(IDS.fetch_add(1, Ordering::SeqCst));
        device.add_chip(
            &chip::CreateParams {
                kind: ProtoChipKind::BLUETOOTH,
                address: "".to_string(),
                name: Some(TEST_CHIP_NAME_1.to_string()),
                manufacturer: "test_manufacturer".to_string(),
                product_name: "test_product_name".to_string(),
            },
            chip_id_1,
            mocked::add_chip(
                &mocked::CreateParams { chip_kind: ProtoChipKind::UNSPECIFIED },
                chip_id_1,
            ),
        )?;
        device.add_chip(
            &chip::CreateParams {
                kind: ProtoChipKind::BLUETOOTH,
                address: "".to_string(),
                name: Some(TEST_CHIP_NAME_2.to_string()),
                manufacturer: "test_manufacturer".to_string(),
                product_name: "test_product_name".to_string(),
            },
            chip_id_2,
            mocked::add_chip(
                &mocked::CreateParams { chip_kind: ProtoChipKind::UNSPECIFIED },
                chip_id_1,
            ),
        )?;
        Ok(device)
    }

    #[ignore = "TODO: include thread_id in names and ids"]
    #[test]
    fn test_exact_target_match() {
        let device = create_test_device().unwrap();
        let result = device.match_target_chip(PATCH_CHIP_KIND, TEST_CHIP_NAME_1);
        assert!(result.is_ok());
        let target = result.unwrap();
        assert!(target.is_some());
        assert_eq!(target.unwrap().name, TEST_CHIP_NAME_1);
        assert_eq!(device.name, TEST_DEVICE_NAME);
    }

    #[ignore = "TODO: include thread_id in names and ids"]
    #[test]
    fn test_substring_target_match() {
        let device = create_test_device().unwrap();
        let result = device.match_target_chip(PATCH_CHIP_KIND, "chip-1");
        assert!(result.is_ok());
        let target = result.unwrap();
        assert!(target.is_some());
        assert_eq!(target.unwrap().name, TEST_CHIP_NAME_1);
        assert_eq!(device.name, TEST_DEVICE_NAME);
    }

    #[ignore = "TODO: include thread_id in names and ids"]
    #[test]
    fn test_ambiguous_target_match() {
        let device = create_test_device().unwrap();
        let result = device.match_target_chip(PATCH_CHIP_KIND, "chip");
        assert!(result.is_err());
        assert_eq!(
            result.err(),
            Some("Multiple ambiguous matches were found with chip name chip".to_string())
        );
    }

    #[ignore = "TODO: include thread_id in names and ids"]
    #[test]
    fn test_ambiguous_empty_target_match() {
        let device = create_test_device().unwrap();
        let result = device.match_target_chip(PATCH_CHIP_KIND, "");
        assert!(result.is_err());
        assert_eq!(
            result.err(),
            Some(format!(
                "No chip name is supplied but multiple chips of chip kind {PATCH_CHIP_KIND:?} exist."
            ))
        );
    }

    #[ignore = "TODO: include thread_id in names and ids"]
    #[test]
    fn test_no_target_match() {
        let device = create_test_device().unwrap();
        let invalid_chip_name = "invalid-chip";
        let result = device.match_target_chip(PATCH_CHIP_KIND, invalid_chip_name);
        assert!(result.is_ok());
        let target = result.unwrap();
        assert!(target.is_none());
    }
}
