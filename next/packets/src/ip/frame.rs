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

use zerocopy::{
    FromBytes, Immutable, IntoBytes, KnownLayout, Ref, U16, U32, Unaligned,
    byteorder::NetworkEndian,
};

use crate::utils::general::ParseResult;

// Protocol numbers
pub const IP_P_HOPOPTS: u8 = 0;
pub const IP_P_ICMP: u8 = 1;
pub const IP_P_TCP: u8 = 6;
pub const IP_P_UDP: u8 = 17;
pub const IP_P_ICMPV6: u8 = 58;

/// Represents the IPv4 header.
#[derive(FromBytes, IntoBytes, Unaligned, KnownLayout, Immutable, Debug)]
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
    ///
    /// Advances past any IPv4 header options (`header_length() > 20`) and clips
    /// the returned payload slice to `total_length` so trailing link-layer
    /// padding (e.g. 60-byte minimum Ethernet frame padding) is excluded.
    pub fn parse(bytes: &[u8]) -> Option<ParseResult<'_, Ipv4Header>> {
        let (header, _) = Ref::<_, Ipv4Header>::from_prefix(bytes).ok()?;
        let hdr_len = header.header_length();
        let total_len = usize::from(header.total_length.get());
        if hdr_len < std::mem::size_of::<Ipv4Header>() || total_len < hdr_len {
            return None;
        }
        let end = bytes.len().min(total_len);
        let payload = bytes.get(hdr_len..end)?;
        Some((header, payload))
    }

    /// Returns the version of the IP protocol (should be 4).
    pub fn version(&self) -> u8 {
        self.version_ihl >> 4
    }

    /// Returns the Internet Header Length (IHL), which is the number of 32-bit
    /// words in the header.
    pub fn ihl(&self) -> u8 {
        self.version_ihl & 0x0f
    }

    pub fn header_length(&self) -> usize {
        (self.ihl() * 4) as usize
    }

    /// Calculates and updates the header checksum.
    pub fn update_checksum(&mut self) {
        self.header_checksum = U16::new(0);
        let bytes = self.as_bytes();
        let mut sum: u32 = 0;
        let (chunks, remainder) = bytes.as_chunks::<2>();
        for &chunk in chunks {
            sum += u32::from(u16::from_be_bytes(chunk));
        }
        if let &[last] = remainder {
            sum += (last as u32) << 8;
        }
        while (sum >> 16) != 0 {
            sum = (sum & 0xFFFF) + (sum >> 16);
        }
        self.header_checksum = U16::new(!sum as u16);
    }
}

