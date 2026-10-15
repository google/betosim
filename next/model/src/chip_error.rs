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

//! This module defines the error types for the chip service.

use thiserror::Error;

use crate::chip::{ChipId, ChipKind};

/// The error type for operations within the chip service.
#[derive(Error, Debug)]
pub enum ChipError {
    /// Error indicating that a chip with the given ID was not found.
    #[error("Chip not found error: {0}")]
    ChipNotFound(ChipId),

    /// An error occurred during I/O.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// The operation is not supported.
    #[error("Unsupported operation")]
    Unsupported,

    /// Error indicating that a chip with the given ID already exists.
    #[error("Chip with ID {0} already exists")]
    ChipExists(u32),

    /// A catch-all for errors originating from the simulation backend.
    #[error("Backend error: {0}")]
    Backend(#[source] Box<dyn std::error::Error + Send + Sync>),

    /// Error indicating an internal error.
    #[error("Internal error: {0}")]
    Internal(#[source] Box<dyn std::error::Error + Send + Sync>),

    #[error("Wrong Chip Variant")]
    WrongVariantError,

    /// Invalid address.
    #[error("Invalid address: {0}")]
    InvalidAddress(#[source] Box<dyn std::error::Error + Send + Sync>),

    /// Unexpected chip kind.
    #[error("Unexpected chip kind: expected {expected:?}, got {actual:?}")]
    UnexpectedChipKind { expected: ChipKind, actual: ChipKind },

    /// Invalid arguments.
    #[error("Invalid arguments: {0}")]
    InvalidArguments(#[source] Box<dyn std::error::Error + Send + Sync>),
}

#[cfg(test)]
mod tests {
    use std::error::Error;

    use super::*;

    #[test]
    fn test_chip_not_found_display_and_debug() {
        let err = ChipError::ChipNotFound(ChipId(42));
        assert_eq!(err.to_string(), "Chip not found error: 42");
        assert!(format!("{err:?}").contains("ChipNotFound"));
    }

    #[test]
    fn test_io_error_display_and_conversion() {
        let io_err = std::io::Error::new(std::io::ErrorKind::BrokenPipe, "pipe broken");
        let err: ChipError = io_err.into();
        assert_eq!(err.to_string(), "I/O error: pipe broken");
        assert!(err.source().is_some());
        assert_eq!(err.source().unwrap().to_string(), "pipe broken");
    }

    #[test]
    fn test_unsupported_display_and_debug() {
        let err = ChipError::Unsupported;
        assert_eq!(err.to_string(), "Unsupported operation");
        assert_eq!(format!("{err:?}"), "Unsupported");
    }

    #[test]
    fn test_chip_exists_display() {
        let err = ChipError::ChipExists(100);
        assert_eq!(err.to_string(), "Chip with ID 100 already exists");
    }

    #[test]
    fn test_backend_error_display_and_source() {
        let backend_err = std::io::Error::other("backend failed");
        let err = ChipError::Backend(Box::new(backend_err));
        assert_eq!(err.to_string(), "Backend error: backend failed");
        assert!(err.source().is_some());
        assert_eq!(err.source().unwrap().to_string(), "backend failed");
    }

    #[test]
    fn test_internal_error_display_and_source() {
        let internal_err = std::io::Error::other("internal failure");
        let err = ChipError::Internal(Box::new(internal_err));
        assert_eq!(err.to_string(), "Internal error: internal failure");
        assert!(err.source().is_some());
        assert_eq!(err.source().unwrap().to_string(), "internal failure");
    }

    #[test]
    fn test_wrong_variant_error_display() {
        let err = ChipError::WrongVariantError;
        assert_eq!(err.to_string(), "Wrong Chip Variant");
    }

    #[test]
    fn test_invalid_address_display_and_source() {
        let addr_err = std::io::Error::new(std::io::ErrorKind::InvalidInput, "bad MAC address");
        let err = ChipError::InvalidAddress(Box::new(addr_err));
        assert_eq!(err.to_string(), "Invalid address: bad MAC address");
        assert!(err.source().is_some());
        assert_eq!(err.source().unwrap().to_string(), "bad MAC address");
    }

    #[test]
    fn test_unexpected_chip_kind_display() {
        let err =
            ChipError::UnexpectedChipKind { expected: ChipKind::BLUETOOTH, actual: ChipKind::WIFI };
        assert_eq!(err.to_string(), "Unexpected chip kind: expected BLUETOOTH, got WIFI");
        match err {
            ChipError::UnexpectedChipKind { expected, actual } => {
                assert_eq!(expected, ChipKind::BLUETOOTH);
                assert_eq!(actual, ChipKind::WIFI);
            }
            _ => panic!("Expected UnexpectedChipKind variant"),
        }
    }

    #[test]
    fn test_invalid_arguments_display_and_source() {
        let arg_err = std::io::Error::new(std::io::ErrorKind::InvalidInput, "invalid range value");
        let err = ChipError::InvalidArguments(Box::new(arg_err));
        assert_eq!(err.to_string(), "Invalid arguments: invalid range value");
        assert!(err.source().is_some());
        assert_eq!(err.source().unwrap().to_string(), "invalid range value");
    }

    #[test]
    fn test_chip_error_traits() {
        fn assert_send_sync<T: Send + Sync + 'static>() {}
        assert_send_sync::<ChipError>();
    }
}
