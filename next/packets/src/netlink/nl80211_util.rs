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

//! Provides utility functions for working with `nl80211` Netlink attributes,
//! particularly for `mac80211_hwsim`.
//!
//! This module offers helpers to build, parse, and interpret the attributes
//! within `nl80211` messages that are sent between a user space daemon and the
//! `mac80211_hwsim` kernel module. These functions simplify tasks like creating
//! Netlink messages to command the simulated hardware (e.g., to transmit a
//! frame) and parsing messages received from it (e.g., an incoming frame
//! notification).

use zerocopy::{FromBytes, LittleEndian, Ref, U32};
#[cfg(test)]
use zerocopy::{IntoBytes, U16};

#[cfg(test)]
use crate::ieee80211::MacHeader3Addr;
#[cfg(test)]
use crate::netlink::nl80211::GenlMsgHdr;
use crate::{
    ethernet::MacAddr as EthernetMacAddr,
    netlink::{nl80211::attr_id, nl80211_attr::NlAttrHdr},
};

/// Netlink attribute type flags.
/// The highest bit of the attribute type indicates if it's nested.
pub const NLA_F_NESTED: u16 = 1 << 15;
/// Mask to extract the attribute type without flags.
pub const NLA_TYPE_MASK: u16 = !(NLA_F_NESTED);

/// The alignment requirement for Netlink attributes.
pub const NLA_ALIGNTO: usize = 4;

/// Size of the Netlink attribute header.
const NLA_HDR_SIZE: usize = core::mem::size_of::<NlAttrHdr>();

/// Size of the Generic Netlink message header.
#[cfg(test)]
const GENL_HDR_SIZE: usize = core::mem::size_of::<GenlMsgHdr>();

/// Error type for Netlink utility functions.
#[derive(Debug, PartialEq, Eq)]
pub enum NetlinkError {
    /// Buffer is too short to contain the expected data.
    BufferTooShort(String),
    /// Payload length does not match expected size for the type.
    InvalidPayloadLength,
    /// String payload is not valid UTF-8 or not null-terminated.
    InvalidString,
    /// Failed to build the Netlink message.
    BuildError(String),
}

impl core::fmt::Display for NetlinkError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            NetlinkError::BufferTooShort(msg) => write!(f, "Buffer too short: {}", msg),
            NetlinkError::InvalidPayloadLength => write!(f, "Invalid payload length"),
            NetlinkError::InvalidString => write!(f, "Invalid string in payload"),
            NetlinkError::BuildError(s) => write!(f, "Netlink build error: {}", s),
        }
    }
}

/// Extracts the attribute ID from the `nla_type` field of an `NlAttrHdr`.
///
/// The `nla_type` field can contain flags in its higher bits. This function
/// masks those flags to return the raw attribute ID.
///
/// # Arguments
/// * `nla_type` - The `nla_type` value from an `NlAttrHdr`.
///
/// # Returns
/// The attribute ID.
pub fn get_attr_id_from_type(nla_type: u16) -> u16 {
    nla_type & NLA_TYPE_MASK
}

/// Checks if the NLA_F_NESTED flag is set in the `nla_type` field.
///
/// # Arguments
/// * `nla_type` - The `nla_type` value from an `NlAttrHdr`.
///
/// # Returns
/// `true` if the attribute is nested, `false` otherwise.
pub fn is_attr_nested(nla_type: u16) -> bool {
    (nla_type & NLA_F_NESTED) != 0
}

