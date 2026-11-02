// Copyright 2023-2025 The Android Open Source Project
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

use netsim_packets::{IP_P_UDP, UdpBuilder};

use crate::ip_fragment::{Reassembler, fragment};

#[test]
fn test_fragmentation() {
    let mut reassembler = Reassembler::new();
    let mut udp_payload = vec![0u8; 2000];
    for (i, item) in udp_payload.iter_mut().enumerate().take(2000) {
        *item = (i % 256) as u8;
    }

    // Build the full UDP packet inside an IP packet
    let mut udp_packet = vec![0u8; 8 + udp_payload.len()];
    let mut udp_builder = UdpBuilder::new(
        &mut udp_packet,
        "10.0.2.2".parse().unwrap(),
        "10.0.2.15".parse().unwrap(),
        1234,
        5678,
    )
    .unwrap();
    udp_builder.payload(&udp_payload).unwrap();
    udp_builder.build().unwrap();

    let mut ipv4_packet = vec![0u8; 20 + udp_packet.len()];
    let (ipv4_header_slice, ipv4_payload_slice) = ipv4_packet.split_at_mut(20);
    let mut ipv4_builder = netsim_packets::Ipv4Builder::new(
        ipv4_header_slice,
        IP_P_UDP,
        "10.0.2.2".parse().unwrap(),
        "10.0.2.15".parse().unwrap(),
    )
    .unwrap();
    ipv4_builder.payload_len(udp_packet.len());
    ipv4_builder.build();
    ipv4_payload_slice.copy_from_slice(&udp_packet);

    // Fragment the packet using the library function
    let fragments = fragment(&ipv4_packet);
    assert_eq!(fragments.len(), 2);

    // Send the fragments to the reassembler.
    let reassembled_packet = reassembler.reassemble(&fragments[0]);
    assert!(reassembled_packet.is_none());
    let reassembled_packet = reassembler.reassemble(&fragments[1]);
    assert!(reassembled_packet.is_some());
}

#[tokio::test]
async fn test_reassembly() {
    let mut reassembler = Reassembler::new();
    // Use a payload large enough to guarantee fragmentation.
    let mut payload = vec![0u8; 2000];
    for (i, item) in payload.iter_mut().enumerate() {
        *item = (i % 256) as u8;
    }

    let mut udp_packet = vec![0u8; 8 + payload.len()];
    let mut udp_builder =
        UdpBuilder::new(&mut udp_packet, [192, 168, 1, 2].into(), [8, 8, 8, 8].into(), 1234, 5678)
            .unwrap();
    udp_builder.payload(&payload).unwrap();
    udp_builder.build().unwrap();

    let mut ipv4_packet = vec![0u8; 20 + udp_packet.len()];
    let (ipv4_header, ipv4_payload) = ipv4_packet.split_at_mut(20);
    let mut ipv4_builder = netsim_packets::Ipv4Builder::new(
        ipv4_header,
        IP_P_UDP,
        [192, 168, 1, 2].into(),
        [8, 8, 8, 8].into(),
    )
    .unwrap();
    ipv4_builder.ttl(64);
    ipv4_builder.payload_len(udp_packet.len());
    ipv4_builder.build();
    ipv4_payload.copy_from_slice(&udp_packet);

    // Use the known-good fragmenter from the library itself.
    let fragments = fragment(&ipv4_packet);
    assert!(fragments.len() > 1);

    // Send all but the last fragment. Expect no response.
    for fragment in fragments.iter().take(fragments.len() - 1) {
        let reassembled_packet = reassembler.reassemble(fragment);
        assert!(reassembled_packet.is_none());
    }

    // Send the last fragment.
    let reassembled_packet = reassembler.reassemble(fragments.last().unwrap());
    assert!(reassembled_packet.is_some());
}
