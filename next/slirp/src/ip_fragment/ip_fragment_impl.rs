// Copyright 2023-2025 The Android Open Source Project
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

//! IP fragmentation and reassembly.

use std::collections::HashMap;

use netsim_packets::Ipv4Header;
use zerocopy::{FromBytes, IntoBytes};

pub const MTU: usize = 1500;

pub fn fragment(packet: &[u8]) -> Vec<Vec<u8>> {
    let (ipv4_header, payload) = Ipv4Header::parse(packet).unwrap();
    let ipv4_header_len = ipv4_header.header_length();
    let max_payload_len = (MTU - ipv4_header_len) & !7;

    let mut fragments = Vec::new();
    let mut offset = 0;

    while offset < payload.len() {
        let fragment_len = std::cmp::min(max_payload_len, payload.len() - offset);
        let mut fragment_packet = vec![0u8; ipv4_header_len + fragment_len];
        let (fragment_ipv4_header_slice, fragment_payload) =
            fragment_packet.split_at_mut(ipv4_header_len);
        let fragment_ipv4_header = Ipv4Header::mut_from_bytes(fragment_ipv4_header_slice).unwrap();

        fragment_ipv4_header.as_mut_bytes().copy_from_slice(ipv4_header.as_bytes());
        let Some(payload_slice) = payload.get(offset..offset + fragment_len) else {
            break;
        };
        fragment_payload.copy_from_slice(payload_slice);

        let mut flags_fragment_offset = ipv4_header.flags_fragment_offset.get();
        if offset + fragment_len < payload.len() {
            flags_fragment_offset |= 0x2000; // More fragments
        }
        flags_fragment_offset |= (offset / 8) as u16;
        fragment_ipv4_header.flags_fragment_offset.set(flags_fragment_offset);

        fragment_ipv4_header.total_length.set((ipv4_header_len + fragment_len) as u16);
        fragment_ipv4_header.header_checksum.set(0);
        // The checksum will be calculated by the caller.

        fragments.push(fragment_packet);
        offset += fragment_len;
    }

    fragments
}

struct FragmentCache {
    fragments: Vec<Vec<u8>>,
    total_len: usize,
    last_fragment_received: bool,
    last_accessed_tick: u64,
}

impl FragmentCache {
    fn new() -> Self {
        Self {
            fragments: Vec::new(),
            total_len: 0,
            last_fragment_received: false,
            last_accessed_tick: 0,
        }
    }

    fn is_complete(&self) -> bool {
        if !self.last_fragment_received {
            return false;
        }

        let mut fragments = self.fragments.clone();
        fragments.sort_by_key(|a| {
            let (h, _) = Ipv4Header::parse(a).expect("fragment should be valid IPv4");
            h.flags_fragment_offset.get() & 0x1FFF
        });

        let mut current_offset = 0;
        for fragment in &fragments {
            let (header, payload) =
                Ipv4Header::parse(fragment).expect("fragment should be valid IPv4");
            let offset = (header.flags_fragment_offset.get() & 0x1FFF) * 8;
            if offset as usize != current_offset {
                return false;
            }
            current_offset += payload.len();
        }
        if current_offset != self.total_len {
            return false;
        }
        true
    }
}

#[derive(Default)]
pub struct Reassembler {
    cache: HashMap<u16, FragmentCache>,
    access_counter: u64,
}

impl Reassembler {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn reassemble(&mut self, fragment: &[u8]) -> Option<Vec<u8>> {
        let (ipv4_header, payload) = Ipv4Header::parse(fragment)?;
        let identification = ipv4_header.identification.get();
        let more_fragments = ipv4_header.flags_fragment_offset.get() & 0x2000 != 0;

        self.access_counter = self.access_counter.wrapping_add(1);
        let current_tick = self.access_counter;

        const MAX_REASSEMBLY_ENTRIES: usize = 64;
        const MAX_FRAGMENTS_PER_PACKET: usize = 128;

        let limit_exceeded = self
            .cache
            .get(&identification)
            .map(|entry| entry.fragments.len() >= MAX_FRAGMENTS_PER_PACKET)
            .unwrap_or(false);

        if limit_exceeded {
            self.cache.remove(&identification);
            return None;
        }

        crate::utils::lru::evict_lru(
            &mut self.cache,
            MAX_REASSEMBLY_ENTRIES,
            &identification,
            |entry| entry.last_accessed_tick,
        );

        let cache_entry = self.cache.entry(identification).or_insert_with(FragmentCache::new);
        cache_entry.last_accessed_tick = current_tick;
        cache_entry.fragments.push(fragment.to_vec());

        if !more_fragments {
            let offset = (ipv4_header.flags_fragment_offset.get() & 0x1FFF) * 8;
            cache_entry.total_len = offset as usize + payload.len();
            cache_entry.last_fragment_received = true;
        }

        if cache_entry.is_complete() {
            let cache_entry = self.cache.remove(&identification)?;
            let mut fragments = cache_entry.fragments;
            fragments.sort_by_key(|a| {
                let (h, _) =
                    Ipv4Header::parse(a).expect("fragment in cache must be valid IPv4 header");
                h.flags_fragment_offset.get() & 0x1FFF
            });

            let first_frag = fragments.first()?;
            let (first_header, _) = Ipv4Header::parse(first_frag)?;
            let header_len = first_header.header_length();
            let total_len = header_len + cache_entry.total_len;
            let mut reassembled = vec![0u8; total_len];

            reassembled.get_mut(..header_len)?.copy_from_slice(first_frag.get(..header_len)?);

            for frag in &fragments {
                let (header, payload) = Ipv4Header::parse(frag)?;
                let offset = (header.flags_fragment_offset.get() & 0x1FFF) * 8;
                let start = header_len + offset as usize;
                reassembled.get_mut(start..start + payload.len())?.copy_from_slice(payload);
            }

            let reassembled_header =
                Ipv4Header::mut_from_bytes(reassembled.get_mut(..header_len)?).ok()?;
            reassembled_header.total_length.set(total_len as u16);
            reassembled_header.flags_fragment_offset.set(0);
            reassembled_header.header_checksum.set(0);

            return Some(reassembled);
        }

        None
    }
}
