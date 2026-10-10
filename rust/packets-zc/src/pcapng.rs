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

//! A minimal pcapng parser.

use crate::util::ParseResult;
use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout, Ref};

#[derive(FromBytes, IntoBytes, KnownLayout, Immutable, Debug, PartialEq, Eq)]
#[repr(C)]
pub struct SectionHeaderBlock {
    pub block_type: u32,
    pub block_total_length: u32,
    pub byte_order_magic: u32,
    pub major_version: u16,
    pub minor_version: u16,
    pub section_length: u64,
}

impl SectionHeaderBlock {
    /// Creates a new `SectionHeaderBlock`.
    pub fn new(
        block_type: u32,
        block_total_length: u32,
        byte_order_magic: u32,
        major_version: u16,
        minor_version: u16,
        section_length: u64,
    ) -> Self {
        Self {
            block_type,
            block_total_length,
            byte_order_magic,
            major_version,
            minor_version,
            section_length,
        }
    }

    /// Parses a `SectionHeaderBlock` from the beginning of the given byte slice.
    pub fn parse(bytes: &[u8]) -> Option<ParseResult<SectionHeaderBlock>> {
        Ref::from_prefix(bytes).ok()
    }
}

#[derive(FromBytes, IntoBytes, KnownLayout, Immutable, Debug, PartialEq, Eq)]
#[repr(C)]
pub struct EnhancedPacketBlock {
    pub block_type: u32,
    pub block_total_length: u32,
    pub interface_id: u32,
    pub timestamp_high: u32,
    pub timestamp_low: u32,
    pub captured_len: u32,
    pub packet_len: u32,
}

impl EnhancedPacketBlock {
    /// Creates a new `EnhancedPacketBlock`.
    pub fn new(
        block_type: u32,
        block_total_length: u32,
        interface_id: u32,
        timestamp_high: u32,
        timestamp_low: u32,
        captured_len: u32,
        packet_len: u32,
    ) -> Self {
        Self {
            block_type,
            block_total_length,
            interface_id,
            timestamp_high,
            timestamp_low,
            captured_len,
            packet_len,
        }
    }

    /// Parses an `EnhancedPacketBlock` from the beginning of the given byte slice.
    pub fn parse(bytes: &[u8]) -> Option<ParseResult<EnhancedPacketBlock>> {
        Ref::from_prefix(bytes).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_section_header_block_parsing() {
        let block = SectionHeaderBlock::new(0x0A0D0D0A, 28, 0x1A2B3C4D, 1, 0, 0xFFFFFFFFFFFFFFFF);
        let bytes = block.as_bytes();
        let (header, rest) =
            SectionHeaderBlock::parse(bytes).expect("Failed to parse Section Header Block");

        assert_eq!(header.block_type, 0x0A0D0D0A);
        assert_eq!(header.block_total_length, 28);
        assert_eq!(header.byte_order_magic, 0x1A2B3C4D);
        assert_eq!(header.major_version, 1);
        assert_eq!(header.minor_version, 0);
        assert_eq!(rest.len(), 0);
    }

    #[test]
    fn test_enhanced_packet_block_parsing() {
        let block = EnhancedPacketBlock::new(6, 32, 0, 0, 0, 10, 10);
        let mut bytes = block.as_bytes().to_vec();
        bytes.extend_from_slice(&[0xDE, 0xAD, 0xBE, 0xEF]);
        let (header, rest) =
            EnhancedPacketBlock::parse(&bytes).expect("Failed to parse Enhanced Packet Block");

        assert_eq!(header.block_type, 6);
        assert_eq!(header.block_total_length, 32);
        assert_eq!(header.interface_id, 0);
        assert_eq!(header.captured_len, 10);
        assert_eq!(header.packet_len, 10);
        assert_eq!(rest.len(), 4);
    }
}
