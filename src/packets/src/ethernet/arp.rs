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

//! Defines structures for ARP (Address Resolution Protocol) packets using
//! `zerocopy`.

use zerocopy::{
    FromBytes, Immutable, IntoBytes, KnownLayout, Ref, U16, Unaligned, byteorder::NetworkEndian,
};

use crate::ethernet::MacAddr;

/// Represents an ARP packet.
#[repr(C)]
#[derive(FromBytes, IntoBytes, Unaligned, KnownLayout, Immutable, Debug)]
pub struct ArpPacket {
    pub hardware_type: U16<NetworkEndian>,
    pub protocol_type: U16<NetworkEndian>,
    pub hardware_addr_len: u8,
    pub protocol_addr_len: u8,
    pub opcode: U16<NetworkEndian>,
    pub sender_hardware_addr: MacAddr,
    pub sender_protocol_addr: [u8; 4],
    pub target_hardware_addr: MacAddr,
    pub target_protocol_addr: [u8; 4],
}

impl ArpPacket {
    /// Parses an `ArpPacket` from the beginning of the given byte slice.
    pub fn parse(bytes: &[u8]) -> Option<Ref<&[u8], ArpPacket>> {
        Ref::from_prefix(bytes).ok().map(|(r, _)| r)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_arp_packet_parsing() {
        let bytes: [u8; 28] = [
            0x00, 0x01, // Hardware type: Ethernet
            0x08, 0x00, // Protocol type: IPv4
            0x06, // Hardware address length
            0x04, // Protocol address length
            0x00, 0x01, // Opcode: Request
            0x11, 0x22, 0x33, 0x44, 0x55, 0x66, // Sender MAC
            0xc0, 0xa8, 0x00, 0x01, // Sender IP
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // Target MAC
            0xc0, 0xa8, 0x00, 0x02, // Target IP
        ];

        let packet = ArpPacket::parse(&bytes).unwrap();
        assert_eq!(packet.hardware_type.get(), 1);
        assert_eq!(packet.protocol_type.get(), 0x0800);
        assert_eq!(packet.opcode.get(), 1);
        assert_eq!(packet.sender_protocol_addr, [192, 168, 0, 1]);
        assert_eq!(packet.target_protocol_addr, [192, 168, 0, 2]);
    }
}