/// Converts a mac80211_hwsim Netlink attribute ID to a human-readable string.
///
/// If the attribute ID is unknown, it returns the hex representation of the
/// value.
///
/// # Arguments
/// * `attr_id_val` - The attribute ID value (e.g., `attr_id::IFACE_MAC`).
///
/// # Examples
/// ```
/// use netsim_packets::{attr_id, attr_id_to_string};
///
/// assert_eq!(attr_id_to_string(attr_id::IFACE_MAC), "IFACE_MAC");
/// assert_eq!(attr_id_to_string(0xFFFF), "Unknown(0xFFFF)");
/// ```
pub fn attr_id_to_string(attr_id_val: u16) -> String {
    let s = match attr_id_val {
        attr_id::HW_INDEX => "HW_INDEX",
        attr_id::IFACE_MAC => "IFACE_MAC",
        attr_id::IFACE_NAME => "IFACE_NAME",
        attr_id::IFACE_TYPE => "IFACE_TYPE",
        attr_id::REQ_IFACE_NUM => "REQ_IFACE_NUM",
        attr_id::MAX_IFACES => "MAX_IFACES",
        attr_id::REG_DOM => "REG_DOM",
        attr_id::REG_ALPHA2 => "REG_ALPHA2",
        attr_id::CHANNEL => "CHANNEL",
        attr_id::FREQUENCY => "FREQUENCY",
        attr_id::CHANNEL_TYPE => "CHANNEL_TYPE",
        attr_id::CHANNEL_FLAGS => "CHANNEL_FLAGS",
        attr_id::MAX_TX_POWER => "MAX_TX_POWER",
        attr_id::CENTER_FREQ1 => "CENTER_FREQ1",
        attr_id::CENTER_FREQ2 => "CENTER_FREQ2",
        attr_id::SIGNAL => "SIGNAL",
        attr_id::NOISE => "NOISE",
        attr_id::RX_RATE => "RX_RATE",
        attr_id::KEY => "KEY",
        attr_id::KEY_IDX => "KEY_IDX",
        attr_id::KEY_DATA => "KEY_DATA",
        attr_id::KEY_SEQ => "KEY_SEQ",
        attr_id::KEY_FLAGS => "KEY_FLAGS",
        attr_id::CIPHER => "CIPHER",
        attr_id::BEACON_INTERVAL => "BEACON_INTERVAL",
        attr_id::DTIM_PERIOD => "DTIM_PERIOD",
        attr_id::HIDDEN_SSID => "HIDDEN_SSID",
        attr_id::SUPPORTED_RATES => "SUPPORTED_RATES",
        attr_id::SHORT_PREAMBLE => "SHORT_PREAMBLE",
        attr_id::SHORT_SLOT_TIME => "SHORT_SLOT_TIME",
        attr_id::EDCA_PARAMS => "EDCA_PARAMS",
        attr_id::WMM_ENABLED => "WMM_ENABLED",
        attr_id::POWER_CONSTRAINT => "POWER_CONSTRAINT",
        attr_id::LOCAL_POWER_CONSTRAINT => "LOCAL_POWER_CONSTRAINT",
        attr_id::TXPOWER => "TXPOWER",
        attr_id::SUPPORTED_CHANNELS => "SUPPORTED_CHANNELS",
        attr_id::MESH_PATH => "MESH_PATH",
        attr_id::MESH_ID => "MESH_ID",
        attr_id::MESH_PLINK_STATE => "MESH_PLINK_STATE",
        attr_id::MESH_GATE_ANNOUNCEMENT => "MESH_GATE_ANNOUNCEMENT",
        attr_id::HWSIM_ATTR_FRAME_DATA => "HWSIM_ATTR_FRAME_DATA",
        attr_id::HWSIM_ATTR_COOKIE => "HWSIM_ATTR_COOKIE",
        attr_id::HWSIM_ATTR_FLAGS => "HWSIM_ATTR_FLAGS",
        _ => return format!("Unknown(0x{:04X})", attr_id_val), // format! already returns String
    };
    s.to_string()
}

/// Aligns a length to the Netlink attribute alignment boundary.
#[inline]
pub fn nla_align(len: usize) -> usize {
    (len + NLA_ALIGNTO - 1) & !(NLA_ALIGNTO - 1)
}

/// An iterator over Netlink attributes in a byte buffer.
///
/// Yields tuples of `(Ref<&'a [u8], NlAttrHdr>, &'a [u8])`, where the first
/// element is a reference to the parsed header and the second is a slice of the
/// attribute's payload.
#[derive(Debug)]
pub struct NlAttrIter<'a> {
    buffer: &'a [u8],
}

impl<'a> NlAttrIter<'a> {
    /// Creates a new iterator for the given buffer.
    pub fn new(buffer: &'a [u8]) -> Self {
        NlAttrIter { buffer }
    }
}

impl<'a> Iterator for NlAttrIter<'a> {
    type Item = (Ref<&'a [u8], NlAttrHdr>, &'a [u8]); // (HeaderRef, PayloadSlice)

