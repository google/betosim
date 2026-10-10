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

//! DNS Proxy and DHCP DNS option generation.

use std::{
    collections::HashMap,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    time::{Duration, Instant},
};

use log::{debug, info};
use netsim_packets::DnsHeader;
use zerocopy::FromBytes;

use crate::Config;

/// Encodes a list of domain names into a sequence of labels.
/// Each label is prefixed by its length, and each domain is terminated by a
/// zero-length label.
pub fn encode_dns_search_list(dns_search: &[String]) -> Vec<u8> {
    let mut dns_search_bytes = Vec::new();
    for domain in dns_search {
        for label in domain.split('.') {
            dns_search_bytes.push(label.len() as u8);
            dns_search_bytes.extend_from_slice(label.as_bytes());
        }
        dns_search_bytes.push(0);
    }
    dns_search_bytes
}

/// Creates a DHCPv4 option for the DNS search list (Option 119).
pub fn create_dns_search_option(config: &Config) -> Option<Vec<u8>> {
    config.dns_search.as_ref().map(|dns_search| {
        let dns_search_bytes = encode_dns_search_list(dns_search);
        let mut option = Vec::new();
        option.extend_from_slice(&[119, dns_search_bytes.len() as u8]);
        option.extend_from_slice(&dns_search_bytes);
        option
    })
}

#[derive(Hash, PartialEq, Eq, Clone, Debug)]
struct DnsCacheKey {
    name: String,
    qtype: u16,
}

#[derive(Debug)]
struct DnsCacheEntry {
    ip: IpAddr,
    ttl: u32,
    created_at: Instant,
}

#[derive(Hash, PartialEq, Eq, Clone, Debug)]
struct DnsRecentQueryKey {
    name: String,
    qtype: u16,
    client_addr: SocketAddr,
}

#[derive(Debug)]
struct DnsRecentQueryValue {
    last_sent_time: Instant,
    server_idx: usize,
}

pub enum DnsQueryResult {
    Cached(Vec<u8>), // Synthesized reply payload
    Forward(IpAddr), // Forward to this upstream DNS server
    Ignore,
}

pub struct DnsProxy {
    cache: HashMap<DnsCacheKey, DnsCacheEntry>,
    recent_queries: HashMap<DnsRecentQueryKey, DnsRecentQueryValue>,
}

impl Default for DnsProxy {
    fn default() -> Self {
        Self::new()
    }
}

impl DnsProxy {
    pub fn new() -> Self {
        Self { cache: HashMap::new(), recent_queries: HashMap::new() }
    }

    pub fn handle_query(
        &mut self,
        query_payload: &[u8],
        client_addr: SocketAddr,
        now: Instant,
        dns_servers: &[IpAddr],
    ) -> DnsQueryResult {
        let is_ipv6 = client_addr.is_ipv6();

        // Helper to get the default/configured server for the correct family
        let get_upstream_server = |servers: &[IpAddr]| {
            let server = servers.iter().find(|ip| ip.is_ipv6() == is_ipv6).copied();
            match (is_ipv6, server) {
                (true, Some(IpAddr::V6(ipv6))) => IpAddr::V6(ipv6),
                (true, _) => IpAddr::V6("2001:4860:4860::8888".parse().unwrap()),
                (false, Some(IpAddr::V4(ipv4))) => IpAddr::V4(ipv4),
                (false, _) => IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8)),
            }
        };

        // Filter servers to match the query family
        let family_servers: Vec<IpAddr> =
            dns_servers.iter().filter(|ip| ip.is_ipv6() == is_ipv6).copied().collect();

        // 1. Parse the question
        let Some((qname, qtype, _, flags)) = parse_dns_query(query_payload) else {
            // FALLBACK: If we cannot parse it, just forward to the
            // default/configured server!
            debug!("DNS Proxy: Failed to parse query, falling back to direct forwarding");
            return DnsQueryResult::Forward(get_upstream_server(dns_servers));
        };

        debug!("DNS Proxy: Query for name='{qname}', type={qtype} from {client_addr}");

        // 2. Check cache (only A and AAAA)
        if qtype == 1 || qtype == 28 {
            let key = DnsCacheKey { name: qname.clone(), qtype };
            if let Some(entry) = self.cache.get(&key) {
                let age = now.saturating_duration_since(entry.created_at);
                if age.as_secs() < entry.ttl as u64 {
                    let remaining_ttl = entry.ttl - age.as_secs() as u32;
                    debug!(
                        "DNS Proxy: Cache hit for '{}' -> {} (TTL={})",
                        qname, entry.ip, remaining_ttl
                    );
                    let Some(reply_payload) =
                        synthesize_dns_reply(query_payload, flags, qtype, remaining_ttl, entry.ip)
                    else {
                        return DnsQueryResult::Ignore;
                    };
                    return DnsQueryResult::Cached(reply_payload);
                } else {
                    debug!("DNS Proxy: Cache expired for '{qname}'");
                    self.cache.remove(&key);
                }
            }
        }

        // 3. Failover logic (reactive)
        if family_servers.is_empty() {
            // No servers of the correct family configured, use hardcoded
            // fallback
            return DnsQueryResult::Forward(get_upstream_server(dns_servers));
        }

        let key = DnsRecentQueryKey { name: qname.clone(), qtype, client_addr };

        let server_idx = if let Some(val) = self.recent_queries.get_mut(&key) {
            let age = now.saturating_duration_since(val.last_sent_time);
            if age < Duration::from_secs(2) {
                // Duplicate query within 2s -> RETRY!
                let next_idx = (val.server_idx + 1) % family_servers.len();
                info!(
                    "DNS Proxy: Detected retry for '{qname}' from {client_addr}. Failing over to server {next_idx}"
                );
                val.server_idx = next_idx;
                val.last_sent_time = now;
                next_idx
            } else {
                // Old query, reset to server 0
                val.server_idx = 0;
                val.last_sent_time = now;
                0
            }
        } else {
            // New query
            self.recent_queries
                .insert(key, DnsRecentQueryValue { last_sent_time: now, server_idx: 0 });
            0
        };

        if let Some(&server) = family_servers.get(server_idx) {
            DnsQueryResult::Forward(server)
        } else {
            DnsQueryResult::Forward(get_upstream_server(dns_servers))
        }
    }

    pub fn handle_reply(&mut self, reply_payload: &[u8], now: Instant) {
        let Some((qname, qtype, ip, ttl)) = parse_dns_reply(reply_payload) else {
            return;
        };

        if qtype == 1 || qtype == 28 {
            debug!("DNS Proxy: Caching '{qname}' -> {ip} (TTL={ttl})");
            let key = DnsCacheKey { name: qname, qtype };
            self.cache.insert(key, DnsCacheEntry { ip, ttl, created_at: now });
        }
    }

    pub fn clear(&mut self) {
        self.cache.clear();
        self.recent_queries.clear();
    }
}

