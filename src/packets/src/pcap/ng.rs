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

use zerocopy::{
    FromBytes, Immutable, IntoBytes, KnownLayout, Ref, U16, U32, U64, byteorder::LittleEndian,
};

use crate::utils::general::ParseResult;

#[derive(FromBytes, IntoBytes, KnownLayout, Immutable, Debug, PartialEq, Eq)]
#[repr(C)]
pub struct SectionHeaderBlock {
    pub block_type: U32<LittleEndian>,
    pub block_total_length: U32<LittleEndian>,
    pub byte_order_magic: U32<LittleEndian>,
    pub major_version: U16<LittleEndian>,
    pub minor_version: U16<LittleEndian>,
    pub section_length: U64<LittleEndian>,
}

impl SectionHeaderBlock {
    /// Creates a new `SectionHeaderBlock`.
    #[cfg(test)]
    pub fn new(
        block_type: u32,
        block_total_length: u32,
        byte_order_magic: u32,
        major_version: u16,
        minor_version: u16,
        section_length: u64,
    ) -> Self {
        Self {
            block_type: U32::new(block_type),
            block_total_length: U32::new(block_total_length),
            byte_order_magic: U32::new(byte_order_magic),
            major_version: U16::new(major_version),
            minor_version: U16::new(minor_version),
            section_length: U64::new(section_length),
        }
    }

    /// Parses a `SectionHeaderBlock` from the beginning of the given byte
    /// slice.
    #[cfg(test)]
    pub fn parse(bytes: &[u8]) -> Option<ParseResult<'_, SectionHeaderBlock>> {
        Ref::from_prefix(bytes).ok()
    }
}

#[derive(FromBytes, IntoBytes, KnownLayout, Immutable, Debug, PartialEq, Eq)]
#[repr(C)]
pub struct EnhancedPacketBlock {
    pub block_type: U32<LittleEndian>,
    pub block_total_length: U32<LittleEndian>,
    pub interface_id: U32<LittleEndian>,
    pub timestamp_high: U32<LittleEndian>,
    pub timestamp_low: U32<LittleEndian>,
    pub captured_len: U32<LittleEndian>,
    pub packet_len: U32<LittleEndian>,
}

impl EnhancedPacketBlock {
    /// Creates a new `EnhancedPacketBlock`.
    #[cfg(test)]
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
            block_type: U32::new(block_type),
            block_total_length: U32::new(block_total_length),
            interface_id: U32::new(interface_id),
            timestamp_high: U32::new(timestamp_high),
            timestamp_low: U32::new(timestamp_low),
            captured_len: U32::new(captured_len),
            packet_len: U32::new(packet_len),
        }
    }

    /// Parses an `EnhancedPacketBlock` from the beginning of the given byte
    /// slice.
    #[cfg(test)]
    pub fn parse(bytes: &[u8]) -> Option<ParseResult<'_, EnhancedPacketBlock>> {
        Ref::from_prefix(bytes).ok()
    }
}

#[derive(FromBytes, IntoBytes, KnownLayout, Immutable, Debug, PartialEq, Eq)]
#[repr(C)]
pub struct InterfaceDescriptionBlock {
    pub block_type: U32<LittleEndian>,
    pub block_total_length: U32<LittleEndian>,
    pub link_type: U16<LittleEndian>,
    pub reserved: U16<LittleEndian>,
    pub snap_len: U32<LittleEndian>,
}

impl InterfaceDescriptionBlock {
    pub fn parse(bytes: &[u8]) -> Option<ParseResult<'_, InterfaceDescriptionBlock>> {
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
