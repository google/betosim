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

//! # Netsim Next
//!
//! This crate provides the main binary for the netsim daemon, `netsimd`.
//! It is responsible for handling frontend requests and managing the simulation
//! state.
//!
//! ## Internal Module Structure
//!
//! Here is a dependency graph of the internal modules:
//!
//! ![Internal Module Graph](modules.png)

pub(crate) mod args;
pub(crate) mod avd_config;
pub(crate) mod cuttlefish;
pub(crate) mod ini_file;
pub(crate) mod logger;
pub(crate) mod netsimd;
pub(crate) mod platform;
pub(crate) mod test_beacons;
pub(crate) mod version;

pub use args::{Args, ClapWifiMode};
pub use avd_config::{is_valid_mac_address, resolve_bluetooth_mac};
pub use ini_file::{IniFile, IniFileAccess, IniFileInitialized};
pub use netsimd::{NetsimDaemon, RunResult, StartUpMode, run};
