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

//! Defines structures for IPv4 and IPv6 headers using `zerocopy`.

use crate::util::ParseResult;
use zerocopy::{
    FromBytes, Immutable, IntoBytes, KnownLayout, Ref, U16, U32, Unaligned,
    byteorder::NetworkEndian,
};

// Protocol numbers
pub const IP_P_HOPOPTS: u8 = 0;
pub const IP_P_ICMP: u8 = 1;
pub const IP_P_ICMPV6: u8 = 58;

/// Represents the IPv4 header.
#[derive(FromBytes, IntoBytes, Unaligned, KnownLayout, Immutable)]
#[repr(C)]
pub struct Ipv4Header {
    pub version_ihl: u8,
    pub dscp_ecn: u8,
    pub total_length: U16<NetworkEndian>,
    pub identification: U16<NetworkEndian>,
    pub flags_fragment_offset: U16<NetworkEndian>,
    pub ttl: u8,
    pub protocol: u8,
    pub header_checksum: U16<NetworkEndian>,
    pub source_addr: [u8; 4],
    pub dest_addr: [u8; 4],
}

impl Ipv4Header {
    /// Parses an `Ipv4Header` from the beginning of the given byte slice.
    pub fn parse(bytes: &[u8]) -> Option<ParseResult<Ipv4Header>> {
        Ref::from_prefix(bytes).ok()
    }

    /// Returns the version of the IP protocol (should be 4).
    pub fn version(&self) -> u8 {
        self.version_ihl >> 4
    }

    /// Returns the Internet Header Length (IHL), which is the number of 32-bit words in the header.
    pub fn ihl(&self) -> u8 {
        self.version_ihl & 0x0f
    }

    /// Returns the total length of the header in bytes.
    pub fn header_length(&self) -> usize {
        (self.ihl() * 4) as usize
    }
}

/// Represents the IPv6 header.
#[derive(FromBytes, IntoBytes, Unaligned, KnownLayout, Immutable)]
#[repr(C)]
pub struct Ipv6Header {
    pub version_tc_fl: U32<NetworkEndian>,
    pub payload_length: U16<NetworkEndian>,
    pub next_header: u8,
    pub hop_limit: u8,
    pub source_addr: [u8; 16],
    pub dest_addr: [u8; 16],
}

impl Ipv6Header {
    /// Parses an `Ipv6Header` from the beginning of the given byte slice.
    pub fn parse(bytes: &[u8]) -> Option<ParseResult<Ipv6Header>> {
        Ref::from_prefix(bytes).ok()
    }

    /// Returns the version of the IP protocol (should be 6).
    pub fn version(&self) -> u8 {
        (self.version_tc_fl.get() >> 28) as u8
    }

    /// Returns the fixed length of the IPv6 header (40 bytes).
    pub fn header_length(&self) -> usize {
        40
    }
}

/// Represents the IPv6 Hop-by-Hop Options extension header.
#[derive(FromBytes, IntoBytes, Unaligned, KnownLayout, Immutable)]
#[repr(C)]
pub struct Ipv6HopByHopHeader {
    pub next_header: u8,
    pub hdr_ext_len: u8,
}

impl Ipv6HopByHopHeader {
    /// Parses an `Ipv6HopByHopHeader` from the beginning of the given byte slice.
    pub fn parse(bytes: &[u8]) -> Option<ParseResult<Ipv6HopByHopHeader>> {
        let (header, _) = Ref::<&[u8], Ipv6HopByHopHeader>::from_prefix(bytes).ok()?;
        let header_len = header.header_length();
        if bytes.len() < header_len {
            return None;
        }
        Some((header, &bytes[header_len..]))
    }

    /// Returns the length of the Hop-by-Hop header in bytes.
    pub fn header_length(&self) -> usize {
        (self.hdr_ext_len as usize + 1) * 8
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ipv4_header_parsing() {
        let bytes: [u8; 20] = [
            0x45, 0x00, 0x00, 0x14, 0x12, 0x34, 0x40, 0x00, 0x40, 0x06, 0x12, 0x34, 0xC0, 0xA8,
            0x00, 0x01, 0xC0, 0xA8, 0x00, 0x02,
        ];
        let (header, rest) = Ipv4Header::parse(&bytes).expect("Failed to parse IPv4 header");

        assert_eq!(header.version(), 4);
        assert_eq!(header.ihl(), 5);
        assert_eq!(header.header_length(), 20);
        assert_eq!(header.total_length.get(), 20);
        assert_eq!(header.protocol, 6); // TCP
        assert_eq!(header.source_addr, [192, 168, 0, 1]);
        assert_eq!(header.dest_addr, [192, 168, 0, 2]);
        assert_eq!(rest.len(), 0);
    }

    #[test]
    fn test_ipv6_header_parsing() {
        let bytes: [u8; 40] = [
            0x60, 0x00, 0x00, 0x00, 0x00, 0x14, 0x06, 0x40, 0x20, 0x01, 0x0D, 0xB8, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x20, 0x01, 0x0D, 0xB8,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x02,
        ];
        let (header, rest) = Ipv6Header::parse(&bytes).expect("Failed to parse IPv6 header");

        assert_eq!(header.version(), 6);
        assert_eq!(header.payload_length.get(), 20);
        assert_eq!(header.next_header, 6); // TCP
        assert_eq!(header.hop_limit, 64);
        assert_eq!(rest.len(), 0);
    }

    #[test]
    fn test_ipv6_hop_by_hop_header_parsing() {
        let bytes: [u8; 8] = [0x11, 0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06];
        let (header, rest) =
            Ipv6HopByHopHeader::parse(&bytes).expect("Failed to parse Hop-by-Hop header");

        assert_eq!(header.next_header, 17); // UDP
        assert_eq!(header.hdr_ext_len, 0);
        assert_eq!(header.header_length(), 8);
        assert_eq!(rest.len(), 0);
    }
}
