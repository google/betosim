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

use zerocopy::{
    FromBytes, Immutable, IntoBytes, KnownLayout, U16, U64, Unaligned, byteorder::LittleEndian,
};

use crate::hci::types::*;

/// HCI OpCodes for Command packets.
#[derive(
    Debug, Copy, Clone, PartialEq, Eq, FromBytes, IntoBytes, Unaligned, Immutable, KnownLayout,
)]
#[repr(transparent)]
pub struct OpCode(pub U16<LittleEndian>);

impl OpCode {
    pub const RESET: Self = Self(U16::from_bytes([0x03, 0x0C]));
    pub const SET_EVENT_MASK: Self = Self(U16::from_bytes([0x01, 0x0C]));
    pub const LE_SET_EVENT_MASK: Self = Self(U16::from_bytes([0x01, 0x20]));
    pub const LE_SET_ADVERTISING_PARAMETERS: Self = Self(U16::from_bytes([0x06, 0x20]));
    pub const LE_SET_ADVERTISING_DATA: Self = Self(U16::from_bytes([0x08, 0x20]));
    pub const LE_SET_SCAN_RESPONSE_DATA: Self = Self(U16::from_bytes([0x09, 0x20]));
    pub const LE_SET_ADVERTISING_ENABLE: Self = Self(U16::from_bytes([0x0A, 0x20]));
    pub const LE_SET_SCAN_PARAMETERS: Self = Self(U16::from_bytes([0x0B, 0x20]));
    pub const LE_SET_SCAN_ENABLE: Self = Self(U16::from_bytes([0x0C, 0x20]));
    pub const LE_SET_EXTENDED_ADVERTISING_PARAMETERS: Self = Self(U16::from_bytes([0x36, 0x20]));
    pub const LE_SET_EXTENDED_ADVERTISING_DATA: Self = Self(U16::from_bytes([0x37, 0x20]));
    pub const LE_SET_EXTENDED_SCAN_RESPONSE_DATA: Self = Self(U16::from_bytes([0x38, 0x20]));
    pub const LE_SET_EXTENDED_ADVERTISING_ENABLE: Self = Self(U16::from_bytes([0x39, 0x20]));
    pub const LE_CLEAR_ADVERTISING_SETS: Self = Self(U16::from_bytes([0x3D, 0x20]));

    pub fn get(self) -> u16 {
        self.0.get()
    }
}

/// HCI Command Packet Header.
#[repr(C)]
#[derive(FromBytes, IntoBytes, Unaligned, Immutable, KnownLayout, Debug)]
pub struct HciCommandHeader {
    pub op_code: OpCode,
    pub parameter_total_length: u8,
}

/// Trait for HCI commands to associate them with their OpCode.
pub trait HciCommand {
    const OP_CODE: OpCode;
}

/// Set Event Mask Command.
#[repr(C)]
#[derive(FromBytes, IntoBytes, Unaligned, Immutable, KnownLayout, Debug)]
pub struct SetEventMask {
    pub event_mask: U64<LittleEndian>,
}

impl HciCommand for SetEventMask {
    const OP_CODE: OpCode = OpCode::SET_EVENT_MASK;
}

/// LE Set Event Mask Command.
#[repr(C)]
#[derive(FromBytes, IntoBytes, Unaligned, Immutable, KnownLayout, Debug)]
pub struct LeSetEventMask {
    pub le_event_mask: U64<LittleEndian>,
}

impl HciCommand for LeSetEventMask {
    const OP_CODE: OpCode = OpCode::LE_SET_EVENT_MASK;
}

/// LE Set Advertising Parameters Command.
#[repr(C)]
#[derive(FromBytes, IntoBytes, Unaligned, Immutable, KnownLayout, Debug)]
pub struct LeSetAdvertisingParameters {
    pub advertising_interval_min: U16<LittleEndian>,
    pub advertising_interval_max: U16<LittleEndian>,
    pub advertising_type: AdvertisingType,
    pub own_address_type: OwnAddressType,
    pub peer_address_type: PeerAddressType,
    pub peer_address: Address,
    pub advertising_channel_map: u8,
    pub advertising_filter_policy: AdvertisingFilterPolicy,
}

