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
