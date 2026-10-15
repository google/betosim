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

//! Defines structures for representing IEEE 802.11 frames using `zerocopy`.
//!
//! This module provides definitions for various 802.11 MAC frame components,
//! suitable for zero-copy parsing of raw wireless packets.
//! It focuses on common frame types and their headers.

use core::fmt;

use zerocopy::byteorder::LittleEndian; // IEEE 802.11 fields are typically little-endian
use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout, U16, Unaligned};

use crate::ethernet::MacAddr;
pub use crate::ethernet::MacAddr as MacAddress;

/// Represents the 2-byte Frame Control field in an 802.11 header.
///
/// The Frame Control field is structured as follows (LSB to MSB for bits within
/// a byte):
/// - Protocol Version (2 bits): Bits 0-1
/// - Type (2 bits): Bits 2-3
/// - Subtype (4 bits): Bits 4-7
/// - To DS (1 bit): Bit 8 (or Bit 0 of the second byte)
/// - From DS (1 bit): Bit 9 (or Bit 1 of the second byte)
/// - More Fragments (1 bit): Bit 10
/// - Retry (1 bit): Bit 11
/// - Power Management (1 bit): Bit 12
/// - More Data (1 bit): Bit 13
/// - Protected Frame (1 bit): Bit 14
/// - Order (1 bit): Bit 15
#[repr(C)]
#[derive(FromBytes, IntoBytes, Unaligned, Immutable, KnownLayout, Copy, Clone)]
pub struct FrameControl {
    /// The raw 2-byte value of the Frame Control field, stored in
    /// little-endian.
    pub field: U16<LittleEndian>,
}

/// Bitmasks for FrameControl fields.
mod fc_bits {
    pub const PROTOCOL_VERSION_MASK: u16 = 0b0000_0000_0000_0011;
    pub const TYPE_MASK: u16 = 0b0000_0000_0000_1100;
    pub const SUBTYPE_MASK: u16 = 0b0000_0000_1111_0000;
    pub const TO_DS_MASK: u16 = 0b0000_0001_0000_0000;
    pub const FROM_DS_MASK: u16 = 0b0000_0010_0000_0000;
    pub const MORE_FRAGMENTS_MASK: u16 = 0b0000_0100_0000_0000;
    pub const RETRY_MASK: u16 = 0b0000_1000_0000_0000;
    pub const POWER_MANAGEMENT_MASK: u16 = 0b0001_0000_0000_0000;
    pub const MORE_DATA_MASK: u16 = 0b0010_0000_0000_0000;
    pub const PROTECTED_FRAME_MASK: u16 = 0b0100_0000_0000_0000;
    pub const ORDER_MASK: u16 = 0b1000_0000_0000_0000;
}
impl FrameControl {
    /// Creates a new FrameControl field.
    pub const fn new(value: u16) -> Self {
        Self { field: U16::new(value) }
    }

    /// Gets the raw u16 value.
    pub fn get(&self) -> u16 {
        self.field.get()
    }

    /// Protocol Version (2 bits). Should typically be 0.
    pub fn protocol_version(&self) -> u8 {
        (self.get() & fc_bits::PROTOCOL_VERSION_MASK) as u8
    }

    /// Frame Type (2 bits). See `FrameType` constants.
    pub fn frame_type(&self) -> u8 {
        ((self.get() & fc_bits::TYPE_MASK) >> 2) as u8
    }

    /// Frame Subtype (4 bits). Meaning depends on Frame Type. See
    /// `FrameSubtype` constants.
    pub fn frame_subtype(&self) -> u8 {
        ((self.get() & fc_bits::SUBTYPE_MASK) >> 4) as u8
    }

    /// To DS flag (1 bit).
    pub fn to_ds(&self) -> bool {
        (self.get() & fc_bits::TO_DS_MASK) != 0
    }

    /// From DS flag (1 bit).
    pub fn from_ds(&self) -> bool {
        (self.get() & fc_bits::FROM_DS_MASK) != 0
    }

    /// More Fragments flag (1 bit).
    pub fn more_fragments(&self) -> bool {
        (self.get() & fc_bits::MORE_FRAGMENTS_MASK) != 0
    }

    /// Retry flag (1 bit).
    pub fn retry(&self) -> bool {
        (self.get() & fc_bits::RETRY_MASK) != 0
    }

    /// Power Management flag (1 bit).
    pub fn power_management(&self) -> bool {
        (self.get() & fc_bits::POWER_MANAGEMENT_MASK) != 0
    }

    /// More Data flag (1 bit).
    pub fn more_data(&self) -> bool {
        (self.get() & fc_bits::MORE_DATA_MASK) != 0
    }

    /// Protected Frame flag (1 bit) (e.g., WEP or WPA/WPA2).
    pub fn protected_frame(&self) -> bool {
        (self.get() & fc_bits::PROTECTED_FRAME_MASK) != 0
    }

    /// Order flag (1 bit) (Strictly Ordered).
    pub fn order(&self) -> bool {
        (self.get() & fc_bits::ORDER_MASK) != 0
    }
}

impl fmt::Debug for FrameControl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FrameControl")
            .field("value", &format_args!("0x{:04X}", self.get()))
            .field("version", &self.protocol_version())
            .field("type", &self.frame_type())
            .field("subtype", &self.frame_subtype())
            .field("to_ds", &self.to_ds())
            .field("from_ds", &self.from_ds())
            .field("more_frag", &self.more_fragments())
            .field("retry", &self.retry())
            .field("pwr_mgmt", &self.power_management())
            .field("more_data", &self.more_data())
            .field("protected", &self.protected_frame())
            .field("order", &self.order())
            .finish()
    }
}

#[cfg(test)]
pub mod frame_type {
    pub const MANAGEMENT: u8 = 0b00;
    pub const CONTROL: u8 = 0b01;
    pub const DATA: u8 = 0b10;
    // 0b11 is reserved
}

#[cfg(test)]
pub mod data_subtype {
    pub const DATA: u8 = 0b0000;
    pub const DATA_CF_ACK: u8 = 0b0001;
    pub const DATA_CF_POLL: u8 = 0b0010;
    pub const DATA_CF_ACK_POLL: u8 = 0b0011;
    pub const NULL: u8 = 0b0100; // No data
    pub const CF_ACK: u8 = 0b0101; // No data
    pub const CF_POLL: u8 = 0b0110; // No data
    pub const CF_ACK_POLL: u8 = 0b0111; // No data
    pub const QOS_DATA: u8 = 0b1000;
    // Other QoS subtypes exist
}

/// IEEE 802.11 Frame Subtypes for Management frames.
pub mod management_subtype {
    pub const ASSOCIATION_REQUEST: u8 = 0b0000;
    pub const ASSOCIATION_RESPONSE: u8 = 0b0001;
    pub const REASSOCIATION_REQUEST: u8 = 0b0010;
    pub const REASSOCIATION_RESPONSE: u8 = 0b0011;
    pub const PROBE_REQUEST: u8 = 0b0100;
    pub const PROBE_RESPONSE: u8 = 0b0101;
    // 0b0110, 0b0111 reserved
    pub const BEACON: u8 = 0b1000;
    pub const ATIM: u8 = 0b1001;
    pub const DISASSOCIATION: u8 = 0b1010;
    pub const AUTHENTICATION: u8 = 0b1011;
    pub const DEAUTHENTICATION: u8 = 0b1100;
    pub const ACTION: u8 = 0b1101;
    // Other subtypes exist
}

