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

use bytes::Bytes;
use netsim_model::Position;
use netsim_packets::{Ieee80211, category, public_action};
use tracing::{debug, warn};
use zerocopy::IntoBytes;

const SPEED_OF_LIGHT: f64 = 299_792_458.0; // m/s
const PICOSECONDS_PER_SECOND: u64 = 1_000_000_000_000;

/// Handles an FTM Request packet and generates responses.
///
/// If the packet is a valid FTM Request, this function calculates the
/// distance-based timestamps (Time of Flight) and returns a list of response
/// frames (FTM Initial + FTM Measurement).
pub fn handle_ftm_request(
    packet: &Ieee80211,
    initiator_pos: &Position,
    responder_pos: &Position,
) -> Option<Vec<Bytes>> {
    // 1. Verify Packet is Public Action -> FTM Request
    let (category_val, action_val, trigger) = parse_ftm_request(packet)?;

    if category_val != category::PUBLIC || action_val != public_action::FTM_REQUEST {
        return None;
    }

    debug!("FTM Request received. Trigger: {}", trigger);

    // 2. Calculate Distance & RTT
    let distance = calculate_distance(initiator_pos, responder_pos);
    if distance <= 0.0 {
        warn!(
            code = crate::err::Code::FtmInvalidDistance as u32,
            "FTM distance is zero or invalid, using minimal RTT"
        );
    }

    // Time of Flight (seconds) = distance / c
    let tof_seconds = distance / SPEED_OF_LIGHT;
    // RTT = 2 * ToF
    let rtt_picoseconds = (tof_seconds * 2.0 * PICOSECONDS_PER_SECOND as f64) as u64;

    // Simulate Timestamps
    // t1: Time of Departure (Responder) - arbitrary base (0)
    // t4: Time of Arrival (Responder) - t1 + RTT
    // Timestamps are 48-bit (6 bytes).

    let t1_bytes = 0u64.to_le_bytes(); // Start at 0
    let t4_bytes = rtt_picoseconds.to_le_bytes(); // RTT later

    // 3. Construct Responses
    let mut responses = Vec::new();

    // Source/Dest are swapped for response
    // Initator (Source of Request) -> Destination of Response
    // Responder (Dest of Request) -> Source of Response
    let dest = packet.get_structure_addr2(); // SA of request
    let src = packet.get_structure_addr1(); // DA of request (us)
    let bssid = packet.get_structure_addr3();

    // Helper to build Action Frame
    let build_action = |body: &[u8]| -> Bytes {
        let mut frame = Vec::new();
        // 802.11 Header (Simplified Management Action)
        let fc = netsim_packets::FrameControl::new(0x00D0);
        let header = netsim_packets::MacHeader3Addr {
            frame_control: fc,
            duration_id: zerocopy::U16::new(0),
            addr1: dest,
            addr2: src,
            addr3: bssid,
            sequence_control: netsim_packets::SequenceControl::new(0),
        };
        frame.extend_from_slice(header.as_bytes());
        frame.extend_from_slice(body);
        Bytes::from(frame)
    };

    // Frame 1: FTM Action (Initial)
    let mut body1 = vec![category::PUBLIC, public_action::FINE_TIMING_MEASUREMENT, 0, 0];
    // Zero timestamps
    body1.extend_from_slice(&[0u8; 6]); // TOD
    body1.extend_from_slice(&[0u8; 6]); // TOA
    body1.extend_from_slice(&[0u8; 6]); // TOD Error / etc
    // body1.extend_from_slice(&[0u8; 6]); // TOA Error

    responses.push(build_action(&body1));

    // Frame 2: FTM Action (With Timestamps)
    let mut body2 = vec![category::PUBLIC, public_action::FINE_TIMING_MEASUREMENT, 0, 0];
    // Timestamps (48-bit usually)
    let [t1 @ .., _, _] = &t1_bytes;
    let [t4 @ .., _, _] = &t4_bytes;
    body2.extend_from_slice(t1); // TOD
    body2.extend_from_slice(t4); // TOA (using t4 = RTT for simplicity, implying t1=0)
    body2.extend_from_slice(&[0u8; 6]); // Error

    responses.push(build_action(&body2));

    Some(responses)
}

