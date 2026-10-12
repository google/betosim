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

//! Netsim Common Types
//!
//! This crate contains common, low-level data structures used across Netsim
//! crates. By separating these types into their own crate, we avoid circular
//! dependencies and reduce compilation times.
//!
//! Major types included:
//! - `ChipInfo`: Information about a chip and its device.
//! - `ChipKind`: Enumeration of supported network technologies.

use serde::{Deserialize, Serialize};

/// Default manufacturer string for virtual chips.
const DEFAULT_MANUFACTURER: &str = "Netsim";

/// Default product name for virtual chips.
const DEFAULT_PRODUCT_NAME: &str = "Virtual Chip";

/// Default fallback name for devices when device information is unavailable.
const UNKNOWN_DEVICE_NAME: &str = "Unknown";

/// Information about the chip and device provided during connection setup.
///
/// This struct is used during the handshake phase to identify the connecting
/// chip and its parent device.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ChipInfo {
    /// The name of the chip (e.g., "bt-main").
    pub name: String,
    /// Detailed chip information, if available.
    pub chip: Option<Chip>,
    /// Detailed device information, if available.
    pub device_info: Option<DeviceInfo>,
}

impl ChipInfo {
    /// Creates a new ChipInfo for a specific chip kind.
    pub fn new<S: Into<String>>(name: S, kind: ChipKind) -> Self {
        let name = name.into();
        ChipInfo {
            device_info: Some(DeviceInfo::new(name.clone(), name.clone())),
            chip: Some(Chip::new(kind, name.clone())),
            name,
        }
    }

    /// Returns the device name from the device_info, or a default value.
    pub fn device_name(&self) -> String {
        self.device_info
            .as_ref()
            .map_or_else(|| UNKNOWN_DEVICE_NAME.to_string(), |d| d.name.clone())
    }

    /// Returns the kind of the chip, or `ChipKind::UNSPECIFIED` if not
    /// configured.
    pub fn chip_kind(&self) -> ChipKind {
        self.chip.as_ref().map_or(ChipKind::UNSPECIFIED, |c| c.kind)
    }
}

/// Details about a specific chip.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Chip {
    /// The kind of chip (Bluetooth, Wifi, etc.).
    pub kind: ChipKind,
    /// Unique identifier for the chip.
    pub id: String,
    /// Human-readable name of the chip.
    pub name: String,
    /// Manufacturer of the chip.
    pub manufacturer: String,
    /// Product name of the chip.
    pub product_name: String,
    /// Address of the chip (e.g. MAC address).
    pub address: String,
    /// SIM card type (0 = No SIM, 1 = Normal SIM, etc.).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sim_type: Option<i32>,
    /// XML SIM ICC profile content.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sim_profile: Option<String>,
    /// Echo every packet back to the sender instead of attaching a chip.
    /// For transport tests and benchmarks (`startup.Chip.loopback`).
    #[serde(default)]
    pub loopback: bool,
}

impl Chip {
    /// Creates a new chip with default virtual metadata.
    pub fn new<S: Into<String>>(kind: ChipKind, name: S) -> Self {
        let name = name.into();
        Chip {
            kind,
            id: name.clone(),
            name,
            manufacturer: DEFAULT_MANUFACTURER.to_string(),
            product_name: DEFAULT_PRODUCT_NAME.to_string(),
            address: String::new(),
            sim_type: None,
            sim_profile: None,
            loopback: false,
        }
    }
}

/// Details about the device hosting the chip.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct DeviceInfo {
    /// Human-readable name of the device.
    pub name: String,
    /// Unique identifier for the device.
    pub id: String,
    /// Identifier for device kind e.g. EMULATOR, CUTTLEFISH, BUMBLE, etc
    pub kind: String,
    /// Version info as applicable e.g. Android emulator version 34.1.15.0, etc
    pub version: String,
    /// SDK version info as applicable e.g. 33, 34, etc
    pub sdk_version: String,
    /// Build ID e.g. TE1A.220922.034, UQ1A.231205.015, etc
    pub build_id: String,
    /// Model/variant e.g. sdk_gphone_x86_64-userdebug, cf_x86_64_phone-user,
    /// etc
    pub variant: String,
    /// CPU architecture e.g. x86_64, arm64-v8a, etc
    pub arch: String,
    /// Path to the AVD directory, if applicable.
    pub avd_path: String,
}

impl DeviceInfo {
    /// Creates a new `DeviceInfo` with specified name and identifier.
    pub fn new<S1: Into<String>, S2: Into<String>>(name: S1, id: S2) -> Self {
        DeviceInfo { name: name.into(), id: id.into(), ..Default::default() }
    }
}

/// The kind of network technology the chip supports.
///
/// This enumeration is used to distinguish between different types of simulated
/// radios and to route packets to the correct handlers.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, Default)]
#[allow(non_camel_case_types)]
pub enum ChipKind {
    #[default]
    UNSPECIFIED,
    BLUETOOTH,
    WIFI,
    UWB,
    NFC,
    CELLULAR,
    CELLULAR_DATA,
    ETHERNET,
}

impl ChipKind {
    /// Returns the canonical uppercase string representation.
    pub const fn as_str(&self) -> &'static str {
        match self {
            ChipKind::UNSPECIFIED => "UNSPECIFIED",
            ChipKind::BLUETOOTH => "BLUETOOTH",
            ChipKind::WIFI => "WIFI",
            ChipKind::UWB => "UWB",
            ChipKind::NFC => "NFC",
            ChipKind::CELLULAR => "CELLULAR",
            ChipKind::CELLULAR_DATA => "CELLULAR_DATA",
            ChipKind::ETHERNET => "ETHERNET",
        }
    }

    /// Returns `true` if this radio technology supports wireless peer-to-peer
    /// or broadcast medium.
    pub const fn is_wireless(&self) -> bool {
        matches!(self, ChipKind::BLUETOOTH | ChipKind::WIFI | ChipKind::UWB | ChipKind::NFC)
    }

    /// Returns `true` if this radio technology is cellular/telephony.
    pub const fn is_cellular(&self) -> bool {
        matches!(self, ChipKind::CELLULAR | ChipKind::CELLULAR_DATA)
    }
}

impl std::str::FromStr for ChipKind {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s.to_uppercase().as_str() {
            "UNSPECIFIED" => Ok(ChipKind::UNSPECIFIED),
            "BLUETOOTH" | "BT" => Ok(ChipKind::BLUETOOTH),
            "WIFI" => Ok(ChipKind::WIFI),
            "UWB" => Ok(ChipKind::UWB),
            "NFC" => Ok(ChipKind::NFC),
            "CELLULAR" | "MODEM" => Ok(ChipKind::CELLULAR),
            "CELLULAR_DATA" => Ok(ChipKind::CELLULAR_DATA),
            "ETHERNET" => Ok(ChipKind::ETHERNET),
            _ => Err(format!("invalid chip kind: {}", s)),
        }
    }
}

impl std::fmt::Display for ChipKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::fmt::Display for ChipInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} on device {}", self.chip_kind(), self.device_name())
    }
}
