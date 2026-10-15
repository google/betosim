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

//! Defines structures for representing Ethernet II frames using `zerocopy` for
//! zero-copy parsing.
//!
//! This module provides definitions for MAC addresses, EtherType constants, and
//! the Ethernet II frame header, suitable for high-performance network packet
//! analysis.

use core::fmt;
use std::str::FromStr;

use serde::{
    de::{self, Deserialize, Deserializer, Visitor},
    ser::{Serialize, SerializeStruct, Serializer},
};
use zerocopy::{
    FromBytes, Immutable, IntoBytes, KnownLayout, Ref, U16, Unaligned, byteorder::NetworkEndian,
};

use crate::utils::general::ParseResult;

/// Represents a 6-byte MAC address.
#[repr(C)]
#[derive(
    Debug, Copy, Clone, PartialEq, Eq, Hash, FromBytes, IntoBytes, Unaligned, Immutable, KnownLayout,
)]
pub struct MacAddr {
    /// The 6 bytes of the MAC address.
    pub bytes: [u8; 6],
}

impl MacAddr {
    /// Broadcast MAC address (FF:FF:FF:FF:FF:FF).
    pub const BROADCAST: Self = Self { bytes: [0xFF; 6] };

    /// Creates a new `MacAddr` from a 6-byte array.
    pub const fn new(bytes: [u8; 6]) -> Self {
        Self { bytes }
    }

    pub fn to_vec(&self) -> Vec<u8> {
        self.bytes.to_vec()
    }

    pub fn as_slice(&self) -> &[u8] {
        &self.bytes
    }

    pub fn is_multicast(&self) -> bool {
        let [b0, ..] = self.bytes;
        (b0 & 1) != 0
    }

    pub fn is_mdns(&self) -> bool {
        self.bytes == [0x01, 0x00, 0x5E, 0x00, 0x00, 0xFB]
            || self.bytes == [0x33, 0x33, 0x00, 0x00, 0x00, 0xFB]
    }

    pub fn is_broadcast(&self) -> bool {
        self.bytes == [0xFF; 6]
    }
}

impl From<[u8; 6]> for MacAddr {
    fn from(bytes: [u8; 6]) -> Self {
        Self { bytes }
    }
}

impl From<&[u8; 6]> for MacAddr {
    fn from(bytes: &[u8; 6]) -> Self {
        Self { bytes: *bytes }
    }
}

impl From<MacAddr> for [u8; 6] {
    fn from(mac: MacAddr) -> Self {
        mac.bytes
    }
}

impl fmt::Display for MacAddr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let [b0, b1, b2, b3, b4, b5] = self.bytes;
        write!(f, "{b0:02X}:{b1:02X}:{b2:02X}:{b3:02X}:{b4:02X}:{b5:02X}")
    }
}

impl FromStr for MacAddr {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let parts: Vec<&str> = s.split(':').collect();
        if parts.len() != 6 {
            return Err("MAC address must have 6 parts".to_string());
        }
        let mut bytes = [0u8; 6];
        for (byte, part) in bytes.iter_mut().zip(parts) {
            *byte = u8::from_str_radix(part, 16).map_err(|e| e.to_string())?;
        }
        Ok(MacAddr { bytes })
    }
}

impl TryFrom<&str> for MacAddr {
    type Error = String;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Self::from_str(value)
    }
}

impl Serialize for MacAddr {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for MacAddr {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct MacAddrVisitor;

        impl<'de> Visitor<'de> for MacAddrVisitor {
            type Value = MacAddr;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a MAC address string")
            }

            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                MacAddr::from_str(value).map_err(E::custom)
            }
        }

        deserializer.deserialize_str(MacAddrVisitor)
    }
}

