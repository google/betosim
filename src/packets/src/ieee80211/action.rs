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

/// 802.11 Action frame control field (Type = Management, Subtype = Action).
pub const FRAME_CONTROL_ACTION: u16 =
    u16::from_le_bytes([super::management_subtype::ACTION, 0]) << 4;

/// Timestamp of Departure (`t1`) reported in the FTM measurement frame,
/// fixed at 1 s in picoseconds.
///
/// Ranging only uses the delta (`t4 - t1`), so any baseline works; 1 s is
/// a round value well below the 48-bit (~281 s) timestamp ceiling.
const INITIAL_TOD_PICOSECONDS: u64 = 1_000_000_000_000;

/// Short Interframe Space (SIFS) duration in picoseconds (10 microseconds).
pub const SIFS_DURATION_PICOSECONDS: u64 = 10_000_000;

/// Maximum 48-bit unsigned timestamp value (`2^48 - 1`).
const MAX_TIMESTAMP_48BIT: u64 = 0x0000_FFFF_FFFF_FFFF;

#[derive(IntoBytes, Immutable)]
#[repr(C)]
struct FtmActionFrame {
    header: crate::ieee80211::MacHeader3Addr,
    action: ActionHeader,
    body: FineTimingMeasurement,
}

/// Wire size in bytes of an 802.11 FTM Public Action frame (`MacHeader3Addr`
/// 24 B + `ActionHeader` 2 B + `FineTimingMeasurement` 20 B = 46 B).
pub const FTM_ACTION_FRAME_LEN: usize = size_of::<FtmActionFrame>();

const _: () = assert!(FTM_ACTION_FRAME_LEN == 46);

/// Encodes the least-significant 48 bits (6 bytes) of a picosecond timestamp
/// into little-endian bytes.
///
/// IEEE 802.11-2016 Section 9.6.8.33 defines FTM `TOD` and `TOA` wire fields
/// in 0.1 ns (100 ps) units; this codec intentionally preserves 1 ps input
/// units (modulo 2^48) for compatibility with existing callers in netsim.
/// Dropping the two most-significant bytes from `to_le_bytes` truncates to 48
/// bits.
pub const fn encode_timestamp_48bit(timestamp_ps: u64) -> [u8; 6] {
    let [timestamp @ .., _, _] = timestamp_ps.to_le_bytes();
    timestamp
}

/// Decodes a 48-bit little-endian `TOD`/`TOA` timestamp (1 ps units in
/// netsim; see [`encode_timestamp_48bit`]) into a `u64`.
pub const fn decode_timestamp_48bit(bytes: &[u8; 6]) -> u64 {
    let &[b0, b1, b2, b3, b4, b5] = bytes;
    u64::from_le_bytes([b0, b1, b2, b3, b4, b5, 0, 0])
}

/// Constructs a serialized 802.11 Public Action frame carrying an FTM payload.
fn build_ftm_action_frame(
    header: &crate::ieee80211::MacHeader3Addr,
    body: &FineTimingMeasurement,
) -> [u8; FTM_ACTION_FRAME_LEN] {
    let frame = FtmActionFrame {
        header: crate::ieee80211::MacHeader3Addr {
            frame_control: header.frame_control,
            duration_id: header.duration_id,
            addr1: header.addr1,
            addr2: header.addr2,
            addr3: header.addr3,
            sequence_control: header.sequence_control,
        },
        action: ActionHeader {
            category: category::PUBLIC,
            action: public_action::FINE_TIMING_MEASUREMENT,
        },
        body: *body,
    };
    let mut bytes = [0u8; FTM_ACTION_FRAME_LEN];
    bytes.copy_from_slice(frame.as_bytes());
    bytes
}

