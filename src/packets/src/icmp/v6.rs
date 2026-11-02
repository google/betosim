// Copyright 2024 The Android Open Source Project
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

//! Defines structures for ICMPv6 (Internet Control Message Protocol for IPv6)
//! headers using `zerocopy`.

use zerocopy::{
    FromBytes, Immutable, IntoBytes, KnownLayout, Ref, U16, Unaligned, byteorder::NetworkEndian,
};

use crate::utils::general::ParseResult;

/// Represents the ICMPv6 header.
#[derive(FromBytes, IntoBytes, Unaligned, KnownLayout, Immutable, Debug)]
#[repr(C)]
pub struct Icmpv6Header {
    /// The type of the ICMPv6 message.
    pub icmpv6_type: u8,
    /// The code of the ICMPv6 message, providing more specific information
    /// about the message type.
    pub icmpv6_code: u8,
    /// The checksum of the ICMPv6 header and data.
    pub icmpv6_checksum: U16<NetworkEndian>,
    /// The rest of the header, which varies depending on the message type.
    pub rest: [u8; 4],
}

impl Icmpv6Header {
    /// Parses an `Icmpv6Header` from the beginning of the given byte slice.
    ///
    /// Returns a reference to the header and a slice for the remaining bytes.
    pub fn parse(bytes: &[u8]) -> Option<ParseResult<'_, Icmpv6Header>> {
        Ref::from_prefix(bytes).ok()
    }

    /// Returns the echo fields (identifier, sequence number) if this is an Echo
    /// Request/Reply.
    pub fn echo_fields(&self) -> Option<(u16, u16)> {
        if self.icmpv6_type == Icmpv6Type::EchoRequest as u8
            || self.icmpv6_type == Icmpv6Type::EchoReply as u8
        {
            let echo = Ref::<&[u8], Icmpv6Echo>::from_bytes(&self.rest).ok()?;
            Some((echo.identifier.get(), echo.sequence_number.get()))
        } else {
            None
        }
    }

    /// Sets the echo fields (identifier, sequence number).
    pub fn set_echo_fields(&mut self, id: u16, seq: u16) {
        if let Ok(echo) = Icmpv6Echo::mut_from_bytes(self.rest.as_mut_slice()) {
            echo.identifier.set(id);
            echo.sequence_number.set(seq);
        }
    }

    /// Returns the MTU if this is a Packet Too Big message.
    pub fn mtu(&self) -> Option<u32> {
        if self.icmpv6_type == Icmpv6Type::PacketTooBig as u8 {
            Some(u32::from_be_bytes(self.rest))
        } else {
            None
        }
    }

    /// Sets the MTU.
    pub fn set_mtu(&mut self, mtu: u32) {
        self.rest = mtu.to_be_bytes();
    }

    /// Returns the pointer if this is a Parameter Problem message.
    pub fn pointer(&self) -> Option<u32> {
        if self.icmpv6_type == Icmpv6Type::ParameterProblem as u8 {
            Some(u32::from_be_bytes(self.rest))
        } else {
            None
        }
    }

    /// Sets the pointer.
    pub fn set_pointer(&mut self, pointer: u32) {
        self.rest = pointer.to_be_bytes();
    }
}

/// Represents the header for an ICMPv6 Echo request or reply message.
/// This structure follows the main `Icmpv6Header`.
#[derive(FromBytes, IntoBytes, Unaligned, KnownLayout, Immutable, Debug, Clone, Copy)]
#[repr(C)]
pub struct Icmpv6Echo {
    pub identifier: U16<NetworkEndian>,
    pub sequence_number: U16<NetworkEndian>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Icmpv6Type {
    DestinationUnreachable = 1,
    PacketTooBig = 2,
    TimeExceeded = 3,
    ParameterProblem = 4,
    EchoRequest = 128,
    EchoReply = 129,
    RouterSolicitation = 133,
    RouterAdvertisement = 134,
    NeighborSolicitation = 135,
    NeighborAdvertisement = 136,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Icmpv6UnreachableCode {
    NoRoute = 0,
    AdminProhibited = 1,
    BeyondScope = 2,
    AddressUnreachable = 3,
    PortUnreachable = 4,
    PolicyFailed = 5,
    RejectRoute = 6,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Icmpv6TimeExceededCode {
    HopLimitExceeded = 0,
    ReassemblyTimeExceeded = 1,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Icmpv6ParameterProblemCode {
    ErroneousHeader = 0,
    UnrecognizedNextHeader = 1,
    UnrecognizedOption = 2,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_icmpv6_header_parsing() {
        // Echo Request: type=128, code=0
        let bytes: [u8; 8] = [128, 0x00, 0x12, 0x34, 0x56, 0x78, 0x9A, 0xBC];
        let (header, rest) = Icmpv6Header::parse(&bytes).expect("Failed to parse ICMPv6 header");

        assert_eq!(header.icmpv6_type, 128);
        assert_eq!(header.icmpv6_code, 0);
        assert_eq!(header.icmpv6_checksum.get(), 0x1234);
        assert_eq!(header.rest, [0x56, 0x78, 0x9A, 0xBC]);
        assert_eq!(rest.len(), 0);
    }

    #[test]
    fn test_icmpv6_header_parsing_with_payload() {
        let bytes: [u8; 12] =
            [128, 0x00, 0x12, 0x34, 0x56, 0x78, 0x9A, 0xBC, 0xDE, 0xF0, 0x12, 0x34];
        let (header, rest) = Icmpv6Header::parse(&bytes).expect("Failed to parse ICMPv6 header");

        assert_eq!(header.icmpv6_type, 128);
        assert_eq!(rest.len(), 4);
        assert_eq!(rest, &[0xDE, 0xF0, 0x12, 0x34]);
    }

    #[test]
    fn test_icmpv6_header_parsing_too_short() {
        let bytes: [u8; 7] = [128, 0x00, 0x12, 0x34, 0x56, 0x78, 0x9A];
        assert!(Icmpv6Header::parse(&bytes).is_none());
    }
}
