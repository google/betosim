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

//! Builders for NDP (Neighbor Discovery Protocol) packets.

use zerocopy::FromBytes;

use super::ndp::{
    NeighborAdvertisement, NeighborSolicitation, PrefixInformationOption, RdnssOption,
    RouterAdvertisement, SourceLinkLayerAddressOption, TargetLinkLayerAddressOption, pio_flags,
};

const DEFAULT_PIO_VALID_LIFETIME_SECS: u32 = 30 * 24 * 60 * 60;
const DEFAULT_PIO_PREFERRED_LIFETIME_SECS: u32 = 7 * 24 * 60 * 60;

/// A builder for creating NDP Neighbor Solicitation packets.
pub struct NeighborSolicitationBuilder<'a> {
    packet: &'a mut NeighborSolicitation,
}

impl<'a> NeighborSolicitationBuilder<'a> {
    /// Creates a new builder from a mutable byte slice.
    pub fn new(buf: &'a mut [u8]) -> Option<Self> {
        NeighborSolicitation::mut_from_prefix(buf).ok().map(|(packet, _)| Self { packet })
    }

    /// Sets the target address for the solicitation.
    #[must_use]
    pub fn target_addr(self, addr: [u8; 16]) -> Self {
        self.packet.target_addr = addr;
        self
    }

    /// Finalizes the packet.
    pub fn build(self) {}
}

/// A builder for creating NDP Neighbor Advertisement packets.
pub struct NeighborAdvertisementBuilder<'a> {
    initial_len: usize,
    packet: &'a mut NeighborAdvertisement,
    rest: &'a mut [u8],
    target_link_layer_addr: Option<[u8; 6]>,
}

impl<'a> NeighborAdvertisementBuilder<'a> {
    /// Creates a new builder from a mutable byte slice.
    pub fn new(buf: &'a mut [u8]) -> Option<Self> {
        let initial_len = buf.len();
        let (packet, rest) = NeighborAdvertisement::mut_from_prefix(buf).ok()?;
        packet.target_addr = [0u8; 16];
        Some(Self { initial_len, packet, rest, target_link_layer_addr: None })
    }

    /// Sets the target address for the advertisement.
    #[must_use]
    pub fn target_addr(self, addr: [u8; 16]) -> Self {
        self.packet.target_addr = addr;
        self
    }

    /// Sets the target link-layer address option for the advertisement.
    #[must_use]
    pub fn target_link_layer_addr(mut self, mac: [u8; 6]) -> Self {
        self.target_link_layer_addr = Some(mac);
        self
    }

    /// Finalizes the packet, returning the number of bytes written.
    pub fn build(self) -> Option<usize> {
        let remaining = if let Some(mac) = self.target_link_layer_addr {
            let (tlla, rest) = TargetLinkLayerAddressOption::mut_from_prefix(self.rest).ok()?;
            *tlla = TargetLinkLayerAddressOption::new(mac);
            rest
        } else {
            self.rest
        };

        Some(self.initial_len - remaining.len())
    }
}

/// A builder for creating NDP Router Advertisement packets with SLLA and PIO
/// options.
pub struct RouterAdvertisementBuilder<'a> {
    initial_len: usize,
    ra: &'a mut RouterAdvertisement,
    slla: &'a mut SourceLinkLayerAddressOption,
    pio: &'a mut PrefixInformationOption,
    rest: &'a mut [u8],
    rdnss: Option<([u8; 16], u32)>,
}

impl<'a> RouterAdvertisementBuilder<'a> {
    /// Creates a new builder from a mutable byte slice.
    pub fn new(buf: &'a mut [u8]) -> Option<Self> {
        let initial_len = buf.len();
        let (ra, rest) = RouterAdvertisement::mut_from_prefix(buf).ok()?;
        let (slla, rest) = SourceLinkLayerAddressOption::mut_from_prefix(rest).ok()?;
        let (pio, rest) = PrefixInformationOption::mut_from_prefix(rest).ok()?;
        Some(Self { initial_len, ra, slla, pio, rest, rdnss: None })
    }

    /// Appends a Recursive DNS Server (RDNSS) option with the given server address
    /// and lifetime (in seconds).
    #[must_use]
    pub fn rdnss(mut self, dns_server: [u8; 16], lifetime_secs: u32) -> Self {
        self.rdnss = Some((dns_server, lifetime_secs));
        self
    }