/// Represents the 2-byte Sequence Control field.
/// Fragment Number (4 bits), Sequence Number (12 bits).
#[repr(C)]
#[derive(FromBytes, IntoBytes, Unaligned, Immutable, KnownLayout, Copy, Clone, Debug)]
pub struct SequenceControl {
    pub field: U16<LittleEndian>,
}

mod sc_bits {
    pub const FRAGMENT_NUMBER_MASK: u16 = 0x000F;
    pub const SEQUENCE_NUMBER_MASK: u16 = 0xFFF0;
    pub const SEQUENCE_NUMBER_SHIFT: u16 = 4;
}
impl SequenceControl {
    pub fn new(value: u16) -> Self {
        Self { field: U16::new(value) }
    }
    pub fn get(&self) -> u16 {
        self.field.get()
    }
    pub fn fragment_number(&self) -> u8 {
        (self.get() & sc_bits::FRAGMENT_NUMBER_MASK) as u8
    }
    pub fn sequence_number(&self) -> u16 {
        (self.get() & sc_bits::SEQUENCE_NUMBER_MASK) >> sc_bits::SEQUENCE_NUMBER_SHIFT
    }
}

/// Represents a generic IEEE 802.11 MAC header with 3 addresses.
/// This is common for Data frames in an IBSS or frames to/from DS.
/// The exact meaning of addr1, addr2, addr3 depends on ToDS/FromDS flags.
/// For ToDS=0, FromDS=0 (e.g. IBSS data, management frames):
///   Addr1: DA (Destination Address)
///   Addr2: SA (Source Address)
///   Addr3: BSSID
/// For ToDS=1, FromDS=0 (e.g. Data from STA to AP):
///   Addr1: BSSID (AP MAC)
///   Addr2: SA (STA MAC)
///   Addr3: DA
/// For ToDS=0, FromDS=1 (e.g. Data from AP to STA):
///   Addr1: DA (STA MAC)
///   Addr2: BSSID (AP MAC)
///   Addr3: SA
/// A 4th address is present if ToDS=1 and FromDS=1 (WDS frames).
#[repr(C)]
#[derive(FromBytes, IntoBytes, Unaligned, Immutable, KnownLayout, Debug)]
pub struct MacHeader3Addr {
    /// Frame Control field.
    pub frame_control: FrameControl,
    /// Duration/ID field.
    pub duration_id: U16<LittleEndian>,
    /// Address 1. Meaning depends on ToDS/FromDS flags.
    pub addr1: MacAddr,
    /// Address 2. Meaning depends on ToDS/FromDS flags.
    pub addr2: MacAddr,
    /// Address 3. Meaning depends on ToDS/FromDS flags.
    pub addr3: MacAddr,
    /// Sequence Control field.
    pub sequence_control: SequenceControl,
    // Addr4 would be here if present (e.g. WDS)
    // QoS Control would be here if it's a QoS Data frame
    // HT Control would be after Addr4/QoS if present
}

impl MacHeader3Addr {
    /// Creates a new generic 3-address MAC header.
    pub fn new(
        frame_control: FrameControl,
        duration_id: u16,
        addr1: MacAddr,
        addr2: MacAddr,
        addr3: MacAddr,
        sequence_control: SequenceControl,
    ) -> Self {
        Self {
            frame_control,
            duration_id: U16::new(duration_id),
            addr1,
            addr2,
            addr3,
            sequence_control,
        }
    }
}

/// Represents an IEEE 802.11 Data frame header (basic, no QoS, 3 addresses).
/// This is a common structure for simple data transmissions.
#[repr(C)]
#[derive(FromBytes, IntoBytes, Unaligned, Immutable, KnownLayout, Debug)]
pub struct DataFrameHeader {
    /// Frame Control field. Type should be DATA.
    pub frame_control: FrameControl,
    /// Duration/ID field.
    pub duration_id: U16<LittleEndian>,
    /// Address 1: Typically Receiver Address (RA) or Destination Address (DA).
    pub addr1: MacAddr,
    /// Address 2: Typically Transmitter Address (TA) or Source Address (SA).
    pub addr2: MacAddr,
    /// Address 3: Varies (e.g., BSSID, SA, DA).
    pub addr3: MacAddr,
    /// Sequence Control field.
    pub sequence_control: SequenceControl,
    // QoS Control (2 bytes) would be here if frame_control indicates QoS Data.
    // HT Control (4 bytes) might follow if it's an HT frame.
}

impl DataFrameHeader {
    /// Creates a new Data Frame Header.
    pub fn new(
        frame_control: FrameControl,
        duration_id: u16,
        addr1: MacAddr,
        addr2: MacAddr,
        addr3: MacAddr,
        sequence_control: SequenceControl,
    ) -> Self {
        Self {
            frame_control,
            duration_id: U16::new(duration_id),
            addr1,
            addr2,
            addr3,
            sequence_control,
        }
    }
}

/// CCMP Header (8 bytes).
#[repr(C)]
#[derive(FromBytes, IntoBytes, Unaligned, Immutable, KnownLayout, Debug, Copy, Clone)]
pub struct CcmpHeader {
    pub pn0: u8,
    pub pn1: u8,
    pub rsvd: u8,
    pub key_id: u8, // Key ID (bits 6-7) | ExtIV (bit 5) | Rsvd (0-4)
    pub pn2: u8,
    pub pn3: u8,
    pub pn4: u8,
    pub pn5: u8,
}

/// Fixed parameters in an Association Request frame.
#[repr(C)]
#[derive(FromBytes, IntoBytes, Unaligned, Immutable, KnownLayout, Debug, Copy, Clone)]
pub struct AssociationRequestFixedFields {
    pub capabilities: U16<LittleEndian>,
    pub listen_interval: U16<LittleEndian>,
}

/// Fixed parameters in an Authentication frame (6 bytes).
#[repr(C)]
#[derive(FromBytes, IntoBytes, Unaligned, Immutable, KnownLayout, Debug, Copy, Clone)]
pub struct AuthenticationFixedFields {
    /// Authentication Algorithm Number (2 bytes).
    pub algorithm: U16<LittleEndian>,
    /// Authentication Transaction Sequence Number (2 bytes).
    pub sequence: U16<LittleEndian>,
    /// Status Code (2 bytes).
    pub status: U16<LittleEndian>,
}

/// Fixed parameters in an Association Response frame (6 bytes).
#[repr(C)]
#[derive(FromBytes, IntoBytes, Unaligned, Immutable, KnownLayout, Debug, Copy, Clone)]
pub struct AssociationResponseFixedFields {
    /// Capability Information (2 bytes).
    pub capabilities: U16<LittleEndian>,
    /// Status Code (2 bytes).
    pub status: U16<LittleEndian>,
    /// Association ID (AID) (2 bytes).
    pub aid: U16<LittleEndian>,
}

/// IEEE 802.11 Frame Type Enum
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameType {
    Management = 0,
    Control = 1,
    Data = 2,
    Reserved = 3,
}

