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

use std::fmt;

use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout, Unaligned};

/// Generic enable/disable.
#[derive(
    Debug, Copy, Clone, PartialEq, Eq, FromBytes, IntoBytes, Unaligned, Immutable, KnownLayout,
)]
#[repr(transparent)]
pub struct Enable(pub u8);

impl Enable {
    pub const DISABLED: Self = Self(0x00);
    pub const ENABLED: Self = Self(0x01);
}

/// Advertising Type for LE Set Advertising Parameters.
#[derive(
    Debug, Copy, Clone, PartialEq, Eq, FromBytes, IntoBytes, Unaligned, Immutable, KnownLayout,
)]
#[repr(transparent)]
pub struct AdvertisingType(pub u8);

impl AdvertisingType {
    pub const ADV_IND: Self = Self(0x00);
    pub const ADV_DIRECT_IND_HIGH: Self = Self(0x01);
    pub const ADV_SCAN_IND: Self = Self(0x02);
    pub const ADV_NONCONN_IND: Self = Self(0x03);
    pub const ADV_DIRECT_IND_LOW: Self = Self(0x04);
}

/// Own Address Type.
#[derive(
    Debug, Copy, Clone, PartialEq, Eq, FromBytes, IntoBytes, Unaligned, Immutable, KnownLayout,
)]
#[repr(transparent)]
pub struct OwnAddressType(pub u8);

impl OwnAddressType {
    pub const PUBLIC_DEVICE_ADDRESS: Self = Self(0x00);
    pub const RANDOM_DEVICE_ADDRESS: Self = Self(0x01);
    pub const RESOLVABLE_OR_PUBLIC_ADDRESS: Self = Self(0x02);
    pub const RESOLVABLE_OR_RANDOM_ADDRESS: Self = Self(0x03);
}

/// Peer Address Type.
#[derive(
    Debug, Copy, Clone, PartialEq, Eq, FromBytes, IntoBytes, Unaligned, Immutable, KnownLayout,
)]
#[repr(transparent)]
pub struct PeerAddressType(pub u8);

impl PeerAddressType {
    pub const PUBLIC_DEVICE_OR_IDENTITY_ADDRESS: Self = Self(0x00);
    pub const RANDOM_DEVICE_OR_IDENTITY_ADDRESS: Self = Self(0x01);
}

/// Advertising Filter Policy.
#[derive(
    Debug, Copy, Clone, PartialEq, Eq, FromBytes, IntoBytes, Unaligned, Immutable, KnownLayout,
)]
#[repr(transparent)]
pub struct AdvertisingFilterPolicy(pub u8);

impl AdvertisingFilterPolicy {
    pub const ALL_DEVICES: Self = Self(0x00);
    pub const LISTED_SCAN: Self = Self(0x01);
    pub const LISTED_CONNECT: Self = Self(0x02);
    pub const LISTED_SCAN_AND_CONNECT: Self = Self(0x03);
}

/// LE Scan Type.
#[derive(
    Debug, Copy, Clone, PartialEq, Eq, FromBytes, IntoBytes, Unaligned, Immutable, KnownLayout,
)]
#[repr(transparent)]
pub struct LeScanType(pub u8);

impl LeScanType {
    pub const PASSIVE: Self = Self(0x00);
    pub const ACTIVE: Self = Self(0x01);
}

/// LE Scanning Filter Policy.
#[derive(
    Debug, Copy, Clone, PartialEq, Eq, FromBytes, IntoBytes, Unaligned, Immutable, KnownLayout,
)]
#[repr(transparent)]
pub struct LeScanningFilterPolicy(pub u8);

impl LeScanningFilterPolicy {
    pub const ACCEPT_ALL: Self = Self(0x00);
    pub const FILTER_ACCEPT_LIST_ONLY: Self = Self(0x01);
    pub const CHECK_INITIATORS_IDENTITY: Self = Self(0x02);
    pub const FILTER_ACCEPT_LIST_AND_INITIATORS_IDENTITY: Self = Self(0x03);
}

/// Standard 6-byte Bluetooth Address.
#[derive(
    Debug, Copy, Clone, PartialEq, Eq, FromBytes, IntoBytes, Unaligned, Immutable, KnownLayout,
)]
#[repr(C)]
pub struct Address {
    pub bytes: [u8; 6],
}

impl fmt::Display for Address {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let [b0, b1, b2, b3, b4, b5] = self.bytes;
        write!(f, "{b5:02X}:{b4:02X}:{b3:02X}:{b2:02X}:{b1:02X}:{b0:02X}")
    }
}

