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

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use crate::{IP_P_ICMPV6, IP_P_TCP, IP_P_UDP};

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

/// Computes the one's-complement pseudo-header checksum over `header` and
/// `payload` when `src_addr` and `dst_addr` share an IP family, or returns
/// `None` if their address families differ.
pub(crate) fn ip_pseudo_header_checksum_parts(
    header: &[u8],
    payload: &[u8],
    src_addr: IpAddr,
    dst_addr: IpAddr,
    protocol: u8,
) -> Option<u16> {
    let checksum =
        |src: &[u8], dst: &[u8]| pseudo_header_checksum_parts(header, payload, src, dst, protocol);
    match (src_addr, dst_addr) {
        (IpAddr::V4(src), IpAddr::V4(dst)) => Some(checksum(&src.octets(), &dst.octets())),
        (IpAddr::V6(src), IpAddr::V6(dst)) => Some(checksum(&src.octets(), &dst.octets())),
        _ => None,
    }
}

fn pseudo_header_checksum_parts(
    header: &[u8],
    payload: &[u8],
    src_octets: &[u8],
    dst_octets: &[u8],
    protocol: u8,
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
    protocol: u8,
) -> u16 {
    pseudo_header_checksum_parts(packet, &[], src_octets, dst_octets, protocol)
}

/// Calculates the TCP checksum for IPv4.
pub fn tcp_checksum(tcp_packet: &[u8], src_addr: Ipv4Addr, dst_addr: Ipv4Addr) -> u16 {
    pseudo_header_checksum(tcp_packet, &src_addr.octets(), &dst_addr.octets(), IP_P_TCP)
}

/// Calculates the UDP checksum for IPv4.
pub fn udp_checksum(udp_packet: &[u8], src_addr: Ipv4Addr, dst_addr: Ipv4Addr) -> u16 {
    normalize_udp_checksum(pseudo_header_checksum(
        udp_packet,
        &src_addr.octets(),
        &dst_addr.octets(),
        IP_P_UDP,
    ))
}

/// Calculates the TCP checksum for IPv6.
pub fn tcp_checksum_v6(tcp_packet: &[u8], src_addr: Ipv6Addr, dst_addr: Ipv6Addr) -> u16 {
    pseudo_header_checksum(tcp_packet, &src_addr.octets(), &dst_addr.octets(), IP_P_TCP)
}

/// Calculates the UDP checksum for IPv6.
pub fn udp_checksum_v6(udp_packet: &[u8], src_addr: Ipv6Addr, dst_addr: Ipv6Addr) -> u16 {
    normalize_udp_checksum(pseudo_header_checksum(
        udp_packet,
        &src_addr.octets(),
        &dst_addr.octets(),
        IP_P_UDP,
    ))
}

/// Calculates the ICMPv6 checksum using the IPv6 pseudo-header.
pub fn icmpv6_checksum(icmpv6_packet: &[u8], src_addr: Ipv6Addr, dst_addr: Ipv6Addr) -> u16 {
    pseudo_header_checksum(icmpv6_packet, &src_addr.octets(), &dst_addr.octets(), IP_P_ICMPV6)
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
