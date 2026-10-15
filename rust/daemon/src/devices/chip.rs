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

/// A `Chip` is a generic struct that wraps a radio specific
/// WirelessChip.` The Chip layer provides for common operations and
/// data.
///
/// The emulated chip facade is a library that implements the
/// controller protocol.
///
use crate::wireless::WirelessChipImpl;
use netsim_proto::common::ChipKind as ProtoChipKind;
use netsim_proto::model::Chip as ProtoChip;
use netsim_proto::stats::NetsimRadioStats as ProtoRadioStats;
use protobuf::EnumOrUnknown;
use std::collections::HashMap;
use std::fmt;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, OnceLock, RwLock};
use std::time::Instant;

use super::device::DeviceIdentifier;

#[derive(Clone, Copy, Default, Eq, Hash, Ord, PartialOrd, PartialEq)]
pub struct ChipIdentifier(pub u32);
#[derive(Clone, Copy, Default, Eq, Hash, Ord, PartialOrd, PartialEq)]
pub struct FacadeIdentifier(pub u32);

impl fmt::Display for ChipIdentifier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl fmt::Debug for ChipIdentifier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl fmt::Display for FacadeIdentifier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl fmt::Debug for FacadeIdentifier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

const INITIAL_CHIP_ID: u32 = 1000;

struct ChipManager {
    ids: AtomicU32,
    chips: RwLock<HashMap<ChipIdentifier, Arc<Chip>>>,
}

static CHIP_MANAGER: OnceLock<ChipManager> = OnceLock::new();

fn get_chip_manager() -> &'static ChipManager {
    CHIP_MANAGER.get_or_init(|| ChipManager::new(INITIAL_CHIP_ID))
}

pub struct CreateParams {
    pub kind: ProtoChipKind,
    pub address: String,
    pub name: Option<String>,
    pub manufacturer: String,
    pub product_name: String,
}

/// Chip contains the common information for each Chip/Controller.
/// Radio-specific information is contained in the wireless_chip.
pub struct Chip {
    pub id: ChipIdentifier,
    pub device_id: DeviceIdentifier,
    pub wireless_chip: WirelessChipImpl,
    pub kind: ProtoChipKind,
    #[allow(dead_code)]
    pub address: String,
    pub name: String,
    // TODO: may not be necessary
    #[allow(dead_code)]
    pub device_name: String,
    // These are patchable
    pub manufacturer: RwLock<String>,
    pub product_name: RwLock<String>,
    pub start: Instant,
}

impl ChipManager {
    fn new(start: u32) -> Self {
        ChipManager { ids: AtomicU32::new(start), chips: RwLock::new(HashMap::new()) }
    }

    fn next_id(&self) -> ChipIdentifier {
        ChipIdentifier(self.ids.fetch_add(1, Ordering::SeqCst))
    }
}

impl Chip {
    fn new(
        id: ChipIdentifier,
        device_id: DeviceIdentifier,
        device_name: &str,
        create_params: &CreateParams,
        wireless_chip: WirelessChipImpl,
    ) -> Self {
        Self {
            id,
            device_id,
            wireless_chip,
            kind: create_params.kind,
            address: create_params.address.clone(),
            name: create_params.name.clone().unwrap_or(format!("chip-{}", id.0)),
            device_name: device_name.to_string(),
            manufacturer: RwLock::new(create_params.manufacturer.clone()),
            product_name: RwLock::new(create_params.product_name.clone()),
            start: Instant::now(),
        }
    }

    // Get the stats protobuf for a chip controller instance.
    //
    // This currently wraps the chip "get" facade method because the
    // counts are phy level. We need a vec since Bluetooth reports
    // stats for BLE and CLASSIC.
    pub fn get_stats(&self) -> Vec<ProtoRadioStats> {
        self.wireless_chip.get_stats(self.start.elapsed().as_secs())
    }

