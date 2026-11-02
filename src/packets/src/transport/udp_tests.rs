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
fn test_udp_header_parsing() {
    // Sample UDP header: source port 12345, dest port 80, length 28, checksum
    // 0xABCD
    let bytes: [u8; 8] = [0x30, 0x39, 0x00, 0x50, 0x00, 0x1C, 0xAB, 0xCD];
    let (header, rest) = UdpHeader::parse(&bytes).expect("Failed to parse UDP header");

    assert_eq!(header.source_port.get(), 12345);
    assert_eq!(header.dest_port.get(), 80);
    assert_eq!(header.length.get(), 28);
    assert_eq!(header.checksum.get(), 0xABCD);
    assert!(rest.is_empty());
}

#[test]
fn test_udp_header_with_payload() {
    let bytes: [u8; 12] = [0x30, 0x39, 0x00, 0x50, 0x00, 0x1C, 0xAB, 0xCD, 0xDE, 0xAD, 0xBE, 0xEF];
    let (header, rest) = UdpHeader::parse(&bytes).expect("Failed to parse UDP header");

    assert_eq!(header.source_port.get(), 12345);
    assert_eq!(rest.len(), 4);
    assert_eq!(rest, &[0xDE, 0xAD, 0xBE, 0xEF]);
}

#[test]
fn test_udp_header_too_short() {
    let bytes: [u8; 7] = [0x30, 0x39, 0x00, 0x50, 0x00, 0x1C, 0xAB];
    assert!(UdpHeader::parse(&bytes).is_none());
}

#[test]
fn test_udp_header_clips_trailing_padding() {
    let mut bytes = Vec::new();
    // 8-byte UDP header: source port 12345, dest port 80, length 12, checksum 0
    bytes.extend_from_slice(&[0x30, 0x39, 0x00, 0x50, 0x00, 0x0C, 0x00, 0x00]);
    // 4 bytes payload
    bytes.extend_from_slice(&[0xDE, 0xAD, 0xBE, 0xEF]);
    // 14 bytes trailing padding
    bytes.extend_from_slice(&[0x00; 14]);

    let (header, payload) =
        UdpHeader::parse(&bytes).expect("Failed to parse UDP header with padding");

    assert_eq!(header.length.get(), 12);
    assert_eq!(payload, &[0xDE, 0xAD, 0xBE, 0xEF]);
}

#[test]
fn test_udp_header_invalid_length() {
    // 8-byte UDP header + 4 bytes data, but length field is 6 (< 8)
    let bytes: [u8; 12] = [0x30, 0x39, 0x00, 0x50, 0x00, 0x06, 0x00, 0x00, 0x01, 0x02, 0x03, 0x04];
    assert!(UdpHeader::parse(&bytes).is_none());
}

#[test]
fn test_udp_header_zero_length_pass_through() {
    // 8-byte UDP header with length == 0 followed by 4 bytes payload
    let bytes: [u8; 12] = [0x30, 0x39, 0x00, 0x50, 0x00, 0x00, 0x00, 0x00, 0xDE, 0xAD, 0xBE, 0xEF];
    let (header, payload) =
        UdpHeader::parse(&bytes).expect("Failed to parse UDP header with zero length");

    assert_eq!(header.length.get(), 0);
    assert_eq!(payload, &[0xDE, 0xAD, 0xBE, 0xEF]);
}
