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

//! This module defines the [`enum@Error`] enum, which represents all possible
//! errors that can occur in this crate.

use std::{io, num::ParseIntError};

use thiserror::Error;

/// Represents all possible errors that can occur in this crate.
#[derive(Error, Debug)]
pub enum Error {
    /// An I/O error.
    #[error(transparent)]
    IoError(#[from] io::Error),

    /// The controller ID is already in use.
    #[error("duplicate controller id: {0}")]
    DuplicateControllerId(u32),

    /// The controller ID is not found.
    #[error("controller not found: {0}")]
    ControllerNotFound(u32),

    /// The address is already in use.
    #[error("address already in use: {0}")]
    AddressInUse(crate::types::Address),

    /// The address is malformed.
    #[error("malformed address: {0}")]
    MalformedAddress(String),

    /// The class of device is malformed.
    #[error("malformed class of device: {0}")]
    MalformedClassOfDevice(String),

    /// The LE features are malformed.
    #[error("malformed LE features: {0}")]
    MalformedLeFeatures(String),

    /// Failed to set controller properties.
    #[error("failed to set controller properties")]
    SetPropertiesFailed,

    /// An error parsing an integer.
    #[error(transparent)]
    ParseIntError(#[from] ParseIntError),
}

/// A type alias for `Result` where the error type is this crate's `Error`.
pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_io_error_chaining() {
        let inner_error = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
        let outer_error: Error = inner_error.into();

        assert!(outer_error.to_string().contains("file not found"));
    }
}