/// EtherType constants representing common L3 protocols.
///
/// These values are used in the EtherType field of an Ethernet frame
/// to indicate the protocol of the encapsulated payload.
/// Values are standard `u16` and should be compared against the
/// host-endian value retrieved from `EthernetFrame.ethertype.get()`.
pub mod ether_type {
    /// EtherType for IPv4 (Internet Protocol version 4).
    pub const IPV4: u16 = 0x0800;
    /// EtherType for ARP (Address Resolution Protocol).
    pub const ARP: u16 = 0x0806;
    /// EtherType for IPv6 (Internet Protocol version 6).
    pub const IPV6: u16 = 0x86DD;
    /// EtherType for VLAN-tagged frames (IEEE 802.1Q).
    pub const VLAN: u16 = 0x8100;
    /// EtherType for EAPOL (Extensible Authentication Protocol over LAN).
    pub const EAPOL: u16 = 0x888E;
}

/// ARP operation codes (RFC 826).
pub mod arp_op {
    /// Asks which hardware address owns a protocol address.
    pub const REQUEST: u16 = 1;
    /// Answers a [`REQUEST`].
    pub const REPLY: u16 = 2;
}

/// ARP hardware address space identifiers (RFC 826).
pub mod arp_hardware {
    /// 10Mb Ethernet, and by extension every Ethernet-like link.
    pub const ETHERNET: u16 = 1;
}

/// Represents an Ethernet II frame header.
///
/// This struct is designed for zero-copy parsing of raw packet data.
/// It uses `#[repr(C)]` to ensure a C-compatible memory layout,
/// and derives `FromBytes`, `IntoBytes`, `Unaligned`, `Immutable`,
/// and `KnownLayout` from `zerocopy`
/// to allow safe casting from a byte slice.
#[repr(C)]
#[derive(FromBytes, IntoBytes, Unaligned, Immutable, KnownLayout, Debug)]
pub struct EthernetFrame {
    /// Destination MAC address (6 bytes).
    pub dst_addr: MacAddr,
    /// Source MAC address (6 bytes).
    pub src_addr: MacAddr,
    /// EtherType field (2 bytes), indicating the protocol of the payload.
    /// Stored in network byte order (big-endian). Use `.get()` to retrieve
    /// the value in host byte order.
    pub ethertype: U16<NetworkEndian>,
}

impl EthernetFrame {
    pub fn parse(bytes: &[u8]) -> Option<ParseResult<'_, EthernetFrame>> {
        Ref::from_prefix(bytes).ok()
    }

    /// Creates a new `EthernetFrame`.
    ///
    /// # Arguments
    /// * `dst_addr` - The destination MAC address.
    /// * `src_addr` - The source MAC address.
    /// * `ethertype_val` - The EtherType value (e.g., `ether_type::IPV4`).
    pub fn new(dst_addr: MacAddr, src_addr: MacAddr, ethertype_val: u16) -> Self {
        Self { dst_addr, src_addr, ethertype: U16::new(ethertype_val) }
    }
}

impl Serialize for EthernetFrame {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut state = serializer.serialize_struct("EthernetFrame", 3)?;
        state.serialize_field("dst_addr", &self.dst_addr)?;
        state.serialize_field("src_addr", &self.src_addr)?;
        state.serialize_field("ethertype", &self.ethertype.get())?;
        state.end()
    }
}

/// Represents the VLAN tag header (IEEE 802.1Q).
#[repr(C)]
#[derive(
    FromBytes, IntoBytes, Unaligned, Immutable, KnownLayout, Debug, Copy, Clone, PartialEq, Eq,
)]
pub struct VlanHeader {
    /// Tag Control Information (TCI), including VLAN ID, PCP, and DEI.
    pub tci: U16<NetworkEndian>,
    /// Inner EtherType field, indicating the protocol of the encapsulated
    /// payload.
    pub ethertype: U16<NetworkEndian>,
}

impl VlanHeader {
    /// Returns the Priority Code Point (PCP) value (3 bits).
    pub fn pcp(&self) -> u8 {
        (self.tci.get() >> 13) as u8
    }