impl HciCommand for LeSetAdvertisingParameters {
    const OP_CODE: OpCode = OpCode::LE_SET_ADVERTISING_PARAMETERS;
}

/// LE Set Advertising Data Command.
#[repr(C)]
#[derive(FromBytes, IntoBytes, Unaligned, Immutable, KnownLayout, Debug)]
pub struct LeSetAdvertisingData {
    pub advertising_data_length: u8,
    pub advertising_data: [u8; 31],
}

impl HciCommand for LeSetAdvertisingData {
    const OP_CODE: OpCode = OpCode::LE_SET_ADVERTISING_DATA;
}

/// LE Set Scan Response Data Command.
#[repr(C)]
#[derive(FromBytes, IntoBytes, Unaligned, Immutable, KnownLayout, Debug)]
pub struct LeSetScanResponseData {
    pub scan_response_data_length: u8,
    pub scan_response_data: [u8; 31],
}

impl HciCommand for LeSetScanResponseData {
    const OP_CODE: OpCode = OpCode::LE_SET_SCAN_RESPONSE_DATA;
}

/// LE Set Advertising Enable Command.
#[repr(C)]
#[derive(FromBytes, IntoBytes, Unaligned, Immutable, KnownLayout, Debug)]
pub struct LeSetAdvertisingEnable {
    pub advertising_enable: Enable,
}

impl HciCommand for LeSetAdvertisingEnable {
    const OP_CODE: OpCode = OpCode::LE_SET_ADVERTISING_ENABLE;
}

/// LE Set Scan Parameters Command.
#[repr(C)]
#[derive(FromBytes, IntoBytes, Unaligned, Immutable, KnownLayout, Debug)]
pub struct LeSetScanParameters {
    pub le_scan_type: LeScanType,
    pub le_scan_interval: U16<LittleEndian>,
    pub le_scan_window: U16<LittleEndian>,
    pub own_address_type: OwnAddressType,
    pub scanning_filter_policy: LeScanningFilterPolicy,
}

impl HciCommand for LeSetScanParameters {
    const OP_CODE: OpCode = OpCode::LE_SET_SCAN_PARAMETERS;
}

/// LE Set Scan Enable Command.
#[repr(C)]
#[derive(FromBytes, IntoBytes, Unaligned, Immutable, KnownLayout, Debug)]
pub struct LeSetScanEnable {
    pub le_scan_enable: Enable,
    pub filter_duplicates: Enable,
}

impl HciCommand for LeSetScanEnable {
    const OP_CODE: OpCode = OpCode::LE_SET_SCAN_ENABLE;
}

/// Reset Command (Empty payload).
#[repr(C)]
#[derive(FromBytes, IntoBytes, Unaligned, Immutable, KnownLayout, Debug)]
pub struct Reset;

impl HciCommand for Reset {
    const OP_CODE: OpCode = OpCode::RESET;
}

/// LE Set Extended Advertising Parameters Command.
#[repr(C)]
#[derive(FromBytes, IntoBytes, Unaligned, Immutable, KnownLayout, Debug)]
pub struct LeSetExtendedAdvertisingParameters {
    pub advertising_handle: u8,
    pub advertising_event_properties: AdvertisingEventProperties,
    pub primary_advertising_interval_min: [u8; 3],
    pub primary_advertising_interval_max: [u8; 3],
    pub primary_advertising_channel_map: u8,
    pub own_address_type: OwnAddressType,
    pub peer_address_type: PeerAddressType,
    pub peer_address: Address,
    pub advertising_filter_policy: AdvertisingFilterPolicy,
    pub advertising_tx_power: i8,
    pub primary_advertising_phy: PrimaryPhyType,
    pub secondary_advertising_max_skip: u8,
    pub secondary_advertising_phy: SecondaryPhyType,
    pub advertising_sid: u8,
    pub scan_request_notification_enable: Enable,
}

