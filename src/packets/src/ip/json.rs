// Copyright 2025 The Android Open Source Project
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

use std::{
    collections::BTreeMap,
    net::{Ipv4Addr, Ipv6Addr},
};

use crate::ip::{Ipv4Header, Ipv6Header};

fn format_ipv4_addr(addr: &[u8; 4]) -> String {
    Ipv4Addr::from(*addr).to_string()
}

fn format_ipv6_addr(addr: &[u8; 16]) -> String {
    let ipv6 = Ipv6Addr::from(*addr);
    ipv6.to_string()
}

pub fn ipv4_to_json(ipv4_packet: &Ipv4Header, _ipv4_payload: &[u8]) -> BTreeMap<String, String> {
    let mut ip = BTreeMap::new();
    ip.insert("ip.version".to_string(), format!("{}", ipv4_packet.version()));
    ip.insert("ip.hdr_len".to_string(), format!("{}", ipv4_packet.ihl() * 4));
    ip.insert("ip.ttl".to_string(), format!("{}", ipv4_packet.ttl));
    ip.insert("ip.proto".to_string(), format!("{}", ipv4_packet.protocol));
    ip.insert("ip.src".to_string(), format_ipv4_addr(&ipv4_packet.source_addr));
    ip.insert("ip.dst".to_string(), format_ipv4_addr(&ipv4_packet.dest_addr));
    ip
}

pub fn ipv6_to_json(ipv6_packet: &Ipv6Header, _ipv6_payload: &[u8]) -> BTreeMap<String, String> {
    let mut ipv6 = BTreeMap::new();
    ipv6.insert("ipv6.version".to_string(), format!("{}", ipv6_packet.version()));
    ipv6.insert("ip.version".to_string(), format!("{}", ipv6_packet.version()));
    ipv6.insert("ipv6.nxt".to_string(), format!("{}", ipv6_packet.next_header));
    ipv6.insert("ipv6.hlim".to_string(), format!("{}", ipv6_packet.hop_limit));
    ipv6.insert("ipv6.src".to_string(), format_ipv6_addr(&ipv6_packet.source_addr));
    ipv6.insert("ipv6.dst".to_string(), format_ipv6_addr(&ipv6_packet.dest_addr));
    ipv6
}

#[cfg(test)]
mod tests {
    use zerocopy::byteorder::{U16, U32};

    use super::*;
    use crate::ip::Ipv4Header;

    #[test]
    fn test_ipv4_to_json() {
        let header = Ipv4Header {
            version_ihl: 0x45,
            dscp_ecn: 0,
            total_length: U16::new(20),
            identification: U16::new(0),
            flags_fragment_offset: U16::new(0),
            ttl: 64,
            protocol: 6,
            header_checksum: U16::new(0),
            source_addr: [127, 0, 0, 1],
            dest_addr: [192, 168, 1, 1],
        };
        let json_map = ipv4_to_json(&header, &[]);
        let value = serde_json::to_value(json_map).unwrap();
        assert_eq!(value["ip.version"], "4");
        assert_eq!(value["ip.hdr_len"], "20");
        assert_eq!(value["ip.ttl"], "64");
        assert_eq!(value["ip.proto"], "6");
        assert_eq!(value["ip.src"], "127.0.0.1");
        assert_eq!(value["ip.dst"], "192.168.1.1");
    }

    #[test]
    fn test_ipv6_to_json() {
        let header = Ipv6Header {
            version_tc_fl: U32::new(0x60000000),
            payload_length: U16::new(0),
            next_header: 58,
            hop_limit: 64,
            source_addr: [0x20, 0x01, 0x0d, 0xb8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1],
            dest_addr: [0; 16],
        };
        let json_map = ipv6_to_json(&header, &[]);
        let value = serde_json::to_value(json_map).unwrap();
        assert_eq!(value["ipv6.version"], "6");
        assert_eq!(value["ip.version"], "6");
        assert_eq!(value["ipv6.nxt"], "58");
        assert_eq!(value["ipv6.hlim"], "64");
        assert_eq!(value["ipv6.src"], "2001:db8::1");
        assert_eq!(value["ipv6.dst"], "::");
    }
}
