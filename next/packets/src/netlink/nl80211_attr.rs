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

//! Defines structures for parsing `nl80211` attributes.

use zerocopy::{
    FromBytes, Immutable, IntoBytes, KnownLayout, U16, Unaligned, byteorder::LittleEndian,
};

/// Represents the Netlink attribute header.
/// This header precedes the actual attribute data in a Netlink message.
#[repr(C)]
#[derive(
    Debug, Copy, Clone, FromBytes, IntoBytes, Unaligned, Immutable, KnownLayout, PartialEq, Eq,
)]
pub struct NlAttrHdr {
    /// Length of the attribute, including the header.
    pub nla_len: U16<LittleEndian>,
    /// Type of the attribute. The lower 14 bits are the attribute ID,
    /// and the upper 2 bits are flags (e.g., NLA_F_NESTED).
    pub attr_type: U16<LittleEndian>,
}

impl NlAttrHdr {
    /// Creates a new Netlink attribute header.
    ///
    /// # Arguments
    ///
    /// * `len` - The total length of the attribute, including the header.
    /// * `attr_type` - The type of the attribute (see `attr_id` module).
    pub fn new(len: u16, attr_type_val: u16) -> Self {
        NlAttrHdr { nla_len: U16::new(len), attr_type: U16::new(attr_type_val) }
    }

    /// Gets the length of the attribute.
    pub fn length(&self) -> u16 {
        self.nla_len.get()
    }

    /// Gets the type of the attribute.
    pub fn attr_type(&self) -> u16 {
        self.attr_type.get()
    }

    /// Gets the type of the attribute (alias for attr_type).
    pub fn type_(&self) -> u16 {
        self.attr_type.get()
    }

    /// Decodes a full `NlAttrHdr` from bytes.
    pub fn decode_full(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() < 4 {
            return Err("Packet too short for NlAttrHdr".into());
        }
        let (hdr, _) = zerocopy::Ref::<&[u8], NlAttrHdr>::from_prefix(bytes)
            .map_err(|err| format!("Failed to read NlAttrHdr: {}", err))?;
        Ok(*hdr)
    }
}

#[cfg(test)]
pub struct NlAttr<'a> {
    /// The attribute type.
    pub attr_type: u16,
    /// The raw byte payload of the attribute.
    pub payload: &'a [u8],
}

#[cfg(test)]
pub fn parse_attributes(mut data: &[u8]) -> Result<Vec<NlAttr<'_>>, String> {
    let mut attrs = Vec::new();
    while !data.is_empty() {
        let (hdr, rest): (&NlAttrHdr, &[u8]) = match FromBytes::ref_from_prefix(data) {
            Ok((hdr, rest)) => (hdr, rest),
            Err(_) => return Err("Failed to parse NlAttrHdr".to_string()),
        };
        data = rest;

        let len = hdr.length() as usize;
        let payload_len = len.saturating_sub(std::mem::size_of::<NlAttrHdr>());

        if payload_len > data.len() {
            return Err(format!(
                "Attribute payload length {} exceeds remaining data length {}",
                payload_len,
                data.len()
            ));
        }

        let (payload, _rest) = data.split_at(payload_len);
        attrs.push(NlAttr { attr_type: hdr.attr_type(), payload });

        // Move to the next attribute, considering alignment.
        let aligned_len = (len + 3) & !3;
        if aligned_len > data.len() {
            data = &data[data.len()..];
        } else {
            data = &data[aligned_len - std::mem::size_of::<NlAttrHdr>()..];
        }
    }
    Ok(attrs)
}

#[cfg(test)]
mod tests {
    use zerocopy::IntoBytes;

    use super::*;
    use crate::netlink::nl80211::attr_id;

    #[test]
    fn test_parse_attributes_success() {
        let mut buffer = Vec::new();
        // Attribute 1: IFACE_MAC
        let hdr1 = NlAttrHdr::new(8, attr_id::IFACE_MAC);
        let payload1: [u8; 4] = [0x00, 0x11, 0x22, 0x33];
        buffer.extend_from_slice(hdr1.as_bytes());
        buffer.extend_from_slice(&payload1);

        // Attribute 2: IFACE_NAME
        let hdr2 = NlAttrHdr::new(12, attr_id::IFACE_NAME);
        let payload2: &[u8] = b"wlan0\0\0\0";
        buffer.extend_from_slice(hdr2.as_bytes());
        buffer.extend_from_slice(payload2);

        let attrs = parse_attributes(&buffer).unwrap();
        assert_eq!(attrs.len(), 2);
        assert_eq!(attrs[0].attr_type, attr_id::IFACE_MAC);
        assert_eq!(attrs[0].payload, &payload1);
        assert_eq!(attrs[1].attr_type, attr_id::IFACE_NAME);
        assert_eq!(attrs[1].payload, payload2);
    }

    #[test]
    fn test_parse_attributes_empty() {
        let buffer: [u8; 0] = [];
        let attrs = parse_attributes(&buffer).unwrap();
        assert!(attrs.is_empty());
    }

    #[test]
    fn test_parse_attributes_incomplete_header() {
        let buffer: [u8; 3] = [0x08, 0x00, 0x02]; // Incomplete header
        let result = parse_attributes(&buffer);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_attributes_invalid_length() {
        let mut buffer = Vec::new();
        let hdr = NlAttrHdr::new(12, attr_id::IFACE_MAC); // Length is 12, but payload is 4
        let payload: [u8; 4] = [0x00, 0x11, 0x22, 0x33];
        buffer.extend_from_slice(hdr.as_bytes());
        buffer.extend_from_slice(&payload);

        let result = parse_attributes(&buffer);
        assert!(result.is_err());
    }
}
