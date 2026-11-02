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

//! Defines structures for NDP (Neighbor Discovery Protocol) packets using
//! `zerocopy`.

use zerocopy::{
    FromBytes, Immutable, IntoBytes, KnownLayout, Ref, U16, U32, Unaligned,
    byteorder::NetworkEndian,
};

/// NDP option type codes (RFC 4861 section 4.6).
pub mod ndp_option {
    /// Carries the sender's link-layer address (section 4.6.1).
    pub const SOURCE_LINK_LAYER_ADDR: u8 = 1;
    /// Carries the advertised target's link-layer address (section 4.6.1).
    ///
    /// Including this in a Neighbor Advertisement lets the peer populate its
    /// neighbor cache without a second round trip.
    pub const TARGET_LINK_LAYER_ADDR: u8 = 2;
    /// Describes an on-link prefix (section 4.6.2).
    pub const PREFIX_INFORMATION: u8 = 3;
    /// Advertises recursive DNS servers (RFC 8106).
    pub const RDNSS: u8 = 25;
}

/// Neighbor Advertisement flag bits (RFC 4861 section 4.4).
pub mod na_flags {
    /// The sender is a router.
    pub const ROUTER: u8 = 0x80;
    /// The advertisement answers a Neighbor Solicitation.
    pub const SOLICITED: u8 = 0x40;
    /// The receiver should overwrite an existing cache entry.
    pub const OVERRIDE: u8 = 0x20;
}

/// The hop limit every NDP message must use (RFC 4861 section 11.2).
///
/// A receiver checks for 255 to prove the message was not forwarded by a
/// router, so anything less is discarded.
pub const NDP_HOP_LIMIT: u8 = 255;

/// Represents a Neighbor Solicitation packet.
#[repr(C)]
#[derive(FromBytes, IntoBytes, Unaligned, KnownLayout, Immutable, Debug)]
pub struct NeighborSolicitation {
    pub reserved: U32<NetworkEndian>,
    pub target_addr: [u8; 16],
    // Options follow, but are not parsed for now.
}

/// Represents a Neighbor Advertisement packet.
#[repr(C)]
#[derive(FromBytes, IntoBytes, Unaligned, KnownLayout, Immutable, Debug)]
pub struct NeighborAdvertisement {
    pub flags: u8,
    pub reserved: [u8; 3],
    pub target_addr: [u8; 16],
    // Options follow, but are not parsed for now.
}

impl NeighborSolicitation {
    /// Parses a `NeighborSolicitation` from the beginning of the given byte
    /// slice.
    pub fn parse(bytes: &[u8]) -> Option<Ref<&[u8], NeighborSolicitation>> {
        Ref::from_prefix(bytes).ok().map(|(r, _)| r)
    }
}

impl NeighborAdvertisement {
    /// Parses a `NeighborAdvertisement` from the beginning of the given byte
    /// slice.
    pub fn parse(bytes: &[u8]) -> Option<Ref<&[u8], NeighborAdvertisement>> {
        Ref::from_prefix(bytes).ok().map(|(r, _)| r)
    }
}

/// Represents a Router Solicitation packet.
#[repr(C)]
#[derive(FromBytes, IntoBytes, Unaligned, KnownLayout, Immutable, Debug)]
pub struct RouterSolicitation {
    pub reserved: U32<NetworkEndian>,
}

/// Represents a Router Advertisement packet.
#[repr(C)]
#[derive(FromBytes, IntoBytes, Unaligned, KnownLayout, Immutable, Debug)]
pub struct RouterAdvertisement {
    pub cur_hop_limit: u8,
    pub flags: u8,
    pub router_lifetime: U16<NetworkEndian>,
    pub reachable_time: U32<NetworkEndian>,
    pub retrans_timer: U32<NetworkEndian>,
}

/// Represents the Source Link-Layer Address option.
#[repr(C)]
#[derive(FromBytes, IntoBytes, Unaligned, KnownLayout, Immutable, Debug)]
pub struct SourceLinkLayerAddressOption {
    pub option_type: u8, // 1
    pub length: u8,      // 1
    pub addr: [u8; 6],
}

/// Represents the Prefix Information option.
#[repr(C)]
#[derive(FromBytes, IntoBytes, Unaligned, KnownLayout, Immutable, Debug)]
pub struct PrefixInformationOption {
    pub option_type: u8, // 3
    pub length: u8,      // 4
    pub prefix_length: u8,
    pub flags: u8,
    pub valid_lifetime: U32<NetworkEndian>,
    pub preferred_lifetime: U32<NetworkEndian>,
    pub reserved: U32<NetworkEndian>,
    pub prefix: [u8; 16],
}

/// Represents the RDNSS (Recursive DNS Server) option.
///
/// Note: RFC 8106 allows for one or more DNS server addresses, but this
/// implementation is currently fixed to exactly one DNS server address.
#[repr(C)]
#[derive(FromBytes, IntoBytes, Unaligned, KnownLayout, Immutable, Debug)]
pub struct RdnssOption {
    pub option_type: u8, // 25
    pub length: u8,      // 3
    pub reserved: U16<NetworkEndian>,
    pub lifetime: U32<NetworkEndian>,
    pub dns_servers: [[u8; 16]; 1],
}

impl RouterSolicitation {
    /// Parses a `RouterSolicitation` from the beginning of the given byte
    /// slice.
    pub fn parse(bytes: &[u8]) -> Option<Ref<&[u8], RouterSolicitation>> {
        Ref::from_prefix(bytes).ok().map(|(r, _)| r)
    }
}

impl RouterAdvertisement {
    /// Parses a `RouterAdvertisement` from the beginning of the given byte
    /// slice.
    pub fn parse(bytes: &[u8]) -> Option<Ref<&[u8], RouterAdvertisement>> {
        Ref::from_prefix(bytes).ok().map(|(r, _)| r)
    }
}

impl SourceLinkLayerAddressOption {
    /// Parses a `SourceLinkLayerAddressOption` from the beginning of the given
    /// byte slice.
    pub fn parse(bytes: &[u8]) -> Option<Ref<&[u8], SourceLinkLayerAddressOption>> {
        Ref::from_prefix(bytes).ok().map(|(r, _)| r)
    }
}

impl PrefixInformationOption {
    /// Parses a `PrefixInformationOption` from the beginning of the given byte
    /// slice.
    pub fn parse(bytes: &[u8]) -> Option<Ref<&[u8], PrefixInformationOption>> {
        Ref::from_prefix(bytes).ok().map(|(r, _)| r)
    }
}

impl RdnssOption {
    /// Parses a `RdnssOption` from the beginning of the given byte slice.
    pub fn parse(bytes: &[u8]) -> Option<Ref<&[u8], RdnssOption>> {
        Ref::from_prefix(bytes).ok().map(|(r, _)| r)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_neighbor_solicitation_parsing() {
        let bytes: [u8; 20] = [
            0, 0, 0, 0, // Reserved
            0xfe, 0x80, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, // Target Address
        ];

        let packet = NeighborSolicitation::parse(&bytes).unwrap();
        assert_eq!(packet.target_addr, [0xfe, 0x80, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1]);
    }
}
