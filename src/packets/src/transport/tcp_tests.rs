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

use super::*;

#[test]
fn test_tcp_header_parsing_syn() {
    // TCP SYN packet, header length 20 bytes (5 * 4)
    let bytes: [u8; 20] = [
        0xC0, 0x14, // Source Port: 49172
        0x00, 0x50, // Dest Port: 80
        0x12, 0x34, 0x56, 0x78, // Sequence Number
        0x00, 0x00, 0x00, 0x00, // Ack Number
        0x50, 0x02, // Data Offset (5), Flags (SYN)
        0x72, 0x10, // Window Size
        0xAB, 0xCD, // Checksum
        0x00, 0x00, // Urgent Pointer
    ];
    let (header, rest) = TcpHeader::parse(&bytes).expect("Failed to parse TCP header");

    assert_eq!(header.source_port.get(), 49172);
    assert_eq!(header.dest_port.get(), 80);
    assert_eq!(header.sequence_num.get(), 0x12345678);
    assert_eq!(header.data_offset(), 5);
    assert_eq!(header.header_length(), 20);
    assert!(header.syn());
    assert!(!header.ack());
    assert!(!header.fin());
    assert_eq!(rest.len(), 0);
}

#[test]
fn test_tcp_header_with_options_and_payload() {
    // TCP ACK packet, header length 24 bytes, payload 4 bytes
    let bytes: [u8; 28] = [
        0x00, 0x50, 0xC0, 0x14, // Ports
        0x12, 0x34, 0x56, 0x79, // Seq Num
        0x9A, 0xBC, 0xDE, 0xF0, // Ack Num
        0x60, 0x10, // Data Offset (6), Flags (ACK)
        0x72, 0x10, // Window
        0xAB, 0xCD, // Checksum
        0x00, 0x00, // Urgent Ptr
        0x01, 0x02, 0x03, 0x04, // Options
        0xDE, 0xAD, 0xBE, 0xEF, // Payload
    ];
    let (header, rest) = TcpHeader::parse(&bytes).expect("Failed to parse TCP header");

    assert_eq!(header.data_offset(), 6);
    assert_eq!(header.header_length(), 24);
    assert!(!header.syn());
    assert!(header.ack());
    assert_eq!(rest.len(), 4);
    assert_eq!(rest, &[0xDE, 0xAD, 0xBE, 0xEF]);
}

#[test]
fn test_tcp_header_too_short() {
    // Header is 19 bytes, which is less than the minimum 20
    let bytes: [u8; 19] = [0; 19];
    assert!(TcpHeader::parse(&bytes).is_none());

    // Header claims to be 24 bytes, but buffer is only 22
    let bytes_long_header: [u8; 22] = [
        0x00, 0x50, 0xC0, 0x14, 0x12, 0x34, 0x56, 0x79, 0x9A, 0xBC, 0xDE, 0xF0, 0x60, 0x10, 0x72,
        0x10, 0xAB, 0xCD, 0x00, 0x00, 0x01, 0x02,
    ];
    assert!(TcpHeader::parse(&bytes_long_header).is_none());
}
