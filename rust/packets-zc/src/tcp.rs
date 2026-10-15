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

//! Defines the TCP (Transmission Control Protocol) header using `zerocopy`.

use crate::util::ParseResult;
use zerocopy::{
    FromBytes, Immutable, IntoBytes, KnownLayout, Ref, U16, U32, Unaligned,
    byteorder::NetworkEndian,
};

/// Represents the TCP header.
#[derive(FromBytes, IntoBytes, Unaligned, KnownLayout, Immutable, Debug)]
#[repr(C)]
pub struct TcpHeader {
    /// The source port number.
    pub source_port: U16<NetworkEndian>,
    /// The destination port number.
    pub dest_port: U16<NetworkEndian>,
    /// The sequence number.
    pub sequence_num: U32<NetworkEndian>,
    /// The acknowledgment number, if the ACK flag is set.
    pub ack_num: U32<NetworkEndian>,
    /// A combined field containing the data offset, reserved bits, and TCP flags.
    pub data_offset_reserved_flags: U16<NetworkEndian>,
    /// The size of the receive window, which specifies the number of window size units.
    pub window_size: U16<NetworkEndian>,
    /// The checksum of the TCP header and data.
    pub checksum: U16<NetworkEndian>,
    /// If the URG flag is set, this field is an offset from the sequence number indicating the last urgent data byte.
    pub urgent_ptr: U16<NetworkEndian>,
}

/// TCP flags.
pub mod flags {
    pub const FIN: u16 = 1;
    pub const SYN: u16 = 1 << 1;
    pub const RST: u16 = 1 << 2;
    pub const PSH: u16 = 1 << 3;
    pub const ACK: u16 = 1 << 4;
    pub const URG: u16 = 1 << 5;
    pub const ECE: u16 = 1 << 6;
    pub const CWR: u16 = 1 << 7;
    pub const NS: u16 = 1 << 8;
}

impl TcpHeader {
    /// Parses a `TcpHeader` from the beginning of the given byte slice.
    pub fn parse(bytes: &[u8]) -> Option<ParseResult<TcpHeader>> {
        let (header, _) = Ref::<&[u8], TcpHeader>::from_prefix(bytes).ok()?;
        let header_len = header.header_length();
        if bytes.len() < header_len {
            return None;
        }
        Some((header, &bytes[header_len..]))
    }

    /// Returns the Data Offset, which is the size of the TCP header in 32-bit words.
    pub fn data_offset(&self) -> u8 {
        (self.data_offset_reserved_flags.get() >> 12) as u8
    }

    /// Returns the length of the TCP header in bytes.
    pub fn header_length(&self) -> usize {
        self.data_offset() as usize * 4
    }

    /// Returns the TCP flags as a u16 value.
    pub fn flags(&self) -> u16 {
        self.data_offset_reserved_flags.get() & 0x1FF
    }

    /// Returns true if the FIN flag is set.
    pub fn fin(&self) -> bool {
        (self.flags() & flags::FIN) != 0
    }

    /// Returns true if the SYN flag is set.
    pub fn syn(&self) -> bool {
        (self.flags() & flags::SYN) != 0
    }

    /// Returns true if the RST flag is set.
    pub fn rst(&self) -> bool {
        (self.flags() & flags::RST) != 0
    }

    /// Returns true if the PSH flag is set.
    pub fn psh(&self) -> bool {
        (self.flags() & flags::PSH) != 0
    }

    /// Returns true if the ACK flag is set.
    pub fn ack(&self) -> bool {
        (self.flags() & flags::ACK) != 0
    }

    /// Returns true if the URG flag is set.
    pub fn urg(&self) -> bool {
        (self.flags() & flags::URG) != 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tcp_header_parsing_syn() {
        // TCP SYN packet, header length 20 bytes (5 * 4)
        let bytes: [u8; 20] = [
            0xC0, 0x14, // Source Port: 49172
            0x00, 0x50, // Dest Port: 80
            0x12, 0x34, 0x56, 0x78, // Sequence Number
            0x00, 0x00, 0x00, 0x00, // Ack Number
            0x50, 0x02, // Data Offset (5), Flags (SYN)
            0x72, 0x10, // Window Size
            0xAB, 0xCD, // Checksum
            0x00, 0x00, // Urgent Pointer
        ];
        let (header, rest) = TcpHeader::parse(&bytes).expect("Failed to parse TCP header");

        assert_eq!(header.source_port.get(), 49172);
        assert_eq!(header.dest_port.get(), 80);
        assert_eq!(header.sequence_num.get(), 0x12345678);
        assert_eq!(header.data_offset(), 5);
        assert_eq!(header.header_length(), 20);
        assert!(header.syn());
        assert!(!header.ack());
        assert!(!header.fin());
        assert_eq!(rest.len(), 0);
    }

    #[test]
    fn test_tcp_header_with_options_and_payload() {
        // TCP ACK packet, header length 24 bytes, payload 4 bytes
        let bytes: [u8; 28] = [
            0x00, 0x50, 0xC0, 0x14, // Ports
            0x12, 0x34, 0x56, 0x79, // Seq Num
            0x9A, 0xBC, 0xDE, 0xF0, // Ack Num
            0x60, 0x10, // Data Offset (6), Flags (ACK)
            0x72, 0x10, // Window
            0xAB, 0xCD, // Checksum
            0x00, 0x00, // Urgent Ptr
            0x01, 0x02, 0x03, 0x04, // Options
            0xDE, 0xAD, 0xBE, 0xEF, // Payload
        ];
        let (header, rest) = TcpHeader::parse(&bytes).expect("Failed to parse TCP header");

        assert_eq!(header.data_offset(), 6);
        assert_eq!(header.header_length(), 24);
        assert!(!header.syn());
        assert!(header.ack());
        assert_eq!(rest.len(), 4);
        assert_eq!(rest, &[0xDE, 0xAD, 0xBE, 0xEF]);
    }

    #[test]
    fn test_tcp_header_too_short() {
        // Header is 19 bytes, which is less than the minimum 20
        let bytes: [u8; 19] = [0; 19];
        assert!(TcpHeader::parse(&bytes).is_none());

        // Header claims to be 24 bytes, but buffer is only 22
        let bytes_long_header: [u8; 22] = [
            0x00, 0x50, 0xC0, 0x14, 0x12, 0x34, 0x56, 0x79, 0x9A, 0xBC, 0xDE, 0xF0, 0x60, 0x10,
            0x72, 0x10, 0xAB, 0xCD, 0x00, 0x00, 0x01, 0x02,
        ];
        assert!(TcpHeader::parse(&bytes_long_header).is_none());
    }
}
