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

//! Conversion between Rust and C configurations.
use std::{
    ffi::CString,
    io,
    net::{Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6},
    path::PathBuf,
};

use tracing::warn;

use crate::libslirp_sys::{self, SLIRP_MAX_DNS_SERVERS};

/// The maximum number of DNS servers supported by libslirp.
const MAX_DNS_SERVERS: usize = SLIRP_MAX_DNS_SERVERS as usize;

/// Configuration options for the Slirp network stack.
#[derive(Clone)]
pub struct SlirpConfig {
    /// Slirp version.
    pub version: u32,
    /// Whether to run in restricted mode.
    pub restricted: i32,
    /// Whether IPv4 is enabled.
    pub in_enabled: bool,
    /// The virtual network address for IPv4.
    pub vnetwork: Ipv4Addr,
    /// The virtual network mask for IPv4.
    pub vnetmask: Ipv4Addr,
    /// The virtual host address for IPv4.
    pub vhost: Ipv4Addr,
    /// Whether IPv6 is enabled.
    pub in6_enabled: bool,
    /// The virtual prefix address for IPv6.
    pub vprefix_addr6: Ipv6Addr,
    /// The length of the virtual prefix for IPv6.
    pub vprefix_len: u8,
    /// The virtual host address for IPv6.
    pub vhost6: Ipv6Addr,
    /// The virtual hostname.
    pub vhostname: Option<String>,
    /// The TFTP server name.
    pub tftp_server_name: Option<String>,
    /// The path to the TFTP root directory.
    pub tftp_path: Option<PathBuf>,
    /// The bootfile name for DHCP.
    pub bootfile: Option<String>,
    /// The starting IP address for the DHCP server.
    pub vdhcp_start: Ipv4Addr,
    /// The primary DNS server address for IPv4.
    pub vnameserver: Ipv4Addr,
    /// The primary DNS server address for IPv6.
    pub vnameserver6: Ipv6Addr,
    /// A list of DNS search domains.
    pub vdnssearch: Vec<String>,
    /// The virtual domain name.
    pub vdomainname: Option<String>,
    /// The interface MTU (Maximum Transmission Unit).
    pub if_mtu: usize,
    /// The interface MRU (Maximum Receive Unit).
    pub if_mru: usize,
    /// Whether to disable the host loopback interface.
    pub disable_host_loopback: bool,
    /// Whether to enable emulation features.
    pub enable_emu: bool,
    /// The outbound IPv4 address to bind to (optional).
    pub outbound_addr: Option<SocketAddrV4>,
    /// The outbound IPv6 address to bind to (optional).
    pub outbound_addr6: Option<SocketAddrV6>,
    /// Whether to disable the built-in DNS server.
    pub disable_dns: bool,
    /// Whether to disable the built-in DHCP server.
    pub disable_dhcp: bool,
    /// The manufacturer ID.
    pub mfr_id: u32,
    /// The out-of-band Ethernet address.
    pub oob_eth_addr: [u8; 6usize],
    /// Whether the HTTP proxy is enabled.
    pub http_proxy_on: bool,
    /// A list of host DNS servers to use.
    pub host_dns: Vec<SocketAddr>,
}

impl Default for SlirpConfig {
    /// Creates a new `SlirpConfig` with default values.
    ///
    /// The default configuration has IPv4 and IPv6 enabled on a private
    /// network, with DHCP starting at `10.0.2.16` and a DNS server at
    /// `10.0.2.3`.
    fn default() -> Self {
        SlirpConfig {
            version: 5,
            // No restrictions by default
            restricted: 0,
            in_enabled: true,
            // Private network address
            vnetwork: Ipv4Addr::new(10, 0, 2, 0),
            vnetmask: Ipv4Addr::new(255, 255, 255, 0),
            // Default host address
            vhost: Ipv4Addr::new(10, 0, 2, 2),
            // IPv6 enabled by default
            in6_enabled: true,
            vprefix_addr6: Ipv6Addr::new(0xfec0, 0, 0, 0, 0, 0, 0, 0),
            vprefix_len: 64,
            vhost6: Ipv6Addr::new(0xfec0, 0, 0, 0, 0, 0, 0, 2),
            vhostname: None, // Some("slirp".to_string()),
            tftp_server_name: None,
            tftp_path: None,
            bootfile: None,
            // DHCP starting address
            vdhcp_start: Ipv4Addr::new(10, 0, 2, 16),
            // Public DNS server
            vnameserver: Ipv4Addr::new(10, 0, 2, 3),
            vnameserver6: Ipv6Addr::new(0xfec0, 0, 0, 0, 0, 0, 0, 3),
            vdnssearch: Vec::new(),
            vdomainname: None,
            // Ethernet MTU
            if_mtu: 0,
            // Ethernet MRU
            if_mru: 0,
            disable_host_loopback: false,
            enable_emu: false,
            outbound_addr: None,
            outbound_addr6: None,
            disable_dns: false,
            disable_dhcp: false,
            mfr_id: 0,
            oob_eth_addr: [0; 6usize],
            http_proxy_on: false,
            host_dns: Vec::new(),
        }
    }
}