    /// Returns the Drop Eligible Indicator (DEI) value (1 bit).
    pub fn dei(&self) -> u8 {
        ((self.tci.get() >> 12) & 1) as u8
    }

    /// Returns the VLAN Identifier (VID) value (12 bits).
    pub fn vid(&self) -> u16 {
        self.tci.get() & 0x0FFF
    }
}

impl Serialize for VlanHeader {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut state = serializer.serialize_struct("VlanHeader", 4)?;
        state.serialize_field("pcp", &self.pcp())?;
        state.serialize_field("dei", &self.dei())?;
        state.serialize_field("vid", &self.vid())?;
        state.serialize_field("ethertype", &self.ethertype.get())?;
        state.end()
    }
}

/// Represents a parsed Ethernet packet, which can be untagged or VLAN-tagged.
#[derive(Debug)]
pub enum EthernetPacket<'a> {
    /// An untagged Ethernet frame.
    Untagged {
        /// The Ethernet frame header.
        frame: Ref<&'a [u8], EthernetFrame>,
        /// The payload of the Ethernet frame.
        payload: &'a [u8],
    },
    /// A VLAN-tagged Ethernet frame (IEEE 802.1Q).
    Vlan {
        /// The outer Ethernet frame header, with EtherType = 0x8100.
        frame: Ref<&'a [u8], EthernetFrame>,
        /// The VLAN header.
        vlan_header: Ref<&'a [u8], VlanHeader>,
        /// The payload of the inner frame.
        payload: &'a [u8],
    },
}

impl<'a> Serialize for EthernetPacket<'a> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            EthernetPacket::Untagged { frame, payload } => {
                let mut state = serializer.serialize_struct("Untagged", 2)?;
                state.serialize_field("frame", &**frame)?;
                state.serialize_field("payload", payload)?;
                state.end()
            }
            EthernetPacket::Vlan { frame, vlan_header, payload } => {
                let mut state = serializer.serialize_struct("Vlan", 3)?;
                state.serialize_field("frame", &**frame)?;
                state.serialize_field("vlan_header", &**vlan_header)?;
                state.serialize_field("payload", payload)?;
                state.end()
            }
        }
    }
}

impl<'a> EthernetPacket<'a> {
    /// Parses a byte slice into an `EthernetPacket`.
    pub fn parse(bytes: &'a [u8]) -> Option<Self> {
        let (frame, rest) = EthernetFrame::parse(bytes)?;
        match frame.ethertype.get() {
            ether_type::VLAN => {
                let (vlan_header, payload) = Ref::<&'a [u8], VlanHeader>::from_prefix(rest).ok()?;
                Some(EthernetPacket::Vlan { frame, vlan_header, payload })
            }
            _ => Some(EthernetPacket::Untagged { frame, payload: rest }),
        }
    }
}

#[cfg(test)]
mod tests {
    use core::mem::size_of;

    use zerocopy::Ref;

    use super::*; // Updated to use Ref as per zerocopy 0.8.x

    /// Tests that the size of the EthernetFrame struct is correct.
    #[test]
    fn test_ethernet_frame_size() {
        assert_eq!(size_of::<EthernetFrame>(), 14, "EthernetFrame size should be 14 bytes");
    }

    /// Tests that a MacAddr can be displayed correctly.
    #[test]
    fn test_mac_addr_new() {
        let bytes = [0x11, 0x22, 0x33, 0xAA, 0xBB, 0xCC];
        let mac = MacAddr::new(bytes);
        assert_eq!(mac.bytes, bytes);
    }

    /// Tests the EthernetFrame::new constructor.
    #[test]
    fn test_ethernet_frame_new() {
        let dst_mac = MacAddr::new([0x01, 0x02, 0x03, 0x04, 0x05, 0x06]);
        let src_mac = MacAddr::new([0x0A, 0x0B, 0x0C, 0x0D, 0x0E, 0x0F]);
        let eth_type = ether_type::IPV6;
        let frame = EthernetFrame::new(dst_mac, src_mac, eth_type);
        assert_eq!(frame.dst_addr, dst_mac);
        assert_eq!(frame.src_addr, src_mac);
        assert_eq!(frame.ethertype.get(), eth_type);
    }

