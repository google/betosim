// Copyright (C) 2025 The Android Open Source Project
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

//! Device Errors
//!
//! This module defines the error types specific to the Device actor and client.
//! These errors cover issues arising from actor communication, invalid
//! arguments, and other device-specific failure modes.

use device_api::DeviceId;
use netsim_model::{ChipKind, ClientError};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum DeviceError {
    #[error("Device not found: {0}")]
    NotFound(String),
    #[error("Device not found (explicit): {0}")]
    DeviceNotFound(String),
    #[error("Device already exists: {0}")]
    AlreadyExists(String),
    #[error("Invalid input: {0}")]
    InvalidInput(String),
    #[error("Chip {0} not found in device {1}")]
    ChipNotFound(String, String),
    #[error("Multiple ambiguous matches were found with chip name {0}")]
    AmbiguousMatch(String),
    #[error("No chip name is supplied but multiple chips of chip kind {kind:?} exist.")]
    AmbiguousEmptyMatch { kind: ChipKind },
    #[error("Duplicate chip name '{name}' for kind {kind:?}")]
    DuplicateChip { kind: ChipKind, name: String },
    #[error("Mismatched chip kind: expected {expected:?}, got {actual:?}")]
    MismatchedChipKind { expected: ChipKind, actual: ChipKind },
    #[error("Actor communication error: {0}")]
    ActorCommunicationError(#[from] ClientError),
    #[error("Chip kind not supported: {0}")]
    ChipKindNotSupported(String),
    #[error(
        "Multiple errors during reset: {device_delete_errors:?} {chip_reset_errors:?} {link_client_error:?}"
    )]
    ResetErrors {
        device_delete_errors: Vec<(DeviceId, Self)>,
        chip_reset_errors: Vec<(u32, ClientError)>,
        link_client_error: Option<ClientError>,
    },
}

impl DeviceError {
    pub fn downcast_from<'a>(err: &'a (dyn std::error::Error + 'static)) -> Option<&'a Self> {
        let mut curr = Some(err);
        while let Some(e) = curr {
            if let Some(dev_err) = e.downcast_ref::<Self>() {
                return Some(dev_err);
            }
            curr = e.source();
        }
        None
    }
}