/// Event Type for LE Advertising Report.
#[derive(
    Debug, Copy, Clone, PartialEq, Eq, FromBytes, IntoBytes, Unaligned, Immutable, KnownLayout,
)]
#[repr(transparent)]
pub struct LeAdvertisingEventType(pub u8);

impl LeAdvertisingEventType {
    pub const ADV_IND: Self = Self(0x00);
    pub const ADV_DIRECT_IND: Self = Self(0x01);
    pub const ADV_SCAN_IND: Self = Self(0x02);
    pub const ADV_NONCONN_IND: Self = Self(0x03);
    pub const SCAN_RSP: Self = Self(0x04);
}

impl fmt::Display for LeAdvertisingEventType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::ADV_IND => write!(f, "ADV_IND"),
            Self::ADV_DIRECT_IND => write!(f, "ADV_DIRECT_IND"),
            Self::ADV_SCAN_IND => write!(f, "ADV_SCAN_IND"),
            Self::ADV_NONCONN_IND => write!(f, "ADV_NONCONN_IND"),
            Self::SCAN_RSP => write!(f, "SCAN_RSP"),
            _ => write!(f, "UNKNOWN ({:#04X})", self.0),
        }
    }
}

/// Generic Access Profile (GAP) Data Types for Advertising Data payload
/// parsing.
#[derive(
    Debug, Copy, Clone, PartialEq, Eq, FromBytes, IntoBytes, Unaligned, Immutable, KnownLayout,
)]
#[repr(transparent)]
pub struct GapDataType(pub u8);

impl GapDataType {
    pub const FLAGS: Self = Self(0x01);
    pub const INCOMPLETE_16BIT_UUIDS: Self = Self(0x02);
    pub const COMPLETE_16BIT_UUIDS: Self = Self(0x03);
    pub const INCOMPLETE_32BIT_UUIDS: Self = Self(0x04);
    pub const COMPLETE_32BIT_UUIDS: Self = Self(0x05);
    pub const INCOMPLETE_128BIT_UUIDS: Self = Self(0x06);
    pub const COMPLETE_128BIT_UUIDS: Self = Self(0x07);
    pub const SHORTENED_LOCAL_NAME: Self = Self(0x08);
    pub const COMPLETE_LOCAL_NAME: Self = Self(0x09);
    pub const TX_POWER_LEVEL: Self = Self(0x0A);
    pub const SERVICE_DATA_16BIT: Self = Self(0x16);
    pub const APPEARANCE: Self = Self(0x19);
    pub const SERVICE_DATA_32BIT: Self = Self(0x20);
    pub const SERVICE_DATA_128BIT: Self = Self(0x21);
    pub const MANUFACTURER_SPECIFIC: Self = Self(0xFF);
}

impl fmt::Display for GapDataType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::FLAGS => write!(f, "Flags"),
            Self::INCOMPLETE_16BIT_UUIDS => write!(f, "Incomplete 16-bit UUIDs"),
            Self::COMPLETE_16BIT_UUIDS => write!(f, "Complete 16-bit UUIDs"),
            Self::INCOMPLETE_32BIT_UUIDS => write!(f, "Incomplete 32-bit UUIDs"),
            Self::COMPLETE_32BIT_UUIDS => write!(f, "Complete 32-bit UUIDs"),
            Self::INCOMPLETE_128BIT_UUIDS => write!(f, "Incomplete 128-bit UUIDs"),
            Self::COMPLETE_128BIT_UUIDS => write!(f, "Complete 128-bit UUIDs"),
            Self::SHORTENED_LOCAL_NAME => write!(f, "Shortened Local Name"),
            Self::COMPLETE_LOCAL_NAME => write!(f, "Complete Local Name"),
            Self::TX_POWER_LEVEL => write!(f, "Tx Power Level"),
            Self::SERVICE_DATA_16BIT => write!(f, "Service Data 16-bit"),
            Self::APPEARANCE => write!(f, "Appearance"),
            Self::SERVICE_DATA_32BIT => write!(f, "Service Data 32-bit"),
            Self::SERVICE_DATA_128BIT => write!(f, "Service Data 128-bit"),
            Self::MANUFACTURER_SPECIFIC => write!(f, "Manufacturer Specific Data"),
            _ => write!(f, "Unknown ({:#04X})", self.0),
        }
    }
}