fn parse_dns_name(payload: &[u8], mut offset: usize) -> Option<(String, usize)> {
    let mut name = String::new();
    loop {
        let &len_byte = payload.get(offset)?;
        let len = len_byte as usize;
        offset += 1;
        if len == 0 {
            break;
        }
        let label_bytes = payload.get(offset..offset + len)?;
        let label = std::str::from_utf8(label_bytes).ok()?;
        if !name.is_empty() {
            name.push('.');
        }
        name.push_str(label);
        offset += len;
    }
    Some((name, offset))
}

fn parse_dns_query(payload: &[u8]) -> Option<(String, u16, u16, u16)> {
    let header_bytes = payload.get(..12)?;
    let header = DnsHeader::read_from_bytes(header_bytes).ok()?;
    let tx_id = header.transaction_id.get();
    let flags = header.flags.get();
    let num_questions = header.num_questions.get();

    if num_questions != 1 {
        return None;
    }

    let (qname, offset) = parse_dns_name(payload, 12)?;
    let (&qtype_bytes, _) = payload.get(offset..)?.first_chunk::<4>()?.split_first_chunk::<2>()?;
    let qtype = u16::from_be_bytes(qtype_bytes);
    Some((qname, qtype, tx_id, flags))
}

fn parse_dns_name_or_pointer(payload: &[u8], offset: usize) -> Option<(String, usize)> {
    let &first = payload.get(offset)?;
    if (first & 0xC0) == 0xC0 {
        payload.get(offset..offset + 2)?;
        Some((String::new(), offset + 2))
    } else {
        parse_dns_name(payload, offset)
    }
}

fn parse_dns_reply(payload: &[u8]) -> Option<(String, u16, IpAddr, u32)> {
    let header_bytes = payload.get(..12)?;
    let header = DnsHeader::read_from_bytes(header_bytes).ok()?;
    let num_questions = header.num_questions.get() as usize;
    let num_answers = header.num_answers.get() as usize;

    let mut offset = 12;

    let mut first_question = None;
    for _ in 0..num_questions {
        let (name, next_offset) = parse_dns_name(payload, offset)?;
        let (&qtype_bytes, _) =
            payload.get(next_offset..)?.first_chunk::<4>()?.split_first_chunk::<2>()?;
        let qtype = u16::from_be_bytes(qtype_bytes);
        if first_question.is_none() {
            first_question = Some((name, qtype));
        }
        offset = next_offset + 4;
    }

    let (qname, qtype) = first_question?;

    for _ in 0..num_answers {
        let (_, next_offset) = parse_dns_name_or_pointer(payload, offset)?;
        let &[t0, t1, c0, c1, ttl0, ttl1, ttl2, ttl3, l0, l1, ref rest @ ..] =
            payload.get(next_offset..)?
        else {
            return None;
        };
        let rtype = u16::from_be_bytes([t0, t1]);
        let rclass = u16::from_be_bytes([c0, c1]);
        let ttl = u32::from_be_bytes([ttl0, ttl1, ttl2, ttl3]);
        let rd_len = u16::from_be_bytes([l0, l1]) as usize;

        let rdata = rest.get(..rd_len)?;
        offset = next_offset + 10 + rd_len;

        if rclass == 1 {
            let ip = match (rtype, rdata) {
                (1, &[a, b, c, d]) => Some(IpAddr::V4(Ipv4Addr::new(a, b, c, d))),
                (28, _) => <[u8; 16]>::try_from(rdata).ok().map(|b| IpAddr::V6(Ipv6Addr::from(b))),
                _ => None,
            };
            if let Some(ip) = ip {
                return Some((qname, qtype, ip, ttl));
            }
        }
    }

    None
}