    /// Builds the Router Advertisement packet, optionally appending the RDNSS
    /// option. Returns the number of bytes written.
    pub fn build(
        self,
        reachable_time: u32,
        retrans_timer: u32,
        slla_mac: [u8; 6],
        prefix: [u8; 16],
        prefix_len: u8,
    ) -> Option<usize> {
        self.ra.reachable_time = reachable_time.into();
        self.ra.retrans_timer = retrans_timer.into();
        *self.slla = SourceLinkLayerAddressOption::new(slla_mac);
        *self.pio = PrefixInformationOption::new(
            prefix_len,
            pio_flags::ON_LINK | pio_flags::AUTONOMOUS,
            DEFAULT_PIO_VALID_LIFETIME_SECS,
            DEFAULT_PIO_PREFERRED_LIFETIME_SECS,
            prefix,
        );

        let remaining = if let Some((dns_server, lifetime_secs)) = self.rdnss {
            let (rdnss, rest) = RdnssOption::mut_from_prefix(self.rest).ok()?;
            *rdnss = RdnssOption::new(lifetime_secs, dns_server);
            rest
        } else {
            self.rest
        };

        Some(self.initial_len - remaining.len())
    }
}

#[cfg(test)]
mod tests {
    use std::net::Ipv6Addr;

    use super::*;
    use crate::ndp_option;

    const TEST_BUF_CAP: usize = 256;
    const TEST_PREFIX_LEN: u8 = 64;
    const TEST_RDNSS_LIFETIME_SECS: u32 = 1800;

    #[test]
    fn test_neighbor_solicitation_builder() {
        let mut buf = [0u8; TEST_BUF_CAP];
        let target = Ipv6Addr::new(0xfe80, 0, 0, 0, 0, 0, 0, 1).octets();
        let builder = NeighborSolicitationBuilder::new(&mut buf).unwrap();
        builder.target_addr(target).build();

        let (packet, rest) = NeighborSolicitation::ref_from_prefix(&buf).unwrap();
        assert_eq!(packet.target_addr, target);

        let ns_len = buf.len() - rest.len();
        assert!(NeighborSolicitationBuilder::new(&mut buf[..ns_len - 1]).is_none());
    }

    #[test]
    fn test_neighbor_advertisement_builder_with_tlla() {
        let mut buf = [0u8; TEST_BUF_CAP];
        let target = Ipv6Addr::new(0xfe80, 0, 0, 0, 0, 0, 0, 2).octets();
        let mac = [0x52, 0x54, 0x00, 0x12, 0x34, 0x56];
        let builder = NeighborAdvertisementBuilder::new(&mut buf).unwrap();
        let written = builder.target_addr(target).target_link_layer_addr(mac).build().unwrap();

        let (packet, rest) = NeighborAdvertisement::ref_from_prefix(&buf[..written]).unwrap();
        assert_eq!(packet.target_addr, target);

        let (tlla, rest) = TargetLinkLayerAddressOption::ref_from_prefix(rest).unwrap();
        assert_eq!(tlla.option_type, ndp_option::TARGET_LINK_LAYER_ADDR);
        assert_eq!(tlla.length, TargetLinkLayerAddressOption::LENGTH_UNITS);
        assert_eq!(tlla.addr, mac);
        assert!(rest.is_empty());
    }

    #[test]
    fn test_neighbor_advertisement_builder_without_tlla() {
        let mut buf = [0u8; TEST_BUF_CAP];
        let target = Ipv6Addr::new(0xfe80, 0, 0, 0, 0, 0, 0, 2).octets();
        let builder = NeighborAdvertisementBuilder::new(&mut buf).unwrap();
        let written = builder.target_addr(target).build().unwrap();

        let (packet, rest) = NeighborAdvertisement::ref_from_prefix(&buf[..written]).unwrap();
        assert_eq!(packet.target_addr, target);
        assert!(rest.is_empty());
    }

