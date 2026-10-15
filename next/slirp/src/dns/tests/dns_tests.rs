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

use std::{
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    time::{Duration, Instant},
};

use crate::{
    Config,
    dns::{
        DnsProxy, DnsQueryResult, create_dns_search_option, discover_host_dns_servers,
        encode_dns_search_list, parse_resolv_conf_content,
    },
};

fn build_dns_query_bytes(tx_id: u16, name: &str, qtype: u16) -> Vec<u8> {
    let mut buf = vec![0u8; 512];
    let mut builder = netsim_packets::DnsPacketBuilder::new(&mut buf).unwrap();
    builder.transaction_id(tx_id);
    builder.flags(0x0100); // Recursion Desired
    builder.add_question(name, qtype, 1); // class IN
    let len = builder.build();
    buf.truncate(len);
    buf
}

fn build_dns_reply_bytes(tx_id: u16, name: &str, qtype: u16, ttl: u32, ip: IpAddr) -> Vec<u8> {
    let query = build_dns_query_bytes(tx_id, name, qtype);
    let mut reply = query.clone();
    let reply_flags: u16 = 0x8180; // Response, NoError
    reply[2..4].copy_from_slice(&reply_flags.to_be_bytes());
    reply[6..8].copy_from_slice(&1u16.to_be_bytes()); // answers = 1

    reply.extend_from_slice(&[0xC0, 0x0C]); // Pointer to name
    reply.extend_from_slice(&qtype.to_be_bytes());
    reply.extend_from_slice(&1u16.to_be_bytes()); // Class IN
    reply.extend_from_slice(&ttl.to_be_bytes());

    match ip {
        IpAddr::V4(ipv4) => {
            reply.extend_from_slice(&4u16.to_be_bytes()); // RdLength
            reply.extend_from_slice(&ipv4.octets());
        }
        IpAddr::V6(ipv6) => {
            reply.extend_from_slice(&16u16.to_be_bytes()); // RdLength
            reply.extend_from_slice(&ipv6.octets());
        }
    }
    reply
}

#[test]
fn test_dns_proxy_cache_hit_and_expiry() {
    let mut proxy = DnsProxy::new();
    let now = Instant::now();
    let client: SocketAddr = "10.0.2.15:12345".parse().unwrap();
    let servers = vec!["8.8.8.8".parse().unwrap()];

    // 1. Initial query -> Cache Miss, should Forward
    let query = build_dns_query_bytes(0x1234, "google.com", 1); // A query
    let res = proxy.handle_query(&query, client, now, &servers);
    assert!(matches!(res, DnsQueryResult::Forward(ip) if ip == servers[0]));

    // 2. Simulate upstream reply -> Cache it!
    let reply = build_dns_reply_bytes(0x1234, "google.com", 1, 300, "1.2.3.4".parse().unwrap());
    proxy.handle_reply(&reply, now);

    // 3. Query again -> Cache Hit!
    let res2 = proxy.handle_query(&query, client, now, &servers);
    let cached_payload = match res2 {
        DnsQueryResult::Cached(payload) => payload,
        _ => panic!("Expected Cache Hit"),
    };

    // Verify synthesized reply
    assert!(cached_payload.len() > query.len());
    let tx_id = u16::from_be_bytes([cached_payload[0], cached_payload[1]]);
    assert_eq!(tx_id, 0x1234);
    let flags = u16::from_be_bytes([cached_payload[2], cached_payload[3]]);
    assert_eq!(flags & 0x8000, 0x8000); // Response bit set
    let num_answers = u16::from_be_bytes([cached_payload[6], cached_payload[7]]);
    assert_eq!(num_answers, 1);

    // 4. Advance time by 299 seconds -> Still valid!
    let now_299 = now + Duration::from_secs(299);
    let res3 = proxy.handle_query(&query, client, now_299, &servers);
    assert!(matches!(res3, DnsQueryResult::Cached(_)));

    // 5. Advance time by 301 seconds -> Expired!
    let now_301 = now + Duration::from_secs(301);
    let res4 = proxy.handle_query(&query, client, now_301, &servers);
    assert!(matches!(res4, DnsQueryResult::Forward(_)));
}