impl HciCommand for LeSetExtendedAdvertisingParameters {
    const OP_CODE: OpCode = OpCode::LE_SET_EXTENDED_ADVERTISING_PARAMETERS;
}

/// LE Set Extended Advertising Data Command.
#[repr(C)]
#[derive(FromBytes, IntoBytes, Unaligned, Immutable, KnownLayout, Debug)]
pub struct LeSetExtendedAdvertisingData<T: ?Sized = [u8]> {
    pub advertising_handle: u8,
    pub operation: Operation,
    pub fragment_preference: FragmentPreference,
    pub advertising_data_length: u8,
    pub advertising_data: T,
}

impl<T: ?Sized> HciCommand for LeSetExtendedAdvertisingData<T> {
    const OP_CODE: OpCode = OpCode::LE_SET_EXTENDED_ADVERTISING_DATA;
}

/// LE Set Extended Scan Response Data Command.
#[repr(C)]
#[derive(FromBytes, IntoBytes, Unaligned, Immutable, KnownLayout, Debug)]
pub struct LeSetExtendedScanResponseData<T: ?Sized = [u8]> {
    pub advertising_handle: u8,
    pub operation: Operation,
    pub fragment_preference: FragmentPreference,
    pub scan_response_data_length: u8,
    pub scan_response_data: T,
}

impl<T: ?Sized> HciCommand for LeSetExtendedScanResponseData<T> {
    const OP_CODE: OpCode = OpCode::LE_SET_EXTENDED_SCAN_RESPONSE_DATA;
}

/// LE Set Extended Advertising Enable Command.
#[repr(C)]
#[derive(FromBytes, IntoBytes, Unaligned, Immutable, KnownLayout, Debug)]
pub struct LeSetExtendedAdvertisingEnable<T: ?Sized = [EnabledSet]> {
    pub enable: Enable,
    pub num_sets: u8,
    pub enabled_sets: T,
}

impl<T: ?Sized> HciCommand for LeSetExtendedAdvertisingEnable<T> {
    const OP_CODE: OpCode = OpCode::LE_SET_EXTENDED_ADVERTISING_ENABLE;
}

/// LE Clear Advertising Sets Command (Empty payload).
#[repr(C)]
#[derive(FromBytes, IntoBytes, Unaligned, Immutable, KnownLayout, Debug)]
pub struct LeClearAdvertisingSets;

impl HciCommand for LeClearAdvertisingSets {
    const OP_CODE: OpCode = OpCode::LE_CLEAR_ADVERTISING_SETS;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_le_set_extended_advertising_parameters_wire_layout() {
        assert_eq!(std::mem::size_of::<LeSetExtendedAdvertisingParameters>(), 25);
        assert_eq!(
            LeSetExtendedAdvertisingParameters::OP_CODE,
            OpCode::LE_SET_EXTENDED_ADVERTISING_PARAMETERS
        );

        let cmd = LeSetExtendedAdvertisingParameters {
            advertising_handle: 0x01,
            advertising_event_properties: AdvertisingEventProperties::LEGACY_ADV_SCAN_IND,
            primary_advertising_interval_min: [0x20, 0x03, 0x00],
            primary_advertising_interval_max: [0x40, 0x06, 0x00],
            primary_advertising_channel_map: 0x07,
            own_address_type: OwnAddressType::PUBLIC_DEVICE_ADDRESS,
            peer_address_type: PeerAddressType::PUBLIC_DEVICE_OR_IDENTITY_ADDRESS,
            peer_address: Address { bytes: [0x01, 0x02, 0x03, 0x04, 0x05, 0x06] },
            advertising_filter_policy: AdvertisingFilterPolicy::ALL_DEVICES,
            advertising_tx_power: -15,
            primary_advertising_phy: PrimaryPhyType::LE_1M,
            secondary_advertising_max_skip: 0x02,
            secondary_advertising_phy: SecondaryPhyType::LE_1M,
            advertising_sid: 0x04,
            scan_request_notification_enable: Enable::DISABLED,
        };

        let expected_bytes: [u8; 25] = [
            0x01, 0x12, 0x00, 0x20, 0x03, 0x00, 0x40, 0x06, 0x00, 0x07, 0x00, 0x00, 0x01, 0x02,
            0x03, 0x04, 0x05, 0x06, 0x00, 0xF1, 0x01, 0x02, 0x01, 0x04, 0x00,
        ];
        assert_eq!(cmd.as_bytes(), &expected_bytes[..]);
    }

