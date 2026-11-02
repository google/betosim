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

use zerocopy::{
    FromBytes, Immutable, IntoBytes, KnownLayout, Ref, U16, U32, Unaligned,
    byteorder::NetworkEndian,
};

use crate::utils::general::ParseResult;

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
    /// A combined field containing the data offset, reserved bits, and TCP
    /// flags.
    pub data_offset_reserved_flags: U16<NetworkEndian>,
    /// The size of the receive window, which specifies the number of window
    /// size units.
    pub window_size: U16<NetworkEndian>,
    /// The checksum of the TCP header and data.
    pub checksum: U16<NetworkEndian>,
    /// If the URG flag is set, this field is an offset from the sequence number
    /// indicating the last urgent data byte.
    pub urgent_ptr: U16<NetworkEndian>,
}

/// TCP Option Kinds.
pub mod option_kind {
    pub const EOL: u8 = 0;
    pub const NOP: u8 = 1;
    pub const MSS: u8 = 2;
    pub const WS: u8 = 3;
    pub const SACK_PERMITTED: u8 = 4;
}

/// TCP MSS Option.
#[derive(FromBytes, IntoBytes, Unaligned, KnownLayout, Immutable, Debug)]
#[repr(C)]
pub struct TcpOptionMss {
    pub kind: u8,
    pub len: u8,
    pub mss: U16<NetworkEndian>,
}

/// TCP Window Scale Option.
#[derive(FromBytes, IntoBytes, Unaligned, KnownLayout, Immutable, Debug)]
#[repr(C)]
pub struct TcpOptionWindowScale {
    pub kind: u8,
    pub len: u8,
    pub shift: u8,
}

/// TCP SACK Permitted Option.
#[derive(FromBytes, IntoBytes, Unaligned, KnownLayout, Immutable, Debug)]
#[repr(C)]
pub struct TcpOptionSackPermitted {
    pub kind: u8,
    pub len: u8,
}

/// TCP flags.
pub mod flags {
    pub const FIN: u16 = 1;
    pub const SYN: u16 = 1 << 1;
    pub const RST: u16 = 1 << 2;
    pub const PSH: u16 = 1 << 3;
    pub const ACK: u16 = 1 << 4;
    pub const URG: u16 = 1 << 5;
}

impl TcpHeader {
    /// Parses a `TcpHeader` from the beginning of the given byte slice.
    pub fn parse(bytes: &[u8]) -> Option<ParseResult<'_, TcpHeader>> {
        let (header, _) = Ref::<&[u8], TcpHeader>::from_prefix(bytes).ok()?;
        let header_len = header.header_length();
        Some((header, bytes.get(header_len..)?))
    }

    /// Returns the Data Offset, which is the size of the TCP header in 32-bit
    /// words.
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

    /// Calculates and updates the TCP checksum.
    pub fn update_checksum(&mut self, src_ip: [u8; 4], dst_ip: [u8; 4], payload: &[u8]) {
        self.checksum = U16::new(0);
        let mut sum: u32 = 0;

        // Pseudo-header
        // Source IP
        for &chunk in src_ip.as_chunks::<2>().0 {
            sum += u32::from(u16::from_be_bytes(chunk));
        }
        // Dest IP
        for &chunk in dst_ip.as_chunks::<2>().0 {
            sum += u32::from(u16::from_be_bytes(chunk));
        }
        // Zero + Protocol (6 for TCP)
        sum += 6;
        // TCP Length (Header + Data)
        let tcp_len = self.header_length() + payload.len();
        sum += tcp_len as u32;

        // TCP Header
        let header_bytes = self.as_bytes();
        let (header_chunks, header_remainder) = header_bytes.as_chunks::<2>();
        for &chunk in header_chunks {
            sum += u32::from(u16::from_be_bytes(chunk));
        }
        if let &[last] = header_remainder {
            sum += (last as u32) << 8;
        }

        // Payload
        let (payload_chunks, payload_remainder) = payload.as_chunks::<2>();
        for &chunk in payload_chunks {
            sum += u32::from(u16::from_be_bytes(chunk));
        }
        if let &[last] = payload_remainder {
            sum += (last as u32) << 8;
        }

        while (sum >> 16) != 0 {
            sum = (sum & 0xFFFF) + (sum >> 16);
        }
        self.checksum = U16::new(!sum as u16);
    }
}

#[cfg(test)]
#[path = "tcp_tests.rs"]
mod tests;