#[test]
fn test_dns_proxy_failover() {
    let mut proxy = DnsProxy::new();
    let mut now = Instant::now();
    let client: SocketAddr = "10.0.2.15:12345".parse().unwrap();
    let servers = vec!["8.8.8.8".parse().unwrap(), "1.1.1.1".parse().unwrap()];

    let query = build_dns_query_bytes(0x1234, "google.com", 1);

    // 1. First query -> Forward to server 0
    let res1 = proxy.handle_query(&query, client, now, &servers);
    assert!(matches!(res1, DnsQueryResult::Forward(ip) if ip == servers[0]));

    // 2. Retry immediately (0.5s later) -> Failover to server 1!
    now += Duration::from_millis(500);
    let res2 = proxy.handle_query(&query, client, now, &servers);
    assert!(matches!(res2, DnsQueryResult::Forward(ip) if ip == servers[1]));

    // 3. Retry again (0.5s later) -> Cycle back to server 0!
    now += Duration::from_millis(500);
    let res3 = proxy.handle_query(&query, client, now, &servers);
    assert!(matches!(res3, DnsQueryResult::Forward(ip) if ip == servers[0]));

    // 4. New query (3s later, no retry) -> Reset to server 0!
    now += Duration::from_secs(3);
    let res4 = proxy.handle_query(&query, client, now, &servers);
    assert!(matches!(res4, DnsQueryResult::Forward(ip) if ip == servers[0]));
}

#[test]
fn test_parse_resolv_conf_standard() {
    let resolv_conf = r#"
# Dynamic resolv.conf file for glibc resolver
nameserver 8.8.8.8
nameserver 2001:4860:4860::8888
search corp.example.com example.com
options edns0 trust-ad
"#;
    let servers = parse_resolv_conf_content(resolv_conf);
    assert_eq!(
        servers,
        vec![
            IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8)),
            IpAddr::V6(Ipv6Addr::new(0x2001, 0x4860, 0x4860, 0, 0, 0, 0, 0x8888)),
        ]
    );
}

#[test]
fn test_parse_resolv_conf_ipv6_scope_id() {
    let resolv_conf = r#"
; Resolv.conf with IPv6 scope ID
nameserver fe80::1%eth0
nameserver fe80::2%wlan0
"#;
    let servers = parse_resolv_conf_content(resolv_conf);
    assert_eq!(
        servers,
        vec![
            IpAddr::V6(Ipv6Addr::new(0xfe80, 0, 0, 0, 0, 0, 0, 1)),
            IpAddr::V6(Ipv6Addr::new(0xfe80, 0, 0, 0, 0, 0, 0, 2)),
        ]
    );
}

#[test]
fn test_parse_resolv_conf_skips_loopback_and_unspecified() {
    let resolv_conf = r#"
# Loopback and unspecified addresses should be filtered
nameserver 127.0.0.53
nameserver 127.0.0.1
nameserver ::1
nameserver 0.0.0.0
nameserver ::
nameserver 1.1.1.1
"#;
    let servers = parse_resolv_conf_content(resolv_conf);
    assert_eq!(servers, vec![IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1))]);
}

#[test]
fn test_parse_resolv_conf_deduplication() {
    let resolv_conf = r#"
nameserver 8.8.8.8
nameserver 8.8.4.4
nameserver 8.8.8.8
"#;
    let servers = parse_resolv_conf_content(resolv_conf);
    assert_eq!(
        servers,
        vec![IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8)), IpAddr::V4(Ipv4Addr::new(8, 8, 4, 4)),]
    );
}

#[test]
fn test_encode_dns_search_list() {
    let domains = vec!["corp.example.com".to_string(), "example.com".to_string()];
    let encoded = encode_dns_search_list(&domains);

    let mut expected = Vec::new();
    // "corp.example.com" -> 4 "corp" 7 "example" 3 "com" 0
    expected.push(4u8);
    expected.extend_from_slice(b"corp");
    expected.push(7u8);
    expected.extend_from_slice(b"example");
    expected.push(3u8);
    expected.extend_from_slice(b"com");
    expected.push(0u8);
    // "example.com" -> 7 "example" 3 "com" 0
    expected.push(7u8);
    expected.extend_from_slice(b"example");
    expected.push(3u8);
    expected.extend_from_slice(b"com");
    expected.push(0u8);

    assert_eq!(encoded, expected);
}

#[test]
fn test_create_dns_search_option() {
    let no_search_config = Config::default();
    assert_eq!(create_dns_search_option(&no_search_config), None);

    let search_config =
        Config { dns_search: Some(vec!["example.com".to_string()]), ..Default::default() };
    let option = create_dns_search_option(&search_config).expect("Option 119 should be created");
    assert_eq!(option[0], 119); // Option code
    let len = option[1] as usize;
    assert_eq!(option.len(), 2 + len);
}

#[tokio::test]
async fn test_discover_host_dns_servers_async() {
    let servers = discover_host_dns_servers().await;
    // Host discovery should always return at least one non-empty address (real
    // or localhost fallback)
    assert!(!servers.is_empty());
    for server in &servers {
        assert!(!server.is_unspecified());
    }
}
