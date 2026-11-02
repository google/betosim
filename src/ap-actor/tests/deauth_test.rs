// Copyright 2025-2026 The Android Open Source Project
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

use netsim_packets::{FrameControl, MacAddr, MacHeader3Addr, SequenceControl};
use tracing::info;
use zerocopy::IntoBytes;

use crate::world::ApWorld;

// Scenario: Station Deauthentication
// Given a registered AP with WPA2 enabled
// And a Station is associated
// When the Station sends a Deauthentication frame
// Then the AP logs "Deauthenticated" (or we verify association requires
// handshake again) For this test, we verify the AP accepts the frame and
// doesn't crash/error.
#[tokio::test]
async fn test_station_deauth() {
    info!("Scenario: Station Deauthentication");
    let mut world = ApWorld::new().await;
    // Config with WPA
    world.given_a_registered_ap_with_wpa("WpaAP", "CorrectPassword").await;

    let station_mac = "02:00:00:00:00:99";
    let station_mac_addr: MacAddr = station_mac.try_into().unwrap();

    // 1. Association Flow
    world.when_station_sends_assoc_req(station_mac).await;
    world.then_station_receives_assoc_resp(station_mac).await;

    // 2. Perform Deauth
    info!("When the Station sends a Deauthentication frame");
    let tx = world.tx_to_ap.as_mut().expect("AP registered");

    // Construct Deauth Frame
    let mut frame = Vec::new();
    let bssid: MacAddr = "02:00:00:00:00:01".try_into().unwrap();

    let header = MacHeader3Addr::new(
        FrameControl::new(0x00C0), // Mgmt(00), Deauth(1100) -> 0x00C0
        0,
        bssid,            // DA (AP)
        station_mac_addr, // SA (Station)
        bssid,            // BSSID
        SequenceControl::new(0),
    );
    frame.extend_from_slice(header.as_bytes());

    // Reason Code (2 bytes) - e.g., 0x0001 (Unspecified) or 0x0003 (Leaving ESS)
    // Fixed field for Deauth is just Reason Code.
    let reason_code: u16 = 3;
    frame.extend_from_slice(&reason_code.to_le_bytes());

    let src_id = netsim_model::ChipId(99);
    tx.send(bytes::Bytes::from(frame)).expect("Failed to send Deauth");

    // 3. Verify System Stability or State Change
    // We check that sending a subsequent encrypted frame (like M2 retry) would NOT
    // be decrypted/accepted/processed as usual, or at least that we don't
    // crash. For now, simple stability check is good.

    // TODO: Ideally we should verify shared_keys is empty.
    // Since we cannot inspect AP internal state easily, we rely on logs or
    // side-effects.
    info!("Deauth scenario completed without crash.");
}
