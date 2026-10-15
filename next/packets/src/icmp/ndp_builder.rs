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

use zerocopy::{FromBytes, U16, U32};

use super::ndp::{
    NeighborAdvertisement, NeighborSolicitation, PrefixInformationOption, RdnssOption,
    RouterAdvertisement, RouterSolicitation, SourceLinkLayerAddressOption, ndp_option,
};

/// A builder for creating NDP Neighbor Solicitation packets.
pub struct NeighborSolicitationBuilder<'a> {
    packet: &'a mut NeighborSolicitation,
}

impl<'a> NeighborSolicitationBuilder<'a> {
    /// Creates a new builder from a mutable byte slice.
    pub fn new(buf: &'a mut [u8]) -> Option<Self> {
        NeighborSolicitation::mut_from_bytes(buf).ok().map(|packet| Self { packet })
    }

    /// Sets the target address for the solicitation.
    pub fn target_addr(self, addr: [u8; 16]) -> Self {
        self.packet.target_addr = addr;
        self
    }

    /// Finalizes the packet.
    pub fn build(self) {
        self.packet.reserved = U32::new(0);
    }
}

/// A builder for creating NDP Neighbor Advertisement packets.
pub struct NeighborAdvertisementBuilder<'a> {
    packet: &'a mut NeighborAdvertisement,
}

impl<'a> NeighborAdvertisementBuilder<'a> {
    /// Creates a new builder from a mutable byte slice.
    pub fn new(buf: &'a mut [u8]) -> Option<Self> {
        NeighborAdvertisement::mut_from_bytes(buf).ok().map(|packet| Self { packet })
    }

    /// Sets the flags for the advertisement.
    pub fn flags(self, flags: u8) -> Self {
        self.packet.flags = flags;
        self
    }

    /// Sets the target address for the advertisement.
    pub fn target_addr(self, addr: [u8; 16]) -> Self {
        self.packet.target_addr = addr;
        self
    }

    /// Finalizes the packet.
    pub fn build(self) {
        self.packet.reserved = [0; 3];
    }
}

/// A builder for creating NDP Router Solicitation packets.
pub struct RouterSolicitationBuilder<'a> {
    packet: &'a mut RouterSolicitation,
}

impl<'a> RouterSolicitationBuilder<'a> {
    /// Creates a new builder from a mutable byte slice.
    pub fn new(buf: &'a mut [u8]) -> Option<Self> {
        RouterSolicitation::mut_from_bytes(buf).ok().map(|packet| Self { packet })
    }

    /// Finalizes the packet.
    pub fn build(self) {
        self.packet.reserved = U32::new(0);
    }
}

/// A builder for creating NDP Router Advertisement packets with SLLA and PIO
/// options.
pub struct RouterAdvertisementBuilder<'a> {
    buffer: &'a mut [u8],
}

impl<'a> RouterAdvertisementBuilder<'a> {
    /// Creates a new builder from a mutable byte slice.
    pub fn new(buf: &'a mut [u8]) -> Option<Self> {
        if buf.len() < 12 {
            return None;
        }
        Some(Self { buffer: buf })
    }