    fn next(&mut self) -> Option<Self::Item> {
        if self.buffer.len() < NLA_HDR_SIZE {
            return None; // Not enough data for even a header
        }

        // Parse the header using from_prefix to get a Ref
        let hdr_ref = match Ref::<&'a [u8], NlAttrHdr>::from_prefix(self.buffer) {
            Ok((hdr, _)) => hdr,
            Err(_) => return None, // Error during parsing (e.g. alignment, size)
        };

        let nla_len = hdr_ref.length() as usize;
        let payload = self.buffer.get(NLA_HDR_SIZE..nla_len)?;

        let total_aligned_len = nla_align(nla_len);
        self.buffer = self.buffer.get(total_aligned_len..).unwrap_or(&[]);

        Some((hdr_ref, payload))
    }
}

/// Creates an iterator over Netlink attributes in the given buffer.
///
/// This is a convenience function for `NlAttrIter::new(buffer)`.
pub fn iter_nl_attrs(buffer: &[u8]) -> NlAttrIter<'_> {
    NlAttrIter::new(buffer)
}

/// Creates a payload for a Netlink attribute containing a u32 value.
/// The value is encoded in little-endian format.
///
/// # Arguments
/// * `value` - The u32 value to encode.
/// # Returns A `Vec<u8>` containing the little-endian byte representation of the value.
pub fn create_u32_attr_payload(value: u32) -> Vec<u8> {
    value.to_le_bytes().to_vec()
}

/// Parses a u32 value from a Netlink attribute payload.
pub fn parse_u32_from_payload(payload: &[u8]) -> Result<u32, NetlinkError> {
    if payload.len() == 4 {
        // U32 implements FromBytes, so we can use read_from_prefix if the length is
        // checked. read_from_prefix returns Option, which we unwrap as the
        // length is confirmed. An alternative, more robust if FromBytes wasn't
        // directly usable, would be: Ref::<&[u8],
        // U32<LittleEndian>>::from_prefix(payload).map(|(val, _rest)|
        // val.get()).ok_or(NetlinkError::InvalidPayloadLength)
        let (val, _) = U32::<LittleEndian>::read_from_prefix(payload)
            .map_err(|err| NetlinkError::BufferTooShort(err.to_string()))?;
        Ok(val.get())
    } else {
        Err(NetlinkError::InvalidPayloadLength)
    }
}

/// Creates a payload for a Netlink attribute containing a C-style
/// null-terminated string.
///
/// # Arguments
/// * `s` - The string slice to encode. A null terminator will be appended.
///
/// # Returns A `Vec<u8>` containing the UTF-8 bytes of the string followed by a null byte.
pub fn create_string_attr_payload(s: &str) -> Vec<u8> {
    let mut bytes = s.as_bytes().to_vec();
    bytes.push(0); // Null terminator
    bytes
}

/// Parses a C-style null-terminated string from a Netlink attribute payload.
///
/// # Arguments
/// * `payload` - A byte slice expected to contain a null-terminated UTF-8
///   string.
///
/// # Returns
/// A `Result` containing the parsed `String` or a `NetlinkError` if parsing
/// fails.
pub fn parse_string_from_payload(payload: &[u8]) -> Result<String, NetlinkError> {
    if let Some(null_pos) = payload.iter().position(|&b| b == 0) {
        let str_bytes = payload.get(..null_pos).ok_or(NetlinkError::InvalidString)?;
        std::str::from_utf8(str_bytes).map(String::from).map_err(|_err| NetlinkError::InvalidString)
    } else {
        Err(NetlinkError::InvalidString) // No null terminator
    }
}

/// Creates a payload for a Netlink attribute containing a MAC address.
///
/// # Arguments
/// * `mac` - A reference to an `EthernetMacAddr`.
///
/// # Returns A `Vec<u8>` containing the 6 bytes of the MAC address.
pub fn create_mac_addr_attr_payload(mac: &EthernetMacAddr) -> Vec<u8> {
    mac.bytes.to_vec()
}

