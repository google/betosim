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

/// Prefix Information Option flag bits (RFC 4861 section 4.6.2).
pub mod pio_flags {
    /// On-link flag ("L-bit").
    pub const ON_LINK: u8 = 0x80;
    /// Autonomous address-configuration flag ("A-bit").
    pub const AUTONOMOUS: u8 = 0x40;
}

/// The hop limit every NDP message must use (RFC 4861 section 11.2).
///
/// A receiver checks for 255 to prove the message was not forwarded by a
/// router, so anything less is discarded.
pub const NDP_HOP_LIMIT: u8 = 255;

/// Represents a Neighbor Solicitation packet body following `Icmpv6Header`.
#[repr(C)]
#[derive(FromBytes, IntoBytes, Unaligned, KnownLayout, Immutable, Debug)]
pub struct NeighborSolicitation {
    pub target_addr: [u8; 16],
    // Options follow, but are not parsed for now.
}

/// Represents a Neighbor Advertisement packet body following `Icmpv6Header`.
#[repr(C)]
#[derive(FromBytes, IntoBytes, Unaligned, KnownLayout, Immutable, Debug)]
pub struct NeighborAdvertisement {
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

/// Represents a Router Advertisement packet body following `Icmpv6Header`.
#[repr(C)]
#[derive(FromBytes, IntoBytes, Unaligned, KnownLayout, Immutable, Debug)]
pub struct RouterAdvertisement {
    pub reachable_time: U32<NetworkEndian>,
    pub retrans_timer: U32<NetworkEndian>,
}

/// Represents the Target Link-Layer Address option (RFC 4861 section 4.6.1).
#[repr(C)]
#[derive(FromBytes, IntoBytes, Unaligned, KnownLayout, Immutable, Debug)]
pub struct TargetLinkLayerAddressOption {
    pub option_type: u8,
    pub length: u8,
    pub addr: [u8; 6],
}

/// Represents the Source Link-Layer Address option.
#[repr(C)]
#[derive(FromBytes, IntoBytes, Unaligned, KnownLayout, Immutable, Debug)]
pub struct SourceLinkLayerAddressOption {
    pub option_type: u8,
    pub length: u8,
    pub addr: [u8; 6],
}

/// Represents the Prefix Information option.
#[repr(C)]
#[derive(FromBytes, IntoBytes, Unaligned, KnownLayout, Immutable, Debug)]
pub struct PrefixInformationOption {
    pub option_type: u8,
    pub length: u8,
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
    pub option_type: u8,
    pub length: u8,
    pub reserved: U16<NetworkEndian>,
    pub lifetime: U32<NetworkEndian>,
    pub dns_servers: [[u8; 16]; 1],
}

impl RouterAdvertisement {
    /// Parses a `RouterAdvertisement` from the beginning of the given byte
    /// slice.
    pub fn parse(bytes: &[u8]) -> Option<Ref<&[u8], RouterAdvertisement>> {
        Ref::from_prefix(bytes).ok().map(|(r, _)| r)
    }
}

impl SourceLinkLayerAddressOption {
    /// Option length in units of 8 octets (RFC 4861 section 4.6.1).
    pub const LENGTH_UNITS: u8 = 1;

    /// Creates a new `SourceLinkLayerAddressOption` for the given MAC address.
    #[must_use]
    pub const fn new(addr: [u8; 6]) -> Self {
        Self { option_type: ndp_option::SOURCE_LINK_LAYER_ADDR, length: Self::LENGTH_UNITS, addr }
    }

    /// Parses a `SourceLinkLayerAddressOption` from the beginning of the given
    /// byte slice.
    pub fn parse(bytes: &[u8]) -> Option<Ref<&[u8], SourceLinkLayerAddressOption>> {
        Ref::from_prefix(bytes).ok().map(|(r, _)| r)
    }
}

impl PrefixInformationOption {
    /// Option length in units of 8 octets (RFC 4861 section 4.6.2).
    pub const LENGTH_UNITS: u8 = 4;

    /// Creates a new `PrefixInformationOption`.
    #[must_use]
    pub const fn new(
        prefix_length: u8,
        flags: u8,
        valid_lifetime_secs: u32,
        preferred_lifetime_secs: u32,
        prefix: [u8; 16],
    ) -> Self {
        Self {
            option_type: ndp_option::PREFIX_INFORMATION,
            length: Self::LENGTH_UNITS,
            prefix_length,
            flags,
            valid_lifetime: U32::new(valid_lifetime_secs),
            preferred_lifetime: U32::new(preferred_lifetime_secs),
            reserved: U32::new(0),
            prefix,
        }
    }

    /// Parses a `PrefixInformationOption` from the beginning of the given byte
    /// slice.
    pub fn parse(bytes: &[u8]) -> Option<Ref<&[u8], PrefixInformationOption>> {
        Ref::from_prefix(bytes).ok().map(|(r, _)| r)
    }
}

impl RdnssOption {
    /// Option length in units of 8 octets (RFC 8106 section 5.1).
    pub const LENGTH_UNITS: u8 = 3;

    /// Creates a new `RdnssOption`.
    #[must_use]
    pub const fn new(lifetime_secs: u32, dns_server: [u8; 16]) -> Self {
        Self {
            option_type: ndp_option::RDNSS,
            length: Self::LENGTH_UNITS,
            reserved: U16::new(0),
            lifetime: U32::new(lifetime_secs),
            dns_servers: [dns_server],
        }
    }

