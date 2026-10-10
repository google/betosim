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

//! Defines structures for ICMP (Internet Control Message Protocol) headers using `zerocopy`.

use crate::util::ParseResult;
use zerocopy::{
    FromBytes, Immutable, IntoBytes, KnownLayout, Ref, U16, Unaligned, byteorder::NetworkEndian,
};

/// Represents the ICMP header.
#[derive(FromBytes, IntoBytes, Unaligned, KnownLayout, Immutable)]
#[repr(C)]
pub struct IcmpHeader {
    /// The type of the ICMP message.
    pub icmp_type: u8,
    /// The code of the ICMP message, providing more specific information about the message type.
    pub icmp_code: u8,
    /// The checksum of the ICMP header and data.
    pub icmp_checksum: U16<NetworkEndian>,
    /// The rest of the header, which varies depending on the message type.
    /// For echo request/reply, this contains the identifier and sequence number.
    pub rest: [u8; 4],
}

impl IcmpHeader {
    /// Parses an `IcmpHeader` from the beginning of the given byte slice.
    ///
    /// Returns a reference to the header and a slice for the remaining bytes.
    pub fn parse(bytes: &[u8]) -> Option<ParseResult<IcmpHeader>> {
        Ref::from_prefix(bytes).ok()
    }
}

/// Represents the header for an ICMP Echo (ping) request or reply message.
/// This structure follows the main `IcmpHeader`.
#[derive(FromBytes, IntoBytes, Unaligned, KnownLayout, Immutable)]
#[repr(C)]
pub struct IcmpEchoHeader {
    /// The identifier used to match echo requests and replies.
    pub id: U16<NetworkEndian>,
    /// The sequence number used to match echo requests and replies.
    pub sequence: U16<NetworkEndian>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use zerocopy::Ref;

    #[test]
    fn test_icmp_header_parsing() {
        // Echo Request: type=8, code=0
        let bytes: [u8; 8] = [0x08, 0x00, 0x12, 0x34, 0x56, 0x78, 0x9A, 0xBC];
        let (header, rest) = IcmpHeader::parse(&bytes).expect("Failed to parse ICMP header");

        assert_eq!(header.icmp_type, 8);
        assert_eq!(header.icmp_code, 0);
        assert_eq!(header.icmp_checksum.get(), 0x1234);
        assert_eq!(rest.len(), 0);

        // Check the `rest` field, which contains the echo header data
        let echo_header = Ref::<&[u8], IcmpEchoHeader>::from_bytes(&header.rest)
            .expect("Failed to parse echo header from rest");
        assert_eq!(echo_header.id.get(), 0x5678);
        assert_eq!(echo_header.sequence.get(), 0x9ABC);
    }

    #[test]
    fn test_icmp_header_parsing_with_payload() {
        let bytes: [u8; 12] =
            [0x08, 0x00, 0x12, 0x34, 0x56, 0x78, 0x9A, 0xBC, 0xDE, 0xF0, 0x12, 0x34];
        let (header, rest) = IcmpHeader::parse(&bytes).expect("Failed to parse ICMP header");

        assert_eq!(header.icmp_type, 8);
        assert_eq!(rest.len(), 4);
        assert_eq!(rest, &[0xDE, 0xF0, 0x12, 0x34]);
    }

    #[test]
    fn test_icmp_header_parsing_too_short() {
        let bytes: [u8; 7] = [0x08, 0x00, 0x12, 0x34, 0x56, 0x78, 0x9A];
        assert!(IcmpHeader::parse(&bytes).is_none());
    }
}
