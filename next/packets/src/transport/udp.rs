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

//! Defines the UDP (User Datagram Protocol) header using `zerocopy`.

use zerocopy::{
    FromBytes, Immutable, IntoBytes, KnownLayout, Ref, U16, Unaligned, byteorder::NetworkEndian,
};

use crate::utils::general::ParseResult;

/// Represents the UDP header.
#[derive(FromBytes, IntoBytes, Unaligned, KnownLayout, Immutable, Debug)]
#[repr(C)]
pub struct UdpHeader {
    /// The source port number.
    pub source_port: U16<NetworkEndian>,
    /// The destination port number.
    pub dest_port: U16<NetworkEndian>,
    /// The length of the UDP header and data in bytes.
    pub length: U16<NetworkEndian>,
    /// The UDP checksum.
    pub checksum: U16<NetworkEndian>,
}

impl UdpHeader {
    /// Parses a `UdpHeader` from the beginning of the given byte slice.
    ///
    /// Returns a reference to the header and a slice for the remaining bytes
    /// (the UDP payload).
    pub fn parse(bytes: &[u8]) -> Option<ParseResult<'_, UdpHeader>> {
        Ref::from_prefix(bytes).ok()
    }
}

#[cfg(test)]
#[path = "udp_tests.rs"]
mod tests;