/// IEEE 802.11 Data Frame Subtypes
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataSubType {
    Data = 0,
    DataCfAck = 1,
    DataCfPoll = 2,
    DataCfAckPoll = 3,
    Nodata = 4,
    CfAck = 5,
    CfPoll = 6,
    CfAckPoll = 7,
    QosData = 8,
    QosDataCfAck = 9,
    QosDataCfPoll = 10,
    QosDataCfAckPoll = 11,
    QosNodata = 12,
    QosCfAck = 13,
    QosCfPoll = 14,
    QosCfAckPoll = 15,
}

impl From<DataSubType> for u8 {
    fn from(val: DataSubType) -> Self {
        val as u8
    }
}

/// Direction of the frame for conversion from IEEE 802.3
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameDirection {
    /// Uplink: From Generic Station to Access Point (ToDS=1, FromDS=0)
    ToAp,
    /// Downlink: From Access Point to Generic Station (ToDS=0, FromDS=1)
    FromAp,
}

/// A generic wrapper for IEEE 802.11 frames, providing helper methods.
#[derive(Debug, Clone)]
pub struct Ieee80211 {
    bytes: Vec<u8>,
}

impl fmt::Display for Ieee80211 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Ieee80211 {{ len: {} }}", self.bytes.len())
    }
}