fn synthesize_dns_reply(
    query_payload: &[u8],
    query_flags: u16,
    qtype: u16,
    ttl: u32,
    ip: IpAddr,
) -> Option<Vec<u8>> {
    let (header, question) = query_payload.split_first_chunk::<12>()?;
    let mut header_buf = *header;

    // flags: Response (0x8000) | copied Opcode/RD | Recursion Available
    // (0x0080)
    let reply_flags = 0x8000 | (query_flags & 0x7F00) | 0x0080;
    header_buf.get_mut(2..4)?.copy_from_slice(&reply_flags.to_be_bytes());
    header_buf.get_mut(6..8)?.copy_from_slice(&1u16.to_be_bytes()); // num_answers = 1

    let rdata_bytes: &[u8] = match ip {
        IpAddr::V4(ref ipv4) => &ipv4.octets(),
        IpAddr::V6(ref ipv6) => &ipv6.octets(),
    };
    let rd_len = rdata_bytes.len() as u16;

    Some(
        [
            &header_buf[..],
            question,
            &[0xC0, 0x0C], // Pointer to question name: 0xC00C
            &qtype.to_be_bytes(),
            &1u16.to_be_bytes(), // Class: IN (1)
            &ttl.to_be_bytes(),
            &rd_len.to_be_bytes(),
            rdata_bytes,
        ]
        .concat(),
    )
}

/// Parses resolv.conf format text into a list of valid non-loopback,
/// non-unspecified IP addresses.
pub fn parse_resolv_conf_content(content: &str) -> Vec<IpAddr> {
    let mut servers = Vec::new();
    for line in content.lines() {
        let line = line.trim();
        if line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        let mut words = line.split_whitespace();
        if let (Some("nameserver"), Some(ip_token)) = (words.next(), words.next()) {
            // Strip any IPv6 scope ID suffix (e.g. fe80::1%eth0 -> fe80::1)
            let clean_ip = ip_token.split('%').next().unwrap();
            if let Ok(ip) = clean_ip.parse::<IpAddr>()
                && !ip.is_unspecified()
                && !ip.is_loopback()
                && !servers.contains(&ip)
            {
                servers.push(ip);
            }
        }
    }
    servers
}

/// Discovers host DNS servers from host OS configuration.
/// - Linux: Parses `/run/systemd/resolve/resolv.conf` (if present) or
///   `/etc/resolv.conf`.
/// - macOS: Parses `/etc/resolv.conf`.
/// - Windows: Queries active network adapter DNS servers via
///   `GetAdaptersAddresses`.
#[cfg(unix)]
pub async fn discover_host_dns_servers() -> Vec<IpAddr> {
    let paths = ["/run/systemd/resolve/resolv.conf", "/etc/resolv.conf"];
    let mut servers = Vec::new();

    for path in paths {
        if let Ok(content) = tokio::fs::read_to_string(path).await {
            servers = parse_resolv_conf_content(&content);
            if !servers.is_empty() {
                break;
            }
        }
    }

    if servers.is_empty() {
        log::info!("No non-loopback DNS servers found in resolv.conf; falling back to localhost");
        servers.push(IpAddr::V4(Ipv4Addr::LOCALHOST));
    } else {
        log::info!("Discovered host DNS servers: {servers:?}");
    }
    servers
}

#[cfg(windows)]
pub async fn discover_host_dns_servers() -> Vec<IpAddr> {
    tokio::task::spawn_blocking(discover_host_dns_servers_windows).await.unwrap_or_default()
}