/// Struct to hold a "C" `SlirpConfig` and the Rust storage that is
/// referenced by `SlirpConfig`.
pub(crate) struct SlirpConfigs {
    /// The "C" representation of the Slirp configuration.
    c_slirp_config: libslirp_sys::SlirpConfig,

    // fields that hold the managed storage for "C" struct.
    _c_bootfile: Option<CString>,
    _c_tftp_server_name: Option<CString>,
    _c_vdomainname: Option<CString>,
    _c_vhostname: Option<CString>,
    _c_tftp_path: Option<CString>,
    _c_host_dns: [libslirp_sys::sockaddr_storage; MAX_DNS_SERVERS],
    // TODO: add other fields
}

/// Asynchronously looks up the IP addresses for a given hostname or
/// comma-separated list of hostnames.
///
/// Each hostname in the input string is resolved using
/// `tokio::net::lookup_host`. The port in the resolved `SocketAddr` will be 0.
///
/// # Arguments
///
/// * `host_dns` - A string containing a single hostname or a comma-separated
///   list of hostnames.
///
/// # Returns
///
/// A `Result` containing a `Vec` of `SocketAddr` on success, or an `io::Error`
/// on failure.
pub async fn lookup_host_dns(host_dns: &str) -> io::Result<Vec<SocketAddr>> {
    let mut set = tokio::task::JoinSet::new();
    if host_dns.is_empty() {
        return Ok(Vec::new());
    }

    for addr in host_dns.split(',') {
        set.spawn(tokio::net::lookup_host(format_dns_addr(addr)));
    }

    let mut addrs = Vec::new();
    while let Some(result) = set.join_next().await {
        addrs.push(result??.next().ok_or(io::Error::from(io::ErrorKind::NotFound))?);
    }
    Ok(addrs)
}

/// Formats a DNS address for `tokio::net::lookup_host`.
///
/// This function ensures that standard IPv4 and IPv6 addresses are correctly
/// appended with `:53`. If the user specifies an explicit custom port (e.g.,
/// `:5354`), we actively strip it and revert to standard port `53`.
///
/// This is because `libslirp` internally overwrites the destination port of
/// intercepted DNS packets with standard port `53` (as seen in
/// `external/libslirp/src/socket.c`). Silently passing a custom port will be
/// ignored by `libslirp`, so stripping it ensures our expectations match
/// reality.
///
/// We append `:53` instead of `:0` (which Classic used) because it:
/// 1. Is logically more accurate (it's a DNS server being connected to).
/// 2. Avoids errors if standard Rust / tokio libraries in `next` ever try to
///    use the `SocketAddr` directly (connecting to port 0 is an error in
///    standard networking).
///
/// Note: Both `:0` and `:53` work identically for `libslirp` because it
/// discards the port in both cases.
fn format_dns_addr(addr: &str) -> String {
    if let Ok(ip) = addr.parse::<std::net::IpAddr>() {
        match ip {
            std::net::IpAddr::V4(_) => format!("{addr}:53"),
            std::net::IpAddr::V6(_) => format!("[{addr}]:53"),
        }
    } else if let Ok(socket) = addr.parse::<std::net::SocketAddr>() {
        match socket {
            std::net::SocketAddr::V4(s) => format!("{}:{}", s.ip(), s.port()),
            std::net::SocketAddr::V6(s) => format!("[{}]:{}", s.ip(), s.port()),
        }
    } else {
        // Handle bracketed IPv6 without port specified: [2001:db8::1]
        let stripped = addr.trim_matches(|c| c == '[' || c == ']');
        if stripped.parse::<std::net::Ipv6Addr>().is_ok() {
            return format!("[{stripped}]:53");
        }

        // Fallback for hostnames (e.g., localhost:5354 or localhost)
        if let Some((host, port)) = addr.rsplit_once(':') {
            format!("{host}:{port}")
        } else {
            format!("{addr}:53")
        }
    }
}