impl Ieee80211 {
    pub fn decode(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() < 10 {
            return Err(format!("Packet too short: len {} < 10", bytes.len()));
        }
        Ok(Self { bytes: bytes.to_vec() })
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn is_multicast(&self) -> bool {
        // Addr1 is at offset 4
        self.bytes.get(4).is_some_and(|b| b & 1 == 1)
    }

    pub fn is_broadcast(&self) -> bool {
        self.bytes.get(4..10) == Some(&[0xFF; 6])
    }

    pub fn needs_encryption(&self) -> bool {
        // Frame Control byte 1, bit 6 (0x40) is Protected Frame
        self.bytes.get(1).is_some_and(|b| b & 0x40 != 0)
    }

    pub fn needs_decryption(&self) -> bool {
        self.needs_encryption()
    }

    pub fn hdr_length(&self) -> usize {
        let Some(&fc_bytes) = self.bytes.first_chunk::<2>() else {
            return 24;
        };
        let fc = u16::from_le_bytes(fc_bytes);
        let to_ds = (fc & 0x0100) != 0;
        let from_ds = (fc & 0x0200) != 0;
        let mut len = if to_ds && from_ds { 30 } else { 24 };

        // QoS Data check: Type Data (10) and Subtype has bit 3 (1000) set
        // FC bits 2-3 are Type. FC bits 4-7 are Subtype.
        // Type Data is 10 binary -> 0x0008 mask in u16?
        // Byte 0: [Subtype 4][Type 2][Ver 2]
        // Data Type: 10 binary -> bit 3 (0x08) set. Mask 0x0C.
        // QoS Subtype: 1xxx binary -> bit 7 (0x80) set. Mask 0xF0.
        if (fc & 0x000C) == 0x0008 && (fc & 0x0080) != 0 {
            len += 2;
        }

        // HT Control check: Order bit 15 is set (0x8000) in 802.11n
        if (fc & 0x8000) != 0 {
            len += 4;
        }

        len
    }

    pub fn get_nonce(&self, pn: &[u8]) -> [u8; 13] {
        let mut nonce = [0u8; 13];
        if let (Some(dest), Some(src)) = (nonce.get_mut(1..7), self.bytes.get(10..16)) {
            dest.copy_from_slice(src);
        }
        if let (Some(dest), Some(src)) = (nonce.get_mut(7..13), pn.get(..6)) {
            dest.copy_from_slice(src);
        }
        nonce
    }

    pub fn get_payload(&self) -> Vec<u8> {
        let offset = self.hdr_length();
        self.bytes.get(offset..).map_or_else(Vec::new, |p| p.to_vec())
    }

    pub fn get_aad(&self) -> Vec<u8> {
        let Some((hdr, _)) = self.bytes.split_first_chunk::<24>() else {
            return Vec::new();
        };
        let &[fc0, fc1, _, _, ref addr1_2_3 @ .., seq0, _] = hdr;
        // Mask FC: Retry(11), PwrMgmt(12), MoreData(13) -> 0 (!0x38 in fc1)
        // Protected(14) -> 1 (| 0x40 in fc1)
        let mut aad =
            [&[fc0, (fc1 & !0x38) | 0x40][..], addr1_2_3.as_slice(), &[seq0 & 0x0f, 0x00]].concat();

        let fc = u16::from_le_bytes([fc0, fc1]);

        let to_ds = (fc & 0x0100) != 0;
        let from_ds = (fc & 0x0200) != 0;
        let has_addr4 = to_ds && from_ds;
        let mut qos_offset = 24;

        if has_addr4 && let Some(addr4) = self.bytes.get(24..30) {
            aad.extend_from_slice(addr4);
            qos_offset = 30;
        }

        // QoS Control masked (TID kept, bits 4-15 -> 0)
        let is_qos = (fc & 0x000C) == 0x0008 && (fc & 0x0080) != 0;
        if is_qos && let Some(&[qos_ctrl_0, _]) = self.bytes.get(qos_offset..qos_offset + 2) {
            aad.extend_from_slice(&[qos_ctrl_0 & 0x0F, 0x00]);
        }

        aad
    }

    pub fn get_packet_number(&self) -> [u8; 6] {
        let offset = self.hdr_length();
        if let Some(&[c0, c1, _, _, c4, c5, c6, c7]) =
            self.bytes.get(offset..).and_then(|s| s.first_chunk::<8>())
        {
            [c7, c6, c5, c4, c1, c0]
        } else {
            [0; 6]
        }
    }

    fn get_addr_at(&self, offset: usize) -> MacAddress {
        self.bytes
            .get(offset..offset + 6)
            .and_then(|slice| slice.try_into().ok())
            .map(MacAddress::new)
            .unwrap_or(MacAddress::new([0; 6]))
    }

    pub fn get_addr1(&self) -> MacAddress {
        self.get_addr_at(4)
    }

    pub fn get_addr2(&self) -> MacAddress {
        self.get_addr_at(10)
    }

    pub fn get_addr3(&self) -> MacAddress {
        self.get_addr_at(16)
    }

    pub fn get_fc(&self) -> u16 {
        self.bytes.first_chunk::<2>().map_or(0, |&b| u16::from_le_bytes(b))
    }

    pub fn is_to_ds(&self) -> bool {
        (self.get_fc() & 0x0100) != 0
    }

    pub fn is_from_ds(&self) -> bool {
        (self.get_fc() & 0x0200) != 0
    }

    pub fn get_destination(&self) -> MacAddress {
        if self.is_to_ds() { self.get_addr3() } else { self.get_addr1() }
    }

    pub fn set_destination(&mut self, addr: &MacAddress) {
        let offset = if self.is_to_ds() {
            16 // Addr3
        } else {
            4 // Addr1
        };
        if let Some(dest_slice) = self.bytes.get_mut(offset..offset + 6) {
            dest_slice.copy_from_slice(&addr.bytes);
        }
    }

    pub fn get_source(&self) -> MacAddress {
        if self.is_from_ds() {
            if self.is_to_ds() { self.get_addr_at(24) } else { self.get_addr3() }
        } else {
            self.get_addr2()
        }
    }

    pub fn get_bssid(&self) -> Option<MacAddress> {
        if self.is_to_ds() && self.is_from_ds() {
            None
        } else if self.is_to_ds() {
            Some(self.get_addr1())
        } else if self.is_from_ds() {
            Some(self.get_addr2())
        } else {
            Some(self.get_addr3())
        }
    }

    pub fn is_mgmt(&self) -> bool {
        (self.get_fc() & 0x000C) == 0
    }

    pub fn is_data(&self) -> bool {
        (self.get_fc() & 0x000C) == 0x0008
    }

    /// Checks if this frame is a payload-bearing data frame (Data or QoS Data).
    /// Excluding Data frames without a frame body (e.g., Null or CF-Ack).
    pub fn is_payload_bearing_data(&self) -> bool {
        // Bit 2 (0x04) of the 4-bit Subtype field is the 'No Data' bit for Type 2
        // (Data) frames in the IEEE 802.11 spec. It is 0 for data-bearing
        // frames, and 1 for frames without a frame body (like Null Data or QoS
        // Null).
        self.is_data() && (self.stype() & 0x04) == 0
    }

    pub fn stype(&self) -> u8 {
        ((self.get_fc() & 0x00F0) >> 4) as u8
    }

    pub fn is_to_ap(&self) -> bool {
        self.is_to_ds() && !self.is_from_ds()
    }

    pub fn is_eapol(&self) -> Result<bool, String> {
        let offset = self.hdr_length();
        let Some(llc) = self.bytes.get(offset..offset + 8) else {
            return Ok(false);
        };
        Ok(llc == [0xAA, 0xAA, 0x03, 0x00, 0x00, 0x00, 0x88, 0x8E])
    }

    pub fn is_qos_data(&self) -> bool {
        self.is_data() && (self.stype() & 0x8 != 0)
    }

    pub fn is_qos_nodata(&self) -> bool {
        self.is_data() && self.stype() == DataSubType::QosNodata as u8
    }

    pub fn decode_full(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() < 10 {
            return Err(format!("Packet too short: len {} < 10", bytes.len()));
        }
        Ok(Self { bytes: bytes.to_vec() })
    }

    pub fn encode_to_vec(&self) -> Result<Vec<u8>, String> {
        Ok(self.bytes.clone())
    }

    pub fn from_ieee8023(
        packet: &[u8],
        bssid: MacAddress,
        direction: FrameDirection,
        seq: u16,
    ) -> Result<Self, String> {
        Self::from_ieee8023_qos(packet, bssid, direction, false, seq)
    }

    pub fn from_ieee8023_qos(
        packet: &[u8],
        bssid: MacAddress,
        direction: FrameDirection,
        is_qos: bool,
        seq: u16,
    ) -> Result<Self, String> {
        let Some((&dst_bytes, rest)) = packet.split_first_chunk::<6>() else {
            return Err("Packet too short".to_string());
        };
        let Some((&src_bytes, rest)) = rest.split_first_chunk::<6>() else {
            return Err("Packet too short".to_string());
        };
        let Some((&ethertype, payload)) = rest.split_first_chunk::<2>() else {
            return Err("Packet too short".to_string());
        };
        let dst = MacAddress::new(dst_bytes);
        let src = MacAddress::new(src_bytes);

        // If is_qos: Data (0x88)
        let fc: u16 = match (direction, is_qos) {
            (FrameDirection::FromAp, true) => 0x0288,
            (FrameDirection::FromAp, false) => 0x0208,
            (FrameDirection::ToAp, true) => 0x0188,
            (FrameDirection::ToAp, false) => 0x0108,
        };

        let (addr1, addr2, addr3) = match direction {
            FrameDirection::FromAp => (&dst.bytes, &bssid.bytes, &src.bytes),
            FrameDirection::ToAp => (&bssid.bytes, &src.bytes, &dst.bytes),
        };

        let seq_ctrl = (seq << 4).to_le_bytes();
        let qos_slice: &[u8] = if is_qos { &[0x00, 0x00] } else { &[] };

        let new_packet = [
            &fc.to_le_bytes()[..],
            &0u16.to_le_bytes(), // Duration/ID
            addr1.as_slice(),
            addr2.as_slice(),
            addr3.as_slice(),
            &seq_ctrl,
            qos_slice,
            &[0xAA, 0xAA, 0x03, 0x00, 0x00, 0x00],
            &ethertype,
            payload,
        ]
        .concat();

        Ok(Self { bytes: new_packet })
    }

    pub fn to_ieee8023(&self) -> Result<Vec<u8>, String> {
        let da = self.get_destination();
        let sa = self.get_source();
        let payload = self.get_payload();

        let Some((&[0xAA, 0xAA, 0x03, 0x00, 0x00, 0x00], rest @ [_, _, ..])) =
            payload.split_at_checked(6)
        else {
            return Err("Not LLC/SNAP encapsulated or unknown OUI".to_string());
        };

        Ok([da.bytes.as_slice(), sa.bytes.as_slice(), rest].concat())
    }

    pub fn into_from_ap(&self) -> Result<Ieee80211FromAp, String> {
        let Some((header, _)) = self.bytes.split_first_chunk::<24>() else {
            return Err("Frame too short".to_string());
        };
        let &[_, _, dur0, dur1, .., seq0, seq1] = header;
        let duration_id = u16::from_le_bytes([dur0, dur1]);
        let seq_ctrl = u16::from_le_bytes([seq0, seq1]);

        let fc = self.get_fc();
        let ftype = match (fc & 0x000C) >> 2 {
            0 => FrameType::Management,
            1 => FrameType::Control,
            2 => FrameType::Data,
            _ => FrameType::Reserved,
        };
        let stype = ((fc & 0x00F0) >> 4) as u8;

        let is_qos = (fc & 0x000C) == 0x0008 && (stype & 0x08) != 0;
        let qos_ctrl = if is_qos {
            self.bytes.get(24..26).and_then(|s| s.first_chunk::<2>().copied())
        } else {
            None
        };

        Ok(Ieee80211FromAp {
            duration_id,
            ftype,
            stype,
            destination: self.get_destination(),
            source: self.get_source(),
            bssid: self.get_bssid().ok_or_else(|| "Missing BSSID".to_string())?,
            seq_ctrl,
            qos_ctrl,
            protected: if (fc & 0x4000) != 0 { 1 } else { 0 },
            order: if (fc & 0x8000) != 0 { 1 } else { 0 },
            more_frags: if (fc & 0x0400) != 0 { 1 } else { 0 },
            retry: if (fc & 0x0800) != 0 { 1 } else { 0 },
            pm: if (fc & 0x1000) != 0 { 1 } else { 0 },
            more_data: if (fc & 0x2000) != 0 { 1 } else { 0 },
            version: (fc & 0x0003) as u8,
            payload: self.get_payload(),
        })
    }
}

/// Helper struct for creating Ieee80211 frames in tests.
#[derive(Debug, Clone)]
pub struct Ieee80211ToAp {
    pub duration_id: u16,
    pub ftype: FrameType,
    pub stype: u8,
    pub destination: MacAddress,
    pub source: MacAddress,
    pub bssid: MacAddress,
    pub seq_ctrl: u16,
    pub qos_ctrl: Option<[u8; 2]>,
    pub protected: u8,
    pub order: u8,
    pub more_frags: u8,
    pub retry: u8,
    pub pm: u8,
    pub more_data: u8,
    pub version: u8,
    pub payload: Vec<u8>,
}

impl Ieee80211ToAp {
    fn encode_with_direction(
        &self,
        direction: FrameDirection,
        payload: &[u8],
    ) -> Result<Vec<u8>, String> {
        let mut fc = 0u16;
        fc |= (self.version as u16) & 0x03;
        fc |= ((self.ftype as u16) & 0x03) << 2;
        fc |= ((self.stype as u16) & 0x0F) << 4;
        let (ds_flag, addrs) = match direction {
            FrameDirection::ToAp => {
                // ToDS=1, Addr1=BSSID, Addr2=SA, Addr3=DA
                (0x0100, [&self.bssid.bytes, &self.source.bytes, &self.destination.bytes])
            }
            FrameDirection::FromAp => {
                // FromDS=1, Addr1=DA, Addr2=BSSID, Addr3=SA
                (0x0200, [&self.destination.bytes, &self.bssid.bytes, &self.source.bytes])
            }
        };
        fc |= ds_flag;

        if self.protected != 0 {
            fc |= 0x4000;
        }
        if self.order != 0 {
            fc |= 0x8000;
        }
        if self.more_frags != 0 {
            fc |= 0x0400;
        }
        if self.retry != 0 {
            fc |= 0x0800;
        }
        if self.pm != 0 {
            fc |= 0x1000;
        }
        if self.more_data != 0 {
            fc |= 0x2000;
        }

        let [a1, a2, a3] = addrs;
        let qos_slice: &[u8] = self.qos_ctrl.as_ref().map_or(&[], |q| q.as_slice());
        Ok([
            &fc.to_le_bytes()[..],
            &self.duration_id.to_le_bytes(),
            a1.as_slice(),
            a2.as_slice(),
            a3.as_slice(),
            &self.seq_ctrl.to_le_bytes(),
            qos_slice,
            payload,
        ]
        .concat())
    }