#[cfg(windows)]
#[allow(clippy::cast_ptr_alignment, clippy::ptr_as_ptr)]
fn discover_host_dns_servers_windows() -> Vec<IpAddr> {
    use std::net::Ipv6Addr;

    use windows_sys::Win32::{
        Foundation::{ERROR_BUFFER_OVERFLOW, ERROR_SUCCESS},
        NetworkManagement::{
            IpHelper::{
                GAA_FLAG_SKIP_ANYCAST, GAA_FLAG_SKIP_MULTICAST, GetAdaptersAddresses,
                IP_ADAPTER_ADDRESSES_LH,
            },
            Ndis::IfOperStatusUp,
        },
        Networking::WinSock::{AF_INET, AF_INET6, AF_UNSPEC, SOCKADDR_IN, SOCKADDR_IN6},
    };

    let mut servers = Vec::new();
    let mut buf_len: u32 = 15000;
    let mut buf = vec![0u8; buf_len as usize];

    // First call to determine buffer size needed.
    // SAFETY: `buf` is a valid, contiguous byte allocation of `buf_len` bytes.
    // `GetAdaptersAddresses` writes up to `buf_len` bytes and updates `buf_len`
    // on overflow.
    let mut ret = unsafe {
        GetAdaptersAddresses(
            AF_UNSPEC as u32,
            GAA_FLAG_SKIP_ANYCAST | GAA_FLAG_SKIP_MULTICAST,
            core::ptr::null_mut(),
            buf.as_mut_ptr().cast::<IP_ADAPTER_ADDRESSES_LH>(),
            &mut buf_len,
        )
    };

    if ret == ERROR_BUFFER_OVERFLOW {
        buf.resize(buf_len as usize, 0);
        // SAFETY: `buf` is resized to the exact capacity requested by the
        // previous call.
        ret = unsafe {
            GetAdaptersAddresses(
                AF_UNSPEC as u32,
                GAA_FLAG_SKIP_ANYCAST | GAA_FLAG_SKIP_MULTICAST,
                core::ptr::null_mut(),
                buf.as_mut_ptr().cast::<IP_ADAPTER_ADDRESSES_LH>(),
                &mut buf_len,
            )
        };
    }

    if ret != ERROR_SUCCESS {
        log::warn!("GetAdaptersAddresses failed with error code {ret}");
        return servers;
    }

    let mut adapter = buf.as_ptr().cast::<IP_ADAPTER_ADDRESSES_LH>();
    while !adapter.is_null() {
        // SAFETY: `ret == ERROR_SUCCESS` guarantees that `buf` contains a
        // valid, properly aligned linked list of
        // `IP_ADAPTER_ADDRESSES_LH` terminated by a null pointer.
        // `adapter` is checked non-null before dereferencing.
        let (oper_status, mut dns_server, next_adapter) =
            unsafe { ((*adapter).OperStatus, (*adapter).FirstDnsServerAddress, (*adapter).Next) };

        if oper_status == IfOperStatusUp {
            while !dns_server.is_null() {
                // SAFETY: `dns_server` points to a valid
                // `IP_ADAPTER_DNS_SERVER_ADDRESS_XP` within the
                // OS-populated adapter structure and is checked non-null.
                let (sockaddr, next_dns) =
                    unsafe { ((*dns_server).Address.lpSockaddr, (*dns_server).Next) };

                if !sockaddr.is_null() {
                    // SAFETY: `sockaddr` is checked non-null and points to a
                    // valid OS-initialized `SOCKADDR`.
                    let family = unsafe { (*sockaddr).sa_family as u32 };

                    let ip = if family == AF_INET as u32 {
                        let sin = sockaddr.cast::<SOCKADDR_IN>();
                        // SAFETY: `family == AF_INET` guarantees `sockaddr` is
                        // a valid `SOCKADDR_IN`.
                        let ip_bytes = unsafe { (*sin).sin_addr.S_un.S_addr.to_ne_bytes() };
                        Some(IpAddr::V4(Ipv4Addr::from(ip_bytes)))
                    } else if family == AF_INET6 as u32 {
                        let sin6 = sockaddr.cast::<SOCKADDR_IN6>();
                        // SAFETY: `family == AF_INET6` guarantees `sockaddr` is
                        // a valid `SOCKADDR_IN6`.
                        let ip_bytes = unsafe { (*sin6).sin6_addr.u.Byte };
                        Some(IpAddr::V6(Ipv6Addr::from(ip_bytes)))
                    } else {
                        None
                    };

                    if let Some(ip) = ip
                        && !ip.is_unspecified()
                        && !ip.is_loopback()
                        && !servers.contains(&ip)
                    {
                        servers.push(ip);
                    }
                }
                dns_server = next_dns;
            }
        }
        adapter = next_adapter;
    }

    if servers.is_empty() {
        log::info!(
            "No DNS servers discovered on Windows via GetAdaptersAddresses; using default fallback"
        );
    } else {
        log::info!("Discovered Windows host DNS servers: {servers:?}");
    }
    servers
}

#[cfg(not(any(unix, windows)))]
pub async fn discover_host_dns_servers() -> Vec<IpAddr> {
    Vec::new()
}
