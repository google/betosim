// Copyright 2023 The Android Open Source Project
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

use clap::{Args, Parser};

#[derive(Debug, Parser)]
pub struct NetsimdArgs {
    /// File descriptor start up info proto
    #[arg(short = 's', long, alias = "fd_startup_str")]
    pub fd_startup_str: Option<String>,

    /// Disable grpc server for CLI
    #[arg(long, alias = "no_cli_ui")]
    pub no_cli_ui: bool,

    /// Disable web server
    #[arg(long, alias = "no_web_ui")]
    pub no_web_ui: bool,

    /// Enable packet capture
    #[arg(long)]
    pub pcap: bool,

    /// Disable Address Reuse for test model
    #[arg(long, alias = "disable_address_reuse")]
    pub disable_address_reuse: bool,

    /// Set custom hci port
    #[arg(long, alias = "hci_port")]
    pub hci_port: Option<u32>,

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

    /// Forwards mDNS from the host to the guest, allowing emulator to discover mDNS services running on the host.
    ///
    /// # Limitations
    /// * Currently only supports a single emulator.
    /// * May impact Wi-Fi connectivity between emulators.
    #[arg(long, verbatim_doc_comment)]
    pub forward_host_mdns: bool,

    /// Set the vsock port number to be listened by the frontend grpc server
    #[arg(long)]
    pub vsock: Option<u16>,

    /// The name of a config file to load
    #[arg(long)]
    pub config: Option<String>,

    /// Comma separated list of host DNS servers
    #[arg(long)]
    pub host_dns: Option<String>,

    /// Redirect all TCP connections through the specified HTTP/HTTPS proxy.
    /// Can be one of the following:
    ///     http://<server>:<port>
    ///     http://<username>:<password>@<server>:<port>
    ///     (the 'http://' prefix can be omitted)
    /// WARNING: This flag is still working in progress.
    #[arg(long, verbatim_doc_comment)]
    #[cfg_attr(not(feature = "cuttlefish"), arg(env = "http_proxy"))]
    pub http_proxy: Option<String>,

    /// Use TAP interface instead of libslirp for Wi-Fi
    /// WARNING: This flag is still working in progress.
    #[arg(long)]
    pub wifi_tap: Option<String>,

    /// Customize Wi-Fi with a required SSID and optional password (min 8 characters)
    #[arg(short, long, num_args = 1..=2, value_names = &["ssid", "password"])]
    pub wifi: Option<Vec<String>>,

    /// Start with test beacons
    #[arg(long, alias = "test_beacons", overrides_with("no_test_beacons"))]
    pub test_beacons: bool,

    /// Do not start with test beacons
    #[arg(long, alias = "no_test_beacons", overrides_with("test_beacons"))]
    pub no_test_beacons: bool,

    /// Disable netsimd from shutting down automatically.
    /// WARNING: This flag is for development purpose. netsimd will not shutdown without SIGKILL.
    #[arg(long, alias = "no_shutdown")]
    pub no_shutdown: bool,

    /// Entering Verbose mode
    #[arg(short = 'v', long)]
    pub verbose: bool,

    /// Print Netsimd version information
    #[arg(long)]
    pub version: bool,

    #[command(flatten)]
    pub debug: DebugArgs,

    /// Set RSSI (in dBm) for all links on a specified PhyKind.
    /// Accepts `PHY_KIND:RSSI_VALUE` (e.g., `BLE:-65`).
    /// This flag can be specified multiple times for different PhyKinds (e.g., `--rssi=bt_classic:-65 --rssi=ble:-72`).
    /// `PHY_KIND` is case-insensitive (aliases like "ble" supported). `RSSI_VALUE` must be an i8.
    ///
    /// # Limitations
    /// * RSSI control is currently implemented for BLE and BT_CLASSIC only.
    #[arg(long, value_name = "PHY_KIND:RSSI_VALUE", action = clap::ArgAction::Append, verbatim_doc_comment)]
    pub rssi: Option<Vec<String>>,
}

#[derive(Args, Debug, Clone, Default)]
pub struct DebugArgs {
    /// Disable all packet processing and forwarding.
    #[arg(long)]
    pub debug_no_traffic: bool,

    /// Disable forwarding packets to the external network.
    #[arg(long)]
    pub debug_no_network: bool,

    /// Disable packet forwarding between simulated devices.
    #[arg(long)]
    pub debug_no_wmedium: bool,

    /// Disable mDNS traffic between simulated devices.
    #[arg(long)]
    pub debug_no_mdns_wmedium: bool,

    /// Disable mDNS traffic from guest to host.
    #[arg(long)]
    pub debug_no_guest_to_host_mdns: bool,
}