/// Constructs the two FTM response frames (Initial + Measurement).
///
/// Both frames copy `header` verbatim (including caller `SequenceControl` and
/// `Retry` bits); `SIFS_DURATION_PICOSECONDS` (`10 us`) follows 2.4 GHz SIFS convention.
///
/// Per IEEE 802.11-2016 Section 9.6.8.33, Frame 1 sets `dialog_token` to
/// `dialog_token` (normalizing `0` to `1`) and `follow_up_dialog_token` to `0`.
/// Frame 2 sets `dialog_token` to `0` to signal the final frame of the burst
/// (no subsequent follow-up frame) and `follow_up_dialog_token` to Frame 1's
/// `dialog_token`.
///
/// `flight_ps` is the one-way time of flight in picoseconds, so the measurement
/// frame encodes `t4 - t1 = 2 * flight_ps + SIFS_DURATION_PICOSECONDS`. If
/// `t1 + 2 * flight_ps + SIFS_DURATION_PICOSECONDS` exceeds 48 bits, `toa`
/// saturates at the 48-bit ceiling (`0x0000_FFFF_FFFF_FFFF`) rather than
/// wrapping below `tod`.
pub fn build_ftm_response_frames(
    header: &crate::ieee80211::MacHeader3Addr,
    dialog_token: u8,
    flight_ps: u64,
) -> [[u8; FTM_ACTION_FRAME_LEN]; 2] {
    let dialog_token_1 = if dialog_token == 0 { 1 } else { dialog_token };

    let body_1 = FineTimingMeasurement {
        dialog_token: dialog_token_1,
        follow_up_dialog_token: 0,
        tod: [0; 6],
        toa: [0; 6],
        tod_error: [0; 6],
    };

    let t1_val = INITIAL_TOD_PICOSECONDS;
    let t4_val = t1_val
        .saturating_add((2u64).saturating_mul(flight_ps))
        .saturating_add(SIFS_DURATION_PICOSECONDS)
        .min(MAX_TIMESTAMP_48BIT);
    let body_2 = FineTimingMeasurement {
        dialog_token: 0,
        follow_up_dialog_token: dialog_token_1,
        tod: encode_timestamp_48bit(t1_val),
        toa: encode_timestamp_48bit(t4_val),
        tod_error: [0; 6],
    };

    [build_ftm_action_frame(header, &body_1), build_ftm_action_frame(header, &body_2)]
}

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

#[cfg(test)]
mod tests {
    use zerocopy::U16;

    use super::*;
    use crate::ieee80211::{FrameControl, MacAddress, MacHeader3Addr, SequenceControl};

    #[test]
    fn test_encode_timestamp_48bit() {
        let val: u64 = 0x0000_1234_5678_9ABC;
        let encoded = encode_timestamp_48bit(val);
        assert_eq!(encoded, [0xBC, 0x9A, 0x78, 0x56, 0x34, 0x12]);
    }

    #[test]
    fn test_encode_timestamp_48bit_zero() {
        assert_eq!(encode_timestamp_48bit(0), [0; 6]);
    }

    #[test]
    fn test_decode_timestamp_48bit_direct() {
        let bytes: [u8; 6] = [0xBC, 0x9A, 0x78, 0x56, 0x34, 0x12];
        assert_eq!(decode_timestamp_48bit(&bytes), 0x0000_1234_5678_9ABC);
    }

    #[test]
    fn test_decode_timestamp_48bit_roundtrip() {
        assert_eq!(decode_timestamp_48bit(&encode_timestamp_48bit(0)), 0);
        assert_eq!(
            decode_timestamp_48bit(&encode_timestamp_48bit(INITIAL_TOD_PICOSECONDS)),
            INITIAL_TOD_PICOSECONDS
        );
        assert_eq!(
            decode_timestamp_48bit(&encode_timestamp_48bit(0x0000_1234_5678_9ABC)),
            0x0000_1234_5678_9ABC
        );
        assert_eq!(
            decode_timestamp_48bit(&encode_timestamp_48bit(MAX_TIMESTAMP_48BIT)),
            MAX_TIMESTAMP_48BIT
        );
    }

    #[test]
    fn test_timestamp_48bit_truncation() {
        let val_with_upper_bits: u64 = 0xABCD_1234_5678_9ABC;
        let encoded = encode_timestamp_48bit(val_with_upper_bits);
        assert_eq!(encoded, [0xBC, 0x9A, 0x78, 0x56, 0x34, 0x12]);
        assert_eq!(decode_timestamp_48bit(&encoded), 0x0000_1234_5678_9ABC);
    }

    fn parse_ftm_frame(frame: &[u8]) -> (&MacHeader3Addr, &ActionHeader, &FineTimingMeasurement) {
        let (mac, rest) = MacHeader3Addr::ref_from_prefix(frame).expect("valid MAC header");
        let (action_hdr, rest) = ActionHeader::ref_from_prefix(rest).expect("valid Action header");
        let (ftm, rest) = FineTimingMeasurement::ref_from_prefix(rest).expect("valid FTM body");
        assert!(rest.is_empty(), "unexpected trailing bytes in FTM frame");
        (mac, action_hdr, ftm)
    }

