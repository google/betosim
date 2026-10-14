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

#![warn(missing_docs)]

//! # Bluetooth Controller
//!
//! This crate provides a simplified interface for interacting with a Rootcanal
//! emulated Bluetooth controller. It is designed to be used in testing and
//! simulation scenarios.
//!
//! The main entry point is the [`Rootcanal`] struct, which represents the
//! Bluetooth subsystem. It provides methods for creating and managing
//! emulated Bluetooth controllers.
// The [`rootcanal`] module contains the core Rootcanal implementation.
//!
//! The [`error`] module defines the error types used in this crate.
//!
//! The `ffi` module contains the low-level bindings to the C++ FFI. This
//! module is not intended to be used directly by consumers of this crate.
//!
//! ## Example

pub mod controller;
pub mod error;
mod ffi;
mod rootcanal;
pub mod types;

pub use controller::{Id, Stats};
pub use rootcanal::{Callbacks, Rootcanal};
pub use types::{Address, Phy};

#[cfg(test)]
mod tests {
    use std::{ffi::c_int, str::FromStr};

    use bytes::Bytes;

    use super::*;
    use crate::{controller::Callbacks as ControllerCallbacks, types::Address};

    struct MockRootcanalCallbacks;
    impl rootcanal::Callbacks for MockRootcanalCallbacks {
        fn on_send_ll(
            &self,
            _source_id: u32,
            _destination_id: u32,
            _packet: &[u8],
            _phy: Phy,
            tx_power: i32,
        ) -> Option<i32> {
            Some(tx_power)
        }

        fn estimate_distance(&self, _source_id: u32, _destination_id: u32) -> u32 {
            0
        }
    }

    struct MockControllerCallbacks;
    impl ControllerCallbacks for MockControllerCallbacks {
        fn send_hci(&self, _source_id: Id, _data: Bytes) {}
        fn on_receive_ll(&self, _sender_id: Id, _packet: &[u8], _phy: Phy, _rssi: i32) {}
        fn invalid_packet_received(
            &self,
            _source_id: Id,
            _reason: c_int,
            _message: &str,
            _data: &[u8],
        ) {
        }
    }

    // It ensures that the controller count is correctly managed when a new
    // controller is added and subsequently removed.
    #[test]
    fn test_create_and_delete_controller() {
        let rootcanal =
            Rootcanal::new(Box::new(MockRootcanalCallbacks), false, Box::new(|_, _, _, _| {}));
        let address = Address::from_str("01:02:03:04:05:06").unwrap();
        let id = 1;
        rootcanal.new_controller(id, address, Box::new(MockControllerCallbacks), None).unwrap();

        assert_eq!(rootcanal.len(), 1);

        rootcanal.remove_controller(id).unwrap();
        assert!(rootcanal.is_empty());
    }
}
