// Copyright 2024 The Android Open Source Project
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

/// This module provides a reverse-dns function that caches the domain
/// name (FQDNs) and IpAddr from DNS answer records.
///
/// This manager exists for two reasons:
///
/// 1. RFC2817 Compliance (b/37055721): Requires converting IP address to
/// hostname for HTTP CONNECT requests.
///
/// 2. Proxy bypass/exclusion list requires matching on host name
/// patterns.
///
use crate::dns;
use etherparse::{PacketHeaders, PayloadSlice, TransportHeader};
use log::debug;
use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Mutex;

/// DNS Manager of IP addresses to FQDN
pub struct DnsManager {
    map: Mutex<HashMap<IpAddr, String>>,
}

impl Default for DnsManager {
    fn default() -> Self {
        Self::new()
    }
}

impl DnsManager {
    const DNS_PORT: u16 = 53;

    /// Creates a new `DnsManager`.
    pub fn new() -> Self {
        DnsManager { map: Mutex::new(HashMap::new()) }
    }

    /// Add potential DNS entries to the cache.
    pub fn add_from_packet_headers(&self, headers: &PacketHeaders) {
        // Check if the packet contains a UDP header
        // with source port from DNS server
        // and DNS answers with A/AAAA records
        if let Some(TransportHeader::Udp(udp_header)) = &headers.transport {
            // with source port from DNS server
            if udp_header.source_port == Self::DNS_PORT {
                if let PayloadSlice::Udp(payload) = headers.payload {
                    // Add any A/AAAA domain names
                    if let Ok(answers) = dns::parse_answers(payload) {
                        for (ip_addr, name) in answers {
                            self.map.lock().unwrap().insert(ip_addr, name.clone());
                            debug!("Added {} ({}) to DNS cache", name, ip_addr);
                        }
                    }
                }
            }
        }
    }

    /// Adds potential DNS entries from an Ethernet slice.
    pub fn add_from_ethernet_slice(&self, packet: &[u8]) {
        let headers = PacketHeaders::from_ethernet_slice(packet).unwrap();
        self.add_from_packet_headers(&headers);
    }

    /// Return a FQDN from a prior DNS response for ip address
    pub fn get(&self, ip_addr: &IpAddr) -> Option<String> {
        self.map.lock().unwrap().get(ip_addr).cloned()
    }

    /// Returns the number of entries in the cache.
    pub fn len(&self) -> usize {
        self.map.lock().unwrap().len()
    }

    /// Checks if the cache is empty.
    pub fn is_empty(&self) -> bool {
        self.map.lock().unwrap().len() == 0
    }
}
