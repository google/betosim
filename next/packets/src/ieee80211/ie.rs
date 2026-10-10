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

//! Information Element (IE) parsing for IEEE 802.11 frames.

use std::fmt;

/// Common Information Element Tags (IDs).
pub mod tags {
    pub const SSID: u8 = 0;
    pub const SUPPORTED_RATES: u8 = 1;
    pub const DS_PARAMETER_SET: u8 = 3;
    pub const TRAFFIC_INDICATION_MAP: u8 = 5;
    pub const TIM: u8 = 5;
    pub const COUNTRY: u8 = 7;
    pub const IBSS_PARAMETER_SET: u8 = 6;
    pub const ERP_INFORMATION: u8 = 42;
    pub const HT_CAPABILITIES: u8 = 45;
    pub const HT_OPERATION: u8 = 61;
    pub const RSN: u8 = 48; // Robust Security Network
    pub const EXTENDED_SUPPORTED_RATES: u8 = 50;
    pub const VENDOR_SPECIFIC: u8 = 221;
    pub const EXTENSION: u8 = 255;

    /// Default supported rates for 802.11 b/g/n (all standard rates).
    /// Values are in 500kbps units.
    /// 0x02(1M), 0x04(2M), 0x0B(5.5M), 0x16(11M), 0x0C(6M), 0x12(9M),
    /// 0x18(12M), 0x24(18M), 0x30(24M), 0x48(36M), 0x60(48M), 0x6C(54M)
    pub const SUPPORTED_RATES_DEFAULT: &[u8] =
        &[0x02, 0x04, 0x0B, 0x16, 0x0C, 0x12, 0x18, 0x24, 0x30, 0x48, 0x60, 0x6C];

    pub const HE_CAPABILITIES: u8 = 35;

    // RSN Constants
    pub const RSN_VER: u16 = 1;
    pub const CIPHER_CCMP: &[u8] = &[0x00, 0x0F, 0xAC, 0x04];
    pub const AKM_PSK: &[u8] = &[0x00, 0x0F, 0xAC, 0x02];

    pub const EXTENDED_CAPABILITIES: u8 = 127;
    // Bit 70 is FTM Responder.
    // This requires at least 9 bytes (70 / 8 = 8.75).
    // Byte 0 (Bits 0-7), Byte 8 (Bits 64-71).
    // FTM Responder is Bit 6 in Byte 8 (0-indexed byte).
    // 8 * 8 = 64. 70 - 64 = 6. 1 << 6 = 0x40.
    pub const EXTENDED_CAPABILITIES_FTM_RESPONDER_BIT: u8 = 70;

    // Default Extended Caps (9 bytes) with just FTM Responder enabled for now
    // (if requested) or a helper to build it.
}

/// Helper to set a specific bit in Extended Capabilities
/// Resizes vec if needed.
pub fn set_ext_cap(body: &mut Vec<u8>, bit: u8) {
    let byte_idx = (bit / 8) as usize;
    if byte_idx >= body.len() {
        body.resize(byte_idx + 1, 0);
    }
    if let Some(b) = body.get_mut(byte_idx) {
        *b |= 1 << (bit % 8);
    }
}

/// Represents a parsed Information Element.
#[derive(Clone, Copy)]
pub struct InformationElement<'a> {
    pub id: u8,
    pub length: u8,
    pub body: &'a [u8],
}

impl<'a> fmt::Debug for InformationElement<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("InformationElement")
            .field("id", &self.id)
            .field("length", &self.length)
            .field("body_len", &self.body.len())
            .finish()
    }
}

/// Iterator for Information Elements in a byte slice.
pub struct IeIterator<'a> {
    data: &'a [u8],
    offset: usize,
}

impl<'a> IeIterator<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, offset: 0 }
    }
}

impl<'a> Iterator for IeIterator<'a> {
    type Item = InformationElement<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        let (&[id, len], rest) = self.data.get(self.offset..)?.split_first_chunk::<2>()?;
        let body = rest.get(..len as usize)?;
        self.offset += 2 + len as usize;
        Some(InformationElement { id, length: len, body })
    }
}

/// Writes an Information Element to a byte vector.
pub fn write_ie(buf: &mut Vec<u8>, id: u8, body: &[u8]) {
    buf.push(id);
    // Length is u8, so max 255 bytes.
    // Ideally we should check body.len() <= 255 or panic/result.
    // For now, simple truncation or cast (risky but matches current manual
    // behavior).
    buf.push(body.len() as u8);
    buf.extend_from_slice(body);
}
