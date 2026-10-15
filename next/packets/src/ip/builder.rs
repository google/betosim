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

use std::net::{Ipv4Addr, Ipv6Addr};

use zerocopy::FromBytes;

use crate::ip::{Ipv4Header, Ipv6Header};

pub struct Ipv4Builder<'a> {
    header: &'a mut Ipv4Header,
    payload: &'a mut [u8],
    payload_len: usize,
}

impl<'a> Ipv4Builder<'a> {
    pub fn new(
        buffer: &'a mut [u8],
        protocol: u8,
        src_addr: Ipv4Addr,
        dst_addr: Ipv4Addr,
    ) -> Option<Self> {
        if buffer.len() < std::mem::size_of::<Ipv4Header>() {
            return None;
        }
        let (header_slice, payload) = buffer.split_at_mut(std::mem::size_of::<Ipv4Header>());
        let header = Ipv4Header::mut_from_bytes(header_slice).ok()?;
        header.version_ihl = (4 << 4) | 5;
        header.dscp_ecn = 0;
        header.total_length.set(std::mem::size_of::<Ipv4Header>() as u16); // Default: 20 bytes (0 payload)
        header.identification.set(0);
        header.flags_fragment_offset.set(0);
        header.ttl = 64;
        header.protocol = protocol;
        header.header_checksum.set(0);
        header.source_addr = src_addr.octets();
        header.dest_addr = dst_addr.octets();
        Some(Self { header, payload, payload_len: 0 })
    }

    pub fn ttl(&mut self, ttl: u8) -> &mut Self {
        self.header.ttl = ttl;
        self
    }

    pub fn dscp_ecn(&mut self, dscp_ecn: u8) -> &mut Self {
        self.header.dscp_ecn = dscp_ecn;
        self
    }

    pub fn identification(&mut self, identification: u16) -> &mut Self {
        self.header.identification.set(identification);
        self
    }

    pub fn flags_fragment_offset(&mut self, flags_fragment_offset: u16) -> &mut Self {
        self.header.flags_fragment_offset.set(flags_fragment_offset);
        self
    }

    pub fn payload_mut(&mut self) -> &mut [u8] {
        self.payload
    }

    pub fn payload_len(&mut self, len: usize) -> &mut Self {
        self.payload_len = len;
        self
    }

    /// Sets the payload data.
    pub fn payload<'b>(&'b mut self, payload: &[u8]) -> Option<&'b mut Self> {
        let total_len = std::mem::size_of::<Ipv4Header>() + payload.len();
        if total_len > u16::MAX as usize {
            return None;
        }
        self.payload.get_mut(..payload.len())?.copy_from_slice(payload);
        self.payload_len = payload.len();
        Some(self)
    }

    pub fn build(self) -> Option<usize> {
        let total_len = std::mem::size_of::<Ipv4Header>() + self.payload_len;
        if total_len > u16::MAX as usize || self.payload_len > self.payload.len() {
            return None;
        }
        self.header.total_length.set(total_len as u16);
        self.header.update_checksum();
        Some(total_len)
    }
}

pub struct Ipv6Builder<'a> {
    header: &'a mut Ipv6Header,
    payload: &'a mut [u8],
    payload_len: usize,
}

impl<'a> Ipv6Builder<'a> {
    pub fn new(
        buffer: &'a mut [u8],
        protocol: u8,
        src_addr: Ipv6Addr,
        dst_addr: Ipv6Addr,
    ) -> Option<Self> {
        if buffer.len() < std::mem::size_of::<Ipv6Header>() {
            return None;
        }
        let (header_slice, payload) = buffer.split_at_mut(std::mem::size_of::<Ipv6Header>());
        let header = Ipv6Header::mut_from_bytes(header_slice).ok()?;
        header.version_tc_fl.set(6 << 28);
        header.payload_length.set(0); // Will be set later
        header.next_header = protocol;
        header.hop_limit = 64;
        header.source_addr = src_addr.octets();
        header.dest_addr = dst_addr.octets();
        Some(Self { header, payload, payload_len: 0 })
    }

    pub fn payload_mut(&mut self) -> &mut [u8] {
        self.payload
    }

    pub fn payload_len(&mut self, len: usize) -> &mut Self {
        self.payload_len = len;
        self
    }

    /// Sets the payload data.
    pub fn payload<'b>(&'b mut self, payload: &[u8]) -> Option<&'b mut Self> {
        if payload.len() > u16::MAX as usize {
            return None;
        }
        self.payload.get_mut(..payload.len())?.copy_from_slice(payload);
        self.payload_len = payload.len();
        Some(self)
    }

    pub fn hop_limit(&mut self, hop_limit: u8) -> &mut Self {
        self.header.hop_limit = hop_limit;
        self
    }

    pub fn build(self) -> Option<usize> {
        if self.payload_len > u16::MAX as usize || self.payload_len > self.payload.len() {
            return None;
        }
        self.header.payload_length.set(self.payload_len as u16);
        Some(self.payload_len + std::mem::size_of::<Ipv6Header>())
    }
}