/// Parses a MAC address from a Netlink attribute payload.
///
/// # Arguments
/// * `payload` - A byte slice expected to contain 6 bytes representing a MAC
///   address.
///
/// # Returns A `Result` containing the parsed `EthernetMacAddr` or a `NetlinkError`.
pub fn parse_mac_addr_from_payload(payload: &[u8]) -> Result<EthernetMacAddr, NetlinkError> {
    if payload.len() == 6 {
        let mut bytes = [0u8; 6];
        bytes.copy_from_slice(payload);
        Ok(EthernetMacAddr { bytes })
    } else {
        Err(NetlinkError::InvalidPayloadLength)
    }
}

/// Creates a payload for a Netlink attribute containing raw bytes.
///
/// # Arguments
/// * `bytes` - A byte slice to be used as the payload.
///
/// # Returns A `Vec<u8>` containing a copy of the input bytes.
pub fn create_bytes_attr_payload(bytes: &[u8]) -> Vec<u8> {
    bytes.to_vec()
}

/// Builds a generic Netlink message.
///
/// # Arguments
/// * `cmd` - The command for the `GenlMsgHdr`.
/// * `version` - The version for the `GenlMsgHdr`.
/// * `attributes` - A slice of tuples, where each tuple is `(attribute_id,
///   attribute_payload_bytes)`.
///
/// # Returns
/// A `Result` containing a `Vec<u8>` with the serialized Netlink message, or a
/// `NetlinkError`.
#[cfg(test)]
pub fn build_netlink_message(
    cmd: u8,
    version: u8,
    attributes: &[(u16, Vec<u8>)],
) -> Result<Vec<u8>, NetlinkError> {
    let mut message = Vec::new();

    // Add GenlMsgHdr
    let genl_hdr = GenlMsgHdr { cmd, version, reserved: U16::new(0) };
    message.extend_from_slice(genl_hdr.as_bytes());

    // Add attributes
    for (attr_type, attr_payload) in attributes {
        let nla_len = (NLA_HDR_SIZE + attr_payload.len()) as u16;
        let nl_attr_hdr = NlAttrHdr::new(nla_len, *attr_type);
        message.extend_from_slice(nl_attr_hdr.as_bytes());
        message.extend_from_slice(attr_payload);
        // Add padding to align to NLA_ALIGNTO
        message.resize(nla_align(message.len()), 0);
    }
    Ok(message)
}

/// Type alias for the result of successfully extracting an 802.11 frame from a
/// Netlink message.
#[cfg(test)]
pub type ExtractedMacFrame<'a> =
    (Ref<&'a [u8], GenlMsgHdr>, Ref<&'a [u8], MacHeader3Addr>, &'a [u8]);

/// Attempts to parse a `GenlMsgHdr` and then extract an IEEE 802.11
/// `MacHeader3Addr` from a specific Netlink attribute within the message.
///
/// The function expects the `netlink_packet_bytes` to start with a
/// `GenlMsgHdr`, followed by Netlink attributes. It searches for an attribute
/// with the ID `attr_id::HWSIM_ATTR_FRAME_DATA` and tries to parse its payload
/// as an 802.11 MAC header.
///
/// # Arguments
/// * `netlink_packet_bytes`: A byte slice representing the full Netlink
///   message.
///
/// # Returns
/// `Some((genl_hdr, mac_hdr, mac_payload))` if successful, where:
///   - `genl_hdr` is a reference to the parsed `GenlMsgHdr`.
///   - `mac_hdr` is a reference to the parsed `MacHeader3Addr`.
///   - `mac_payload` is a slice of the bytes following the `MacHeader3Addr`
///     (the 802.11 payload). `None` if parsing fails at any stage (e.g.,
///     insufficient data, attribute not found, or 802.11 header parsing error).
#[cfg(test)]
pub fn extract_mac80211_frame_from_netlink<'a>(
    netlink_packet_bytes: &'a [u8],
) -> Option<ExtractedMacFrame<'a>> {
    if netlink_packet_bytes.len() < GENL_HDR_SIZE {
        return None;
    }

    let (genl_hdr, attributes_bytes) =
        Ref::<&'a [u8], GenlMsgHdr>::from_prefix(netlink_packet_bytes).ok()?;

    iter_nl_attrs(attributes_bytes).find_map(|(nl_attr_hdr, nl_attr_payload)| {
        if get_attr_id_from_type(nl_attr_hdr.attr_type()) == attr_id::HWSIM_ATTR_FRAME_DATA {
            Ref::<&'a [u8], MacHeader3Addr>::from_prefix(nl_attr_payload)
                .map(|(mac_hdr, mac_payload)| (genl_hdr, mac_hdr, mac_payload))
                .ok()
        } else {
            None
        }
    })
}

