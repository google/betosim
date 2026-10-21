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

impl std::str::FromStr for WifiMode {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "a" | "802.11a" => Ok(Self::A),
            "b" | "802.11b" => Ok(Self::B),
            "g" | "802.11g" => Ok(Self::G),
            "n" | "802.11n" => Ok(Self::N),
            "ac" | "802.11ac" => Ok(Self::Ac),
            "ax" | "802.11ax" => Ok(Self::Ax),
            _ => Err(format!("Unsupported Wi-Fi mode: {s}")),
        }
    }
}

/// Parameters for creating an Access Point chip.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
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

impl Default for ApCreate {
    fn default() -> Self {
        Self {
            ssid: DEFAULT_WIFI_SSID.to_string(),
            bssid: String::new(),
            channel: 6,
            hw_mode: WifiMode::G,
            wpa_passphrase: None,
            beacon_interval: 200,
            country_code: Some("US".to_string()),
            dtim_period: 2,
            hidden_ssid: false,
            sae: false,
            wmm_enabled: true,
            enterprise_enabled: false,
            mac_acl_mode: 0,
            mac_acl_list: Vec::new(),
            ftm_responder_enabled: true,
        }
    }
}

fn null_to_default<'de, D, T>(d: D) -> Result<T, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Default + Deserialize<'de>,
{
    Ok(Option::<T>::deserialize(d)?.unwrap_or_default())
}

/// Parameters for updating an Access Point chip.
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
pub struct ApUpdate {
    pub ssid: Option<String>,
    pub channel: Option<u8>,
    #[serde(default, deserialize_with = "null_to_default")]
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

    #[test]
    fn ap_create_defaults_fill_omitted_json_fields() {
        let ap_create: ApCreate =
            serde_json::from_str("{}").expect("empty JSON must deserialize with default fields");
        assert_eq!(ap_create.ssid, DEFAULT_WIFI_SSID);
        assert_eq!(ap_create.bssid, "");
        assert_eq!(ap_create.channel, 6);
        assert_eq!(ap_create.hw_mode, WifiMode::G);
        assert!(ap_create.wpa_passphrase.is_none());
        assert_eq!(ap_create.beacon_interval, 200);
        assert_eq!(ap_create.country_code.as_deref(), Some("US"));
        assert_eq!(ap_create.dtim_period, 2);
        assert!(!ap_create.hidden_ssid);
        assert!(!ap_create.sae);
        assert!(ap_create.wmm_enabled);
        assert!(!ap_create.enterprise_enabled);
        assert_eq!(ap_create.mac_acl_mode, 0);
        assert!(ap_create.mac_acl_list.is_empty());
        assert!(ap_create.ftm_responder_enabled);

        let partial: ApCreate = serde_json::from_str(r#"{"ssid":"CustomSSID","channel":11}"#)
            .expect("partial JSON must deserialize with defaults");
        assert_eq!(partial.ssid, "CustomSSID");
        assert_eq!(partial.channel, 11);
        assert_eq!(partial.bssid, "");
        assert_eq!(partial.hw_mode, WifiMode::G);
    }

    #[test]
    fn ap_update_null_or_omitted_force_disconnect() {
        let update_null: ApUpdate = serde_json::from_str(r#"{"force_disconnect": null}"#)
            .expect("null force_disconnect must deserialize to empty vec");
        assert!(update_null.force_disconnect.is_empty());

        let update_omitted: ApUpdate =
            serde_json::from_str("{}").expect("omitted force_disconnect must deserialize to empty");
        assert!(update_omitted.force_disconnect.is_empty());

        let update_list: ApUpdate =
            serde_json::from_str(r#"{"force_disconnect": ["02:15:b2:00:01:00"]}"#)
                .expect("explicit force_disconnect array must deserialize");
        assert_eq!(update_list.force_disconnect, vec!["02:15:b2:00:01:00"]);
    }

    #[test]
    fn wifi_mode_from_str() {
        assert_eq!("a".parse::<WifiMode>().unwrap(), WifiMode::A);
        assert_eq!("802.11a".parse::<WifiMode>().unwrap(), WifiMode::A);
        assert_eq!("B".parse::<WifiMode>().unwrap(), WifiMode::B);
        assert_eq!("802.11b".parse::<WifiMode>().unwrap(), WifiMode::B);
        assert_eq!("g".parse::<WifiMode>().unwrap(), WifiMode::G);
        assert_eq!("802.11G".parse::<WifiMode>().unwrap(), WifiMode::G);
        assert_eq!("n".parse::<WifiMode>().unwrap(), WifiMode::N);
        assert_eq!("802.11n".parse::<WifiMode>().unwrap(), WifiMode::N);
        assert_eq!("ac".parse::<WifiMode>().unwrap(), WifiMode::Ac);
        assert_eq!("802.11AC".parse::<WifiMode>().unwrap(), WifiMode::Ac);
        assert_eq!("ax".parse::<WifiMode>().unwrap(), WifiMode::Ax);
        assert_eq!("802.11ax".parse::<WifiMode>().unwrap(), WifiMode::Ax);
        assert!("unsupported".parse::<WifiMode>().is_err());
    }
}
