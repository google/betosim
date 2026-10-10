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

//! # Netsim Device API
//!
//! This crate defines the public API for the Netsim device service.
//! It includes the behavioral contract (actions) for the `device-actor`.
//!
//! ## Architecture
//!
//! ### Structure
//!
//! The device service is structured as follows:
//!
//! - **`netsim-model`**: Remains the base layer for data models (e.g.,
//!   `Device`, `Chip`, `Position`). It does not depend on `device-api`.
//! - **`device-api`**: Defines the behavioral contract for the device service.
//!     - Depends on: `netsim-model`
//!     - Contains: `DeviceAction` (enum of supported operations),
//!       `DeviceActionResult`, and re-exports of relevant models from
//!       `netsim-model` for convenience.
//! - **`device-actor`**: Implements the device service.
//!     - Depends on: `device-api`, `netsim-model`, and other service crates.
//!     - Implements the handling logic for `DeviceAction`.
//! - **`netsim-client`**: Provides a client-side wrapper for the device
//!   service.
//!     - Depends on: `device-api`, `netsim-model`.
//! - **Consumer Crates** (e.g., `wifi`, `cell`, `bluetooth`):
//!     - Depend on: `device-api`, `netsim-client` (and `netsim-model` if
//!       needed).
//!     - Do not depend directly on `device-actor` for API definitions.
//!
//! ### Dependency Graph (Simplified)
//!
//! ```text
//! graph TD
//!     ConsumerCrates[Consumer Crates<br>(wifi, cell, bluetooth)] --> NetsimClient[netsim-client]
//!     ConsumerCrates --> DeviceAPI[device-api]
//!     NetsimClient --> DeviceAPI
//!     DeviceActor[device-actor] --> DeviceAPI
//!     DeviceActor --> NetsimModel[netsim-model]
//!     DeviceAPI --> NetsimModel
//!     NetsimClient --> NetsimModel
//! ```
//!
//! ## API Definition
//!
//! The `device-api` crate defines the following key components:
//!
//! ### `DeviceAction`
//!
//! An enum representing the actions that can be performed on the device
//! service. This serves as the primary interface for consumers.
//!
//! ### Re-exports
//!
//! To simplify consumption, `device-api` re-exports commonly used types from
//! `netsim-model`.
//!
//! ## Benefits
//!
//! 1. **Clearer Boundaries**: The separation between API and implementation is
//!    now explicit.
//! 2. **Reduced Coupling**: Consumer crates only depend on the API, not the
//!    implementation.
//! 3. **Improved Build Times**: Changes to the `device-actor` implementation do
//!    not require recompiling consumer crates, as long as the API remains
//!    stable.
//! 4. **Easier Testing**: Mocking the device service is now a matter of
//!    implementing the `DeviceAction` contract.

use std::fmt;

pub use netsim_model::{
    ChipCreateVariant, Device, DeviceAddChip, DeviceChipCreate, DeviceConfig, DeviceCreate,
    DeviceId, DeviceUpdate, Orientation, PoseUpdate, Position,
};
use netsim_model::{ChipId, NetsimRadioStats, PacketSink, PacketStream};
use serde::{Deserialize, Serialize};

// Behavioral Contract

pub enum DeviceAction {
    Reset,
    GetRadioStats,
    NotifyChipRemoved(DeviceId, ChipId),
    AddChip {
        chip_config: Box<DeviceChipCreate>,
        packet_stream: Option<PacketStream>,
        packet_sink: Option<PacketSink>,
    },
    /// Adds a chip to a device identified by its GUID.
    ///
    /// If a device with the given GUID exists, the chip is added to it.
    /// If no such device exists, a new device is created with the provided
    /// configuration, and the chip is added to the new device.
    ///
    /// This operation is atomic regarding device creation, preventing race
    /// conditions when multiple sources try to initialize the same device
    /// simultaneously.
    AddChipByGuid {
        params: Box<DeviceAddChip>,
    },
    /// Deletes an internal device by ID.
    DeleteDevice(DeviceId),
    /// Triggers a persistence of the current statistics to disk.
    /// This is primarily used internally by the actor for periodic saves
    /// and lifecycle events, but can be invoked externally if needed.
    SaveStats,
}

impl fmt::Debug for DeviceAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DeviceAction::Reset => write!(f, "Reset"),
            DeviceAction::GetRadioStats => write!(f, "GetRadioStats"),
            DeviceAction::NotifyChipRemoved(device_id, chip_id) => {
                f.debug_tuple("NotifyChipRemoved").field(device_id).field(chip_id).finish()
            }
            DeviceAction::AddChip { chip_config, .. } => f
                .debug_struct("AddChip")
                .field("chip_config", chip_config)
                .field("packet_stream", &"Option<PacketStream>")
                .field("packet_sink", &"Option<PacketSink>")
                .finish(),
            DeviceAction::AddChipByGuid { params } => {
                f.debug_struct("AddChipByGuid").field("params", params).finish()
            }
            DeviceAction::SaveStats => write!(f, "SaveStats"),
            DeviceAction::DeleteDevice(device_id) => {
                f.debug_tuple("DeleteDevice").field(device_id).finish()
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DeviceActionResult {
    Success,
    Statistics(Vec<NetsimRadioStats>),
    ChipId(ChipId),
    /// Result of an `AddChipByGuid` operation.
    /// Returns both the `DeviceId` (found or created) and the `ChipId` (newly
    /// added).
    AddChipByGuidSuccess {
        device_id: DeviceId,
        chip_id: ChipId,
    },
}