    pub fn encode_to_bytes(&self) -> Result<Vec<u8>, String> {
        self.encode_with_direction(FrameDirection::ToAp, &self.payload)
    }
}

impl TryFrom<Ieee80211ToAp> for Ieee80211 {
    type Error = String;
    fn try_from(val: Ieee80211ToAp) -> Result<Self, Self::Error> {
        let bytes = val.encode_to_bytes()?;
        Ok(Ieee80211 { bytes })
    }
}

/// Helper struct for creating Ieee80211 frames from AP.
#[derive(Debug, Clone)]
pub struct Ieee80211FromAp {
    pub duration_id: u16,
    pub ftype: FrameType,
    pub stype: u8,
    pub destination: MacAddress,
    pub source: MacAddress,
    pub bssid: MacAddress,
    pub seq_ctrl: u16,
    pub qos_ctrl: Option<[u8; 2]>,
    pub protected: u8,
    pub order: u8,
    pub more_frags: u8,
    pub retry: u8,
    pub pm: u8,
    pub more_data: u8,
    pub version: u8,
    pub payload: Vec<u8>,
}

impl Ieee80211FromAp {
    fn as_to_ap_header(&self) -> Ieee80211ToAp {
        Ieee80211ToAp {
            duration_id: self.duration_id,
            ftype: self.ftype,
            stype: self.stype,
            destination: self.destination,
            source: self.source,
            bssid: self.bssid,
            seq_ctrl: self.seq_ctrl,
            qos_ctrl: self.qos_ctrl,
            protected: self.protected,
            order: self.order,
            more_frags: self.more_frags,
            retry: self.retry,
            pm: self.pm,
            more_data: self.more_data,
            version: self.version,
            payload: Vec::new(),
        }
    }

    pub fn encode_to_bytes(&self) -> Result<Vec<u8>, String> {
        self.as_to_ap_header().encode_with_direction(FrameDirection::FromAp, &self.payload)
    }
}

impl TryFrom<Ieee80211FromAp> for Ieee80211 {
    type Error = String;
    fn try_from(val: Ieee80211FromAp) -> Result<Self, Self::Error> {
        let bytes = val.encode_to_bytes()?;
        Ok(Ieee80211 { bytes })
    }
}

#[cfg(test)]
mod tests {
    use core::mem::size_of;

    use zerocopy::Ref;

    use super::*;
    use crate::ieee80211::{BeaconFrameHeader, data_subtype, frame_type, management_subtype};

    #[test]
    fn test_struct_sizes() {
        assert_eq!(size_of::<FrameControl>(), 2);
        assert_eq!(size_of::<SequenceControl>(), 2);
        assert_eq!(size_of::<MacAddr>(), 6);
        // FC (2) + Dur (2) + Addr1 (6) + Addr2 (6) + Addr3 (6) + SeqCtl (2) = 24
        assert_eq!(size_of::<MacHeader3Addr>(), 24);
        assert_eq!(size_of::<DataFrameHeader>(), 24);
        assert_eq!(size_of::<BeaconFrameHeader>(), 24);
    }

    #[test]
    fn test_frame_control_parsing() {
        // Example FrameControl value:
        // Protocol Version: 0
        // Type: Data (0b10)
        // Subtype: Data (0b0000)
        // ToDS=1, FromDS=0 -> 0b00000001 = 0x01
        // So, 0x0108 (byte order on wire: 08 01)
        let fc_val: u16 = 0x0108;
        let fc = FrameControl::new(fc_val);
        assert_eq!(fc.protocol_version(), 0);
        assert_eq!(fc.frame_type(), frame_type::DATA); // 0b10 = 2
        assert_eq!(fc.frame_subtype(), data_subtype::DATA); // 0b0000 = 0
        assert!(fc.to_ds());
        assert!(!fc.from_ds());
    }

    #[test]
    fn test_frame_control_all_flags() {
        // All flags set, except protocol version (usually 0)
        // Type=MGMT (00), Subtype=Beacon (1000) -> 001000 = 0x20
        // ToDS=1, FromDS=1, MoreFrag=1, Retry=1, PwrMgmt=1, MoreData=1, Protected=1,
        // Order=1 -> 11111111 = 0xFF FC value: 0xFF80 (LSB: Type=MGMT 00,
        // Subtype=Beacon 1000 => 10000000 = 0x80; MSB: All flags set => 11111111 =
        // 0xFF)
        let fc_val: u16 = 0xFF80;
        let fc = FrameControl::new(fc_val);
        assert_eq!(fc.protocol_version(), 0);
        assert_eq!(fc.frame_type(), frame_type::MANAGEMENT);
        assert_eq!(fc.frame_subtype(), management_subtype::BEACON);
        assert!(
            fc.to_ds()
                && fc.from_ds()
                && fc.more_fragments()
                && fc.retry()
                && fc.power_management()
                && fc.more_data()
                && fc.protected_frame()
                && fc.order()
        );

        let fc_default = FrameControl::new(0x0000); // All flags zero, type=MGMT, subtype=AssocReq
        assert!(!fc_default.to_ds() && !fc_default.from_ds() && !fc_default.protected_frame());
    }
    #[test]
    fn test_sequence_control_parsing() {
        // SeqNo = 10 (0xA), FragNo = 1 (0x1)
        // Value = (10 << 4) | 1 = (0xA0) | 1 = 0xA1
        let sc = SequenceControl::new(0x00A1); // Stored as little-endian
        assert_eq!(sc.sequence_number(), 10);
        assert_eq!(sc.fragment_number(), 1);
    }

