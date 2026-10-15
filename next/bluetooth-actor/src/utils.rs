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

//! This module provides utility functions and traits for the bluetooth crate.
//!
//! It includes helpers for error conversion, such as the `ToChipError` trait,
//! which simplifies the mapping of `rootcanal` emulator errors to the
//! `ChipError` type.

use netsim_model::ChipError;
use rootcanal::error::Error as RadioError;

/// A helper trait to simplify error conversion from `RadioError` to
/// `ChipError`.
pub(crate) trait ToChipError<T> {
    fn to_chip_error(self) -> Result<T, ChipError>;
}

impl<T> ToChipError<T> for Result<T, RadioError> {
    fn to_chip_error(self) -> Result<T, ChipError> {
        self.map_err(|e| ChipError::Backend(Box::new(e)))
    }
}