fn parse_ftm_request(packet: &Ieee80211) -> Option<(u8, u8, u8)> {
    // Body starts at offset 24 for 3-address Management Action frames.
    if !packet.is_mgmt() || packet.stype() != netsim_packets::management_subtype::ACTION {
        return None;
    }

    let bytes = packet.as_bytes();
    let category = *bytes.get(24)?;
    let action = *bytes.get(25)?;
    let trigger = *bytes.get(26)?;

    Some((category, action, trigger))
}

fn calculate_distance(p1: &Position, p2: &Position) -> f64 {
    let dx = p1.x - p2.x;
    let dy = p1.y - p2.y;
    let dz = p1.z - p2.z;
    (dx * dx + dy * dy + dz * dz).sqrt() as f64
}

// Helpers for extracting addresses directly if inherent helpers are restrictive
trait AddrHelpers {
    fn get_structure_addr1(&self) -> netsim_packets::MacAddress;
    fn get_structure_addr2(&self) -> netsim_packets::MacAddress;
    fn get_structure_addr3(&self) -> netsim_packets::MacAddress;
}

impl AddrHelpers for Ieee80211 {
    fn get_structure_addr1(&self) -> netsim_packets::MacAddress {
        self.get_addr1()
    }
    fn get_structure_addr2(&self) -> netsim_packets::MacAddress {
        self.get_addr2()
    }
    fn get_structure_addr3(&self) -> netsim_packets::MacAddress {
        self.get_addr3()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use common::util::netsim_logger::{ErrorStatsLayer, ErrorStatsTable};
    use netsim_packets::{FrameControl, MacAddress, MacHeader3Addr, SequenceControl};
    use tracing_subscriber::layer::SubscriberExt;

    use super::*;
    use crate::err::Code;

    const ACTION_FRAME_CONTROL: u16 = 0x00D0;
    const FTM_TRIGGER_START: u8 = 1;
    const INITIATOR_MAC: [u8; 6] = [0x02, 0x00, 0x00, 0x00, 0x00, 0x01];
    const RESPONDER_MAC: [u8; 6] = [0x02, 0x00, 0x00, 0x00, 0x00, 0x02];

    fn build_test_ftm_request() -> Ieee80211 {
        let header = MacHeader3Addr {
            frame_control: FrameControl::new(ACTION_FRAME_CONTROL),
            duration_id: zerocopy::U16::new(0),
            addr1: MacAddress::from(RESPONDER_MAC),
            addr2: MacAddress::from(INITIATOR_MAC),
            addr3: MacAddress::from(RESPONDER_MAC),
            sequence_control: SequenceControl::new(0),
        };
        let mut frame_bytes = Vec::new();
        frame_bytes.extend_from_slice(header.as_bytes());
        frame_bytes.extend_from_slice(&[
            category::PUBLIC,
            public_action::FTM_REQUEST,
            FTM_TRIGGER_START,
        ]);
        Ieee80211::decode(&frame_bytes).expect("valid FTM request frame")
    }

    #[test]
    fn handle_ftm_request_zero_distance_records_invalid_distance_error() {
        let table = Arc::new(ErrorStatsTable::new());
        let subscriber =
            tracing_subscriber::registry().with(ErrorStatsLayer::with_table(Arc::clone(&table)));
        let packet = build_test_ftm_request();
        let origin = Position::default();

        tracing::subscriber::with_default(subscriber, || {
            let responses =
                handle_ftm_request(&packet, &origin, &origin).expect("co-located FTM succeeds");
            assert_eq!(responses.len(), 2);
        });

        assert_eq!(table.snapshot(), vec![(Code::FtmInvalidDistance as u32, 1)]);
    }

    #[test]
    fn handle_ftm_request_positive_distance_records_no_error_stats() {
        let table = Arc::new(ErrorStatsTable::new());
        let subscriber =
            tracing_subscriber::registry().with(ErrorStatsLayer::with_table(Arc::clone(&table)));
        let packet = build_test_ftm_request();
        let origin = Position::default();
        let peer = Position { x: 3.0, y: 4.0, z: 0.0 };

        tracing::subscriber::with_default(subscriber, || {
            let responses =
                handle_ftm_request(&packet, &origin, &peer).expect("positive-distance FTM");
            assert_eq!(responses.len(), 2);
        });

        assert!(table.snapshot().is_empty());
    }
}