    #[test]
    fn test_data_frame_header_parsing() {
        let bytes: [u8; 24] = [
            0x08, 0x01, // FrameControl: Data, ToDS=1 (0x0108)
            0x00, 0x00, // DurationID
            0x01, 0x02, 0x03, 0x04, 0x05, 0x06, // Addr1 (BSSID)
            0x11, 0x12, 0x13, 0x14, 0x15, 0x16, // Addr2 (SA)
            0x21, 0x22, 0x23, 0x24, 0x25, 0x26, // Addr3 (DA)
            0xA1, 0x00, // SequenceControl: Seq=10, Frag=1 (0x00A1)
        ];

        let header = Ref::<&[u8], DataFrameHeader>::from_bytes(&bytes)
            .expect("Failed to parse DataFrameHeader");

        assert_eq!(header.frame_control.get(), 0x0108);
        assert!(header.frame_control.to_ds());
        assert!(!header.frame_control.from_ds());
        assert_eq!(header.frame_control.frame_type(), frame_type::DATA);
        assert_eq!(header.addr1.bytes, [0x01, 0x02, 0x03, 0x04, 0x05, 0x06]);
        assert_eq!(header.sequence_control.sequence_number(), 10);
        assert_eq!(header.sequence_control.fragment_number(), 1);
    }

    #[test]
    fn test_beacon_frame_header_parsing() {
        let bytes: [u8; 24] = [
            0x80, 0x00, // FrameControl: Beacon (MGMT type 00, subtype 1000 -> 0x0080)
            0x00, 0x00, // Duration
            0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, // DA (Broadcast)
            0x11, 0x22, 0x33, 0x44, 0x55, 0x66, // SA (BSSID)
            0x11, 0x22, 0x33, 0x44, 0x55, 0x66, // BSSID
            0x12,
            0x03, // SequenceControl: Value 0x0312 (Seq=0x31, Frag=2). LE bytes: 0x12, 0x03.
        ];

        let header = Ref::<&[u8], BeaconFrameHeader>::from_bytes(&bytes)
            .expect("Failed to parse BeaconFrameHeader");

        assert_eq!(header.frame_control.get(), 0x0080);
        assert!(!header.frame_control.to_ds());
        assert!(!header.frame_control.from_ds());
        assert_eq!(header.frame_control.frame_type(), frame_type::MANAGEMENT);
        assert_eq!(header.frame_control.frame_subtype(), management_subtype::BEACON);
        assert_eq!(header.da.bytes, [0xFF; 6]);
        assert_eq!(header.sa.bytes, [0x11, 0x22, 0x33, 0x44, 0x55, 0x66]);
        assert_eq!(header.bssid.bytes, [0x11, 0x22, 0x33, 0x44, 0x55, 0x66]);
        assert_eq!(header.sequence_control.get(), 0x0312); // Value as read
        assert_eq!(header.sequence_control.sequence_number(), 0x031); // 49
        assert_eq!(header.sequence_control.fragment_number(), 2);
    }
    #[test]
    fn test_qos_data_parsing() {
        // QoS Data Frame:
        // Frame Control: 0x88 (Type=Data, Subtype=QoS Data)
        // Flags: 0x01 (ToDS) -> 0x0188
        // Duration: 0
        // Addr1 (BSSID): 01:02:03:04:05:06
        // Addr2 (SA): 11:12:13:14:15:16
        // Addr3 (DA): 21:22:23:24:25:26
        // Seq: 0
        // QoS Control: 0x0000
        // Payload: DEADBEEF
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&[0x88, 0x01]); // FC
        bytes.extend_from_slice(&[0x00, 0x00]); // Duration
        bytes.extend_from_slice(&[0x01, 0x02, 0x03, 0x04, 0x05, 0x06]); // Addr1
        bytes.extend_from_slice(&[0x11, 0x12, 0x13, 0x14, 0x15, 0x16]); // Addr2
        bytes.extend_from_slice(&[0x21, 0x22, 0x23, 0x24, 0x25, 0x26]); // Addr3
        bytes.extend_from_slice(&[0x00, 0x00]); // Seq
        bytes.extend_from_slice(&[0x00, 0x00]); // QoS Control
        bytes.extend_from_slice(&[0xDE, 0xAD, 0xBE, 0xEF]); // Payload

        let frame = Ieee80211::decode_full(&bytes).expect("Failed to decode QoS Data");

        assert!(frame.is_qos_data());
        assert_eq!(frame.get_payload(), vec![0xDE, 0xAD, 0xBE, 0xEF]);
    }

    #[test]
    fn test_from_ieee8023_downlink() {
        let payload = [0xde, 0xad, 0xbe, 0xef];
        let mut eth_frame = Vec::new();
        let dst = MacAddress::new([0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF]);
        let src = MacAddress::new([0x11, 0x22, 0x33, 0x44, 0x55, 0x66]);
        let bssid = MacAddress::new([0x02, 0x00, 0x00, 0x00, 0x01, 0x01]);

        eth_frame.extend_from_slice(&dst.bytes);
        eth_frame.extend_from_slice(&src.bytes);
        eth_frame.extend_from_slice(&[0x08, 0x00]); // IPv4
        eth_frame.extend_from_slice(&payload);

        let frame = Ieee80211::from_ieee8023(&eth_frame, bssid, FrameDirection::FromAp, 100)
            .expect("Failed to convert");

        // Verify Flags
        // FC should be Data(2) | FromDS(1) -> 0x0208
        // Little Endian: 08 02
        assert_eq!(frame.bytes[0], 0x08);
        assert_eq!(frame.bytes[1], 0x02);

        assert!(frame.is_data());
        assert!(!frame.is_to_ds());
        assert!(frame.is_from_ds());

        // Verify Addresses
        assert_eq!(frame.get_destination(), dst);
        assert_eq!(frame.get_source(), src);
        assert_eq!(frame.get_bssid(), Some(bssid));

        // Verify addresses locations manually to ensure order
        // Addr1 (DA)
        assert_eq!(&frame.bytes[4..10], dst.bytes);
        // Addr2 (BSSID)
        assert_eq!(&frame.bytes[10..16], bssid.bytes);
        // Addr3 (SA)
        assert_eq!(&frame.bytes[16..22], src.bytes);
    }

    #[test]
    fn test_from_ieee8023_uplink() {
        let payload = [0xde, 0xad, 0xbe, 0xef];
        let mut eth_frame = Vec::new();
        let dst = MacAddress::new([0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF]);
        let src = MacAddress::new([0x11, 0x22, 0x33, 0x44, 0x55, 0x66]);
        let bssid = MacAddress::new([0x02, 0x00, 0x00, 0x00, 0x01, 0x01]);

        eth_frame.extend_from_slice(&dst.bytes);
        eth_frame.extend_from_slice(&src.bytes);
        eth_frame.extend_from_slice(&[0x08, 0x00]); // IPv4
        eth_frame.extend_from_slice(&payload);

        let frame = Ieee80211::from_ieee8023(&eth_frame, bssid, FrameDirection::ToAp, 100)
            .expect("Failed to convert");

        // Verify Flags
        // FC should be Data(2) | ToDS(1) -> 0x0108
        // Little Endian: 08 01
        assert_eq!(frame.bytes[0], 0x08);
        assert_eq!(frame.bytes[1], 0x01);

        assert!(frame.is_data());
        assert!(frame.is_to_ds());
        assert!(!frame.is_from_ds());

        // Verify Addresses
        assert_eq!(frame.get_destination(), dst);
        assert_eq!(frame.get_source(), src);
        assert_eq!(frame.get_bssid(), Some(bssid));

        // Verify addresses locations manually to ensure order
        // Addr1 (BSSID)
        assert_eq!(&frame.bytes[4..10], bssid.bytes);
        // Addr2 (SA)
        assert_eq!(&frame.bytes[10..16], src.bytes);
        // Addr3 (DA)
        assert_eq!(&frame.bytes[16..22], dst.bytes);
    }

