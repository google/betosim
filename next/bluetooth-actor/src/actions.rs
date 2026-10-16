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

use netsim_model::{ChipId, NetsimRadioStats};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
/// Actions that can be performed on the Bluetooth actor.
pub enum BluetoothAction {
    /// Resets the chip with the given ID.
    Reset {
        /// The ID of the chip to reset.
        id: ChipId,
    },
    /// Retrieves statistics for all chips.
    GetStatistics,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// The result of a Bluetooth action.
pub enum BluetoothActionResult {
    /// The action returned statistics.
    Statistics(Box<[NetsimRadioStats]>),
    /// The action returned the updated chip state.
    Chip(Box<netsim_model::Chip>),
}