    const FRAME_CONTROL_RETRY_MASK: u16 = 0b0000_1000_0000_0000;

    fn sample_ftm_header() -> MacHeader3Addr {
        MacHeader3Addr {
            frame_control: FrameControl::new(FRAME_CONTROL_ACTION | FRAME_CONTROL_RETRY_MASK),
            duration_id: U16::new(0x013A),
            addr1: MacAddress::from([0x02, 0x00, 0x00, 0x00, 0x00, 0x01]),
            addr2: MacAddress::from([0x02, 0x00, 0x00, 0x00, 0x00, 0x02]),
            addr3: MacAddress::from([0x02, 0x00, 0x00, 0x00, 0x00, 0x03]),
            sequence_control: SequenceControl::new(0x1230),
        }
    }

    #[test]
    fn test_build_ftm_response_frames() {
        let header = sample_ftm_header();
        const TEST_DIALOG_TOKEN: u8 = 42;
        const TEST_FLIGHT_PS: u64 = 1_000;
        let [frame_1, frame_2] =
            build_ftm_response_frames(&header, TEST_DIALOG_TOKEN, TEST_FLIGHT_PS);

        let (mac_1, action_1, ftm_1) = parse_ftm_frame(&frame_1);
        assert_eq!(mac_1.frame_control.get(), FRAME_CONTROL_ACTION | FRAME_CONTROL_RETRY_MASK);
        assert!(mac_1.frame_control.retry());
        assert_eq!(mac_1.duration_id, header.duration_id);
        assert_eq!(mac_1.addr1, header.addr1);
        assert_eq!(mac_1.addr2, header.addr2);
        assert_eq!(mac_1.addr3, header.addr3);
        assert_eq!(mac_1.sequence_control.get(), header.sequence_control.get());
        assert_eq!(mac_1.as_bytes(), header.as_bytes());
        assert_eq!(action_1.category, category::PUBLIC);
        assert_eq!(action_1.action, public_action::FINE_TIMING_MEASUREMENT);
        assert_eq!(ftm_1.dialog_token, TEST_DIALOG_TOKEN);
        assert_eq!(ftm_1.follow_up_dialog_token, 0);
        assert_eq!(ftm_1.tod, [0; 6]);
        assert_eq!(ftm_1.toa, [0; 6]);
        assert_eq!(ftm_1.tod_error, [0; 6]);

        let (mac_2, action_2, ftm_2) = parse_ftm_frame(&frame_2);
        assert_eq!(mac_2.frame_control.get(), FRAME_CONTROL_ACTION | FRAME_CONTROL_RETRY_MASK);
        assert!(mac_2.frame_control.retry());
        assert_eq!(mac_2.duration_id, header.duration_id);
        assert_eq!(mac_2.addr1, header.addr1);
        assert_eq!(mac_2.addr2, header.addr2);
        assert_eq!(mac_2.addr3, header.addr3);
        assert_eq!(mac_2.sequence_control.get(), header.sequence_control.get());
        assert_eq!(mac_2.as_bytes(), header.as_bytes());
        assert_eq!(action_2.category, category::PUBLIC);
        assert_eq!(action_2.action, public_action::FINE_TIMING_MEASUREMENT);
        assert_eq!(ftm_2.dialog_token, 0);
        assert_eq!(ftm_2.follow_up_dialog_token, TEST_DIALOG_TOKEN);
        // Concrete expected little-endian 48-bit timestamps (picoseconds):
        // t1 = 1_000_000_000_000 ps -> [0x00, 0x10, 0xa5, 0xd4, 0xe8, 0x00]
        // t4 = 1_000_010_002_000 ps -> [0x50, 0xae, 0x3d, 0xd5, 0xe8, 0x00]
        let expected_tod: [u8; 6] = [0x00, 0x10, 0xa5, 0xd4, 0xe8, 0x00];
        let expected_toa: [u8; 6] = [0x50, 0xae, 0x3d, 0xd5, 0xe8, 0x00];
        assert_eq!(ftm_2.tod, expected_tod);
        assert_eq!(ftm_2.toa, expected_toa);
        assert_eq!(ftm_2.tod_error, [0; 6]);
    }