    #[test]
    fn test_into_from_ap_qos_preservation() {
        // QoS Data Frame (ToDS=1):
        // Frame Control: 0x88 (Type=Data, Subtype=QoS Data)
        // Flags: 0x01 (ToDS) -> 0x0188
        // Duration: 0
        // Addr1 (BSSID): 01:02:03:04:05:06
        // Addr2 (SA): 11:12:13:14:15:16
        // Addr3 (DA): 21:22:23:24:25:26
        // Seq: 0
        // QoS Control: 0x1234
        // Payload: DEADBEEF
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&[0x88, 0x01]); // FC
        bytes.extend_from_slice(&[0x00, 0x00]); // Duration
        bytes.extend_from_slice(&[0x01, 0x02, 0x03, 0x04, 0x05, 0x06]); // BSSID
        bytes.extend_from_slice(&[0x11, 0x12, 0x13, 0x14, 0x15, 0x16]); // SA
        bytes.extend_from_slice(&[0x21, 0x22, 0x23, 0x24, 0x25, 0x26]); // DA
        bytes.extend_from_slice(&[0x00, 0x00]); // Seq
        bytes.extend_from_slice(&[0x12, 0x34]); // QoS Control
        bytes.extend_from_slice(&[0xDE, 0xAD, 0xBE, 0xEF]); // Payload

        let frame = Ieee80211::decode_full(&bytes).expect("Failed to decode QoS Data");

        // Convert the frame to represent a transmission *from* the AP.
        let from_ap = frame.into_from_ap().expect("Failed to convert into_from_ap");

        // Assert the extracted values match exactly what was parsed from the source
        // frame
        assert_eq!(from_ap.qos_ctrl, Some([0x12, 0x34]));
        assert_eq!(from_ap.payload, vec![0xDE, 0xAD, 0xBE, 0xEF]);

        // Re-encode to test encoding behavior.
        let encoded_bytes = from_ap.encode_to_bytes().expect("Failed to encode into bytes");

        // Validate that QoS is accurately written back out.
        let re_decoded_frame =
            Ieee80211::decode_full(&encoded_bytes).expect("Failed to re-decode into_from_ap bytes");

