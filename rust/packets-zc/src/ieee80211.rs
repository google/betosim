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

use crate::ethernet::MacAddr;
use core::fmt;
use zerocopy::byteorder::LittleEndian; // IEEE 802.11 fields are typically little-endian
use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout, U16, Unaligned};

/// Represents the 2-byte Frame Control field in an 802.11 header.
///
/// The Frame Control field is structured as follows (LSB to MSB for bits within a byte):
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
    /// The raw 2-byte value of the Frame Control field, stored in little-endian.
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

    /// Frame Subtype (4 bits). Meaning depends on Frame Type. See `FrameSubtype` constants.
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

/// IEEE 802.11 Frame Types.
pub mod frame_type {
    pub const MANAGEMENT: u8 = 0b00;
    pub const CONTROL: u8 = 0b01;
    pub const DATA: u8 = 0b10;
    // 0b11 is reserved
}

/// IEEE 802.11 Frame Subtypes for Data frames.
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

/// Represents an IEEE 802.11 Beacon frame header.
/// Management frames like Beacon typically have 3 addresses.
#[repr(C)]
#[derive(FromBytes, IntoBytes, Unaligned, Immutable, KnownLayout, Debug)]
pub struct BeaconFrameHeader {
    /// Frame Control field. Type=MGMT, Subtype=BEACON.
    pub frame_control: FrameControl,
    /// Duration field.
    pub duration: U16<LittleEndian>,
    /// Address 1: Destination MAC Address (typically broadcast FF:FF:FF:FF:FF:FF).
    pub da: MacAddr,
    /// Address 2: Source MAC Address (Transmitter Address / BSSID).
    pub sa: MacAddr,
    /// Address 3: BSSID.
    pub bssid: MacAddr,
    /// Sequence Control field.
    pub sequence_control: SequenceControl,
    // Followed by fixed parameters (Timestamp, Beacon Interval, Capability Info)
    // and then tagged parameters (SSID, Rates, etc.).
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::mem::size_of;
    use zerocopy::Ref;

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
        // ToDS=1, FromDS=1, MoreFrag=1, Retry=1, PwrMgmt=1, MoreData=1, Protected=1, Order=1 -> 11111111 = 0xFF
        // FC value: 0xFF80 (LSB: Type=MGMT 00, Subtype=Beacon 1000 => 10000000 = 0x80; MSB: All flags set => 11111111 = 0xFF)
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
}