#[cfg(test)]
mod tests {
    use zerocopy::U16;

    use super::*;
    use crate::ieee80211::{FrameControl, MacHeader3Addr, SequenceControl};

    #[test]
    fn test_get_attr_id_and_nested_flag() {
        let plain_type = attr_id::IFACE_MAC; // 0x0002
        assert_eq!(get_attr_id_from_type(plain_type), attr_id::IFACE_MAC);
        assert!(!is_attr_nested(plain_type));

        let nested_type = attr_id::KEY | NLA_F_NESTED; // 0x0013 | 0x8000 = 0x8013
        assert_eq!(get_attr_id_from_type(nested_type), attr_id::KEY);
        assert!(is_attr_nested(nested_type));
    }

    #[test]
    fn test_attr_id_to_string_conversion() {
        assert_eq!(attr_id_to_string(attr_id::HW_INDEX), "HW_INDEX");
        assert_eq!(attr_id_to_string(attr_id::MESH_GATE_ANNOUNCEMENT), "MESH_GATE_ANNOUNCEMENT");
        assert_eq!(attr_id_to_string(0xABCD), "Unknown(0xABCD)");
    }

    #[test]
    fn test_nla_align_calculation() {
        assert_eq!(nla_align(0), 0);
        assert_eq!(nla_align(1), 4);
        assert_eq!(nla_align(3), 4);
        assert_eq!(nla_align(4), 4);
        assert_eq!(nla_align(5), 8);
        assert_eq!(
            nla_align(NLA_HDR_SIZE),
            NLA_HDR_SIZE,
            "Header size should be self-aligned or align correctly"
        );
    }

    #[test]
    fn test_nl_attr_iter_empty_buffer() {
        let data: [u8; 0] = [];
        let mut iter = iter_nl_attrs(&data);
        assert!(iter.next().is_none());
    }

    #[test]
    fn test_nl_attr_iter_single_attribute() {
        // Attr: len=8 (hdr=4, payload=4), type=HW_INDEX (1),
        // payload=[0xAA,0xBB,0xCC,0xDD] NlAttrHdr (LE): 08 00 01 00
        let data: [u8; 8] = [0x08, 0x00, 0x01, 0x00, 0xAA, 0xBB, 0xCC, 0xDD];
        let mut iter = iter_nl_attrs(&data);
        if let Some((hdr, payload)) = iter.next() {
            assert_eq!(hdr.length(), 8);
            assert_eq!(get_attr_id_from_type(hdr.attr_type()), attr_id::HW_INDEX);
            assert_eq!(payload, &[0xAA, 0xBB, 0xCC, 0xDD]);
        } else {
            panic!("Expected one attribute");
        }
        assert!(iter.next().is_none());
    }

    #[test]
    fn test_nl_attr_iter_multiple_attributes_with_padding() {
        // Attr1: len=7 (hdr=4, payload=3), type=HW_INDEX (1), payload=[1,2,3]. Aligned
        // len=8.        NlAttrHdr (LE): 07 00 01 00
        // Attr2: len=5 (hdr=4, payload=1), type=IFACE_MAC (2), payload=[0xEE]. Aligned
        // len=8.        NlAttrHdr (LE): 05 00 02 00
        let data: [u8; 16] = [
            0x07, 0x00, 0x01, 0x00, 0x01, 0x02, 0x03, 0x00, // Attr1 + padding
            0x05, 0x00, 0x02, 0x00, 0xEE, 0x00, 0x00, 0x00, // Attr2 + padding
        ];

        let mut iter = iter_nl_attrs(&data);

        if let Some((hdr, payload)) = iter.next() {
            assert_eq!(hdr.length(), 7);
            assert_eq!(get_attr_id_from_type(hdr.attr_type()), attr_id::HW_INDEX);
            assert_eq!(payload, &[0x01, 0x02, 0x03]);
        } else {
            panic!("Expected first attribute");
        }

        if let Some((hdr, payload)) = iter.next() {
            assert_eq!(hdr.length(), 5);
            assert_eq!(get_attr_id_from_type(hdr.attr_type()), attr_id::IFACE_MAC);
            assert_eq!(payload, &[0xEE]);
        } else {
            panic!("Expected second attribute");
        }
        assert!(iter.next().is_none());
    }