/// Represents the IPv6 header.
#[derive(FromBytes, IntoBytes, Unaligned, KnownLayout, Immutable, Debug)]
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
    ///
    /// Clips the returned payload slice to `payload_length` so trailing
    /// link-layer padding is excluded.
    pub fn parse(bytes: &[u8]) -> Option<ParseResult<'_, Ipv6Header>> {
        let (header, rest) = Ref::<_, Ipv6Header>::from_prefix(bytes).ok()?;
        let payload_len = usize::from(header.payload_length.get());
        let end = rest.len().min(payload_len);
        let payload = rest.get(..end)?;
        Some((header, payload))
    }

    /// Returns the version of the IP protocol (should be 6).
    pub fn version(&self) -> u8 {
        (self.version_tc_fl.get() >> 28) as u8
    }

    /// Returns the fixed length of the IPv6 header (40 bytes).
    pub fn header_length(&self) -> usize {
        40
    }

    /// Creates a new Ipv6Header with default values for version, tc, fl, and hop_limit.
    pub fn new(source_addr: [u8; 16], dest_addr: [u8; 16], next_header: u8) -> Self {
        Self {
            version_tc_fl: U32::new(0x6000_0000), // Version 6
            payload_length: U16::new(0),
            next_header,
            hop_limit: 64,
            source_addr,
            dest_addr,
        }
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
    /// Parses an `Ipv6HopByHopHeader` from the beginning of the given byte
    /// slice.
    pub fn parse(bytes: &[u8]) -> Option<ParseResult<'_, Ipv6HopByHopHeader>> {
        let (header, _) = Ref::<&[u8], Ipv6HopByHopHeader>::from_prefix(bytes).ok()?;
        let header_len = header.header_length();
        Some((header, bytes.get(header_len..)?))
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

    #[test]
    fn test_ipv4_header_with_options_and_padding() {
        let mut bytes = Vec::new();
        // 20-byte fixed header with IHL = 6 (24 bytes) and total_length = 28
        bytes.extend_from_slice(&[
            0x46, 0x00, 0x00, 0x1C, 0x12, 0x34, 0x40, 0x00, 0x40, 0x06, 0x00, 0x00, 192, 168, 0, 1,
            192, 168, 0, 2,
        ]);
        // 4 bytes of options
        bytes.extend_from_slice(&[0x01, 0x01, 0x01, 0x01]);
        // 4 bytes of payload
        bytes.extend_from_slice(&[0xDE, 0xAD, 0xBE, 0xEF]);
        // 18 bytes of trailing Ethernet padding (total length = 46 bytes)
        bytes.extend_from_slice(&[0x00; 18]);

        let (header, payload) =
            Ipv4Header::parse(&bytes).expect("Failed to parse IPv4 header with options");

        assert_eq!(header.version(), 4);
        assert_eq!(header.ihl(), 6);
        assert_eq!(header.header_length(), 24);
        assert_eq!(header.total_length.get(), 28);
        assert_eq!(payload, &[0xDE, 0xAD, 0xBE, 0xEF]);

        // Truncated IPv4 slice where total_length (28) > available bytes (26)
        let (header, truncated_payload) =
            Ipv4Header::parse(&bytes[..26]).expect("Failed to parse truncated IPv4 slice");
        assert_eq!(header.total_length.get(), 28);
        assert_eq!(truncated_payload, &[0xDE, 0xAD]);
    }

    #[test]
    fn test_ipv4_header_invalid_lengths() {
        let valid_bytes: [u8; 20] = [
            0x45, 0x00, 0x00, 0x14, 0x12, 0x34, 0x40, 0x00, 0x40, 0x06, 0x12, 0x34, 192, 168, 0, 1,
            192, 168, 0, 2,
        ];

        // 1. IHL < 5 (version_ihl = 0x44 -> IHL = 4 -> 16 bytes < 20)
        let mut ihl_too_small = valid_bytes;
        ihl_too_small[0] = 0x44;
        assert!(Ipv4Header::parse(&ihl_too_small).is_none());

        // 2. bytes.len() < header_length() (IHL = 6 -> 24 bytes in 20-byte slice)
        let mut truncated_header = valid_bytes;
        truncated_header[0] = 0x46;
        truncated_header[2] = 0x00;
        truncated_header[3] = 0x1C; // total_length = 28
        assert!(Ipv4Header::parse(&truncated_header).is_none());

        // 3. total_length < header_length() (total_length = 16 < 20)
        let mut total_len_too_small = valid_bytes;
        total_len_too_small[2] = 0x00;
        total_len_too_small[3] = 0x10;
        assert!(Ipv4Header::parse(&total_len_too_small).is_none());
    }

    #[test]
    fn test_ipv6_header_clips_trailing_padding() {
        let mut bytes = Vec::new();
        // 40-byte IPv6 header with payload_length = 4
        bytes.extend_from_slice(&[
            0x60, 0x00, 0x00, 0x00, 0x00, 0x04, 0x06, 0x40, 0x20, 0x01, 0x0D, 0xB8, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x20, 0x01, 0x0D, 0xB8,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x02,
        ]);
        // 4 bytes payload
        bytes.extend_from_slice(&[0x01, 0x02, 0x03, 0x04]);
        // 10 bytes trailing padding
        bytes.extend_from_slice(&[0x00; 10]);

        let (header, payload) =
            Ipv6Header::parse(&bytes).expect("Failed to parse IPv6 header with padding");

        assert_eq!(header.payload_length.get(), 4);
        assert_eq!(payload, &[0x01, 0x02, 0x03, 0x04]);
    }
}
