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

use zerocopy::{
    FromBytes, Immutable, IntoBytes, KnownLayout, Ref, U16, Unaligned, byteorder::NetworkEndian,
};

use crate::utils::general::ParseResult;

/// Represents the LLC (Logical Link Control) header.
#[repr(C)]
#[derive(
    FromBytes, IntoBytes, Unaligned, Immutable, KnownLayout, Debug, Copy, Clone, PartialEq, Eq,
)]
pub struct LlcHeader {
    /// Destination Service Access Point (DSAP).
    pub dsap: u8,
    /// Source Service Access Point (SSAP).
    pub ssap: u8,
    /// Control field.
    pub control: u8,
}

impl LlcHeader {
    /// Creates a new `LlcHeader`.
    pub fn new(dsap: u8, ssap: u8, control: u8) -> Self {
        Self { dsap, ssap, control }
    }

    /// Parses an `LlcHeader` from the beginning of the given byte slice.
    pub fn parse(bytes: &[u8]) -> Option<ParseResult<'_, LlcHeader>> {
        Ref::from_prefix(bytes).ok()
    }
}

/// Represents the SNAP (Subnetwork Access Protocol) header.
#[repr(C)]
#[derive(
    FromBytes, IntoBytes, Unaligned, Immutable, KnownLayout, Debug, Copy, Clone, PartialEq, Eq,
)]
pub struct SnapHeader {
    /// Organizationally Unique Identifier (OUI).
    pub oui: [u8; 3],
    /// Protocol ID (often an EtherType).
    pub protocol_id: U16<NetworkEndian>,
}

impl SnapHeader {
    /// Creates a new `SnapHeader`.
    pub fn new(oui: [u8; 3], protocol_id: u16) -> Self {
        Self { oui, protocol_id: U16::new(protocol_id) }
    }

    /// Parses a `SnapHeader` from the beginning of the given byte slice.
    pub fn parse(bytes: &[u8]) -> Option<ParseResult<'_, SnapHeader>> {
        Ref::from_prefix(bytes).ok()
    }
}

/// Represents a combined LLC and SNAP header.
/// This is common for carrying IP packets over IEEE 802.11.
#[repr(C)]
#[derive(
    FromBytes, IntoBytes, Unaligned, Immutable, KnownLayout, Debug, Copy, Clone, PartialEq, Eq,
)]
pub struct LlcSnapHeader {
    /// The LLC header part.
    pub llc: LlcHeader,
    /// The SNAP header part.
    pub snap: SnapHeader,
}

impl LlcSnapHeader {
    /// Creates a new `LlcSnapHeader`.
    pub fn new(dsap: u8, ssap: u8, control: u8, oui: [u8; 3], protocol_id: u16) -> Self {
        Self { llc: LlcHeader::new(dsap, ssap, control), snap: SnapHeader::new(oui, protocol_id) }
    }

    /// Parses an `LlcSnapHeader` from the beginning of the given byte slice.
    pub fn parse(bytes: &[u8]) -> Option<ParseResult<'_, LlcSnapHeader>> {
        Ref::from_prefix(bytes).ok()
    }
}

/// Constants for LLC SAP (Service Access Point) values.
pub mod sap {
    /// Null LSAP.
    pub const NULL: u8 = 0x00;
    /// Individual LLC Sublayer Management.
    pub const ILMI: u8 = 0x04;
    /// Global DSAP.
    pub const GLOBAL: u8 = 0xFF;
    /// Subnetwork Access Protocol (SNAP).
    pub const SNAP: u8 = 0xAA;
    /// Spanning Tree Protocol (BPDU).
    pub const STP: u8 = 0x42;
}

/// Constants for the LLC Control field (for Unnumbered Information frames).
pub mod control_field {
    /// Unnumbered Information (UI).
    pub const UI: u8 = 0x03;
    /// Exchange Identification (XID).
    pub const XID: u8 = 0xAF;
    /// Test (TEST).
    pub const TEST: u8 = 0xE3;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_llc_header_parsing() {
        let bytes: [u8; 3] = [sap::SNAP, sap::SNAP, control_field::UI];
        let (header, rest) = LlcHeader::parse(&bytes).expect("Failed to parse LLC header");

        assert_eq!(header.dsap, sap::SNAP);
        assert_eq!(header.ssap, sap::SNAP);
        assert_eq!(header.control, control_field::UI);
        assert_eq!(rest.len(), 0);
    }

    #[test]
    fn test_snap_header_parsing() {
        let bytes: [u8; 5] = [0x00, 0x01, 0x02, 0x08, 0x00];
        let (header, rest) = SnapHeader::parse(&bytes).expect("Failed to parse SNAP header");

        assert_eq!(header.oui, [0x00, 0x01, 0x02]);
        assert_eq!(header.protocol_id.get(), 0x0800); // IPv4
        assert_eq!(rest.len(), 0);
    }

    #[test]
    fn test_llc_snap_header_parsing() {
        let bytes: [u8; 8] =
            [sap::SNAP, sap::SNAP, control_field::UI, 0x00, 0x01, 0x02, 0x08, 0x00];
        let (header, rest) = LlcSnapHeader::parse(&bytes).expect("Failed to parse LLC SNAP header");

        assert_eq!(header.llc.dsap, sap::SNAP);
        assert_eq!(header.snap.protocol_id.get(), 0x0800);
        assert_eq!(rest.len(), 0);
    }
}
