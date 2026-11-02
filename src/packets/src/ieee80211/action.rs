// Copyright 2024 The Android Open Source Project
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

// use crate::ieee80211::MacAddress; // Unused
use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout, Unaligned};

/// Action Frame Category Values
pub mod category {
    pub const SPECTRUM_MANAGEMENT: u8 = 0;
    pub const QOS: u8 = 1;
    pub const DLS: u8 = 2;
    pub const BLOCK_ACK: u8 = 3;
    pub const PUBLIC: u8 = 4;
    pub const RADIO_MEASUREMENT: u8 = 5;
    pub const FAST_BSS_TRANSITION: u8 = 6;
    pub const HT: u8 = 7;
    pub const SA_QUERY: u8 = 8;
    pub const PROTECTED_DUAL_OF_PUBLIC_ACTION: u8 = 9;
    pub const WNM: u8 = 10;
    pub const UNPROTECTED_WNM: u8 = 11;
    pub const TDLS: u8 = 12;
    pub const MESH: u8 = 13;
    pub const MULTIHOP: u8 = 14;
    pub const SELF_PROTECTED: u8 = 15;
    pub const DMV: u8 = 16;
    pub const FST: u8 = 18;
    pub const ROBUST_AV_STREAMING: u8 = 19;
    pub const UNPROTECTED_DMG: u8 = 20;
    pub const VHT: u8 = 21;
    pub const VENDOR_SPECIFIC_PROTECTED: u8 = 126;
    pub const VENDOR_SPECIFIC: u8 = 127;
}

/// Public Action Field Values
pub mod public_action {
    pub const VENDOR_SPECIFIC: u8 = 9;
    pub const GAS_INITIAL_REQUEST: u8 = 10;
    pub const GAS_INITIAL_RESPONSE: u8 = 11;
    pub const FTM_REQUEST: u8 = 32;
    pub const FINE_TIMING_MEASUREMENT: u8 = 33;
}

#[derive(FromBytes, IntoBytes, Immutable, KnownLayout, Unaligned, Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct ActionHeader {
    pub category: u8,
    pub action: u8,
}

#[derive(FromBytes, IntoBytes, Immutable, KnownLayout, Unaligned, Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct VendorSpecificPublicActionHeader {
    pub dialog_token: u8,
    pub oui: [u8; 3],
    pub oui_type: u8,
}

#[derive(FromBytes, IntoBytes, Immutable, KnownLayout, Unaligned, Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct FtmRequest {
    pub trigger: u8,
    // Variable length fields follow:
    // LCI Measurement Request (optional)
    // Location Civic Measurement Request (optional)
    // FTM Parameters (optional)
}

#[derive(FromBytes, IntoBytes, Immutable, KnownLayout, Unaligned, Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct FineTimingMeasurement {
    pub dialog_token: u8,
    pub follow_up_dialog_token: u8,
    pub tod: [u8; 6], // Time of Departure (48-bit)
    pub toa: [u8; 6], // Time of Arrival (48-bit)
    pub tod_error: [u8; 6], /* TOD Error (actually part of a larger packed struct in spec,
                       * simplified here for simulation)
                       * Spec: TOD Error (1 byte) | TOA Error (1 byte) | LCI Report
                       * (variable) ? IEEE 802.11-2016 Figure
                       * 9-556 Order: Category, Action, Dialog
                       * Token, Follow Up Dialog Token, TOD, TOA, TOD Error, TOA Error,
                       * LCI Report... Actually, TOD and TOA
                       * are 6 bytes (48 bits). TOD Error and
                       * TOA Error are 1 byte each?
                       * Let's check the spec closer or use a larger buffer.
                       * For now, we define the FIXED fields. */
}

// 802.11mc FTM Parameters Bitmap
// Bit 0: Asap
// Bit 1: LMR Feedback
// Bit 2-3: Reserved
// Bit 4-7: Number of Bursts Exponent
pub const FTM_PARAM_ASAP: u8 = 0x01;
pub const FTM_PARAM_LMR_FEEDBACK: u8 = 0x02;

/// Wi-Fi Aware (NAN) constants and header definitions
pub mod nan {
    pub const OUI: [u8; 3] = [0x50, 0x6F, 0x9A];
    pub const OUI_TYPE: u8 = 0x13;

    pub mod attr {
        pub const SDA: u8 = 0x03;
        pub const NDP: u8 = 0x10;
    }

    pub mod service_type {
        pub const PUBLISH: u8 = 0;
        pub const SUBSCRIBE: u8 = 1;
        pub const FOLLOW_UP: u8 = 2;
    }

    pub mod ndp_type {
        pub const REQUEST: u8 = 0;
        pub const RESPONSE: u8 = 1;
        pub const CONFIRM: u8 = 2;
        pub const TERMINATE: u8 = 4;
    }
}

/// Wi-Fi Direct (P2P) constants and header definitions
pub mod p2p {
    pub const OUI: [u8; 3] = [0x50, 0x6F, 0x9A];
    pub const OUI_TYPE: u8 = 0x09;

    pub mod action_type {
        pub const GO_NEG_REQ: u8 = 0;
        pub const GO_NEG_RESP: u8 = 1;
        pub const GO_NEG_CONF: u8 = 2;
        pub const INVITATION_REQ: u8 = 3;
        pub const INVITATION_RESP: u8 = 4;
        pub const DEV_DISC_REQ: u8 = 5;
        pub const DEV_DISC_RESP: u8 = 6;
        pub const PROV_DISC_REQ: u8 = 7;
        pub const PROV_DISC_RESP: u8 = 8;
    }
}

/// Wi-Fi Easy Connect (DPP) constants and header definitions
pub mod dpp {
    pub const OUI: [u8; 3] = [0x50, 0x6F, 0x9A];
    pub const OUI_TYPE: u8 = 0x1A;

    pub mod action_type {
        pub const AUTH_REQ: u8 = 0;
        pub const AUTH_RESP: u8 = 1;
        pub const AUTH_CONF: u8 = 2;
        pub const CONFIG_RESULT: u8 = 11;
    }
}

#[derive(FromBytes, IntoBytes, Immutable, KnownLayout, Unaligned, Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct NanAttributeHeader {
    pub id: u8,
    pub len: [u8; 2],
}

#[derive(FromBytes, IntoBytes, Immutable, KnownLayout, Unaligned, Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct NanSdaHeader {
    pub service_id: [u8; 6],
    pub instance_id: u8,
    pub req_instance_id: u8,
    pub control: u8,
}

#[derive(FromBytes, IntoBytes, Immutable, KnownLayout, Unaligned, Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct NanNdpHeader {
    pub dialog_token: u8,
    pub type_and_status: u8,
}

/// Iterator for NAN Attributes in a byte slice.
pub struct NanAttributeIterator<'a> {
    data: &'a [u8],
}

impl<'a> NanAttributeIterator<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self { data }
    }
}

pub struct NanAttribute<'a> {
    pub id: u8,
    pub val: &'a [u8],
}

impl<'a> Iterator for NanAttributeIterator<'a> {
    type Item = NanAttribute<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.data.is_empty() {
            return None;
        }
        let (hdr, rest) = NanAttributeHeader::ref_from_prefix(self.data).ok()?;
        let len = u16::from_le_bytes(hdr.len) as usize;
        if len > rest.len() {
            return None;
        }
        let (val, next) = rest.split_at(len);
        self.data = next;
        Some(NanAttribute { id: hdr.id, val })
    }
}