        assert!(re_decoded_frame.is_qos_data());
        assert_eq!(re_decoded_frame.get_payload(), vec![0xDE, 0xAD, 0xBE, 0xEF]);
        assert_eq!(re_decoded_frame.bytes[24..26], [0x12, 0x34]);
    }

    #[test]
    fn test_is_payload_bearing_data() {
        let make_fc_bytes = |frame_type: u8, subtype: u8| -> [u8; 32] {
            let mut bytes = [0u8; 32];
            // Protocol Version = 0
            // Type at bits 2-3: (frame_type & 0b11) << 2
            // Subtype at bits 4-7: (subtype & 0b1111) << 4
            bytes[0] = ((subtype & 0b1111) << 4) | ((frame_type & 0b11) << 2);
            bytes
        };

        // 1. Management frame (Type = 0), Subtype = Beacon (8)
        let mgmt_bytes = make_fc_bytes(0, 8);
        let mgmt_frame = Ieee80211::decode(&mgmt_bytes).unwrap();
        assert!(!mgmt_frame.is_payload_bearing_data());

        // 2. Control frame (Type = 1), Subtype = Ack (13)
        let ctrl_bytes = make_fc_bytes(1, 13);
        let ctrl_frame = Ieee80211::decode(&ctrl_bytes).unwrap();
        assert!(!ctrl_frame.is_payload_bearing_data());

        // 3. Data frame (Type = 2), Subtype = Data (0) -> payload-bearing
        let data_bytes = make_fc_bytes(2, 0);
        let data_frame = Ieee80211::decode(&data_bytes).unwrap();
        assert!(data_frame.is_payload_bearing_data());

        // 4. Data frame (Type = 2), Subtype = QoS Data (8) -> payload-bearing
        let qos_bytes = make_fc_bytes(2, 8);
        let qos_frame = Ieee80211::decode(&qos_bytes).unwrap();
        assert!(qos_frame.is_payload_bearing_data());

        // 5. Data frame (Type = 2), Subtype = Null (4) -> control-only (no data)
        let null_bytes = make_fc_bytes(2, 4);
        let null_frame = Ieee80211::decode(&null_bytes).unwrap();
        assert!(!null_frame.is_payload_bearing_data());

        // 6. Data frame (Type = 2), Subtype = QoS Null (12) -> control-only (no data)
        let qos_null_bytes = make_fc_bytes(2, 12);
        let qos_null_frame = Ieee80211::decode(&qos_null_bytes).unwrap();
        assert!(!qos_null_frame.is_payload_bearing_data());
    }

    #[test]
    fn test_get_addr_accessors_and_short_frames() {
        let addr1 = MacAddress::new([0x01, 0x02, 0x03, 0x04, 0x05, 0x06]);
        let addr2 = MacAddress::new([0x11, 0x12, 0x13, 0x14, 0x15, 0x16]);
        let addr3 = MacAddress::new([0x21, 0x22, 0x23, 0x24, 0x25, 0x26]);
        let zero_addr = MacAddress::new([0; 6]);

        let mut bytes = vec![0u8; 4];
        bytes.extend_from_slice(&addr1.bytes);
        bytes.extend_from_slice(&addr2.bytes);
        bytes.extend_from_slice(&addr3.bytes);

        let full_frame = Ieee80211 { bytes: bytes.clone() };
        assert_eq!(full_frame.get_addr1(), addr1);
        assert_eq!(full_frame.get_addr2(), addr2);
        assert_eq!(full_frame.get_addr3(), addr3);

        let short_frame = Ieee80211 { bytes: bytes[..9].to_vec() };
        assert_eq!(short_frame.get_addr1(), zero_addr);
        assert_eq!(short_frame.get_addr2(), zero_addr);
        assert_eq!(short_frame.get_addr3(), zero_addr);

        let partial_frame = Ieee80211 { bytes: bytes[..15].to_vec() };
        assert_eq!(partial_frame.get_addr1(), addr1);
        assert_eq!(partial_frame.get_addr2(), zero_addr);
        assert_eq!(partial_frame.get_addr3(), zero_addr);
    }

    #[test]
    fn test_encode_to_bytes_to_ap_and_from_ap_all_flags() {
        let dst = MacAddress::new([0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF]);
        let src = MacAddress::new([0x11, 0x22, 0x33, 0x44, 0x55, 0x66]);
        let bssid = MacAddress::new([0x02, 0x00, 0x00, 0x00, 0x01, 0x01]);
        let payload = vec![0xCA, 0xFE, 0xBA, 0xBE];

        let to_ap = Ieee80211ToAp {
            duration_id: 0x1234,
            ftype: FrameType::Data,
            stype: 0x08, // QoS Data
            destination: dst,
            source: src,
            bssid,
            seq_ctrl: 0x0420,
            qos_ctrl: Some([0x05, 0x00]),
            protected: 1,
            order: 0,
            more_frags: 1,
            retry: 1,
            pm: 1,
            more_data: 1,
            version: 0,
            payload: payload.clone(),
        };

        let to_ap_bytes = to_ap.encode_to_bytes().expect("encode ToAp should succeed");
        let to_ap_frame = Ieee80211::decode_full(&to_ap_bytes).expect("decode ToAp should succeed");
        // FC: Data (2 << 2 = 0x0008) | QoS (8 << 4 = 0x0080) | ToDS (0x0100)
        //   | MoreFrags (0x0400) | Retry (0x0800) | PM (0x1000)
        //   | MoreData (0x2000) | Protected (0x4000) = 0x7D88
        assert_eq!(to_ap_frame.get_fc(), 0x7D88);
        assert!(to_ap_frame.is_to_ds());
        assert!(!to_ap_frame.is_from_ds());
        assert_eq!(to_ap_frame.get_addr1(), bssid);
        assert_eq!(to_ap_frame.get_addr2(), src);
        assert_eq!(to_ap_frame.get_addr3(), dst);
        assert_eq!(to_ap_frame.get_destination(), dst);
        assert_eq!(to_ap_frame.get_source(), src);
        assert_eq!(to_ap_frame.get_bssid(), Some(bssid));
        assert_eq!(to_ap_frame.get_payload(), payload);

        let mut to_ap_ordered = to_ap.clone();
        to_ap_ordered.order = 1;
        let to_ap_ordered_bytes =
            to_ap_ordered.encode_to_bytes().expect("encode ordered ToAp should succeed");
        let to_ap_ordered_frame = Ieee80211::decode_full(&to_ap_ordered_bytes)
            .expect("decode ordered ToAp should succeed");
        assert_eq!(to_ap_ordered_frame.get_fc(), 0xFD88);

        let from_ap = Ieee80211FromAp {
            duration_id: 0x1234,
            ftype: FrameType::Data,
            stype: 0x08, // QoS Data
            destination: dst,
            source: src,
            bssid,
            seq_ctrl: 0x0420,
            qos_ctrl: Some([0x05, 0x00]),
            protected: 1,
            order: 0,
            more_frags: 1,
            retry: 1,
            pm: 1,
            more_data: 1,
            version: 0,
            payload: payload.clone(),
        };

        let from_ap_bytes = from_ap.encode_to_bytes().expect("encode FromAp should succeed");
        let from_ap_frame =
            Ieee80211::decode_full(&from_ap_bytes).expect("decode FromAp should succeed");
        // FC with FromDS (0x0200) instead of ToDS (0x0100) = 0x7E88
        assert_eq!(from_ap_frame.get_fc(), 0x7E88);
        assert!(!from_ap_frame.is_to_ds());
        assert!(from_ap_frame.is_from_ds());
        assert_eq!(from_ap_frame.get_addr1(), dst);
        assert_eq!(from_ap_frame.get_addr2(), bssid);
        assert_eq!(from_ap_frame.get_addr3(), src);
        assert_eq!(from_ap_frame.get_destination(), dst);
        assert_eq!(from_ap_frame.get_source(), src);
        assert_eq!(from_ap_frame.get_bssid(), Some(bssid));
        assert_eq!(from_ap_frame.get_payload(), payload);
    }

    #[test]
    fn test_get_aad() {
        let addr1 = [0x01, 0x02, 0x03, 0x04, 0x05, 0x06];
        let addr2 = [0x11, 0x12, 0x13, 0x14, 0x15, 0x16];
        let addr3 = [0x21, 0x22, 0x23, 0x24, 0x25, 0x26];
        let addr4 = [0x31, 0x32, 0x33, 0x34, 0x35, 0x36];

        // 1. Short frame (< 24 bytes) returns empty AAD
        let short_frame = Ieee80211 { bytes: vec![0u8; 23] };
        assert!(short_frame.get_aad().is_empty());

        // 2. Non-QoS Data frame (ToDS=1, FromDS=0):
        // fc0 = 0x3B (Subtype=0b0011, Type=Data 0b10, Version=0b11) -> preserved as 0x3B
        // fc1 = 0xBD (ToDS=1, FromDS=0, MoreFrag=1, Retry=1, PwrMgmt=1, MoreData=1, Protected=0, Order=1)
        // After masking Retry/PwrMgmt/MoreData (!0x38) and setting Protected (| 0x40):
        // (0xBD & !0x38) | 0x40 = 0x85 | 0x40 = 0xC5
        // Sequence Control: seq0 = 0xAB (Frag=0xB, Seq low=0xA), seq1 = 0xCD -> [0x0B, 0x00]
        let mut non_qos_bytes = vec![0x3B, 0xBD, 0x12, 0x34];
        non_qos_bytes.extend_from_slice(&addr1);
        non_qos_bytes.extend_from_slice(&addr2);
        non_qos_bytes.extend_from_slice(&addr3);
        non_qos_bytes.extend_from_slice(&[0xAB, 0xCD]);

        let non_qos_frame = Ieee80211 { bytes: non_qos_bytes };
        let mut expected_non_qos = vec![0x3B, 0xC5];
        expected_non_qos.extend_from_slice(&addr1);
        expected_non_qos.extend_from_slice(&addr2);
        expected_non_qos.extend_from_slice(&addr3);
        expected_non_qos.extend_from_slice(&[0x0B, 0x00]);
        assert_eq!(non_qos_frame.get_aad(), expected_non_qos);

        // 3. QoS Data frame with Addr4 (ToDS=1, FromDS=1):
        // fc0 = 0x88 (Type=Data, Subtype=QoS Data)
        // fc1 = 0x3B (ToDS=1, FromDS=1, MoreFrag=0, Retry=1, PwrMgmt=1, MoreData=1, Protected=0, Order=0)
        // Masked fc1: (0x3B & !0x38) | 0x40 = 0x03 | 0x40 = 0x43
        // QoS Control: [0xF5, 0xAA] -> TID (low 4 bits) kept: [0x05, 0x00]
        let mut qos_wds_bytes = vec![0x88, 0x3B, 0x00, 0x00];
        qos_wds_bytes.extend_from_slice(&addr1);
        qos_wds_bytes.extend_from_slice(&addr2);
        qos_wds_bytes.extend_from_slice(&addr3);
        qos_wds_bytes.extend_from_slice(&[0xF7, 0x12]); // Frag = 7
        qos_wds_bytes.extend_from_slice(&addr4);
        qos_wds_bytes.extend_from_slice(&[0xF5, 0xAA]); // QoS Ctrl

        let qos_wds_frame = Ieee80211 { bytes: qos_wds_bytes };
        let mut expected_qos_wds = vec![0x88, 0x43];
        expected_qos_wds.extend_from_slice(&addr1);
        expected_qos_wds.extend_from_slice(&addr2);
        expected_qos_wds.extend_from_slice(&addr3);
        expected_qos_wds.extend_from_slice(&[0x07, 0x00]);
        expected_qos_wds.extend_from_slice(&addr4);
        expected_qos_wds.extend_from_slice(&[0x05, 0x00]);
        assert_eq!(qos_wds_frame.get_aad(), expected_qos_wds);
    }
}