    #[test]
    fn test_mac_addr_display() {
        let mac = MacAddr { bytes: [0xDE, 0xAD, 0xBE, 0xEF, 0x01, 0x02] };
        assert_eq!(format!("{}", mac), "DE:AD:BE:EF:01:02");
    }

    #[test]
    fn test_mac_addr_serde() {
        let mac = MacAddr { bytes: [0xDE, 0xAD, 0xBE, 0xEF, 0x01, 0x02] };
        let json = serde_json::to_string(&mac).expect("Failed to serialize MacAddr");
        assert_eq!(json, "\"DE:AD:BE:EF:01:02\"");

        let deserialized: MacAddr =
            serde_json::from_str(&json).expect("Failed to deserialize MacAddr");
        assert_eq!(deserialized, mac);
    }

    /// Tests parsing a valid Ethernet frame byte slice using LayoutVerified.
    #[test]
    fn test_ethernet_frame_parsing() {
        // Sample Ethernet frame:
        // Dst MAC: DE:AD:BE:EF:00:01
        // Src MAC: C0:DE:FE:ED:00:02
        // EtherType: IPv4 (0x0800)
        let frame_bytes: [u8; 14] = [
            0xDE, 0xAD, 0xBE, 0xEF, 0x00, 0x01, // Dst MAC
            0xC0, 0xDE, 0xFE, 0xED, 0x00, 0x02, // Src MAC
            0x08, 0x00, // EtherType (IPv4)
        ];

        // Attempt to view the byte slice as an EthernetFrame
        let (frame, rest) = Ref::<&[u8], EthernetFrame>::from_prefix(&frame_bytes[..])
            .expect("Should successfully parse the frame prefix");

        assert!(
            rest.is_empty(),
            "Expected no remaining bytes after parsing prefix from exact-size input"
        );

        // Verify destination MAC address
        assert_eq!(
            frame.dst_addr.bytes,
            [0xDE, 0xAD, 0xBE, 0xEF, 0x00, 0x01],
            "Destination MAC address mismatch"
        );

        // Verify source MAC address
        assert_eq!(
            frame.src_addr.bytes,
            [0xC0, 0xDE, 0xFE, 0xED, 0x00, 0x02],
            "Source MAC address mismatch"
        );

        // Verify EtherType (converting from network to host byte order)
        assert_eq!(frame.ethertype.get(), ether_type::IPV4, "EtherType mismatch");

        // Test with a slice that is exactly the size of the frame
        let exact_frame_bytes: &[u8] = &[
            0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B, 0x0C, 0x08,
            0x06, // ARP
        ];
        let frame = Ref::<_, EthernetFrame>::from_bytes(exact_frame_bytes)
            .expect("Should successfully parse exact size frame");
        assert_eq!(frame.dst_addr.bytes, [0x01, 0x02, 0x03, 0x04, 0x05, 0x06],);
        assert_eq!(frame.src_addr.bytes, [0x07, 0x08, 0x09, 0x0A, 0x0B, 0x0C],);
        assert_eq!(frame.ethertype.get(), ether_type::ARP);

        // Test with an insufficient slice
        let short_bytes: &[u8] = &[0xDE, 0xAD, 0xBE, 0xEF, 0x00]; // Too short
        let short_result = Ref::<&[u8], EthernetFrame>::from_prefix(short_bytes);
        assert!(short_result.is_err(), "Should fail to parse too short slice");

        let short_result_exact = Ref::<&[u8], EthernetFrame>::from_bytes(short_bytes);
        assert!(
            short_result_exact.is_err(),
            "Should fail to parse too short slice with from_bytes"
        );
    }