    #[test]
    fn test_neighbor_advertisement_builder_short_buffers() {
        let mut buf = [0u8; TEST_BUF_CAP];
        let target = Ipv6Addr::new(0xfe80, 0, 0, 0, 0, 0, 0, 2).octets();
        let mac = [0x52, 0x54, 0x00, 0x12, 0x34, 0x56];

        let builder_no_tlla = NeighborAdvertisementBuilder::new(&mut buf).unwrap();
        let no_tlla_len = builder_no_tlla.target_addr(target).build().unwrap();
        assert!(NeighborAdvertisementBuilder::new(&mut buf[..no_tlla_len - 1]).is_none());

        let builder_with_tlla = NeighborAdvertisementBuilder::new(&mut buf).unwrap();
        let with_tlla_len =
            builder_with_tlla.target_addr(target).target_link_layer_addr(mac).build().unwrap();
        assert!(
            NeighborAdvertisementBuilder::new(&mut buf[..with_tlla_len - 1])
                .unwrap()
                .target_addr(target)
                .target_link_layer_addr(mac)
                .build()
                .is_none()
        );
    }

    #[test]
    fn test_router_advertisement_builder() {
        let mut buf = [0u8; TEST_BUF_CAP];
        let prefix = Ipv6Addr::new(0xfe80, 0, 0, 0, 0, 0, 0, 0).octets();
        let gateway_mac = [0xde, 0xad, 0xbe, 0xef, 0x12, 0x35];
        let builder = RouterAdvertisementBuilder::new(&mut buf).unwrap();
        let len = builder.build(0, 0, gateway_mac, prefix, TEST_PREFIX_LEN).unwrap();

        let (ra, rest) = RouterAdvertisement::ref_from_prefix(&buf[..len]).unwrap();
        assert_eq!(ra.reachable_time.get(), 0);
        assert_eq!(ra.retrans_timer.get(), 0);

        let (slla, rest) = SourceLinkLayerAddressOption::ref_from_prefix(rest).unwrap();
        assert_eq!(slla.option_type, ndp_option::SOURCE_LINK_LAYER_ADDR);
        assert_eq!(slla.length, SourceLinkLayerAddressOption::LENGTH_UNITS);
        assert_eq!(slla.addr, gateway_mac);

        let (pio, rest) = PrefixInformationOption::ref_from_prefix(rest).unwrap();
        assert_eq!(pio.option_type, ndp_option::PREFIX_INFORMATION);
        assert_eq!(pio.length, PrefixInformationOption::LENGTH_UNITS);
        assert_eq!(pio.flags, pio_flags::ON_LINK | pio_flags::AUTONOMOUS);
        assert_eq!(pio.prefix_length, TEST_PREFIX_LEN);
        assert_eq!(pio.prefix, prefix);
        assert!(rest.is_empty());

        assert!(RouterAdvertisementBuilder::new(&mut buf[..len - 1]).is_none());
    }

    #[test]
    fn test_router_advertisement_builder_with_rdnss() {
        let mut buf = [0u8; TEST_BUF_CAP];
        let prefix = Ipv6Addr::new(0xfe80, 0, 0, 0, 0, 0, 0, 0).octets();
        let gateway_mac = [0xde, 0xad, 0xbe, 0xef, 0x12, 0x35];
        let dns_server = Ipv6Addr::new(0xfe80, 0, 0, 0, 0, 0, 0, 2).octets();

        let builder = RouterAdvertisementBuilder::new(&mut buf).unwrap();
        let len = builder
            .rdnss(dns_server, TEST_RDNSS_LIFETIME_SECS)
            .build(0, 0, gateway_mac, prefix, TEST_PREFIX_LEN)
            .unwrap();

        let (_ra, rest) = RouterAdvertisement::ref_from_prefix(&buf[..len]).unwrap();
        let (_slla, rest) = SourceLinkLayerAddressOption::ref_from_prefix(rest).unwrap();
        let (_pio, rest) = PrefixInformationOption::ref_from_prefix(rest).unwrap();

        let (rdnss, rest) = RdnssOption::ref_from_prefix(rest).unwrap();
        assert_eq!(rdnss.option_type, ndp_option::RDNSS);
        assert_eq!(rdnss.length, RdnssOption::LENGTH_UNITS);
        assert_eq!(rdnss.lifetime.get(), TEST_RDNSS_LIFETIME_SECS);
        assert_eq!(rdnss.dns_servers, [dns_server]);
        assert!(rest.is_empty());

        assert!(
            RouterAdvertisementBuilder::new(&mut buf[..len - 1])
                .unwrap()
                .rdnss(dns_server, TEST_RDNSS_LIFETIME_SECS)
                .build(0, 0, gateway_mac, prefix, TEST_PREFIX_LEN)
                .is_none()
        );
    }
}
