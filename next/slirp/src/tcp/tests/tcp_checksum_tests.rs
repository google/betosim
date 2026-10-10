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

use std::net::Ipv4Addr;

use netsim_packets::tcp_checksum;

#[test]
fn test_tcp_checksum() {
    let src_addr = Ipv4Addr::new(100, 79, 15, 252);
    let dst_addr = Ipv4Addr::new(10, 0, 2, 15);
    let tcp_packet = [
        0x14, 0x51, // Source Port: 5201
        0xce, 0xfc, // Destination Port: 52732
        0x00, 0x00, 0x00, 0x01, // Sequence Number: 1
        0x00, 0x00, 0x00, 0x26, // Acknowledgement Number: 38
        0x50, 0x10, // Data Offset (5) + Flags (ACK)
        0x20, 0x00, // Window Size: 8192
        0x00, 0x00, // Checksum: 0 (to be calculated)
        0x00, 0x00, // Urgent Pointer: 0
        0x09, // Payload: one byte
    ];

    let checksum = tcp_checksum(&tcp_packet, src_addr, dst_addr);

    // This is the correct checksum value calculated manually and with online
    // tools.
    assert_eq!(checksum, 8965);
}
