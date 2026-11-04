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

use netsim_model::{ChipError, ChipId, PacketType};
use netsim_packets::{HciCommand, HciCommandHeader};
use rootcanal::{Rootcanal, error::Error as RadioError};
use zerocopy::{Immutable, IntoBytes, KnownLayout};

/// Send an HCI command to the controller.
pub(crate) fn send_hci_command<T: HciCommand + IntoBytes + Immutable + KnownLayout>(
    rootcanal: &Rootcanal,
    chip_id: ChipId,
    payload: T,
) -> Result<(), ChipError> {
    send_hci_command_with_payload(rootcanal, chip_id, payload, &[])
}

/// Send an HCI command with additional trailing payload bytes to the
/// controller.
pub(crate) fn send_hci_command_with_payload<T: HciCommand + IntoBytes + Immutable + KnownLayout>(
    rootcanal: &Rootcanal,
    chip_id: ChipId,
    payload: T,
    trailing: &[u8],
) -> Result<(), ChipError> {
    let total_len = payload.as_bytes().len().saturating_add(trailing.len());
    let parameter_total_length = u8::try_from(total_len).map_err(|err| {
        ChipError::InvalidArguments(Box::from(format!(
            "hci command payload length {total_len} exceeds 255 bytes: {err}"
        )))
    })?;
    let header = HciCommandHeader { op_code: T::OP_CODE, parameter_total_length };
    let h4_packet = std::iter::once(u8::from(PacketType::Command))
        .chain(header.as_bytes().iter().copied())
        .chain(payload.as_bytes().iter().copied())
        .chain(trailing.iter().copied())
        .collect();
    rootcanal.receive_hci(chip_id.into(), h4_packet).to_chip_error()
}

/// A helper trait to simplify error conversion from `RadioError` to
/// `ChipError`.
pub(crate) trait ToChipError<T> {
    fn to_chip_error(self) -> Result<T, ChipError>;
}

impl<T> ToChipError<T> for Result<T, RadioError> {
    fn to_chip_error(self) -> Result<T, ChipError> {
        self.map_err(|e| match e {
            RadioError::DuplicateControllerId(id) => ChipError::ChipExists(id),
            RadioError::AddressInUse(addr) => ChipError::Io(std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                format!("address already in use: {addr}"),
            )),
            RadioError::MalformedAddress(msg) => ChipError::InvalidAddress(msg.into()),
            other => ChipError::Backend(Box::new(other)),
        })
    }
}

#[cfg(test)]
pub(crate) struct DummyCallbacks;

#[cfg(test)]
impl rootcanal::Callbacks for DummyCallbacks {
    fn on_send_ll(&self, _: u32, _: u32, _: &[u8], _: rootcanal::Phy, _: i32) -> Option<i32> {
        None
    }
    fn estimate_distance(&self, _: u32, _: u32) -> u32 {
        100
    }
}

#[cfg(test)]
mod tests {
    use netsim_packets::LeClearAdvertisingSets;
    use rootcanal::controller::Callbacks as ControllerCallbacks;

    use super::*;

    #[test]
    fn test_to_chip_error_mappings() {
        let dup: Result<(), RadioError> = Err(RadioError::DuplicateControllerId(42));
        assert!(matches!(dup.to_chip_error(), Err(ChipError::ChipExists(42))));

        let addr: rootcanal::Address = "00:11:22:33:44:55".parse().unwrap();
        let in_use: Result<(), RadioError> = Err(RadioError::AddressInUse(addr));
        match in_use.to_chip_error() {
            Err(ChipError::Io(io_err)) => {
                assert_eq!(io_err.kind(), std::io::ErrorKind::AlreadyExists);
            }
            other => panic!("Expected ChipError::Io(AlreadyExists), got {other:?}"),
        }

        let bad_addr: Result<(), RadioError> = Err(RadioError::MalformedAddress("bad".into()));
        assert!(matches!(bad_addr.to_chip_error(), Err(ChipError::InvalidAddress(_))));
    }

    struct DummyControllerCallbacks;
    impl ControllerCallbacks for DummyControllerCallbacks {
        fn send_hci(&self, _source_id: rootcanal::controller::Id, _packet: bytes::Bytes) {}
        fn on_receive_ll(
            &self,
            _source_id: rootcanal::controller::Id,
            _packet: &[u8],
            _phy: rootcanal::types::Phy,
            _tx_power: i32,
        ) {
        }
        fn invalid_packet_received(
            &self,
            _source_id: rootcanal::controller::Id,
            _reason: std::ffi::c_int,
            _message: &str,
            _data: &[u8],
        ) {
        }
    }

    #[test]
    fn test_send_hci_command_with_payload_length_bounds() {
        let rootcanal = Rootcanal::new(Box::new(DummyCallbacks), false, Box::new(|_, _, _, _| {}));
        let chip_id = ChipId(1);
        let address = "00:11:22:33:44:55".parse().unwrap();
        rootcanal
            .new_controller(chip_id.0, address, Box::new(DummyControllerCallbacks), None)
            .unwrap();

        // LeClearAdvertisingSets has empty payload (0 bytes).
        let trailing_255 = [0u8; 255];
        let result_255 = send_hci_command_with_payload(
            &rootcanal,
            chip_id,
            LeClearAdvertisingSets,
            &trailing_255,
        );
        assert!(result_255.is_ok(), "255 bytes payload should succeed, got: {:?}", result_255);

        let trailing_256 = [0u8; 256];
        let result_256 = send_hci_command_with_payload(
            &rootcanal,
            chip_id,
            LeClearAdvertisingSets,
            &trailing_256,
        );
        assert!(
            matches!(result_256, Err(ChipError::InvalidArguments(_))),
            "256 bytes payload should return InvalidArguments, got: {:?}",
            result_256
        );
    }
}
