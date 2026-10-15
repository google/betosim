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

//! Tests for packet checksum utility functions.

use std::net::{Ipv4Addr, Ipv6Addr};

use netsim_packets::{
    icmpv6_checksum, ipv4_checksum, tcp_checksum, tcp_checksum_v6, udp_checksum, udp_checksum_v6,
};

#[test]
fn test_ipv4_checksum() {
    let header = [
        0x45, 0x00, 0x00, 0x3c, 0x1c, 0x46, 0x40, 0x00, 0x40, 0x06, 0x00, 0x00, 0xac, 0x10, 0x0a,
        0x63, 0xac, 0x10, 0x0a, 0x0c,
    ];
    let cksum = ipv4_checksum(&header);
    assert_eq!(cksum, 0xb1e6);

    let mut header_with_cksum = header;
    header_with_cksum[10..12].copy_from_slice(&cksum.to_be_bytes());
    assert_eq!(ipv4_checksum(&header_with_cksum), 0);
}

#[test]
fn test_ipv4_checksum_odd_length() {
    let data = [0x01, 0x02, 0x03];
    let cksum = ipv4_checksum(&data);
    assert_ne!(cksum, 0);
}

#[test]
fn test_tcp_checksum_v4() {
    let src = Ipv4Addr::new(192, 168, 1, 1);
    let dst = Ipv4Addr::new(192, 168, 1, 2);
    let tcp_packet = [
        0x30, 0x39, 0x00, 0x50, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x50, 0x02, 0x72,
        0x10, 0x00, 0x00, 0x00, 0x00,
    ];
    let cksum = tcp_checksum(&tcp_packet, src, dst);
    assert_ne!(cksum, 0);

    let mut tcp_with_cksum = tcp_packet;
    tcp_with_cksum[16..18].copy_from_slice(&cksum.to_be_bytes());
    assert_eq!(tcp_checksum(&tcp_with_cksum, src, dst), 0);
}

#[test]
fn test_udp_checksum_v4() {
    let src = Ipv4Addr::new(10, 0, 0, 1);
    let dst = Ipv4Addr::new(10, 0, 0, 2);
    let mut udp_packet =
        vec![0x04, 0xd2, 0x04, 0xd3, 0x00, 0x0d, 0x00, 0x00, b'h', b'e', b'l', b'l', b'o'];
    let cksum = udp_checksum(&udp_packet, src, dst);
    assert_ne!(cksum, 0);

    udp_packet[6..8].copy_from_slice(&cksum.to_be_bytes());
    // For UDP, a valid packet with checksum verified returns u16::MAX (0 is normalized to u16::MAX)
    assert_eq!(udp_checksum(&udp_packet, src, dst), u16::MAX);
}

#[test]
fn test_udp_checksum_zero_becomes_ffff() {
    // With unspecified src/dst addresses, protocol 17 (0x0011) + length 8 (0x0008) = 0x0019.
    // Setting src_port = 0xFFDE and length = 0x0008 gives a total 16-bit sum of 0xFFFF,
    // whose one's complement is 0x0000 and must be normalized to u16::MAX per RFC 768.
    let udp_packet = [0xff, 0xde, 0x00, 0x00, 0x00, 0x08, 0x00, 0x00];
    assert_eq!(udp_checksum(&udp_packet, Ipv4Addr::UNSPECIFIED, Ipv4Addr::UNSPECIFIED), u16::MAX);
    assert_eq!(
        udp_checksum_v6(&udp_packet, Ipv6Addr::UNSPECIFIED, Ipv6Addr::UNSPECIFIED),
        u16::MAX
    );
}

#[test]
fn test_tcp_checksum_v6() {
    let src = "2001:db8::1".parse::<Ipv6Addr>().unwrap();
    let dst = "2001:db8::2".parse::<Ipv6Addr>().unwrap();
    let tcp_packet = [
        0x30, 0x39, 0x00, 0x50, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x50, 0x02, 0x72,
        0x10, 0x00, 0x00, 0x00, 0x00,
    ];
    let cksum = tcp_checksum_v6(&tcp_packet, src, dst);
    assert_ne!(cksum, 0);

    let mut tcp_with_cksum = tcp_packet;
    tcp_with_cksum[16..18].copy_from_slice(&cksum.to_be_bytes());
    assert_eq!(tcp_checksum_v6(&tcp_with_cksum, src, dst), 0);
}

#[test]
fn test_udp_checksum_v6() {
    let src = "fe80::1".parse::<Ipv6Addr>().unwrap();
    let dst = "fe80::2".parse::<Ipv6Addr>().unwrap();
    let mut udp_packet =
        vec![0x04, 0xd2, 0x04, 0xd3, 0x00, 0x0d, 0x00, 0x00, b'w', b'o', b'r', b'l', b'd'];
    let cksum = udp_checksum_v6(&udp_packet, src, dst);
    assert_ne!(cksum, 0);

    udp_packet[6..8].copy_from_slice(&cksum.to_be_bytes());
    // For UDP, a valid packet with checksum verified returns u16::MAX (0 is normalized to u16::MAX)
    assert_eq!(udp_checksum_v6(&udp_packet, src, dst), u16::MAX);
}

#[test]
fn test_icmpv6_checksum() {
    let src = "fe80::1".parse::<Ipv6Addr>().unwrap();
    let dst = "ff02::1".parse::<Ipv6Addr>().unwrap();
    let mut icmpv6_packet = vec![128, 0, 0, 0, 0, 1, 0, 1, b'p', b'i', b'n', b'g'];
    let cksum = icmpv6_checksum(&icmpv6_packet, src, dst);
    assert_ne!(cksum, 0);

    icmpv6_packet[2..4].copy_from_slice(&cksum.to_be_bytes());
    assert_eq!(icmpv6_checksum(&icmpv6_packet, src, dst), 0);
}
