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

//! A TCP packet builder.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use zerocopy::{FromBytes, IntoBytes};

use super::tcp::TcpHeader;

pub struct TcpBuilder<'a> {
    header: &'a mut TcpHeader,
    rest: &'a mut [u8],
    src_addr: IpAddr,
    dst_addr: IpAddr,
    data_offset: u8, // in 32-bit words
    payload_len: usize,
}

impl<'a> TcpBuilder<'a> {
    fn new_generic(buffer: &'a mut [u8], src_addr: IpAddr, dst_addr: IpAddr) -> Option<Self> {
        let (header, rest) = TcpHeader::mut_from_prefix(buffer).ok()?;
        let data_offset = 5; // Default: 20 bytes
        header.source_port.set(0);
        header.dest_port.set(0);
        header.sequence_num.set(0);
        header.ack_num.set(0);
        header.data_offset_reserved_flags.set((data_offset as u16) << 12);
        header.window_size.set(65535);
        header.checksum.set(0);
        header.urgent_ptr.set(0);
        Some(Self { header, rest, src_addr, dst_addr, data_offset, payload_len: 0 })
    }

    pub fn new(buffer: &'a mut [u8], src_addr: Ipv4Addr, dst_addr: Ipv4Addr) -> Option<Self> {
        Self::new_generic(buffer, IpAddr::V4(src_addr), IpAddr::V4(dst_addr))
    }

    pub fn new_v6(buffer: &'a mut [u8], src_addr: Ipv6Addr, dst_addr: Ipv6Addr) -> Option<Self> {
        Self::new_generic(buffer, IpAddr::V6(src_addr), IpAddr::V6(dst_addr))
    }

    /// Sets the data offset (header length) in 32-bit words.
    ///
    /// # Note
    /// This should be called *before* writing any payload data (via
    /// `payload_mut()`), as changing the data offset shifts the start of
    /// the payload area.
    pub fn data_offset(&mut self, offset_words: u8) -> &mut Self {
        let clamped_offset = offset_words.clamp(5, 15);
        self.data_offset = clamped_offset;
        let current = self.header.data_offset_reserved_flags.get();
        // Preserve only the 9 flags (lower 9 bits: 0x01FF), force Reserved (bits 9-11)
        // to 0, and set the new Data Offset (bits 12-15).
        self.header
            .data_offset_reserved_flags
            .set((current & 0x01FF) | ((clamped_offset as u16) << 12));
        self
    }

    pub fn source_port(&mut self, port: u16) -> &mut Self {
        self.header.source_port.set(port);
        self
    }

    pub fn dest_port(&mut self, port: u16) -> &mut Self {
        self.header.dest_port.set(port);
        self
    }

    pub fn sequence_num(&mut self, seq: u32) -> &mut Self {
        self.header.sequence_num.set(seq);
        self
    }

    pub fn ack_num(&mut self, ack: u32) -> &mut Self {
        self.header.ack_num.set(ack);
        self
    }

    pub fn flags(&mut self, flags: u16) -> &mut Self {
        let current = self.header.data_offset_reserved_flags.get();
        // Preserve only Data Offset (bits 12-15), force Reserved (bits 9-11) to 0,
        // and set the new Flags (masked to 9 bits: 0x01FF).
        self.header.data_offset_reserved_flags.set((current & 0xF000) | (flags & 0x01FF));
        self
    }

    pub fn window_size(&mut self, window_size: u16) -> &mut Self {
        self.header.window_size.set(window_size);
        self
    }

    pub fn urgent_ptr(&mut self, urgent_ptr: u16) -> &mut Self {
        self.header.urgent_ptr.set(urgent_ptr);
        self
    }

    pub fn payload_mut(&mut self) -> Option<&mut [u8]> {
        let options_len = (self.data_offset as usize) * 4 - std::mem::size_of::<TcpHeader>();
        self.rest.get_mut(options_len..)
    }

    /// Returns a mutable reference to the TCP options space.
    ///
    /// Returns `None` if `data_offset` is 5 (no options) or if the header
    /// length exceeds the buffer size.
    pub fn options_mut(&mut self) -> Option<&mut [u8]> {
        if self.data_offset <= 5 {
            return None;
        }
        let options_len = (self.data_offset as usize) * 4 - std::mem::size_of::<TcpHeader>();
        self.rest.get_mut(..options_len)
    }

    pub fn payload<'b>(&'b mut self, payload: &[u8]) -> Option<&'b mut Self> {
        let total_header_len = (self.data_offset as usize) * 4;
        let end = total_header_len.checked_add(payload.len())?;
        if end > u16::MAX as usize {
            return None;
        }
        self.payload_mut()?.get_mut(..payload.len())?.copy_from_slice(payload);
        self.payload_len = payload.len();
        Some(self)
    }

    pub fn payload_len(&mut self, len: usize) -> &mut Self {
        self.payload_len = len;
        self
    }

    pub fn build(self) -> Option<usize> {
        self.header.checksum.set(0);
        let total_header_len = (self.data_offset as usize) * 4;
        let end = total_header_len.checked_add(self.payload_len)?;
        if end > u16::MAX as usize {
            return None;
        }
        let options_and_payload = self.rest.get(..end - std::mem::size_of::<TcpHeader>())?;
        let checksum = match (self.src_addr, self.dst_addr) {
            (IpAddr::V4(src), IpAddr::V4(dst)) => {
                crate::utils::checksum::pseudo_header_checksum_parts(
                    self.header.as_bytes(),
                    options_and_payload,
                    &src.octets(),
                    &dst.octets(),
                    6,
                )
            }
            (IpAddr::V6(src), IpAddr::V6(dst)) => {
                crate::utils::checksum::pseudo_header_checksum_parts(
                    self.header.as_bytes(),
                    options_and_payload,
                    &src.octets(),
                    &dst.octets(),
                    6,
                )
            }
            _ => return None,
        };
        self.header.checksum.set(checksum);
        Some(end)
    }
}