    /// Parses a `RdnssOption` from the beginning of the given byte slice.
    pub fn parse(bytes: &[u8]) -> Option<Ref<&[u8], RdnssOption>> {
        Ref::from_prefix(bytes).ok().map(|(r, _)| r)
    }
}

impl TargetLinkLayerAddressOption {
    /// Option length in units of 8 octets (RFC 4861 section 4.6.1).
    pub const LENGTH_UNITS: u8 = 1;

    /// Creates a new `TargetLinkLayerAddressOption` for the given MAC address.
    #[must_use]
    pub const fn new(addr: [u8; 6]) -> Self {
        Self { option_type: ndp_option::TARGET_LINK_LAYER_ADDR, length: Self::LENGTH_UNITS, addr }
    }

    /// Parses a `TargetLinkLayerAddressOption` from the beginning of the given
    /// byte slice.
    pub fn parse(bytes: &[u8]) -> Option<Ref<&[u8], TargetLinkLayerAddressOption>> {
        Ref::from_prefix(bytes).ok().map(|(r, _)| r)
    }
}

#[cfg(test)]
mod tests {
    use std::net::Ipv6Addr;

    use super::*;

    const TEST_PREFIX_LEN: u8 = 64;
    const TEST_VALID_LIFETIME_SECS: u32 = 30 * 24 * 60 * 60;
    const TEST_PREFERRED_LIFETIME_SECS: u32 = 7 * 24 * 60 * 60;
    const TEST_RDNSS_LIFETIME_SECS: u32 = 1800;

    #[test]
    fn test_neighbor_solicitation_parsing() {
        let target = Ipv6Addr::new(0xfe80, 0, 0, 0, 0, 0, 0, 1).octets();
        let bytes: [u8; 16] = target;

        let packet = NeighborSolicitation::parse(&bytes).unwrap();
        assert_eq!(packet.target_addr, target);
    }

    #[test]
    fn test_neighbor_advertisement_parsing() {
        let target = Ipv6Addr::new(0xfe80, 0, 0, 0, 0, 0, 0, 2).octets();
        let bytes: [u8; 16] = target;

        let packet = NeighborAdvertisement::parse(&bytes).unwrap();
        assert_eq!(packet.target_addr, target);
    }

    #[test]
    fn test_target_link_layer_address_option_parsing() {
        let mac = [0x52, 0x54, 0x00, 0x12, 0x34, 0x56];
        let constructed = TargetLinkLayerAddressOption::new(mac);
        assert_eq!(constructed.option_type, ndp_option::TARGET_LINK_LAYER_ADDR);
        assert_eq!(constructed.length, TargetLinkLayerAddressOption::LENGTH_UNITS);
        assert_eq!(constructed.addr, mac);

        let bytes = [
            ndp_option::TARGET_LINK_LAYER_ADDR,
            TargetLinkLayerAddressOption::LENGTH_UNITS,
            0x52,
            0x54,
            0x00,
            0x12,
            0x34,
            0x56,
        ];
        let option = TargetLinkLayerAddressOption::parse(&bytes).unwrap();
        assert_eq!(option.option_type, ndp_option::TARGET_LINK_LAYER_ADDR);
        assert_eq!(option.length, TargetLinkLayerAddressOption::LENGTH_UNITS);
        assert_eq!(option.addr, mac);
    }

    #[test]
    fn test_source_link_layer_address_option() {
        let mac = [0xde, 0xad, 0xbe, 0xef, 0x12, 0x34];
        let option = SourceLinkLayerAddressOption::new(mac);
        assert_eq!(option.option_type, ndp_option::SOURCE_LINK_LAYER_ADDR);
        assert_eq!(option.length, SourceLinkLayerAddressOption::LENGTH_UNITS);
        assert_eq!(option.addr, mac);
    }

    #[test]
    fn test_prefix_information_option() {
        let prefix = Ipv6Addr::new(0x2001, 0xdb8, 0, 0, 0, 0, 0, 0).octets();
        let option = PrefixInformationOption::new(
            TEST_PREFIX_LEN,
            pio_flags::ON_LINK | pio_flags::AUTONOMOUS,
            TEST_VALID_LIFETIME_SECS,
            TEST_PREFERRED_LIFETIME_SECS,
            prefix,
        );
        assert_eq!(option.option_type, ndp_option::PREFIX_INFORMATION);
        assert_eq!(option.length, PrefixInformationOption::LENGTH_UNITS);
        assert_eq!(option.prefix_length, TEST_PREFIX_LEN);
        assert_eq!(option.flags, pio_flags::ON_LINK | pio_flags::AUTONOMOUS);
        assert_eq!(option.valid_lifetime.get(), TEST_VALID_LIFETIME_SECS);
        assert_eq!(option.preferred_lifetime.get(), TEST_PREFERRED_LIFETIME_SECS);
        assert_eq!(option.reserved.get(), 0);
        assert_eq!(option.prefix, prefix);
    }

    #[test]
    fn test_rdnss_option() {
        let dns_server = Ipv6Addr::new(0x2001, 0x4860, 0x4860, 0, 0, 0, 0, 0x8888).octets();
        let option = RdnssOption::new(TEST_RDNSS_LIFETIME_SECS, dns_server);
        assert_eq!(option.option_type, ndp_option::RDNSS);
        assert_eq!(option.length, RdnssOption::LENGTH_UNITS);
        assert_eq!(option.reserved.get(), 0);
        assert_eq!(option.lifetime.get(), TEST_RDNSS_LIFETIME_SECS);
        assert_eq!(option.dns_servers, [dns_server]);
    }
}
