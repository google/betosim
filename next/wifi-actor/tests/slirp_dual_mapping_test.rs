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

use std::sync::Arc;

use ap_actor::SharedKeyStore;
use netsim_model::ChipId;
use netsim_packets::MacAddress;
use wifi_actor::{DebugArgs, GatewayTrait, Medium, SlirpGateway, SystemClock, WifiStats};

#[tokio::test]
async fn test_slirp_does_not_drop_with_dual_mapping() {
    // 1. Setup dependencies
    let shared_keys = Arc::new(SharedKeyStore::new());
    let clock = Arc::new(SystemClock);
    let wifi_stats = WifiStats::new(clock);
    let debug_args = Arc::new(DebugArgs::default());
    let mut medium = Medium::new(shared_keys.clone(), wifi_stats, debug_args);

    // Add a BSSID to the store to simulate an active AP
    let ap_bssid = MacAddress::new([0x02, 0x00, 0x00, 0x00, 0x00, 0x00]);
    shared_keys.add_bssid(ap_bssid);

    // Add a station to medium to receive the flooded packet
    let sta_mac = MacAddress::new([0x00, 0x00, 0x00, 0x00, 0x00, 0x02]);
    let station = wifi_actor::Station::new(1, sta_mac, sta_mac);
    medium.stations.insert(sta_mac, station);
    medium.add(1);
    medium.set_enabled(1, true);

    let gateway = SlirpGateway::new(None);

    let src_mac = [0x00, 0x00, 0x00, 0x00, 0x00, 0x01];
    let dest_mac = [0x00, 0x00, 0x00, 0x00, 0x00, 0x02]; // Randomized MAC
    let hw_mac = [0x00, 0x00, 0x00, 0x00, 0x00, 0x03]; // Hardware MAC

    // Slirp response is addressed to the hardware MAC
    let eth_frame = create_ethernet_frame(&src_mac, &hw_mac, b"Hello");
    let packet = bytes::Bytes::from(eth_frame);

    // Simulate an outgoing packet from the station to set up the dual mapping
    let outgoing_eth = create_ethernet_frame(&dest_mac, &src_mac, b"Outgoing");
    let hwsim_packet = super::hwsim_helper::wrap_ethernet_in_hwsim(
        &outgoing_eth,
        &ap_bssid.bytes,
        &ap_bssid.bytes, // dest_hwsim_addr (receiver is AP)
        &hw_mac,         // src_hwsim_addr (transmitter is Station's hardware MAC)
        2412,            // freq
    )
    .expect("Failed to wrap ethernet in hwsim");

    let _ = medium
        .resolve_tx_packet(1, &bytes::Bytes::from(hwsim_packet))
        .expect("Failed to resolve tx packet");

    let mut out_queue = Vec::new();

    // Simulate attempt
    println!("Sending packet from Slirp...");
    gateway.handle_incoming(ChipId(0), packet.clone(), &mut medium, &shared_keys, &mut out_queue);

    assert!(!out_queue.is_empty(), "Packet dropped by SlirpGateway!");
    println!("Packet successfully forwarded by SlirpGateway.");
}

fn create_ethernet_frame(src: &[u8; 6], dst: &[u8; 6], payload: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(dst);
    bytes.extend_from_slice(src);
    bytes.extend_from_slice(&[0x08, 0x00]); // IPv4
    bytes.extend_from_slice(payload);
    bytes
}