/// Converts a slice of `SocketAddr` into an array of
/// `libslirp_sys::sockaddr_storage`.
///
/// If the input slice contains more than `MAX_DNS_SERVERS` addresses, a warning
/// is logged, and only the first `MAX_DNS_SERVERS` addresses are converted. The
/// remaining entries in the output array will be default-initialized.
///
/// # Arguments
///
/// * `dns` - A slice of `SocketAddr` representing DNS server addresses.
///
/// # Returns
///
/// An array of `libslirp_sys::sockaddr_storage` containing the converted
/// addresses.
fn to_socketaddr_storage(dns: &[SocketAddr]) -> [libslirp_sys::sockaddr_storage; MAX_DNS_SERVERS] {
    let mut result = [libslirp_sys::sockaddr_storage::default(); MAX_DNS_SERVERS];
    if dns.len() > MAX_DNS_SERVERS {
        warn!("Too many DNS servers, only keeping the first {} ones", MAX_DNS_SERVERS);
    }
    for (dest, src) in result.iter_mut().zip(dns.iter()) {
        *dest = (*src).into();
    }
    result
}

impl SlirpConfigs {
    /// Creates a new `SlirpConfigs` instance from a Rust `SlirpConfig`.
    ///
    /// This function converts the Rust configuration into the "C"
    /// representation used by libslirp, handling string conversions and
    /// storing necessary Rust data to be referenced by the "C" struct.
    ///
    /// # Arguments
    ///
    /// * `config` - A reference to the Rust `SlirpConfig`.
    pub(crate) fn new(config: &SlirpConfig) -> SlirpConfigs {
        let as_cstring =
            |s: &Option<String>| s.as_ref().and_then(|s| CString::new(s.as_bytes()).ok());
        let c_tftp_path = config
            .tftp_path
            .as_ref()
            .and_then(|s| CString::new(s.to_string_lossy().into_owned()).ok());
        let c_vhostname = as_cstring(&config.vhostname);
        let c_tftp_server_name = as_cstring(&config.tftp_server_name);
        let c_bootfile = as_cstring(&config.bootfile);
        let c_vdomainname = as_cstring(&config.vdomainname);

        let c_host_dns = to_socketaddr_storage(&config.host_dns);

        // Convert to a ptr::null() or a raw ptr to managed
        // memory. Whenever storing a ptr in "C" Struct using `as_ptr`
        // this code must have a Rust member is `SlirpConfigs` that
        // holds the storage.
        let as_ptr = |p: &Option<CString>| p.as_ref().map_or(std::ptr::null(), |s| s.as_ptr());

        let c_slirp_config = libslirp_sys::SlirpConfig {
            version: config.version,
            restricted: config.restricted,
            in_enabled: config.in_enabled,
            vnetwork: config.vnetwork.into(),
            vnetmask: config.vnetmask.into(),
            vhost: config.vhost.into(),
            in6_enabled: config.in6_enabled,
            vprefix_addr6: config.vprefix_addr6.into(),
            vprefix_len: config.vprefix_len,
            vhost6: config.vhost6.into(),
            vhostname: as_ptr(&c_vhostname),
            tftp_server_name: as_ptr(&c_tftp_server_name),
            tftp_path: as_ptr(&c_tftp_path),
            bootfile: as_ptr(&c_bootfile),
            vdhcp_start: config.vdhcp_start.into(),
            vnameserver: config.vnameserver.into(),
            vnameserver6: config.vnameserver6.into(),
            // TODO: add field
            vdnssearch: std::ptr::null_mut(),
            vdomainname: as_ptr(&c_vdomainname),
            if_mtu: config.if_mtu,
            if_mru: config.if_mru,
            disable_host_loopback: config.disable_host_loopback,
            enable_emu: config.enable_emu,
            // TODO: add field
            outbound_addr: std::ptr::null_mut(),
            // TODO: add field
            outbound_addr6: std::ptr::null_mut(),
            disable_dns: config.disable_dns,
            disable_dhcp: config.disable_dhcp,
            mfr_id: config.mfr_id,
            oob_eth_addr: config.oob_eth_addr,
            http_proxy_on: config.http_proxy_on,
            host_dns_count: config.host_dns.len(),
            host_dns: c_host_dns,
        };

        // Return the "C" struct and Rust members holding the storage
        // referenced by the "C" struct.
        SlirpConfigs {
            c_slirp_config,
            _c_vhostname: c_vhostname,
            _c_tftp_server_name: c_tftp_server_name,
            _c_bootfile: c_bootfile,
            _c_vdomainname: c_vdomainname,
            _c_tftp_path: c_tftp_path,
            _c_host_dns: c_host_dns,
        }
    }

