// Copyright 2026 The Android Open Source Project
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

use netsim_packets::{
    FineTimingMeasurement, FrameControl, MacAddr, MacHeader3Addr, category, public_action,
};
use zerocopy::{IntoBytes, U16};

use crate::ap_actor::ApState;

/// 802.11 Action frame control field (Type = Management, Subtype = Action).
const FRAME_CONTROL_ACTION: u16 = 0x00D0;

/// Arbitrary initial Timestamp of Departure (TOD) baseline in picoseconds (1,000 seconds).
const INITIAL_TOD_PICOSECONDS: u64 = 1_000_000_000_000;

/// Default simulated station distance in meters.
/// TODO: Calculate distance based on src/dest positions if available.
const DEFAULT_DISTANCE_METERS: f64 = 5.0;

/// Nominal speed of light in vacuum (approx. 0.3 meters per nanosecond).
const SPEED_OF_LIGHT_METERS_PER_NS: f64 = 0.3;

/// Picoseconds per nanosecond conversion factor.
const PICOSECONDS_PER_NANOSECOND: u64 = 1_000;

/// Short Interframe Space (SIFS) duration in picoseconds (10 microseconds).
const SIFS_DURATION_PICOSECONDS: u64 = 10_000 * PICOSECONDS_PER_NANOSECOND;

/// Encodes the least-significant 48 bits (6 bytes) of a picosecond timestamp into little-endian bytes.
///
/// Per IEEE 802.11-2016 Section 9.4.2.167 (Fine Timing Measurement parameter element),
/// the TOD and TOA fields are 48-bit unsigned integer picosecond timestamps (modulo 2^48).
/// In little-endian byte order (`to_le_bytes`), the lowest 6 bytes represent the 48 LSBs,
/// so dropping the two most-significant bytes (`_, _`) performs this 48-bit truncation.
fn encode_timestamp_48bit(timestamp_ps: u64) -> [u8; 6] {
    let [timestamp @ .., _, _] = timestamp_ps.to_le_bytes();
    timestamp
}

/// FTM Responder Logic
#[derive(Debug)]
pub(crate) struct FtmResponder {
    // Stateless responder: responds immediately to each request.
}

impl FtmResponder {
    /// Constructs a serialized 802.11 Public Action frame carrying an FTM payload.
    fn build_ftm_action_frame(header: &MacHeader3Addr, body: &FineTimingMeasurement) -> Vec<u8> {
        [
            header.as_bytes(),
            &[category::PUBLIC, public_action::FINE_TIMING_MEASUREMENT],
            body.as_bytes(),
        ]
        .concat()
    }

    /// Handle an incoming FTM Request and generate the necessary response frames.
    ///
    /// Implements a simplified Single-Burst FTM exchange (ASAP=1):
    /// 1. Receive FTM Request (Trigger=1).
    /// 2. Send Initial FTM Frame (Dialog Token=N, Follow Up=0) with t1/t4 placeholders.
    /// 3. Send Follow-Up FTM Frame (Dialog Token=N, Follow Up=N) containing the simulated
    ///    timestamps (t1, t4) calculated based on a fixed distance.
    ///
    /// Note: In a real physical exchange, t4 would be captured upon packet arrival.
    /// Here, we pre-calculate timestamps to simulate a specific distance (RTT).
    pub(crate) fn handle_ftm_request(
        ap: &mut ApState,
        src: MacAddr,
        dialog_token: u8,
    ) -> Vec<Vec<u8>> {
        let header = MacHeader3Addr {
            frame_control: FrameControl::new(FRAME_CONTROL_ACTION),
            duration_id: U16::new(0),
            addr1: src,
            addr2: ap.config.bssid,
            addr3: ap.config.bssid,
            sequence_control: ap.next_seq_control(),
        };

        // 1. Initial FTM Frame (ASAP=1)
        // Indicates measurement start. Contains dummy timestamps (0).
        let ftm_body_1 = FineTimingMeasurement {
            dialog_token,
            follow_up_dialog_token: 0,
            tod: [0; 6],
            toa: [0; 6],
            tod_error: [0; 6], // Simplified padding
        };
        let frame_1 = Self::build_ftm_action_frame(&header, &ftm_body_1);

        // 2. Second FTM Frame (Follow Up) containing the "measurement" from Frame 1.
        // We simulate the RTT by calculating t4 based on a fixed flight time (distance)
        // plus a fixed SIFS processing time.
        //
        // RTT = (t4 - t1) - (t3 - t2)
        // We report t1 and t4. The client (Station) will measure t2 and t3 locally.
        let flight_ns = (DEFAULT_DISTANCE_METERS / SPEED_OF_LIGHT_METERS_PER_NS) as u64;
        let flight_ps = flight_ns * PICOSECONDS_PER_NANOSECOND;

        // t4 = t1 + Flight + SIFS + Flight
        // Note: This presumes the client turnaround is exactly SIFS.
        let t1_val = INITIAL_TOD_PICOSECONDS;
        let t4_val = t1_val + (2 * flight_ps) + SIFS_DURATION_PICOSECONDS;

        let ftm_body_2 = FineTimingMeasurement {
            dialog_token,
            follow_up_dialog_token: dialog_token,
            tod: encode_timestamp_48bit(t1_val),
            toa: encode_timestamp_48bit(t4_val),
            tod_error: [0; 6],
        };
        let frame_2 = Self::build_ftm_action_frame(&header, &ftm_body_2);

        vec![frame_1, frame_2]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
