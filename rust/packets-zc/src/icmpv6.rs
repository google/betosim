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

//! Defines structures for ICMPv6 (Internet Control Message Protocol for IPv6) headers using `zerocopy`.

use crate::util::ParseResult;
use zerocopy::{
    FromBytes, Immutable, IntoBytes, KnownLayout, Ref, U16, Unaligned, byteorder::NetworkEndian,
};

/// Represents the ICMPv6 header.
#[derive(FromBytes, IntoBytes, Unaligned, KnownLayout, Immutable)]
#[repr(C)]
pub struct Icmpv6Header {
    /// The type of the ICMPv6 message.
    pub icmpv6_type: u8,
    /// The code of the ICMPv6 message, providing more specific information about the message type.
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
    pub fn parse(bytes: &[u8]) -> Option<ParseResult<Icmpv6Header>> {
        Ref::from_prefix(bytes).ok()
    }
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
