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

use std::env;

use clap::Parser;

#[derive(Debug, Parser, Default)]
pub struct Args {
    /// File descriptor start up info proto
    #[arg(short = 's', long, alias = "fd_startup_str")]
    pub fd_startup_str: Option<String>,

    /// Enables connector mode to forward packets to another instance.
    #[arg(short, long, alias = "connector_instance", visible_alias = "connector_instance_num")]
    pub connector_instance: Option<u16>,

    /// Netsimd instance number
    #[arg(short, long, visible_alias = "instance_num")]
    pub instance: Option<u16>,

    /// Set whether log messages go to stderr instead of logfiles
    #[arg(short, long)]
    pub logtostderr: bool,

    /// Enable development mode. This will include additional features
    #[arg(short, long)]
    pub dev: bool,

    /// Disable grpc server for CLI
    #[arg(long, alias = "no_cli_ui")]
    pub no_cli_ui: bool,

    /// Disable web server
    /// No-op: no web UI is currently implemented
    #[arg(long, alias = "no_web_ui")]
    pub no_web_ui: bool,

    /// Redirect all TCP connections through the specified HTTP/HTTPS proxy.
    /// Can be one of the following:
    ///     `http://<server>:<port>`
    ///     `http://<username>:<password>@<server>:<port>`
    ///     (the 'http://' prefix can be omitted)
    /// Specify an empty string (`--http-proxy ""`) to disable an ambient
    /// `http_proxy` environment variable.
    #[arg(long, verbatim_doc_comment)]
    #[cfg_attr(not(feature = "cuttlefish"), arg(env = "http_proxy"))]
    pub http_proxy: Option<String>,

    /// Disable netsimd from shutting down automatically.
    /// WARNING: This flag is for development purpose. netsimd will not shutdown
    /// without SIGKILL.
    #[arg(long, alias = "no_shutdown")]
    pub no_shutdown: bool,

    /// Set the idle shutdown timeout in milliseconds.
    #[arg(long, alias = "idle-shutdown-timeout")]
    pub idle_shutdown_timeout: Option<u64>,

    /// Set the startup timeout in milliseconds.
    #[arg(long, alias = "startup-timeout")]
    pub startup_timeout: Option<u64>,

    /// Enable packet capture
    #[arg(long)]
    pub pcap: bool,

    /// Disable Address Reuse for Bluetooth test model
    #[arg(long, alias = "disable_address_reuse")]
    pub disable_address_reuse: bool,

    /// Entering Verbose mode
    #[arg(short = 'v', long)]
    pub verbose: bool,

    /// Print Netsimd version information
    #[arg(long)]
    pub version: bool,

    /// gRPC port for the netsim service. Overrides env var NETSIM_GRPC_PORT
    #[arg(long, alias = "grpc_port")]
    pub grpc_port: Option<u16>,

    /// Unix Domain Socket path for the netsim gRPC service.
    #[cfg(unix)]
    #[arg(long, alias = "grpc_uds_path")]
    pub grpc_uds_path: Option<String>,

    /// HTTP port serving the /v1 API. Overrides env var NETSIM_HTTP_PORT
    #[arg(long, alias = "http_port")]
    pub http_port: Option<u16>,

    /// HCI port for the raw TCP socket. Overrides env var NETSIM_HCI_PORT
    #[arg(long, alias = "hci_port")]
    pub hci_port: Option<u16>,

    /// TCP port for the packet stream service. Overrides env var
    /// NETSIM_TCP_PORT
    #[arg(long, alias = "tcp_port")]
    pub tcp_port: Option<u16>,

    /// WebSocket port for the web UI. Overrides env var NETSIM_WS_PORT
    #[arg(long)]
    pub ws_port: Option<u16>,

    /// Rootcanal legacy control port for the TCP socket
    #[arg(long, alias = "test_port")]
    #[cfg(feature = "cuttlefish")]
    pub test_port: Option<u16>,

    /// DNS server for the host
    #[arg(long, alias = "host-dns")]
    pub host_dns: Option<String>,

    /// Forward mDNS packets from host to guest Wi-Fi medium
    #[arg(long)]
    pub forward_host_mdns: bool,

    /// Start with test beacons
    #[arg(long, alias = "test_beacons", overrides_with("no_test_beacons"))]
    pub test_beacons: bool,

    /// Do not start with test beacons
    #[arg(long, alias = "no_test_beacons", overrides_with("test_beacons"))]
    pub no_test_beacons: bool,

    /// Enable pure Rust Slirp native backend
    #[arg(
        long,
        alias = "slirp_native",
        alias = "native_slirp",
        alias = "native-slirp",
        overrides_with("no_slirp_native")
    )]
    pub slirp_native: bool,

    /// Disable pure Rust Slirp native backend (use legacy C libslirp FFI)
    #[arg(
        long,
        alias = "no_slirp_native",
        alias = "no_native_slirp",
        alias = "no-native-slirp",
        overrides_with("slirp_native")
    )]
    pub no_slirp_native: bool,

    /// Set the initial SSID for the default Access Point (defaults to
    /// 'AndroidWifi')
    #[command(flatten)]
    pub wifi: WifiConfig,
}

