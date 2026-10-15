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
pub(crate) enum ApCommand {
    /// List all current Access Points
    List(ApList),
    /// Create a new Access Point
    Create(ApCreate),
    /// Modify an existing Access Point
    Patch(ApPatch),
    /// Remove an Access Point
    #[command(alias("delete"))]
    Remove(ApRemove),
    /// Disconnect a client from an Access Point
    Disconnect(ApDisconnect),
}

#[derive(Debug, Args, PartialEq, Default)]
pub(crate) struct ApList {
    /// Continuously print AP information every second
    #[arg(short, long)]
    pub continuous: bool,
}

#[derive(Debug, Args, PartialEq)]
pub(crate) struct ApCreate {
    /// Name (SSID) of the Access Point
    #[arg(long)]
    pub ssid: Option<String>,
    /// Wi-Fi Channel
    #[arg(long)]
    pub channel: Option<u32>,
    /// BSSID (MAC Address)
    #[arg(long)]
    pub bssid: Option<String>,

    /// WPA2 Personal Password
    #[arg(long)]
    pub password: Option<String>,
    /// Wi-Fi Protocol / Hardware Mode ("a", "b", "g", "n", "ac", "ax")
    #[arg(long)]
    pub protocol: Option<String>,
}

#[derive(Debug, Args, PartialEq)]
pub(crate) struct ApPatch {
    /// ID of the Access Point
    pub id: u32,
    /// New Name (SSID)
    #[arg(long)]
    pub ssid: Option<String>,
    /// New Wi-Fi Channel
    #[arg(long)]
    pub channel: Option<u32>,
}

#[derive(Debug, Args, PartialEq)]
pub(crate) struct ApRemove {
    /// ID of the Access Point to remove
    pub id: u32,
}

#[derive(Debug, Args, PartialEq)]
pub(crate) struct ApDisconnect {
    /// ID of the Access Point
    pub id: u32,
    /// MAC Address of the client to disconnect
    pub mac_address: String,
}
