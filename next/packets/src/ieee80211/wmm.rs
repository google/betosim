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
    let mut body = Vec::with_capacity(24);
    body.extend_from_slice(&OUI_MICROSOFT);
    body.push(OUI_TYPE_WMM);
    body.push(OUI_SUBTYPE_WMM_PARAM);
    body.push(WMM_VERSION);

    // QoS Info:
    // Bit 7: U-APSD
    // Bits 0-3: Parameter Set Count
    let mut qos_info = param_set_count & 0x0F;
    if uapsd {
        qos_info |= 0x80;
    }
    body.push(qos_info);
    body.push(0); // Reserved

    // AC Parameters (Record Format: ACI/AIFSN, ECW Min/Max, TXOP Limit)
    // We use default values typical for an AP.

    // AC_BE (Best Effort) - ACI 0
    // AIFSN: 3, ECWmin: 4, ECWmax: 10, TXOP: 0
    body.push(3); // ACI=0, ACM=0, AIFSN=3
    body.push(4 | (10 << 4)); // ECWmin=4, ECWmax=10
    body.extend_from_slice(&0u16.to_le_bytes()); // TXOP Limit

    // AC_BK (Background) - ACI 1
    // AIFSN: 7, ECWmin: 4, ECWmax: 10, TXOP: 0
    body.push((1 << 5) | 7); // ACI=1, ACM=0, AIFSN=7
    body.push(4 | (10 << 4));
    body.extend_from_slice(&0u16.to_le_bytes());

    // AC_VI (Video) - ACI 2
    // AIFSN: 2, ECWmin: 3, ECWmax: 4, TXOP: 94 (approx 3ms)
    body.push((2 << 5) | 2); // ACI=2, ACM=0, AIFSN=2
    body.push(3 | (4 << 4));
    body.extend_from_slice(&94u16.to_le_bytes());

    // AC_VO (Voice) - ACI 3
    // AIFSN: 2, ECWmin: 2, ECWmax: 3, TXOP: 47 (approx 1.5ms)
    body.push((3 << 5) | 2); // ACI=3, ACM=0, AIFSN=2
    body.push(2 | (3 << 4));
    body.extend_from_slice(&47u16.to_le_bytes());

    write_ie(buf, crate::ieee80211::ie::tags::VENDOR_SPECIFIC, &body);
}