    #[test]
    fn test_le_set_extended_advertising_data_wire_layout() {
        assert_eq!(
            LeSetExtendedAdvertisingData::<[u8; 3]>::OP_CODE,
            OpCode::LE_SET_EXTENDED_ADVERTISING_DATA
        );

        let cmd = LeSetExtendedAdvertisingData {
            advertising_handle: 0x02,
            operation: Operation::COMPLETE_ADVERTISEMENT,
            fragment_preference: FragmentPreference::CONTROLLER_MAY_FRAGMENT,
            advertising_data_length: 3,
            advertising_data: [0xAAu8, 0xBBu8, 0xCCu8],
        };

        let expected_bytes: [u8; 7] = [0x02, 0x03, 0x00, 0x03, 0xAA, 0xBB, 0xCC];
        assert_eq!(cmd.as_bytes(), &expected_bytes[..]);
    }

    #[test]
    fn test_le_set_extended_scan_response_data_wire_layout() {
        assert_eq!(
            LeSetExtendedScanResponseData::<[u8; 2]>::OP_CODE,
            OpCode::LE_SET_EXTENDED_SCAN_RESPONSE_DATA
        );

        let cmd = LeSetExtendedScanResponseData {
            advertising_handle: 0x03,
            operation: Operation::COMPLETE_ADVERTISEMENT,
            fragment_preference: FragmentPreference::CONTROLLER_SHOULD_NOT,
            scan_response_data_length: 2,
            scan_response_data: [0x11u8, 0x22u8],
        };

        let expected_bytes: [u8; 6] = [0x03, 0x03, 0x01, 0x02, 0x11, 0x22];
        assert_eq!(cmd.as_bytes(), &expected_bytes[..]);
    }

    #[test]
    fn test_le_set_extended_advertising_enable_wire_layout() {
        assert_eq!(
            LeSetExtendedAdvertisingEnable::<[EnabledSet; 1]>::OP_CODE,
            OpCode::LE_SET_EXTENDED_ADVERTISING_ENABLE
        );

        let cmd = LeSetExtendedAdvertisingEnable {
            enable: Enable::ENABLED,
            num_sets: 1,
            enabled_sets: [EnabledSet {
                advertising_handle: 0x00,
                duration: U16::from_bytes([0x64, 0x00]),
                max_extended_advertising_events: 0x05,
            }],
        };

        let expected_bytes: [u8; 6] = [0x01, 0x01, 0x00, 0x64, 0x00, 0x05];
        assert_eq!(cmd.as_bytes(), &expected_bytes[..]);

        let empty_cmd = LeSetExtendedAdvertisingEnable {
            enable: Enable::DISABLED,
            num_sets: 0,
            enabled_sets: (),
        };
        assert_eq!(empty_cmd.as_bytes(), &[0x00, 0x00]);
    }

    #[test]
    fn test_le_clear_advertising_sets_wire_layout() {
        assert_eq!(LeClearAdvertisingSets::OP_CODE, OpCode::LE_CLEAR_ADVERTISING_SETS);
        let cmd = LeClearAdvertisingSets;
        assert!(cmd.as_bytes().is_empty());
    }
}