    /// Returns a reference to the C SlirpConfig.
    pub(crate) fn c_slirp_config(&self) -> &libslirp_sys::SlirpConfig {
        &self.c_slirp_config
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::needless_range_loop)]
    use tokio::runtime::Runtime;

    use super::*;

    /// Tests the default values of the `SlirpConfig` struct.
    #[test]
    fn test_slirp_config_default() {
        let config = SlirpConfig::default();

        assert_eq!(config.version, 5);
        assert_eq!(config.restricted, 0);
        assert!(config.in_enabled);
        assert_eq!(config.vnetwork, Ipv4Addr::new(10, 0, 2, 0));
        assert_eq!(config.vnetmask, Ipv4Addr::new(255, 255, 255, 0));
        assert_eq!(config.vhost, Ipv4Addr::new(10, 0, 2, 2));
        assert!(config.in6_enabled);
        assert_eq!(config.vprefix_addr6, "fec0::".parse::<Ipv6Addr>().unwrap());
        assert_eq!(config.vprefix_len, 64);
        assert_eq!(config.vhost6, "fec0::2".parse::<Ipv6Addr>().unwrap());
        assert_eq!(config.vhostname, None);
        assert_eq!(config.tftp_server_name, None);
        assert_eq!(config.tftp_path, None);
        assert_eq!(config.bootfile, None);
        assert_eq!(config.vdhcp_start, Ipv4Addr::new(10, 0, 2, 16));
        assert_eq!(config.vnameserver, Ipv4Addr::new(10, 0, 2, 3));
        assert_eq!(config.vnameserver6, "fec0::3".parse::<Ipv6Addr>().unwrap());
        assert!(config.vdnssearch.is_empty());
        assert_eq!(config.vdomainname, None);
        assert_eq!(config.if_mtu, 0);
        assert_eq!(config.if_mru, 0);
        assert!(!config.disable_host_loopback);
        assert!(!config.enable_emu);
        assert_eq!(config.outbound_addr, None);
        assert_eq!(config.outbound_addr6, None);
        assert!(!config.disable_dns);
        assert!(!config.disable_dhcp);
        assert_eq!(config.mfr_id, 0);
        assert_eq!(config.oob_eth_addr, [0; 6]);
        assert!(!config.http_proxy_on);
        assert_eq!(config.host_dns.len(), 0);
    }

    /// Tests the creation of a `SlirpConfigs` instance from a default
    /// `SlirpConfig`.
    #[test]
    fn test_slirp_configs_new() {
        let rust_config = SlirpConfig::default();
        let c_configs = SlirpConfigs::new(&rust_config);

        // Check basic field conversions
        assert_eq!(c_configs.c_slirp_config().version, rust_config.version);
        assert_eq!(c_configs.c_slirp_config().restricted, rust_config.restricted);
        assert_eq!(c_configs.c_slirp_config().in_enabled as i32, rust_config.in_enabled as i32);

        // Check string conversions and null pointers
        assert_eq!(c_configs.c_slirp_config().vhostname, std::ptr::null());
        assert_eq!(c_configs.c_slirp_config().tftp_server_name, std::ptr::null());
    }

    /// Tests the `lookup_host_dns` function with different inputs.
    #[test]
    #[ignore]
    fn test_lookup_host_dns() -> io::Result<()> {
        let rt = Runtime::new()?;
        let results = rt.block_on(lookup_host_dns(""))?;
        assert_eq!(results.len(), 0);

        let results = rt.block_on(lookup_host_dns("localhost"))?;
        assert_eq!(results.len(), 1);

        let results = rt.block_on(lookup_host_dns("example.com"))?;
        assert_eq!(results.len(), 1);

        let results = rt.block_on(lookup_host_dns("localhost,example.com"))?;
        assert_eq!(results.len(), 2);
        Ok(())
    }

    #[test]
    fn test_format_dns_addr() {
        assert_eq!(format_dns_addr("localhost"), "localhost:53");
        assert_eq!(format_dns_addr("localhost:5354"), "localhost:5354"); // Custom port preserved
        assert_eq!(format_dns_addr("example.com"), "example.com:53");
        assert_eq!(format_dns_addr("example.com:5354"), "example.com:5354"); // Custom port preserved
        assert_eq!(format_dns_addr("127.0.0.1"), "127.0.0.1:53");
        assert_eq!(format_dns_addr("127.0.0.1:53"), "127.0.0.1:53");
        assert_eq!(format_dns_addr("127.0.0.1:5354"), "127.0.0.1:5354"); // Custom port preserved
        assert_eq!(format_dns_addr("[2001:db8::1]"), "[2001:db8::1]:53");
        assert_eq!(format_dns_addr("[2001:db8::1]:53"), "[2001:db8::1]:53");
        assert_eq!(format_dns_addr("[2001:db8::1]:5354"), "[2001:db8::1]:5354"); // Custom port preserved
        assert_eq!(format_dns_addr("2001:db8::1"), "[2001:db8::1]:53");
    }

    /// Tests the `to_socketaddr_storage` function with an empty input slice.
    #[test]
    fn test_to_socketaddr_storage_empty_input() {
        let dns: [SocketAddr; 0] = [];
        let result = to_socketaddr_storage(&dns);
        assert_eq!(result.len(), MAX_DNS_SERVERS);
        for entry in result {
            // Assuming `sockaddr_storage::default()` initializes all fields to
            // 0
            assert_eq!(entry.ss_family, 0);
        }
    }

    /// Tests the `to_socketaddr_storage` function with a valid input slice.
    #[test]
    fn test_to_socketaddr_storage() {
        let dns = ["1.1.1.1:53".parse().unwrap(), "8.8.8.8:53".parse().unwrap()];
        let result = to_socketaddr_storage(&dns);
        assert_eq!(result.len(), MAX_DNS_SERVERS);
        for i in 0..dns.len() {
            assert_ne!(result[i].ss_family, 0); // Converted addresses should
            // have a non-zero family
        }
        for i in dns.len()..MAX_DNS_SERVERS {
            assert_eq!(result[i].ss_family, 0); // Remaining entries should be
            // default
        }
    }

    /// Tests the `to_socketaddr_storage` function with a valid input slice at
    /// the maximum allowed size.
    #[test]
    fn test_to_socketaddr_storage_valid_input_at_max() {
        let dns = [
            "1.1.1.1:53".parse().unwrap(),
            "8.8.8.8:53".parse().unwrap(),
            "9.9.9.9:53".parse().unwrap(),
            "1.0.0.1:53".parse().unwrap(),
        ];
        let result = to_socketaddr_storage(&dns);
        assert_eq!(result.len(), MAX_DNS_SERVERS);
        for i in 0..dns.len() {
            assert_ne!(result[i].ss_family, 0);
        }
    }

    /// Tests the `to_socketaddr_storage` function when the input slice exceeds
    /// the maximum allowed size.
    #[test]
    fn test_to_socketaddr_storage_input_exceeds_max() {
        let dns = [
            "1.1.1.1:53".parse().unwrap(),
            "8.8.8.8:53".parse().unwrap(),
            "9.9.9.9:53".parse().unwrap(),
            "1.0.0.1:53".parse().unwrap(),
            "1.2.3.4:53".parse().unwrap(), // Extra address
        ];
        let result = to_socketaddr_storage(&dns);
        assert_eq!(result.len(), MAX_DNS_SERVERS);
        for i in 0..MAX_DNS_SERVERS {
            assert_ne!(result[i].ss_family, 0);
        }
    }
}
