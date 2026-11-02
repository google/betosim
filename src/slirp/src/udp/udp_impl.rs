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

use std::{collections::HashMap, net::SocketAddr};

use bytes::Bytes;
use serde::{Deserialize, Serialize};

use crate::{ConnectionArgs, SlirpResponse, UdpConnectionArgs, UdpConnectionInfo};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct UdpFlow {
    pub guest_addr: SocketAddr,
    pub original_dest: SocketAddr,
    #[serde(skip)]
    pub last_accessed_tick: u64,
}

/// Manages UDP flows.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UdpManager {
    // Maps guest_addr -> (conn_id, real_dest_addr)
    pub(crate) flows: HashMap<SocketAddr, (u64, SocketAddr)>,
    // Maps conn_id -> UdpFlow
    pub(crate) id_to_flow: HashMap<u64, UdpFlow>,
    pub(crate) next_flow_id: u64,
    pub(crate) access_counter: u64,
}

impl Default for UdpManager {
    fn default() -> Self {
        Self::new()
    }
}

impl UdpManager {
    pub fn new() -> Self {
        Self {
            flows: HashMap::new(),
            id_to_flow: HashMap::new(),
            next_flow_id: 1 << 63, // Start UDP IDs with MSB set
            access_counter: 0,
        }
    }

    pub fn get_connections(&self) -> impl Iterator<Item = UdpConnectionInfo> + '_ {
        self.flows.iter().map(|(guest_addr, (_, real_dest))| UdpConnectionInfo {
            local_addr: *guest_addr,
            peer_addr: *real_dest,
        })
    }

    /// Handles an incoming UDP packet from the guest.
    pub fn handle_packet(
        &mut self,
        responses: &mut Vec<SlirpResponse>,
        packet: &[u8],
        source: SocketAddr,
        dest: SocketAddr,
        redirect_dest: Option<SocketAddr>,
    ) {
        const MAX_UDP_FLOWS: usize = 1024;
        self.access_counter = self.access_counter.wrapping_add(1);
        let current_tick = self.access_counter;

        if !self.flows.contains_key(&source) && self.flows.len() >= MAX_UDP_FLOWS {
            // Evict oldest LRU flow by min last_accessed_tick (b/560171321)
            if let Some((&evict_id, evict_flow)) =
                self.id_to_flow.iter().min_by_key(|(_, flow)| flow.last_accessed_tick)
            {
                let guest_addr = evict_flow.guest_addr;
                self.flows.remove(&guest_addr);
                responses.push(SlirpResponse::CloseConnection { conn_id: evict_id, guest_addr });
                self.id_to_flow.remove(&evict_id);
            }
        }

        let real_dest = redirect_dest.unwrap_or(dest);
        let (id, _) = self.flows.entry(source).or_insert_with(|| {
            let id = self.next_flow_id;
            self.next_flow_id += 1;
            let conn_info = UdpConnectionArgs {
                destination: real_dest,
                guest_ip: source.ip(),
                guest_port: source.port(),
            };
            responses.push(SlirpResponse::EstablishConnection(id, ConnectionArgs::Udp(conn_info)));

            self.id_to_flow.insert(
                id,
                UdpFlow {
                    guest_addr: source,
                    original_dest: dest,
                    last_accessed_tick: current_tick,
                },
            );

            (id, real_dest)
        });

        // Fast-path O(1) LRU tick update (zero allocations, zero memory moves)
        if let Some(flow) = self.id_to_flow.get_mut(id) {
            flow.last_accessed_tick = current_tick;
        }

        responses.push(SlirpResponse::WriteToConnection(*id, Bytes::copy_from_slice(packet)));
    }

    /// Explicitly closes a UDP flow by ID.
    pub fn remove_flow(&mut self, conn_id: u64) {
        if let Some(flow) = self.id_to_flow.remove(&conn_id) {
            self.flows.remove(&flow.guest_addr);
        }
    }

    /// Translates a host reply and returns the original destination (to be used
    /// as source), the guest address (destination), and the payload.
    pub fn handle_reply(
        &self,
        conn_id: u64,
        data: &[u8],
    ) -> Option<(SocketAddr, SocketAddr, Vec<u8>)> {
        self.id_to_flow
            .get(&conn_id)
            .map(|flow| (flow.original_dest, flow.guest_addr, data.to_vec()))
    }
}
