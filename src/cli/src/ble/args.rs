// Copyright 2026 The Android Open Source Project
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

use clap::{Args, Subcommand};

#[derive(Debug, Subcommand, PartialEq)]
pub(crate) enum BleCommand {
    /// Start a Bluetooth Low Energy scan
    Scan(BleScan),
    /// Start a baseband packet sniffer
    #[command(aliases = ["sniffer", "snif"])]
    Sniff(BleSniff),
}

#[derive(Debug, Args, PartialEq)]
pub(crate) struct BleScan {
    /// X position of the scanner
    #[arg(long, default_value = "0.0")]
    pub x: f32,
    /// Y position of the scanner
    #[arg(long, default_value = "0.0")]
    pub y: f32,
    /// Z position of the scanner
    #[arg(long, default_value = "0.0")]
    pub z: f32,
    /// Send Active Scan Requests (SCAN_REQ) to trigger Scan Responses from
    /// beacons
    #[arg(long)]
    pub active: bool,
    /// Output raw binary PCAP stream to stdout for piping to tshark/Wireshark
    #[arg(long)]
    pub pcap: bool,
}

#[derive(Debug, Args, PartialEq)]
pub(crate) struct BleSniff {
    /// X position of the sniffer
    #[arg(long, default_value = "0.0")]
    pub x: f32,
    /// Y position of the sniffer
    #[arg(long, default_value = "0.0")]
    pub y: f32,
    /// Z position of the sniffer
    #[arg(long, default_value = "0.0")]
    pub z: f32,
    /// Output to a PCAP file
    #[arg(long, num_args(0..=1), default_missing_value = "-")]
    pub pcap: Option<String>,
    /// Link Type of the PCAP capture
    #[arg(long, value_enum, ignore_case = true, default_value_t = LinkTypeOption::Le)]
    pub link_type: LinkTypeOption,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum LinkTypeOption {
    /// Classic Bluetooth Baseband (LINKTYPE_BLUETOOTH_BREDR_BB)
    Bredr,
    /// Low Energy Link Layer (LINKTYPE_BLUETOOTH_LE_LL)
    Le,
}

impl BleScan {
    pub(super) fn query_path(&self) -> String {
        format!(
            "{}?x={}&y={}&z={}&active={}",
            netsim_rest_api::path::BLE_SCAN,
            self.x,
            self.y,
            self.z,
            self.active
        )
    }
}

impl BleSniff {
    pub(super) fn query_path(&self) -> String {
        format!("{}?x={}&y={}&z={}", netsim_rest_api::path::BLE_SNIFF, self.x, self.y, self.z)
    }
}

impl LinkTypeOption {
    pub(super) const fn link_type_id(self) -> u32 {
        match self {
            Self::Bredr => 255,
            Self::Le => 251,
        }
    }
}