    #[test]
    fn test_nl_attr_iter_invalid_length_in_header_too_short() {
        // Header says len 8, but only 6 bytes available in buffer.
        let data: [u8; 6] = [0x08, 0x00, 0x01, 0x00, 0xAA, 0xBB];
        let mut iter = iter_nl_attrs(&data);
        assert!(iter.next().is_none(), "Should fail as nla_len > buffer length");
    }

    #[test]
    fn test_nl_attr_iter_invalid_length_smaller_than_header() {
        // Header says len 2, which is smaller than NLA_HDR_SIZE.
        let data: [u8; 4] = [0x02, 0x00, 0x01, 0x00];
        let mut iter = iter_nl_attrs(&data);
        assert!(iter.next().is_none(), "Should fail as nla_len < NLA_HDR_SIZE");
    }

    #[test]
    fn test_nl_attr_iter_truncated_attribute_padding() {
        // Attr1: len=7. Aligned len=8. Buffer is only 7 bytes (missing padding).
        let data: [u8; 7] = [0x07, 0x00, 0x01, 0x00, 0x01, 0x02, 0x03];
        let mut iter = iter_nl_attrs(&data);
        let (hdr, payload) = iter.next().expect("Should succeed despite missing padding");
        assert_eq!(hdr.length(), 7);
        assert_eq!(payload, &[0x01, 0x02, 0x03]);
        assert!(iter.next().is_none());
    }

    #[test]
    fn test_u32_attr_payload() {
        let val: u32 = 0x12345678;
        let payload = create_u32_attr_payload(val);
        assert_eq!(payload, val.to_le_bytes());
        let parsed_val = parse_u32_from_payload(&payload).unwrap();
        assert_eq!(parsed_val, val);

        assert_eq!(parse_u32_from_payload(&[1, 2, 3]), Err(NetlinkError::InvalidPayloadLength));
    }

    #[test]
    fn test_string_attr_payload() {
        let s = "hello";
        let payload = create_string_attr_payload(s);
        assert_eq!(payload, vec![b'h', b'e', b'l', b'l', b'o', 0]);
        let parsed_s = parse_string_from_payload(&payload).unwrap();
        assert_eq!(parsed_s, s);

        assert_eq!(
            parse_string_from_payload(b"world"), // No null terminator
            Err(NetlinkError::InvalidString)
        );
        let invalid_utf8_payload = [0xff, 0xfe, 0xfd, 0];
        assert_eq!(
            parse_string_from_payload(&invalid_utf8_payload),
            Err(NetlinkError::InvalidString)
        );
    }

    #[test]
    fn test_mac_addr_attr_payload() {
        let mac_bytes = [0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF];
        let mac = EthernetMacAddr { bytes: mac_bytes };
        let payload = create_mac_addr_attr_payload(&mac);
        assert_eq!(payload, mac_bytes);
        let parsed_mac = parse_mac_addr_from_payload(&payload).unwrap();
        assert_eq!(parsed_mac, mac);

        assert_eq!(
            parse_mac_addr_from_payload(&[1, 2, 3, 4, 5]),
            Err(NetlinkError::InvalidPayloadLength)
        );
    }

    #[test]
    fn test_build_netlink_message_simple() {
        let cmd = 1;
        let version = 1;
        let attributes = [(attr_id::HW_INDEX, create_u32_attr_payload(5u32))];

        let message = build_netlink_message(cmd, version, &attributes).unwrap();

        // Expected: GenlMsgHdr (4) + NlAttrHdr (4) + u32_payload (4) = 12 bytes
        assert_eq!(message.len(), GENL_HDR_SIZE + NLA_HDR_SIZE + 4);

        let (genl_hdr, rest) = Ref::<&[u8], GenlMsgHdr>::from_prefix(&message).unwrap();
        assert_eq!(genl_hdr.cmd, cmd);
        assert_eq!(genl_hdr.version, version);

        let (nl_attr, _payload_data) = Ref::<&[u8], NlAttrHdr>::from_prefix(rest).unwrap();
        assert_eq!(nl_attr.length() as usize, NLA_HDR_SIZE + 4);
        assert_eq!(get_attr_id_from_type(nl_attr.attr_type()), attr_id::HW_INDEX);
    }

