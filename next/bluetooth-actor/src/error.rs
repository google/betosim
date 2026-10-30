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

use netsim_model::{ChipError, ChipId};
use thiserror::Error;

/// Errors that can occur within the Bluetooth actor.
#[derive(Error, Debug)]
pub enum BluetoothError {
    /// Errors related to general chip operations.
    #[error("Chip error: {0}")]
    Chip(#[from] ChipError),

    /// Errors originating from the Rootcanal simulation.
    #[error("Rootcanal error: {0}")]
    Rootcanal(Box<dyn std::error::Error + Send + Sync>),
}

impl BluetoothError {
    /// Helper to create an InvalidArguments error.
    pub(crate) fn invalid_arg(msg: impl Into<String>) -> Self {
        Self::Chip(ChipError::InvalidArguments(Box::from(msg.into())))
    }

    /// Helper to create a ChipNotFound error.
    pub(crate) fn not_found(id: ChipId) -> Self {
        Self::Chip(ChipError::ChipNotFound(id))
    }
}