#[derive(Debug, Default, Clone, clap::Args)]
pub struct WifiConfig {
    /// Set the initial SSID for the default Access Point (defaults to
    /// 'AndroidWifi')
    #[arg(long, alias = "wifi-ssid", help_heading = "WiFi Settings")]
    pub wifi_ssid: Option<String>,

    /// Set the WPA passphrase for the default Access Point (optional)
    #[arg(long, alias = "wifi-password", help_heading = "WiFi Settings")]
    pub wifi_password: Option<String>,

    /// Set the initial radio channel for the default Access Point (defaults to
    /// 11)
    #[arg(long, alias = "wifi-channel", help_heading = "WiFi Settings")]
    pub wifi_channel: Option<u8>,

    /// Set the beacon interval in TU for the default Access Point (defaults to
    /// 100)
    #[arg(long, alias = "wifi-beacon-interval", help_heading = "WiFi Settings")]
    pub wifi_beacon_interval: Option<u16>,

    /// Set the 802.11 mode for the default Access Point (defaults to "g")
    #[arg(long, alias = "wifi-mode", value_enum, help_heading = "WiFi Settings")]
    pub wifi_mode: Option<ClapWifiMode>,

    /// Use a specific TAP interface (e.g. cvd-etap-01) or a pattern (e.g.
    /// cvd-etap-%02d).
    #[arg(long, alias = "wifi-tap", help_heading = "WiFi Settings")]
    #[cfg(target_os = "linux")]
    pub wifi_tap: Option<String>,

    /// Use the standard Cuttlefish TAP pool (cvd-etap-06..10).
    /// Equivalent to --wifi-tap "cvd-etap-%02d".
    #[arg(long, alias = "wifi-cvd-tap", help_heading = "WiFi Settings")]
    #[cfg(target_os = "linux")]
    pub wifi_cvd_tap: bool,
}

#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub enum ClapWifiMode {
    A,
    B,
    G,
    N,
    Ac,
    Ax,
}

use netsim_model::WifiMode as ModelWifiMode;

// We define a local WifiMode enum here to derive clap::ValueEnum,
// as the `netsim-model` crate should not depend on `clap` (UI concern).
// We then implement From to convert the CLI argument into the Model type.
impl From<ClapWifiMode> for ModelWifiMode {
    fn from(mode: ClapWifiMode) -> Self {
        match mode {
            ClapWifiMode::A => ModelWifiMode::A,
            ClapWifiMode::B => ModelWifiMode::B,
            ClapWifiMode::G => ModelWifiMode::G,
            ClapWifiMode::N => ModelWifiMode::N,
            ClapWifiMode::Ac => ModelWifiMode::Ac,
            ClapWifiMode::Ax => ModelWifiMode::Ax,
        }
    }
}

impl Args {
    // Return the command line args along with any NETSIM_ARGS from
    // the environment.
    pub(crate) fn parse() -> Args {
        let mut args: Vec<String> = env::args().collect();
        // This simple split WILL FAIL on quoted values like: --host "my custom
        // host"
        if let Ok(env_str) = env::var("NETSIM_ARGS") {
            args.extend(env_str.split_whitespace().map(|s| s.to_string()));
        }
        let mut parsed = Args::parse_from(args);
        parsed.sanitize();
        parsed
    }

    /// Sanitizes parsed arguments, trimming whitespace and treating empty
    /// strings (e.g. `--http-proxy ""`) as `None`.
    pub fn sanitize(&mut self) {
        if let Some(proxy) = self.http_proxy.take() {
            let trimmed = proxy.trim();
            if !trimmed.is_empty() {
                self.http_proxy = Some(trimmed.to_string());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_http_proxy_cleared() {
        let mut args = Args { http_proxy: Some("   ".to_string()), ..Default::default() };
        args.sanitize();
        assert_eq!(args.http_proxy, None);
    }

    #[test]
    fn test_whitespace_http_proxy_trimmed() {
        let mut args = Args {
            http_proxy: Some("  http://proxy.example.com:8080  ".to_string()),
            ..Default::default()
        };
        args.sanitize();
        assert_eq!(args.http_proxy, Some("http://proxy.example.com:8080".to_string()));
    }

    #[test]
    fn test_http_proxy_empty_cli_flag_clears_proxy() {
        let mut args = Args::parse_from(["netsimd", "--http-proxy", ""]);
        args.sanitize();
        assert_eq!(args.http_proxy, None);
    }

    #[test]
    fn test_none_http_proxy_stays_none() {
        let mut args = Args { http_proxy: None, ..Default::default() };
        args.sanitize();
        assert_eq!(args.http_proxy, None);
    }

    #[test]
    fn test_http_proxy_valid_flag_trimmed() {
        let mut args =
            Args::parse_from(["netsimd", "--http-proxy", "  http://proxy.example.com:8080  "]);
        args.sanitize();
        assert_eq!(args.http_proxy, Some("http://proxy.example.com:8080".to_string()));
    }
}
