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
use netsim_packets::{FrameDirection, HwsimFrame, HwsimMsg, Ieee80211, MacAddress};

/// Wraps an Ethernet II frame (as bytes) into a HwsimMsg suitable for injection
/// into the Wifi actor.
pub fn wrap_ethernet_in_hwsim(
    ethernet_frame: &[u8],
    hostapd_bssid: &[u8; 6],
    dest_hwsim_addr: &[u8; 6],
    src_hwsim_addr: &[u8; 6],
    freq: u32,
) -> Result<Vec<u8>, String> {
    // 1. Convert Ethernet to 802.11
    let bssid = MacAddress::new(*hostapd_bssid);
    let ieee80211 = Ieee80211::from_ieee8023(
        &Bytes::copy_from_slice(ethernet_frame),
        bssid,
        FrameDirection::ToAp,
        100,
    )
    .map_err(|e| format!("Failed to convert Ethernet to 802.11: {}", e))?;
    // 2. Build HwsimMsg using shared utility
    let src_addr = MacAddress::new(*src_hwsim_addr);
    let dest_addr = MacAddress::new(*dest_hwsim_addr);

    let msg =
        wifi_actor::create_hwsim_msg_from_frame(&ieee80211, &dest_addr, freq, Some(&src_addr))
            .map_err(|e| format!("Failed to create HwsimMsg: {}", e))?;

    // 4. Encode to bytes
    msg.encode_to_vec().map_err(|e| format!("Failed to encode HwsimMsg: {}", e))
}

/// Unwraps a HwsimMsg (as bytes or struct) and extracts the Ethernet frame if
/// present. Note: This assumes the payload is LLC SNAP encoded Ethernet II.
pub fn unwrap_hwsim_to_ethernet(packet: &[u8]) -> Result<Vec<u8>, String> {
    // 1. Parse HwsimMsg
    let hwsim_msg =
        HwsimMsg::decode_full(packet).map_err(|e| format!("Failed to decode HwsimMsg: {}", e))?;

    // 2. Parse HwsimFrame (handles attributes and 802.11 decoding)
    let frame = HwsimFrame::parse(&hwsim_msg)
        .map_err(|e| format!("Failed to parse HwsimFrame: {:?}", e))?;
    let ieee80211 = frame.ieee80211;

    if !ieee80211.is_data() {
        return Err("Not a data frame".to_string());
    }

    // 3. Extract Payload and reconstruct Ethernet
    let payload = ieee80211.get_payload();

    // Check for LLC SNAP (AA AA 03 00 00 00)
    if payload.len() < 8 || payload[0..6] != [0xAA, 0xAA, 0x03, 0x00, 0x00, 0x00] {
        return Err("Payload is not LLC SNAP Ethernet II".to_string());
    }

    let ethertype = [payload[6], payload[7]];
    let actual_payload = &payload[8..];

    let dst = ieee80211.get_destination();

    // For FromDS traffic (AP -> STA), Source might be stored in different addresses
    // depending on mapping But usually Ieee80211::get_source() handles the
    // ToDS/FromDS logic to return SA.
    let src = ieee80211.get_source();

    let mut eth_frame = Vec::new();
    eth_frame.extend_from_slice(&dst.bytes);
    eth_frame.extend_from_slice(&src.bytes);
    eth_frame.extend_from_slice(&ethertype);
    eth_frame.extend_from_slice(actual_payload);

    Ok(eth_frame)
}

/// Decodes a HwsimMsg and parses it to extract the Ieee80211 frame.
pub fn unwrap_hwsim_to_ieee80211(packet: &[u8]) -> Option<Ieee80211> {
    let hwsim_msg = HwsimMsg::decode_full(packet).ok()?;
    let frame = HwsimFrame::parse(&hwsim_msg).ok()?;
    Some(frame.ieee80211)
}
