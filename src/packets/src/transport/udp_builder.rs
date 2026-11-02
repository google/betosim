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

//! A UDP packet builder.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use zerocopy::{FromBytes, IntoBytes};

use super::udp::UdpHeader;

/// A builder for a UDP packet.
pub struct UdpBuilder<'a> {
    header: &'a mut UdpHeader,
    payload: &'a mut [u8],
    src_addr: IpAddr,
    dst_addr: IpAddr,
    payload_len: usize,
}

impl<'a> UdpBuilder<'a> {
    fn new_generic(
        buffer: &'a mut [u8],
        src_addr: IpAddr,
        dst_addr: IpAddr,
        src_port: u16,
        dst_port: u16,
    ) -> Option<Self> {
        let (header, payload) = UdpHeader::mut_from_prefix(buffer).ok()?;
        header.source_port.set(src_port);
        header.dest_port.set(dst_port);
        header.length.set(std::mem::size_of::<UdpHeader>() as u16);
        header.checksum.set(0);
        Some(Self { header, payload, src_addr, dst_addr, payload_len: 0 })
    }

    /// Creates a new `UdpBuilder` for IPv4.
    pub fn new(
        buffer: &'a mut [u8],
        src_addr: Ipv4Addr,
        dst_addr: Ipv4Addr,
        src_port: u16,
        dst_port: u16,
    ) -> Option<Self> {
        Self::new_generic(buffer, IpAddr::V4(src_addr), IpAddr::V4(dst_addr), src_port, dst_port)
    }

    /// Creates a new `UdpBuilder` for IPv6.
    pub fn new_v6(
        buffer: &'a mut [u8],
        src_addr: Ipv6Addr,
        dst_addr: Ipv6Addr,
        src_port: u16,
        dst_port: u16,
    ) -> Option<Self> {
        Self::new_generic(buffer, IpAddr::V6(src_addr), IpAddr::V6(dst_addr), src_port, dst_port)
    }

    /// Returns a mutable reference to the payload.
    pub fn payload_mut(&mut self) -> &mut [u8] {
        self.payload
    }

    /// Sets the payload data.
    pub fn payload<'b>(&'b mut self, payload: &[u8]) -> Option<&'b mut Self> {
        let total_len = std::mem::size_of::<UdpHeader>() + payload.len();
        if total_len > u16::MAX as usize {
            return None;
        }
        self.payload.get_mut(..payload.len())?.copy_from_slice(payload);
        self.payload_len = payload.len();
        Some(self)
    }

    /// Sets the payload length.
    pub fn payload_len(&mut self, len: usize) -> &mut Self {
        self.payload_len = len;
        self
    }

    /// Builds the UDP packet.
    pub fn build(self) -> Option<usize> {
        self.header.checksum.set(0);
        let total_len = std::mem::size_of::<UdpHeader>() + self.payload_len;
        if total_len > u16::MAX as usize || self.payload_len > self.payload.len() {
            return None;
        }
        self.header.length.set(total_len as u16);
        let payload_slice = self.payload.get(..self.payload_len)?;
        let checksum = match (self.src_addr, self.dst_addr) {
            (IpAddr::V4(src), IpAddr::V4(dst)) => crate::utils::checksum::normalize_udp_checksum(
                crate::utils::checksum::pseudo_header_checksum_parts(
                    self.header.as_bytes(),
                    payload_slice,
                    &src.octets(),
                    &dst.octets(),
                    17,
                ),
            ),
            (IpAddr::V6(src), IpAddr::V6(dst)) => crate::utils::checksum::normalize_udp_checksum(
                crate::utils::checksum::pseudo_header_checksum_parts(
                    self.header.as_bytes(),
                    payload_slice,
                    &src.octets(),
                    &dst.octets(),
                    17,
                ),
            ),
            _ => return None,
        };
        self.header.checksum.set(checksum);
        Some(total_len)
    }
}

/// A legacy compatibility builder for a UDP packet.
pub struct UdpPacketBuilder<'a> {
    header: &'a mut UdpHeader,
    payload: &'a mut [u8],
}

impl<'a> UdpPacketBuilder<'a> {
    /// Creates a new `UdpPacketBuilder`, defaulting the UDP length field to
    /// the provided buffer length (capped at `u16::MAX`).
    pub fn new(buffer: &'a mut [u8], src_port: u16, dst_port: u16) -> Option<Self> {
        if buffer.len() < std::mem::size_of::<UdpHeader>() {
            return None;
        }
        let default_len = u16::try_from(buffer.len()).unwrap_or(u16::MAX);
        let (header_slice, payload) = buffer.split_at_mut(std::mem::size_of::<UdpHeader>());
        let header = UdpHeader::mut_from_bytes(header_slice).ok()?;
        header.source_port.set(src_port);
        header.dest_port.set(dst_port);
        header.length.set(default_len);
        header.checksum.set(0);
        Some(Self { header, payload })
    }

    /// Returns a mutable reference to the payload.
    pub fn payload_mut(&mut self) -> &mut [u8] {
        self.payload
    }

    /// Sets the payload length.
    pub fn payload_len(&mut self, len: usize) {
        self.header.length.set((std::mem::size_of::<UdpHeader>() + len) as u16);
    }

    /// Builds the UDP packet.
    pub fn build(self) -> usize {
        self.header.checksum.set(0);
        self.header.length.get() as usize
    }
}
