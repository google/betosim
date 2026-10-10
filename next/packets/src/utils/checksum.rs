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

//! Checksum utility functions for network packets.

use std::net::{Ipv4Addr, Ipv6Addr};

/// Calculates the IPv4 checksum.
///
/// The checksum is calculated by summing the 16-bit words of the data,
/// adding any carry-over back at the end, and then taking the one's
/// complement.
pub fn ipv4_checksum(data: &[u8]) -> u16 {
    fold_checksum(sum_slice(data))
}

/// Normalizes a computed UDP checksum per RFC 768.
///
/// In UDP, a checksum field of `0` indicates that no checksum was computed,
/// so a calculated one's complement checksum of `0` is transmitted as all
/// ones (`u16::MAX`, which is equivalent to `0` in one's complement arithmetic).
#[inline]
pub(crate) fn normalize_udp_checksum(cksum: u16) -> u16 {
    if cksum == 0 { u16::MAX } else { cksum }
}

pub(crate) fn pseudo_header_checksum_parts(
    header: &[u8],
    payload: &[u8],
    src_octets: &[u8],
    dst_octets: &[u8],
    protocol: u16,
) -> u16 {
    let total_len = (header.len() + payload.len()) as u32;
    let sum = sum_slice(src_octets)
        + sum_slice(dst_octets)
        + u64::from(protocol)
        + sum_slice(&total_len.to_be_bytes())
        + sum_slice(header)
        + sum_slice(payload);
    fold_checksum(sum)
}

fn pseudo_header_checksum(
    packet: &[u8],
    src_octets: &[u8],
    dst_octets: &[u8],
    protocol: u16,
) -> u16 {
    pseudo_header_checksum_parts(packet, &[], src_octets, dst_octets, protocol)
}

/// Calculates the TCP checksum for IPv4.
pub fn tcp_checksum(tcp_packet: &[u8], src_addr: Ipv4Addr, dst_addr: Ipv4Addr) -> u16 {
    pseudo_header_checksum(tcp_packet, &src_addr.octets(), &dst_addr.octets(), 6)
}

/// Calculates the UDP checksum for IPv4.
pub fn udp_checksum(udp_packet: &[u8], src_addr: Ipv4Addr, dst_addr: Ipv4Addr) -> u16 {
    normalize_udp_checksum(pseudo_header_checksum(
        udp_packet,
        &src_addr.octets(),
        &dst_addr.octets(),
        17,
    ))
}

/// Calculates the TCP checksum for IPv6.
pub fn tcp_checksum_v6(tcp_packet: &[u8], src_addr: Ipv6Addr, dst_addr: Ipv6Addr) -> u16 {
    pseudo_header_checksum(tcp_packet, &src_addr.octets(), &dst_addr.octets(), 6)
}

/// Calculates the UDP checksum for IPv6.
pub fn udp_checksum_v6(udp_packet: &[u8], src_addr: Ipv6Addr, dst_addr: Ipv6Addr) -> u16 {
    normalize_udp_checksum(pseudo_header_checksum(
        udp_packet,
        &src_addr.octets(),
        &dst_addr.octets(),
        17,
    ))
}

/// Calculates the ICMPv6 checksum using the IPv6 pseudo-header.
pub fn icmpv6_checksum(icmpv6_packet: &[u8], src_addr: Ipv6Addr, dst_addr: Ipv6Addr) -> u16 {
    pseudo_header_checksum(icmpv6_packet, &src_addr.octets(), &dst_addr.octets(), 58)
}

fn sum_slice(data: &[u8]) -> u64 {
    let mut sum = 0u64;
    let (chunks, remainder) = data.as_chunks::<2>();
    for &chunk in chunks {
        sum += u64::from(u16::from_be_bytes(chunk));
    }
    if let &[last] = remainder {
        sum += u64::from(u16::from_be_bytes([last, 0]));
    }
    sum
}

fn fold_checksum(mut sum: u64) -> u16 {
    while sum >> 16 != 0 {
        sum = (sum & 0xFFFF) + (sum >> 16);
    }
    !sum as u16
}
