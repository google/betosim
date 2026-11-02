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

use netsim_model::{ChipId, NetsimRadioStats};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
/// Actions that can be performed on the UWB actor.
pub enum UwbAction {
    Reset {
        /// The ID of the chip to reset.
        id: ChipId,
    },
    /// Retrieves statistics for all chips.
    GetStatistics,
    /// Starts ranging for the given chip and session.
    #[cfg(any(test, feature = "testing"))]
    StartRanging {
        /// The ID of the chip to start ranging on.
        id: ChipId,
        /// The ID of the session to start ranging for.
        session_id: u32,
    },
    /// Stops ranging for the given chip and session.
    #[cfg(any(test, feature = "testing"))]
    StopRanging {
        /// The ID of the chip to stop ranging on.
        id: ChipId,
        /// The ID of the session to stop ranging for.
        session_id: u32,
    },
    /// Retrieves global statistics.
    GetGlobalStats,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// The result of a UWB action.
pub enum UwbActionResult {
    /// The action succeeded with no return value.
    Success,
    /// The action returned statistics.
    Statistics(Box<[NetsimRadioStats]>),
    /// The action returned the updated chip state.
    Chip(Box<netsim_model::Chip>),
    /// The action returned global statistics bytes.
    GlobalStats(Vec<u8>),
}