    #[test]
    fn test_ethernet_frame_parsing_with_remainder() {
        // Frame + extra data
        let frame_bytes_extra: [u8; 16] = [
            0xDE, 0xAD, 0xBE, 0xEF, 0x00, 0x01, // Dst MAC
            0xC0, 0xDE, 0xFE, 0xED, 0x00, 0x02, // Src MAC
            0x08, 0x00, // EtherType (IPv4)
            0xFF, 0xFF, // Extra data
        ];

        let (frame, rest) = Ref::<&[u8], EthernetFrame>::from_prefix(&frame_bytes_extra[..])
            .expect("Should successfully parse the frame prefix with remainder");

        assert_eq!(frame.ethertype.get(), ether_type::IPV4);
        assert_eq!(rest.len(), 2, "Expected 2 remaining bytes");
        assert_eq!(rest, &[0xFF, 0xFF]);
    }

    #[test]
    fn test_vlan_frame_parsing() {
        // VLAN-tagged frame:
        // Dst MAC: 01:02:03:04:05:06
        // Src MAC: 0A:0B:0C:0D:0E:0F
        // Outer EtherType (TPID): 0x8100 (VLAN)
        // TCI: PCP=2, DEI=0, VID=101 -> 0x4065
        // Inner EtherType: 0x0800 (IPv4)
        // Payload: DEADBEEF
        let vlan_frame_bytes: [u8; 22] = [
            0x01, 0x02, 0x03, 0x04, 0x05, 0x06, // Dst MAC
            0x0A, 0x0B, 0x0C, 0x0D, 0x0E, 0x0F, // Src MAC
            0x81, 0x00, // Outer EtherType (VLAN)
            0x40, 0x65, // TCI (PCP=2, VID=101)
            0x08, 0x00, // Inner EtherType (IPv4)
            0xDE, 0xAD, 0xBE, 0xEF, // Payload
        ];

        let packet = EthernetPacket::parse(&vlan_frame_bytes).expect("Should parse VLAN frame");

        if let EthernetPacket::Vlan { frame, vlan_header, payload } = packet {
            assert_eq!(frame.dst_addr.bytes, [0x01, 0x02, 0x03, 0x04, 0x05, 0x06]);
            assert_eq!(frame.src_addr.bytes, [0x0A, 0x0B, 0x0C, 0x0D, 0x0E, 0x0F]);
            assert_eq!(frame.ethertype.get(), ether_type::VLAN);

            assert_eq!(vlan_header.tci.get(), 0x4065);
            assert_eq!(vlan_header.pcp(), 2);
            assert_eq!(vlan_header.dei(), 0);
            assert_eq!(vlan_header.vid(), 101);
            assert_eq!(vlan_header.ethertype.get(), ether_type::IPV4);

            assert_eq!(payload, &[0xDE, 0xAD, 0xBE, 0xEF]);
        } else {
            panic!("Parsed packet is not a VLAN packet");
        }
    }

    #[test]
    fn test_untagged_frame_parsing_with_ethernet_packet() {
        let frame_bytes: [u8; 18] = [
            0xDE, 0xAD, 0xBE, 0xEF, 0x00, 0x01, // Dst MAC
            0xC0, 0xDE, 0xFE, 0xED, 0x00, 0x02, // Src MAC
            0x08, 0x00, // EtherType (IPv4)
            0x11, 0x22, 0x33, 0x44, // Payload
        ];

        let packet = EthernetPacket::parse(&frame_bytes).expect("Should parse untagged frame");

        if let EthernetPacket::Untagged { frame, payload } = packet {
            assert_eq!(frame.dst_addr.bytes, [0xDE, 0xAD, 0xBE, 0xEF, 0x00, 0x01]);
            assert_eq!(frame.src_addr.bytes, [0xC0, 0xDE, 0xFE, 0xED, 0x00, 0x02]);
            assert_eq!(frame.ethertype.get(), ether_type::IPV4);
            assert_eq!(payload, &[0x11, 0x22, 0x33, 0x44]);
        } else {
            panic!("Parsed packet is not an Untagged packet");
        }
    }
}