    /// Create the model protobuf
    pub fn get(&self) -> Result<ProtoChip, String> {
        let mut proto_chip = self.wireless_chip.get();
        proto_chip.kind = EnumOrUnknown::new(self.kind);
        proto_chip.id = self.id.0;
        proto_chip.name.clone_from(&self.name);
        proto_chip.manufacturer.clone_from(&self.manufacturer.read().unwrap());
        proto_chip.product_name.clone_from(&self.product_name.read().unwrap());
        Ok(proto_chip)
    }

    /// Patch processing for the chip. Validate and move state from the patch
    /// into the chip changing the ChipFacade as needed.
    pub fn patch(&self, patch: &ProtoChip) -> Result<(), String> {
        if !patch.manufacturer.is_empty() {
            self.manufacturer.write().unwrap().clone_from(&patch.manufacturer);
        }
        if !patch.product_name.is_empty() {
            self.product_name.write().unwrap().clone_from(&patch.product_name);
        }
        self.wireless_chip.patch(patch);
        Ok(())
    }

    pub fn reset(&self) -> Result<(), String> {
        self.wireless_chip.reset();
        Ok(())
    }
}

/// Obtains a Chip with given chip_id
pub fn get_chip(chip_id: &ChipIdentifier) -> Option<Arc<Chip>> {
    get_chip_manager().get_chip(chip_id)
}

/// Remove a Chip with given chip_id
pub fn remove_chip(chip_id: &ChipIdentifier) -> Option<Arc<Chip>> {
    get_chip_manager().remove_chip(chip_id)
}

pub fn next_id() -> ChipIdentifier {
    get_chip_manager().next_id()
}

/// Allocates a new chip.
pub fn new(
    id: ChipIdentifier,
    device_id: DeviceIdentifier,
    device_name: &str,
    create_params: &CreateParams,
    wireless_chip: WirelessChipImpl,
) -> Result<Arc<Chip>, String> {
    get_chip_manager().new_chip(id, device_id, device_name, create_params, wireless_chip)
}

impl ChipManager {
    fn new_chip(
        &self,
        id: ChipIdentifier,
        device_id: DeviceIdentifier,
        device_name: &str,
        create_params: &CreateParams,
        wireless_chip: WirelessChipImpl,
    ) -> Result<Arc<Chip>, String> {
        let chip = Arc::new(Chip::new(id, device_id, device_name, create_params, wireless_chip));
        self.chips.write().unwrap().insert(id, Arc::clone(&chip));
        Ok(chip)
    }

    fn get_chip(&self, chip_id: &ChipIdentifier) -> Option<Arc<Chip>> {
        self.chips.read().unwrap().get(chip_id).cloned()
    }

    fn remove_chip(&self, chip_id: &ChipIdentifier) -> Option<Arc<Chip>> {
        self.chips.write().unwrap().remove(chip_id)
    }
}

#[cfg(test)]
mod tests {
    use netsim_proto::stats::netsim_radio_stats;

    use crate::wireless::mocked;

    use super::*;

    const DEVICE_ID: DeviceIdentifier = DeviceIdentifier(0);
    const CHIP_ID: ChipIdentifier = ChipIdentifier(1000);
    const DEVICE_NAME: &str = "device";
    const CHIP_KIND: ProtoChipKind = ProtoChipKind::UNSPECIFIED;
    const ADDRESS: &str = "address";
    const MANUFACTURER: &str = "manufacturer";
    const PRODUCT_NAME: &str = "product_name";

    impl ChipManager {
        fn new_test_chip(&self, wireless_chip: WirelessChipImpl) -> Arc<Chip> {
            let create_params = CreateParams {
                kind: CHIP_KIND,
                address: ADDRESS.to_string(),
                name: None,
                manufacturer: MANUFACTURER.to_string(),
                product_name: PRODUCT_NAME.to_string(),
            };
            self.new_chip(CHIP_ID, DEVICE_ID, DEVICE_NAME, &create_params, wireless_chip).unwrap()
        }
    }

