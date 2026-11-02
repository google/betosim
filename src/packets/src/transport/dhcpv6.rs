// Copyright 2026 The Android Open Source Project
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

//! Defines structures for DHCPv6 (Dynamic Host Configuration Protocol for IPv6)
//! packets using `zerocopy`.

use zerocopy::{
    FromBytes, Immutable, IntoBytes, KnownLayout, Ref, U16, Unaligned, byteorder::NetworkEndian,
};

pub const DHCPV6_CLIENT_PORT: u16 = 546;
pub const DHCPV6_SERVER_PORT: u16 = 547;

// DHCPv6 Message Types
pub const MSG_REPLY: u8 = 7;
pub const MSG_INFORMATION_REQUEST: u8 = 11;

// DHCPv6 Option Codes
pub const OPTION_CLIENTID: u16 = 1;
pub const OPTION_SERVERID: u16 = 2;
pub const OPTION_DNS_SERVERS: u16 = 23;
pub const OPTION_DOMAIN_LIST: u16 = 24;

/// Represents the DHCPv6 Header (4 bytes).
#[repr(C)]
#[derive(FromBytes, IntoBytes, Unaligned, KnownLayout, Immutable, Debug, Clone, Copy)]
pub struct Dhcpv6Header {
    /// The message type.
    pub msg_type: u8,
    /// The transaction ID (24 bits).
    pub transaction_id: [u8; 3],
}

pub type Dhcpv6HeaderRef<'a> = Ref<&'a [u8], Dhcpv6Header>;

impl Dhcpv6Header {
    /// Parses a `Dhcpv6Header` from the beginning of the given byte slice.
    pub fn parse(bytes: &[u8]) -> Option<(Dhcpv6HeaderRef<'_>, &[u8])> {
        Ref::from_prefix(bytes).ok()
    }
}

/// Represents a DHCPv6 Option Header (4 bytes).
#[repr(C)]
#[derive(FromBytes, IntoBytes, Unaligned, KnownLayout, Immutable, Debug, Clone, Copy)]
pub struct Dhcpv6OptionHeader {
    /// The option code.
    pub code: U16<NetworkEndian>,
    /// The option length (excluding code and len fields).
    pub len: U16<NetworkEndian>,
}

pub type Dhcpv6OptionHeaderRef<'a> = Ref<&'a [u8], Dhcpv6OptionHeader>;

impl Dhcpv6OptionHeader {
    /// Parses a `Dhcpv6OptionHeader` from the beginning of the given byte
    /// slice.
    pub fn parse(bytes: &[u8]) -> Option<(Dhcpv6OptionHeaderRef<'_>, &[u8])> {
        Ref::from_prefix(bytes).ok()
    }
}

/// Helper iterator to safely parse DHCPv6 options from a byte slice.
pub struct Dhcpv6OptionIterator<'a> {
    buffer: &'a [u8],
}

impl<'a> Dhcpv6OptionIterator<'a> {
    pub fn new(buffer: &'a [u8]) -> Self {
        Self { buffer }
    }
}

impl<'a> Iterator for Dhcpv6OptionIterator<'a> {
    type Item = (u16, &'a [u8]);

    fn next(&mut self) -> Option<Self::Item> {
        if self.buffer.is_empty() {
            return None;
        }

        let (opt_hdr, rest) = Dhcpv6OptionHeader::parse(self.buffer)?;
        let opt_len = opt_hdr.len.get() as usize;
        if rest.len() < opt_len {
            // Truncated option!
            return None;
        }

        let (opt_val, remaining) = rest.split_at(opt_len);
        self.buffer = remaining;
        Some((opt_hdr.code.get(), opt_val))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dhcpv6_header_parsing() {
        let bytes: [u8; 4] = [11, 0x12, 0x34, 0x56];
        let (hdr, rest) = Dhcpv6Header::parse(&bytes).unwrap();
        assert_eq!(hdr.msg_type, 11);
        assert_eq!(hdr.transaction_id, [0x12, 0x34, 0x56]);
        assert_eq!(rest.len(), 0);
    }

    #[test]
    fn test_dhcpv6_option_iterator() {
        let bytes: [u8; 12] = [
            0x00, 0x17, // Option code 23 (DNS Servers)
            0x00, 0x04, // Option len 4
            0x11, 0x22, 0x33, 0x44, // Option val
            0x00, 0x18, // Option code 24 (Domain List)
            0x00, 0x00, // Option len 0
        ];

        let mut iter = Dhcpv6OptionIterator::new(&bytes);

        let (code1, val1) = iter.next().unwrap();
        assert_eq!(code1, 23);
        assert_eq!(val1, &[0x11, 0x22, 0x33, 0x44]);

        let (code2, val2) = iter.next().unwrap();
        assert_eq!(code2, 24);
        assert_eq!(val2.len(), 0);

        assert!(iter.next().is_none());
    }
}
