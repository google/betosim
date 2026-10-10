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

use zerocopy::{FromBytes, U16};

use super::arp::ArpPacket;
use crate::ethernet::MacAddr;

/// A builder for creating ARP packets.
pub struct ArpPacketBuilder<'a> {
    packet: &'a mut ArpPacket,
}

impl<'a> ArpPacketBuilder<'a> {
    /// Creates a new `ArpPacketBuilder` from a mutable byte slice.
    pub fn new(buf: &'a mut [u8]) -> Option<Self> {
        ArpPacket::mut_from_bytes(buf).ok().map(|packet| Self { packet })
    }

    pub fn hardware_type(self, val: u16) -> Self {
        self.packet.hardware_type = U16::new(val);
        self
    }

    pub fn protocol_type(self, val: u16) -> Self {
        self.packet.protocol_type = U16::new(val);
        self
    }

    pub fn hardware_addr_len(self, val: u8) -> Self {
        self.packet.hardware_addr_len = val;
        self
    }

    pub fn protocol_addr_len(self, val: u8) -> Self {
        self.packet.protocol_addr_len = val;
        self
    }

    pub fn opcode(self, val: u16) -> Self {
        self.packet.opcode = U16::new(val);
        self
    }

    pub fn sender_hardware_addr(self, addr: MacAddr) -> Self {
        self.packet.sender_hardware_addr = addr;
        self
    }

    pub fn sender_protocol_addr(self, addr: [u8; 4]) -> Self {
        self.packet.sender_protocol_addr = addr;
        self
    }

    pub fn target_hardware_addr(self, addr: MacAddr) -> Self {
        self.packet.target_hardware_addr = addr;
        self
    }

    pub fn target_protocol_addr(self, addr: [u8; 4]) -> Self {
        self.packet.target_protocol_addr = addr;
        self
    }

    pub fn build(self) {}
}

#[cfg(test)]
mod tests {
    use super::{super::arp::ArpPacket, *};

    #[test]
    fn test_arp_packet_builder() {
        let mut buf = [0u8; 28];
        let builder = ArpPacketBuilder::new(&mut buf).unwrap();
        builder
            .hardware_type(1)
            .protocol_type(0x0800)
            .hardware_addr_len(6)
            .protocol_addr_len(4)
            .opcode(2)
            .sender_hardware_addr(MacAddr::new([1, 2, 3, 4, 5, 6]))
            .sender_protocol_addr([192, 168, 0, 1])
            .target_hardware_addr(MacAddr::new([7, 8, 9, 10, 11, 12]))
            .target_protocol_addr([192, 168, 0, 2])
            .build();

        let packet = ArpPacket::parse(&buf).unwrap();
        assert_eq!(packet.hardware_type.get(), 1);
        assert_eq!(packet.protocol_type.get(), 0x0800);
        assert_eq!(packet.opcode.get(), 2);
        assert_eq!(packet.sender_protocol_addr, [192, 168, 0, 1]);
        assert_eq!(packet.target_protocol_addr, [192, 168, 0, 2]);
    }
}