    #[test]
    fn test_new_and_get_with_singleton() {
        let mocked_adaptor = mocked::add_chip(
            &mocked::CreateParams { chip_kind: ProtoChipKind::UNSPECIFIED },
            ChipIdentifier(0),
        );
        let chip_manager = ChipManager::new(INITIAL_CHIP_ID);
        let chip = chip_manager.new_test_chip(mocked_adaptor);

        // Check if the Chip has been successfully inserted
        let chip_id = chip.id;
        assert!(chip_manager.chips.read().unwrap().contains_key(&chip_id));

        // Check if the chip_manager can successfully fetch the chip
        let chip_result = chip_manager.get_chip(&chip_id);
        assert!(chip_result.is_some());

        // Check if the fields are correctly populated
        let chip = chip_result.unwrap();
        assert_eq!(chip.device_id, DEVICE_ID);
        assert_eq!(chip.device_name, DEVICE_NAME);
        assert_eq!(chip.kind, CHIP_KIND);
        assert_eq!(chip.address, ADDRESS);
        assert_eq!(chip.name, format!("chip-{chip_id}"));
        assert_eq!(chip.manufacturer.read().unwrap().to_string(), MANUFACTURER);
        assert_eq!(chip.product_name.read().unwrap().to_string(), PRODUCT_NAME);
    }

    #[test]
    fn test_chip_get_stats() {
        // When wireless_chip is constructed
        let mocked_adaptor = mocked::add_chip(
            &mocked::CreateParams { chip_kind: ProtoChipKind::UNSPECIFIED },
            ChipIdentifier(0),
        );
        let chip_manager = ChipManager::new(INITIAL_CHIP_ID);

        let chip = chip_manager.new_test_chip(mocked_adaptor);
        assert_eq!(netsim_radio_stats::Kind::UNSPECIFIED, chip.get_stats().first().unwrap().kind());
    }

    #[test]
    fn test_chip_get() {
        let mocked_adaptor = mocked::add_chip(
            &mocked::CreateParams { chip_kind: ProtoChipKind::UNSPECIFIED },
            ChipIdentifier(0),
        );
        let chip_manager = ChipManager::new(INITIAL_CHIP_ID);
        let chip = chip_manager.new_test_chip(mocked_adaptor);

        // Obtain actual chip.get()
        let actual = chip.get().unwrap();

        // Construct expected ProtoChip
        let mut expected = chip.wireless_chip.get();
        expected.kind = EnumOrUnknown::new(chip.kind);
        expected.id = chip.id.0;
        expected.name.clone_from(&chip.name);
        expected.manufacturer.clone_from(&chip.manufacturer.read().unwrap());
        expected.product_name.clone_from(&chip.product_name.read().unwrap());

        // Compare
        assert_eq!(expected, actual);
    }

    #[test]
    fn test_chip_patch() {
        let mocked_adaptor = mocked::add_chip(
            &mocked::CreateParams { chip_kind: ProtoChipKind::UNSPECIFIED },
            ChipIdentifier(0),
        );
        let chip_manager = ChipManager::new(INITIAL_CHIP_ID);
        let chip = chip_manager.new_test_chip(mocked_adaptor);

        // Construct the patch body for modifying manufacturer and product_name
        let mut patch_body = ProtoChip::new();
        patch_body.manufacturer = "patched_manufacturer".to_string();
        patch_body.product_name = "patched_product_name".to_string();

        // Perform Patch
        assert!(chip.patch(&patch_body).is_ok());

        // Check if fields of chip has been patched
        assert_eq!(patch_body.manufacturer, chip.manufacturer.read().unwrap().to_string());
        assert_eq!(patch_body.product_name, chip.product_name.read().unwrap().to_string());
    }

    // TODO (b/309529194)
    // Implement wireless/mocked.rs to test wireless_chip level of patch and resets.
}