    #[test]
    fn test_build_ftm_response_frames_zero_dialog_token_normalizes_to_one() {
        let header = sample_ftm_header();
        let [frame_1, frame_2] = build_ftm_response_frames(&header, 0, 0);

        let (_, _, ftm_1) = parse_ftm_frame(&frame_1);
        assert_eq!(ftm_1.dialog_token, 1);
        assert_eq!(ftm_1.follow_up_dialog_token, 0);

        let (_, _, ftm_2) = parse_ftm_frame(&frame_2);
        assert_eq!(ftm_2.dialog_token, 0);
        assert_eq!(ftm_2.follow_up_dialog_token, 1);
    }

    #[test]
    fn test_build_ftm_response_frames_max_dialog_token() {
        let header = sample_ftm_header();
        let [frame_1, frame_2] = build_ftm_response_frames(&header, u8::MAX, 0);

        let (_, _, ftm_1) = parse_ftm_frame(&frame_1);
        assert_eq!(ftm_1.dialog_token, 255);
        assert_eq!(ftm_1.follow_up_dialog_token, 0);

        let (_, _, ftm_2) = parse_ftm_frame(&frame_2);
        assert_eq!(ftm_2.dialog_token, 0);
        assert_eq!(ftm_2.follow_up_dialog_token, 255);
    }

    const MAX_UNCLAMPED_FLIGHT_PS: u64 =
        (MAX_TIMESTAMP_48BIT - 1 - INITIAL_TOD_PICOSECONDS - SIFS_DURATION_PICOSECONDS) / 2;
    const MIN_OVERFLOW_FLIGHT_PS: u64 = MAX_UNCLAMPED_FLIGHT_PS + 1;
    const UPPER_WRAP_WINDOW_FLIGHT_PS: u64 =
        (u64::MAX - INITIAL_TOD_PICOSECONDS - SIFS_DURATION_PICOSECONDS) / 2;

    fn assert_toa_saturates(flight_ps: u64) {
        let header = sample_ftm_header();
        let [_, frame_2] = build_ftm_response_frames(&header, 1, flight_ps);
        let (_, _, ftm_2) = parse_ftm_frame(&frame_2);
        assert_eq!(ftm_2.toa, [0xFF; 6]);
        assert_eq!(decode_timestamp_48bit(&ftm_2.toa), MAX_TIMESTAMP_48BIT);
    }

    #[test]
    fn test_build_ftm_response_frames_max_unclamped_48bit_timestamp() {
        let header = sample_ftm_header();
        // Highest non-saturating input: flight_ps = MAX_UNCLAMPED_FLIGHT_PS causes
        // t1 + 2 * flight_ps + SIFS = 281_474_976_710_654 (MAX_TIMESTAMP_48BIT - 1).
        let [_, frame_2] = build_ftm_response_frames(&header, 1, MAX_UNCLAMPED_FLIGHT_PS);
        let (_, _, ftm_2) = parse_ftm_frame(&frame_2);
        assert_eq!(ftm_2.toa, [0xFE, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF]);
        assert_eq!(decode_timestamp_48bit(&ftm_2.toa), MAX_TIMESTAMP_48BIT - 1);
    }

    #[test]
    fn test_build_ftm_response_frames_overflow_saturates_to_48bit_max() {
        // Boundary crossing: flight_ps = MIN_OVERFLOW_FLIGHT_PS causes
        // t1 + 2 * flight_ps + SIFS = 281_474_976_710_656 (0x0001_0000_0000_0000).
        // Without 48-bit clamping, this wraps modulo 2^48 to 0 (below tod).
        assert_toa_saturates(MIN_OVERFLOW_FLIGHT_PS);
    }

    #[test]
    fn test_build_ftm_response_frames_upper_wrap_window_saturates_to_48bit_max() {
        // Upper wrap window: flight_ps = UPPER_WRAP_WINDOW_FLIGHT_PS causes
        // t1 + 2 * flight_ps + SIFS = u64::MAX - 1 (0xFFFF_FFFF_FFFF_FFFE).
        // Lower 48 bits equal 0xFFFF_FFFF_FFFE, which without 48-bit clamping
        // would falsely appear as an unclamped timestamp.
        assert_toa_saturates(UPPER_WRAP_WINDOW_FLIGHT_PS);
    }

    #[test]
    fn test_build_ftm_response_frames_max_u64_flight_ps_does_not_panic() {
        assert_toa_saturates(u64::MAX);
    }
}
