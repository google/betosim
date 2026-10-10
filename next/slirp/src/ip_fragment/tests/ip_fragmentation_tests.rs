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

use netsim_packets::Ipv4Header;

use crate::ip_fragment::fragment;

#[test]
fn test_fragmentation() {
    let mut payload = vec![0u8; 2000];
    for (i, item) in payload.iter_mut().enumerate().take(2000) {
        *item = (i % 256) as u8;
    }

    let mut udp_packet = vec![0u8; 8 + payload.len()];
    let mut udp_builder = netsim_packets::UdpBuilder::new(
        &mut udp_packet,
        "10.0.2.2".parse().unwrap(),
        "10.0.2.15".parse().unwrap(),
        8080,
        80,
    )
    .unwrap();
    udp_builder.payload(&payload).unwrap();
    udp_builder.build().unwrap();

    let mut ip_packet = vec![0u8; 20 + udp_packet.len()];
    let (ip_header_slice, ip_payload_slice) = ip_packet.split_at_mut(20);
    let mut ipv4_builder = netsim_packets::Ipv4Builder::new(
        ip_header_slice,
        netsim_packets::IP_P_UDP,
        "10.0.2.2".parse().unwrap(),
        "10.0.2.15".parse().unwrap(),
    )
    .unwrap();
    ipv4_builder.payload_len(udp_packet.len());
    ipv4_builder.build();
    ip_payload_slice.copy_from_slice(&udp_packet);

    // Send a large packet that should be fragmented.
    let fragments = fragment(&ip_packet);

    assert_eq!(fragments.len(), 2);

    let packet1 = &fragments[0];
    let packet2 = &fragments[1];

    // Verify the first fragment.
    let (header1, _payload1) = Ipv4Header::parse(packet1).unwrap();
    assert_ne!(header1.flags_fragment_offset.get() & 0x2000, 0); // More fragments flag
    assert_eq!(header1.flags_fragment_offset.get() & 0x1FFF, 0); // Offset is 0

    // Verify the second fragment.
    let (header2, _payload2) = Ipv4Header::parse(packet2).unwrap();
    assert_eq!(header2.flags_fragment_offset.get() & 0x2000, 0); // More fragments flag is not set
    assert_ne!(header2.flags_fragment_offset.get() & 0x1FFF, 0); // Offset is not 0
}