    #[test]
    fn test_build_netlink_message_with_string_and_padding() {
        let cmd = 2;
        let version = 1;
        let if_name = "wlan_sim0"; // 9 chars + null = 10 bytes. Padded to 12 for attr.
        let attributes = [(attr_id::IFACE_NAME, create_string_attr_payload(if_name))];

        let message = build_netlink_message(cmd, version, &attributes).unwrap();

        // Expected: GenlMsgHdr (4)
        // Attr: NlAttrHdr (4) + string_payload (10) = 14. Padded to 16.
        // Total = 4 + 16 = 20
        let expected_attr_payload_len = if_name.len() + 1; // +1 for null
        let expected_attr_total_unaligned_len = NLA_HDR_SIZE + expected_attr_payload_len;
        let expected_attr_total_aligned_len = nla_align(expected_attr_total_unaligned_len);

        assert_eq!(message.len(), GENL_HDR_SIZE + expected_attr_total_aligned_len);

        let (_genl_hdr, attrs_bytes) = Ref::<&[u8], GenlMsgHdr>::from_prefix(&message).unwrap();
        let (nl_attr_hdr, nl_payload) = iter_nl_attrs(attrs_bytes).next().unwrap();
        assert_eq!(nl_attr_hdr.length() as usize, expected_attr_total_unaligned_len);
        assert_eq!(parse_string_from_payload(nl_payload).unwrap(), if_name);
    }

    #[test]
    fn test_extract_mac80211_frame_from_netlink_success() {
        // 1. Construct the inner 802.11 frame
        let mac_hdr = MacHeader3Addr {
            frame_control: FrameControl::new(0x0008), // Data frame
            duration_id: U16::new(0),
            addr1: EthernetMacAddr::new([1; 6]),
            addr2: EthernetMacAddr::new([2; 6]),
            addr3: EthernetMacAddr::new([3; 6]),
            sequence_control: SequenceControl::new(0),
        };
        let mac_payload = [0xDE, 0xAD, 0xBE, 0xEF];
        let mut frame_bytes = Vec::new();
        frame_bytes.extend_from_slice(mac_hdr.as_bytes());
        frame_bytes.extend_from_slice(&mac_payload);

        // 2. Build the Netlink message with this frame as an attribute
        let attributes =
            [(attr_id::HWSIM_ATTR_FRAME_DATA, create_bytes_attr_payload(&frame_bytes))];
        let netlink_message = build_netlink_message(5, 1, &attributes).unwrap();

        // 3. Attempt to extract the frame
        let result = extract_mac80211_frame_from_netlink(&netlink_message);

        // 4. Assert success and correctness
        assert!(result.is_some());
        let (genl_hdr, extracted_mac_hdr, extracted_mac_payload) = result.unwrap();

        assert_eq!(genl_hdr.cmd, 5);
        assert_eq!(extracted_mac_hdr.addr1.bytes, [1; 6]);
        assert_eq!(extracted_mac_hdr.addr2.bytes, [2; 6]);
        assert_eq!(extracted_mac_payload, &mac_payload);
    }

    #[test]
    fn test_extract_mac80211_frame_from_netlink_no_frame_attr() {
        // Build a message *without* the HWSIM_ATTR_FRAME_DATA attribute
        let attributes = [(attr_id::HW_INDEX, create_u32_attr_payload(123))];
        let netlink_message = build_netlink_message(5, 1, &attributes).unwrap();

        let result = extract_mac80211_frame_from_netlink(&netlink_message);
        assert!(result.is_none());
    }

    #[test]
    fn test_extract_mac80211_frame_from_netlink_buffer_too_short() {
        // Message is too short to even contain a GenlMsgHdr
        let short_message = [0x01, 0x02, 0x03];
        let result = extract_mac80211_frame_from_netlink(&short_message);
        assert!(result.is_none());
    }
}
