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

mod ip_fragment_impl;
#[cfg(test)]
mod tests;

use bytes::Bytes;
pub use ip_fragment_impl::{MTU, Reassembler, fragment};
use netsim_packets::Ipv4Header;
use zerocopy::FromBytes;

use crate::{
    SlirpResponse,
    packet::{IpPacket, NetworkPacket, ParsedPacket},
};

/// Post-processes a list of SlirpResponses, fragmenting any outgoing IP packets
/// that exceed the MTU.
pub fn fragment_outgoing_packets(responses: &mut Vec<SlirpResponse>) {
    let mut i = 0;
    while i < responses.len() {
        let parsed = match responses.get(i) {
            Some(SlirpResponse::Packet(packet_bytes)) => ParsedPacket::parse(packet_bytes)
                .and_then(|p| {
                    let eth_header_len = match p.ethernet {
                        netsim_packets::EthernetPacket::Untagged { .. } => 14,
                        netsim_packets::EthernetPacket::Vlan { .. } => 18,
                    };
                    match p.network {
                        Some(NetworkPacket::Ip(IpPacket::V4(ref ip_header, _))) => {
                            Some((packet_bytes, eth_header_len, ip_header.header_length()))
                        }
                        _ => None,
                    }
                }),
            _ => None,
        };
        if let Some((packet_bytes, eth_header_len, ip_header_len)) = parsed {
            let Some((eth_header, ip_packet)) = packet_bytes.split_at_checked(eth_header_len)
            else {
                i += 1;
                continue;
            };

            if ip_packet.len() > MTU {
                log::trace!("Fragmenting outgoing packet of size {}", packet_bytes.len());

                let ip_fragments = fragment(ip_packet);

                let mut eth_fragments = Vec::with_capacity(ip_fragments.len());
                for ip_frag in ip_fragments {
                    let mut eth_frag = Vec::with_capacity(eth_header_len + ip_frag.len());
                    eth_frag.extend_from_slice(eth_header);
                    eth_frag.extend_from_slice(&ip_frag);

                    let Some(ip_header_slice) =
                        eth_frag.get_mut(eth_header_len..eth_header_len + ip_header_len)
                    else {
                        continue;
                    };
                    let Ok(mut_ip_header) = Ipv4Header::mut_from_bytes(ip_header_slice) else {
                        continue;
                    };
                    mut_ip_header.update_checksum();

                    eth_fragments.push(SlirpResponse::Packet(Bytes::copy_from_slice(&eth_frag)));
                }

                let num_frags = eth_fragments.len();
                responses.splice(i..=i, eth_fragments);
                i += num_frags;
                continue;
            }
        }
        i += 1;
    }
}
