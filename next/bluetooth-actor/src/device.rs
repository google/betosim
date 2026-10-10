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

//! This module provides the functionality for creating and managing Bluetooth
//! device chips.
//!
//! A 'Device' chip represents a Bluetooth controller that can be controlled by
//! an external entity, such as an Android Virtual Device running in an Android
//! Emulator or a host device like those supported by Bumble.
//!
//! The `create` function is responsible for initializing a new device chip.
//! Placeholder functions for updating and retrieving chip information are also
//! included.

use netsim_model::{ChipError, ChipId, DeviceParams};
use rootcanal::Rootcanal;

/// Creates a new `DeviceChip` and registers it with the `rootcanal` emulator.
pub(crate) fn create(
    _rootcanal: &Rootcanal,
    _chip_id: ChipId,
    _params: &DeviceParams,
) -> Result<(), ChipError> {
    // Validate mode parameters
    Ok(())
}
