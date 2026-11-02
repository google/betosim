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

use std::{collections::HashMap, net::Ipv6Addr};

use netsim_packets::{
    MacAddr, NeighborAdvertisement, NeighborAdvertisementBuilder, NeighborSolicitation,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct NdpEntry {
    pub mac: MacAddr,
    #[serde(skip)]
    pub last_accessed_tick: u64,
}

/// Handles Neighbor Discovery Protocol (NDP) for IPv6.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NdpTable {
    pub(crate) cache: HashMap<Ipv6Addr, NdpEntry>,
    pub(crate) access_counter: u64,
}

impl Default for NdpTable {
    fn default() -> Self {
        Self::new()
    }
}

impl NdpTable {
    pub fn new() -> Self {
        Self { cache: HashMap::new(), access_counter: 0 }
    }

    /// Adds or updates an entry in the NDP table with bounded size limit and
    /// LRU eviction.
    pub fn add_entry(&mut self, ip: Ipv6Addr, mac: MacAddr) {
        const MAX_NDP_ENTRIES: usize = 512;
        self.access_counter = self.access_counter.wrapping_add(1);
        let current_tick = self.access_counter;

        crate::utils::lru::evict_lru(&mut self.cache, MAX_NDP_ENTRIES, &ip, |entry| {
            entry.last_accessed_tick
        });

        self.cache.insert(ip, NdpEntry { mac, last_accessed_tick: current_tick });
    }

    /// Looks up a MAC address for the given IPv6 address.
    pub fn lookup(&self, ip: &Ipv6Addr) -> Option<MacAddr> {
        self.cache.get(ip).map(|entry| entry.mac)
    }

    /// Handles an incoming Neighbor Solicitation packet.
    ///
    /// If the packet is a request for an IP address owned by this
    /// table, it will generate and return a Neighbor Advertisement reply
    /// packet.
    pub fn handle_packet(&mut self, packet: &NeighborSolicitation) -> Option<Vec<u8>> {
        let target_addr = Ipv6Addr::from(packet.target_addr);
        if self.lookup(&target_addr).is_some() {
            // This is a request for an IP we own. Generate a reply.
            let mut reply_buf = [0u8; std::mem::size_of::<NeighborAdvertisement>()];
            let builder = NeighborAdvertisementBuilder::new(&mut reply_buf)?;
            builder
                .flags(0b01100000) // Router, Solicited, Override
                .target_addr(target_addr.octets())
                .build();
            return Some(reply_buf.to_vec());
        }
        None
    }
}
