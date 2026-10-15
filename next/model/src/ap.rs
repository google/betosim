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

//! Access Point (AP) configuration module.
//!
//! This module defines the types and structures required to configure
//! and manage Access Points within the simulation. It includes the
//! `WifiMode` for PHY layer configuration and `ApCreate`/`ApUpdate`
//! for lifecycle management.

use std::fmt;

use serde::{Deserialize, Serialize};

pub const DEFAULT_WIFI_SSID: &str = "AndroidWifi";
pub const DEFAULT_WIFI_BSSID: &str = "02:15:b2:00:00:00";

/// Supported Wi-Fi 802.11 PHY modes.
///
/// This enum maps 1:1 with the expected "hw_mode" configurations
/// used by hostapd and typically exposed via CLI or configuration files.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum WifiMode {
    /// 802.11a (5 GHz)
    #[serde(rename = "a")]
    A,
    /// 802.11b (2.4 GHz)
    #[serde(rename = "b")]
    B,
    /// 802.11g (2.4 GHz)
    #[serde(rename = "g")]
    #[default]
    G,
    /// 802.11n (Wi-Fi 4)
    #[serde(rename = "n")]
    N,
    /// 802.11ac (Wi-Fi 5)
    #[serde(rename = "ac")]
    Ac,
    /// 802.11ax (Wi-Fi 6)
    #[serde(rename = "ax")]
    Ax,
}

impl fmt::Display for WifiMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::A => write!(f, "a"),
            Self::B => write!(f, "b"),
            Self::G => write!(f, "g"),
            Self::N => write!(f, "n"),
            Self::Ac => write!(f, "ac"),
            Self::Ax => write!(f, "ax"),
        }
    }
}

/// Parameters for creating an Access Point chip.
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
pub struct ApCreate {
    pub ssid: String,
    pub bssid: String,
    pub channel: u8,
    pub hw_mode: WifiMode,
    pub wpa_passphrase: Option<String>,
    pub beacon_interval: u16,
    pub country_code: Option<String>,
    pub dtim_period: u8,
    pub hidden_ssid: bool,
    pub sae: bool,
    pub wmm_enabled: bool,
    pub enterprise_enabled: bool,
    pub mac_acl_mode: u8,
    pub mac_acl_list: Vec<String>,
    pub ftm_responder_enabled: bool,
}

/// Parameters for updating an Access Point chip.
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
pub struct ApUpdate {
    pub ssid: Option<String>,
    pub channel: Option<u8>,
    #[serde(default)]
    pub force_disconnect: Vec<String>,
}

/// Access Point specific chip information.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Ap {
    #[serde(default)]
    pub id: u32,
    pub config: ApCreate,
    /// MAC addresses of the stations currently associated with this AP.
    ///
    /// Mirrors `AccessPoint.connected_devices` on the gRPC surface. Defaulted
    /// rather than required so a client may omit it when sending an `Ap`.
    #[serde(default)]
    pub associations: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `associations` carried a `#[serde(skip)]`, so `GET /v1/aps` computed the
    /// association list and serde silently dropped it from the response body.
    #[test]
    fn ap_associations_are_serialized() {
        let ap = Ap {
            id: 1,
            config: ApCreate::default(),
            associations: vec!["02:15:B2:00:00:01".to_string()],
        };

        let val = serde_json::to_value(&ap).expect("Ap must serialize");

        assert_eq!(
            val["associations"],
            serde_json::json!(["02:15:B2:00:00:01"]),
            "associations must reach the wire, got: {val}"
        );
    }

    /// The fix replaced `skip` with `default` rather than dropping the
    /// attribute, so a client may still omit the field when sending an `Ap`.
    #[test]
    fn ap_associations_may_be_omitted_on_the_wire() {
        let ap: Ap = serde_json::from_value(serde_json::json!({
            "config": ApCreate::default()
        }))
        .expect("associations must be optional on input");

        assert!(ap.associations.is_empty());
    }
}
