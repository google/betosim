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

use netsim_model::Position;
use netsim_packets::{
    DataSubType, FrameControl, FrameType, Ieee80211, MacAddress, MacHeader3Addr, SequenceControl,
    category, public_action,
};
use wifi_actor::handle_ftm_request;
use zerocopy::IntoBytes;

use crate::world::World;

#[test]
fn test_handle_ftm_request() {
    // Given: An initiator at (0,0,0) and a responder at (300,0,0).
    // The distance is exactly 300m, which implies a specific Round Trip Time (RTT).
    // c = 299,792,458 m/s.
    // 300m / c ~= 1.000692us.
    // RTT = 2 * ToF ~= 2.001384us ~= 2,001,384 ps.
    let initiator_pos = Position { x: 0.0, y: 0.0, z: 0.0 };
    let responder_pos = Position { x: 300.0, y: 0.0, z: 0.0 };

    // And: A valid FTM Request frame (Public Action, Category=4, Action=32).
    let sa = MacAddress::from([0x02, 0x00, 0x00, 0x00, 0x00, 0x01]);
    let da = MacAddress::from([0x02, 0x00, 0x00, 0x00, 0x00, 0x02]);
    let bssid = MacAddress::from([0x02, 0x00, 0x00, 0x00, 0x00, 0x02]);

    let body = vec![category::PUBLIC, public_action::FTM_REQUEST, 1];

    let fc = FrameControl::new(0x00D0);
    let header = MacHeader3Addr {
        frame_control: fc,
        duration_id: zerocopy::U16::new(0),
        addr1: da,
        addr2: sa,
        addr3: bssid,
        sequence_control: SequenceControl::new(0),
    };

    let mut frame_bytes = Vec::new();
    frame_bytes.extend_from_slice(header.as_bytes());
    frame_bytes.extend_from_slice(&body);
    let packet = Ieee80211::decode(&frame_bytes).expect("Failed to decode Ieee80211");

    // When: handle_ftm_request is called.
    let responses = handle_ftm_request(&packet, &initiator_pos, &responder_pos)
        .expect("Should return responses");

    // Then: Two response frames are generated (Initial Ack/Action + Measurement).
    assert_eq!(responses.len(), 2);

    // And: The second frame contains the Fine Timing Measurement action.
    let meas_frame_bytes = &responses[1];
    let _meas_packet = Ieee80211::decode(meas_frame_bytes).expect("Failed to decode response");
    let payload = &meas_frame_bytes[24..];

    assert_eq!(payload[0], category::PUBLIC);
    assert_eq!(payload[1], public_action::FINE_TIMING_MEASUREMENT);

    // And: The timestamps in the measurement frame reflect the distance-based RTT.
    // payload[4..10] = TOD (t1) -> 0
    // payload[10..16] = TOA (t4) -> RTT
    let t1_bytes = &payload[4..10];
    let t4_bytes = &payload[10..16];

    let mut t1 = 0u64;
    for (i, &b) in t1_bytes.iter().enumerate() {
        t1 |= (b as u64) << (8 * i);
    }

    let mut t4 = 0u64;
    for (i, &b) in t4_bytes.iter().enumerate() {
        t4 |= (b as u64) << (8 * i);
    }

    let rtt = t4 - t1;
    let expected_rtt = 2001384;
    let diff = (rtt as i64 - expected_rtt).abs();

    assert!(diff < 100, "RTT {} too far from expected {}", rtt, expected_rtt);
}

const DISTANCE_M: f32 = 300.0;
const EXPECTED_300M_RTT_PS: u64 = 2_001_384;
const RTT_TOLERANCE_PS: u64 = 100;
const FTM_TRIGGER_START: u8 = 1;
const FC_TYPE_SHIFT: u16 = 2;
const FC_SUBTYPE_SHIFT: u16 = 4;

/// Scenario: handle_ftm_request ignores non-Management frames (e.g. Data frames)
/// even when their subtype matches Action and payload begins with FTM Request bytes.
#[test]
fn test_handle_ftm_request_ignores_non_mgmt_frame_with_action_subtype() {
    // Given: Initiator at (0, 0, 0) and Responder at (300, 0, 0)
    let initiator_pos = Position { x: 0.0, y: 0.0, z: 0.0 };
    let responder_pos = Position { x: DISTANCE_M, y: 0.0, z: 0.0 };

    // And: A station-to-station Data packet with Action subtype carrying FTM Request bytes
    let sa = MacAddress::from([0x02, 0x00, 0x00, 0x00, 0x00, 0x01]);
    let da = MacAddress::from([0x02, 0x00, 0x00, 0x00, 0x00, 0x02]);
    let bssid = da;

    let body = vec![category::PUBLIC, public_action::FTM_REQUEST, FTM_TRIGGER_START];

    let fc_val = ((FrameType::Data as u16) << FC_TYPE_SHIFT)
        | ((u8::from(DataSubType::QosCfAck) as u16) << FC_SUBTYPE_SHIFT);
    let fc = FrameControl::new(fc_val);
    let header = MacHeader3Addr::new(fc, 0, da, sa, bssid, SequenceControl::new(0));

    let mut frame_bytes = Vec::new();
    frame_bytes.extend_from_slice(header.as_bytes());
    frame_bytes.extend_from_slice(&body);
    let packet = Ieee80211::decode(&frame_bytes).expect("Failed to decode Ieee80211");

    // When: handle_ftm_request is called on the non-Management frame
    let responses = handle_ftm_request(&packet, &initiator_pos, &responder_pos);

    // Then: handle_ftm_request rejects the non-Management frame and returns None
    assert!(
        responses.is_none(),
        "handle_ftm_request should return None for non-management frame with Action subtype"
    );
}

/// Scenario: WifiActor intercepts Management Action FTM Request between two chips
/// and generates simulated FTM measurement responses without forwarding to responder.
#[tokio::test]
async fn test_wifi_actor_ftm_request_interception() {
    let mut world = World::new().await;
    let _chip0 = world.given_a_chip(1).await;
    let _chip1 = world.given_a_chip(2).await;

    // Given: Initiator at (0, 0, 0) and Responder at (300, 0, 0)
    world.given_chip_position(0, Position { x: 0.0, y: 0.0, z: 0.0 }).await;
    world.given_chip_position(1, Position { x: DISTANCE_M, y: 0.0, z: 0.0 }).await;

    // When: Initiator transmits an FTM Request to Responder
    world.when_chip_transmits_ftm_request(0, 1).await;

    // Then: Initiator receives simulated FTM measurement responses with ~2,001,384 ps RTT
    world
        .then_chip_receives_ftm_measurement_rtt_ps(0, EXPECTED_300M_RTT_PS, RTT_TOLERANCE_PS)
        .await;

    // And: Responder receives nothing (interception/offload suppressed forwarding)
    world.then_chip_receives_nothing(1).await;
}
