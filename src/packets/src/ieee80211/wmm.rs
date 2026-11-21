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

use crate::ieee80211::ie::write_ie;

pub const OUI_MICROSOFT: [u8; 3] = [0x00, 0x50, 0xf2];
pub const OUI_TYPE_WMM: u8 = 2;
pub const OUI_SUBTYPE_WMM_PARAM: u8 = 1;
pub const WMM_VERSION: u8 = 1;

const QOS_INFO_UAPSD_BIT: u8 = 0x80;
const QOS_INFO_PARAM_SET_COUNT_MASK: u8 = 0x0F;
const WMM_RESERVED: u8 = 0;

const fn ac_param_record(aci: u8, aifsn: u8, ecw_min: u8, ecw_max: u8, txop_limit: u16) -> [u8; 4] {
    let [txop_lo, txop_hi] = txop_limit.to_le_bytes();
    [(aci << 5) | aifsn, ecw_min | (ecw_max << 4), txop_lo, txop_hi]
}

// Default AP AC records: AC_BE, AC_BK, AC_VI (~3ms TXOP), AC_VO (~1.5ms TXOP).
const DEFAULT_AC_RECORDS: [[u8; 4]; 4] = [
    ac_param_record(0, 3, 4, 10, 0),
    ac_param_record(1, 7, 4, 10, 0),
    ac_param_record(2, 2, 3, 4, 94),
    ac_param_record(3, 2, 2, 3, 47),
];

/// Writes the WMM Parameter Element to the buffer.
///
/// Structure:
/// - OUI (3 bytes): 00:50:f2
/// - OUI Type (1 byte): 2
/// - OUI Subtype (1 byte): 1
/// - Version (1 byte): 1
/// - QoS Info (1 byte)
/// - Reserved (1 byte)
/// - AC Parameters (4 records * 4 bytes each = 16 bytes)
pub fn write_wmm_param_element(buf: &mut Vec<u8>, uapsd: bool, param_set_count: u8) {
    let qos_info = (param_set_count & QOS_INFO_PARAM_SET_COUNT_MASK)
        | if uapsd { QOS_INFO_UAPSD_BIT } else { 0 };
    let header = [OUI_TYPE_WMM, OUI_SUBTYPE_WMM_PARAM, WMM_VERSION, qos_info, WMM_RESERVED];
    let body = [OUI_MICROSOFT.as_slice(), &header, DEFAULT_AC_RECORDS.as_flattened()].concat();

    write_ie(buf, crate::ieee80211::ie::tags::VENDOR_SPECIFIC, &body);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_wmm_param_element_default_wire_bytes() {
        let mut out = Vec::new();
        write_wmm_param_element(&mut out, false, 0);
        assert_eq!(
            out,
            [
                0xdd, 24, 0x00, 0x50, 0xf2, 0x02, 0x01, 0x01, 0x00, 0x00, 0x03, 0xa4, 0x00, 0x00,
                0x27, 0xa4, 0x00, 0x00, 0x42, 0x43, 0x5e, 0x00, 0x62, 0x32, 0x2f, 0x00,
            ]
        );
    }

    #[test]
    fn write_wmm_param_element_uapsd_and_masks_param_set_count() {
        const QOS_INFO_OFFSET: usize = 8; // IE tag + len + OUI + type + subtype + version
        let mut out = Vec::new();
        write_wmm_param_element(&mut out, true, 0x1F);
        assert_eq!(out[QOS_INFO_OFFSET], 0x80 | 0x0F);
    }
}
