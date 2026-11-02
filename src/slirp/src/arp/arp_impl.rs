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

use std::{collections::HashMap, net::Ipv4Addr};

use netsim_packets::{ArpPacket, ArpPacketBuilder, MacAddr};
use serde::{Deserialize, Serialize};

/// Handles Address Resolution Protocol (ARP).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct ArpEntry {
    pub mac: MacAddr,
    #[serde(skip)]
    pub last_accessed_tick: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ArpTable {
    pub(crate) cache: HashMap<Ipv4Addr, ArpEntry>,
    pub(crate) access_counter: u64,
}

impl Default for ArpTable {
    fn default() -> Self {
        Self::new()
    }
}

impl ArpTable {
    pub fn new() -> Self {
        Self { cache: HashMap::new(), access_counter: 0 }
    }

    /// Adds or updates an entry in the ARP table with bounded size limit and
    /// LRU eviction.
    pub fn add_entry(&mut self, ip: Ipv4Addr, mac: MacAddr) {
        const MAX_ARP_ENTRIES: usize = 512;
        self.access_counter = self.access_counter.wrapping_add(1);
        let current_tick = self.access_counter;

        crate::utils::lru::evict_lru(&mut self.cache, MAX_ARP_ENTRIES, &ip, |entry| {
            entry.last_accessed_tick
        });

        self.cache.insert(ip, ArpEntry { mac, last_accessed_tick: current_tick });
    }

    /// Looks up a MAC address for the given IPv4 address.
    pub fn lookup(&self, ip: &Ipv4Addr) -> Option<MacAddr> {
        self.cache.get(ip).map(|entry| entry.mac)
    }

    /// Handles an incoming ARP packet.
    ///
    /// If the packet is a "who-has" request for an IP address owned by this
    /// table, it will generate and return an "is-at" reply packet.
    pub fn handle_packet(&mut self, packet: &ArpPacket) -> Option<Vec<u8>> {
        // We only care about ARP requests for IPv4 over Ethernet.
        if packet.hardware_type.get() != 1 || packet.protocol_type.get() != 0x0800 {
            return None;
        }

        // Record sender IP -> MAC mapping with bounded cache size & LRU
        let sender_ip = Ipv4Addr::from(packet.sender_protocol_addr);
        let sender_mac = packet.sender_hardware_addr;
        self.add_entry(sender_ip, sender_mac);

        // Opcode 1 is "who-has"
        if packet.opcode.get() == 1 {
            let target_ip = Ipv4Addr::from(packet.target_protocol_addr);
            if let Some(mac_addr) = self.lookup(&target_ip) {
                // This is a request for an IP we own. Generate a reply.
                let mut reply_buf = [0u8; 28];
                let builder = ArpPacketBuilder::new(&mut reply_buf)?;
                builder
                    .hardware_type(1)
                    .protocol_type(0x0800)
                    .hardware_addr_len(6)
                    .protocol_addr_len(4)
                    .opcode(2) // Reply
                    .sender_hardware_addr(mac_addr)
                    .sender_protocol_addr(target_ip.octets())
                    .target_hardware_addr(packet.sender_hardware_addr)
                    .target_protocol_addr(packet.sender_protocol_addr)
                    .build();
                return Some(reply_buf.to_vec());
            }
        }
        None
    }
}