    /// Builds the Router Advertisement packet, optionally appending the RDNSS
    /// option. Returns the number of bytes written (52 or 76).
    #[allow(clippy::too_many_arguments)]
    pub fn build(
        self,
        cur_hop_limit: u8,
        flags: u8,
        router_lifetime: u16,
        reachable_time: u32,
        retrans_timer: u32,
        gateway_mac: [u8; 6],
        prefix: [u8; 16],
        prefix_len: u8,
        dns_server: Option<[u8; 16]>,
    ) -> Option<usize> {
        let required_len = if dns_server.is_some() { 76 } else { 52 };
        if self.buffer.len() < required_len {
            return None;
        }

        let (ra_slice, rest) = self.buffer.split_at_mut(12);
        let ra = RouterAdvertisement::mut_from_bytes(ra_slice).ok()?;
        ra.cur_hop_limit = cur_hop_limit;
        ra.flags = flags;
        ra.router_lifetime = U16::new(router_lifetime);
        ra.reachable_time = U32::new(reachable_time);
        ra.retrans_timer = U32::new(retrans_timer);

        let (slla_slice, pio_and_rdnss) = rest.split_at_mut(8);
        let slla = SourceLinkLayerAddressOption::mut_from_bytes(slla_slice).ok()?;
        slla.option_type = ndp_option::SOURCE_LINK_LAYER_ADDR;
        slla.length = 1;
        slla.addr = gateway_mac;

        let (pio_slice, rdnss_slice) = pio_and_rdnss.split_at_mut(32);
        let pio = PrefixInformationOption::mut_from_bytes(pio_slice).ok()?;
        pio.option_type = ndp_option::PREFIX_INFORMATION;
        pio.length = 4;
        pio.prefix_length = prefix_len;
        pio.flags = 0b11000000; // On-link (L) and Autonomous (A)
        pio.valid_lifetime = U32::new(2592000);
        pio.preferred_lifetime = U32::new(604800);
        pio.reserved = U32::new(0);
        pio.prefix = prefix;

        if let Some(dns_addr) = dns_server {
            let rdnss = RdnssOption::mut_from_bytes(rdnss_slice.get_mut(..24)?).ok()?;
            rdnss.option_type = ndp_option::RDNSS;
            rdnss.length = 3;
            rdnss.reserved = U16::new(0);
            // Note: RFC 8106 suggests RDNSS lifetime should be at least
            // MaxRtrAdvInterval. As a builder shortcut, we set it to match
            // the router lifetime.
            rdnss.lifetime = U32::new(router_lifetime as u32);
            rdnss.dns_servers = [dns_addr];
            Some(76)
        } else {
            Some(52)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_neighbor_solicitation_builder() {
        let mut buf = [0u8; std::mem::size_of::<NeighborSolicitation>()];
        let target = [0xfe, 0x80, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1];
        let builder = NeighborSolicitationBuilder::new(&mut buf).unwrap();
        builder.target_addr(target).build();

        let packet = NeighborSolicitation::parse(&buf).unwrap();
        assert_eq!(packet.target_addr, target);
    }

    #[test]
    fn test_neighbor_advertisement_builder() {
        let mut buf = [0u8; std::mem::size_of::<NeighborAdvertisement>()];
        let target = [0xfe, 0x80, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2];
        let builder = NeighborAdvertisementBuilder::new(&mut buf).unwrap();
        builder.flags(0b01100000).target_addr(target).build();

        let packet = NeighborAdvertisement::parse(&buf).unwrap();
        assert_eq!(packet.flags, 0b01100000);
        assert_eq!(packet.target_addr, target);
    }

    #[test]
    fn test_router_solicitation_builder() {
        let mut buf = [0u8; std::mem::size_of::<RouterSolicitation>()];
        let builder = RouterSolicitationBuilder::new(&mut buf).unwrap();
        builder.build();

        let packet = RouterSolicitation::parse(&buf).unwrap();
        assert_eq!(packet.reserved.get(), 0);
    }

    #[test]
    fn test_router_advertisement_builder() {
        let mut buf = [0u8; 52];
        let builder = RouterAdvertisementBuilder::new(&mut buf).unwrap();
        let prefix = [0xfe, 0x80, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        let gateway_mac = [0xde, 0xad, 0xbe, 0xef, 0x12, 0x35];
        let len = builder.build(64, 0, 1800, 0, 0, gateway_mac, prefix, 64, None).unwrap();
        assert_eq!(len, 52);

        let ra = RouterAdvertisement::parse(&buf[..12]).unwrap();
        assert_eq!(ra.cur_hop_limit, 64);
        assert_eq!(ra.router_lifetime.get(), 1800);

        let slla = SourceLinkLayerAddressOption::mut_from_bytes(&mut buf[12..20]).unwrap();
        assert_eq!(slla.option_type, 1);
        assert_eq!(slla.addr, gateway_mac);

        let pio = PrefixInformationOption::mut_from_bytes(&mut buf[20..52]).unwrap();
        assert_eq!(pio.option_type, 3);
        assert_eq!(pio.prefix_length, 64);
        assert_eq!(pio.prefix, prefix);
    }

    #[test]
    fn test_router_advertisement_builder_with_rdnss() {
        let mut buf = [0u8; 76];
        let builder = RouterAdvertisementBuilder::new(&mut buf).unwrap();
        let prefix = [0xfe, 0x80, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        let gateway_mac = [0xde, 0xad, 0xbe, 0xef, 0x12, 0x35];
        let dns_server = [0xfe, 0x80, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2];
        let len =
            builder.build(64, 0, 1800, 0, 0, gateway_mac, prefix, 64, Some(dns_server)).unwrap();
        assert_eq!(len, 76);

        let ra = RouterAdvertisement::parse(&buf[..12]).unwrap();
        assert_eq!(ra.cur_hop_limit, 64);
        assert_eq!(ra.router_lifetime.get(), 1800);

        let slla = SourceLinkLayerAddressOption::mut_from_bytes(&mut buf[12..20]).unwrap();
        assert_eq!(slla.option_type, 1);
        assert_eq!(slla.addr, gateway_mac);

        let pio = PrefixInformationOption::mut_from_bytes(&mut buf[20..52]).unwrap();
        assert_eq!(pio.option_type, 3);
        assert_eq!(pio.prefix_length, 64);
        assert_eq!(pio.prefix, prefix);

        let rdnss = RdnssOption::mut_from_bytes(&mut buf[52..76]).unwrap();
        assert_eq!(rdnss.option_type, 25);
        assert_eq!(rdnss.length, 3);
        assert_eq!(rdnss.lifetime.get(), 1800);
        assert_eq!(rdnss.dns_servers, [dns_server]);
    }
}
