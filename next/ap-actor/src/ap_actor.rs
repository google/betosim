// Copyright 2025-2026 The Android Open Source Project
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

use std::collections::HashMap;

use netsim_model::{
    Ap, ApCreate, ApUpdate as ModelApUpdate, DEFAULT_WIFI_SSID, Position, WifiMode,
};
use netsim_packets::MacAddr;
use serde::{Deserialize, Serialize};

use crate::{ieee802_11::Ieee80211Manager, shared, wpa_auth};

/// ID for an Access Point instance within this actor.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ApId(pub u32);

impl std::fmt::Display for ApId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<u32> for ApId {
    fn from(id: u32) -> Self {
        Self(id)
    }
}

impl From<ApId> for u32 {
    fn from(id: ApId) -> Self {
        id.0
    }
}

/// Shared Stream ID for the singleton packet stream (matching SLIRP_ID
/// convention)
pub(crate) const WIFI_STREAM_ID: u32 = u32::MAX - 1;

/// Configuration for creating a new Access Point.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ApConfig {
    pub ssid: String,
    pub bssid: MacAddr,
    pub channel: u8,
    pub hw_mode: WifiMode,
    pub wpa_passphrase: Option<String>,
    #[serde(default = "default_beacon_interval")]
    pub beacon_interval: u16,
    pub country_code: Option<String>,
    #[serde(default = "default_dtim_period")]
    pub dtim_period: u8,
    #[serde(default)]
    pub hidden_ssid: bool,
    #[serde(default)]
    pub sae: bool,
    #[serde(default = "default_wmm_enabled")]
    pub wmm_enabled: bool,
    #[serde(default)]
    pub enterprise_enabled: bool,
    #[serde(default)]
    pub mac_acl_mode: u8, // 0=Disable, 1=Deny, 2=Allow
    #[serde(default)]
    pub mac_acl_list: Vec<MacAddr>,
    #[serde(default = "default_ftm_responder_enabled")]
    pub ftm_responder_enabled: bool,
    #[serde(default)]
    pub position: Position,
}

/// Base MAC address for simulated Wi-Fi Access Points (02:15:b2:00:00:00).
pub const DEFAULT_AP_BSSID: MacAddr = MacAddr::new([0x02, 0x15, 0xb2, 0x00, 0x00, 0x00]);

const ZERO_BSSID: MacAddr = MacAddr::new([0; 6]);

/// Computes the unique BSSID for an AP based on its ID offset.
const fn compute_ap_bssid(id: ApId) -> MacAddr {
    let offset_id = (id.0 as u16) + 1;
    let [b0, b1] = offset_id.to_be_bytes();
    let mut bytes = DEFAULT_AP_BSSID.bytes;
    let [.., b4, b5] = &mut bytes;
    *b4 = b0;
    *b5 = b1;
    MacAddr::new(bytes)
}

/// Reserved ID for the built-in default AP created at daemon startup; dynamic
/// AP IDs start at 1.
pub(crate) const DEFAULT_AP_ID: ApId = ApId(0);

/// Precomputed BSSID for the Built-in Default AP (ID 0).
pub const EXPECTED_DEFAULT_AP_BSSID: MacAddr = compute_ap_bssid(DEFAULT_AP_ID);

/// Returns the expected BSSID for the Built-in Default AP (ID 0).
pub const fn expected_default_ap_bssid() -> MacAddr {
    EXPECTED_DEFAULT_AP_BSSID
}

fn default_ftm_responder_enabled() -> bool {
    true
}

fn default_wmm_enabled() -> bool {
    true
}

fn default_beacon_interval() -> u16 {
    200
}

fn default_dtim_period() -> u8 {
    2
}

impl Default for ApConfig {
    fn default() -> Self {
        Self {
            ssid: DEFAULT_WIFI_SSID.to_string(),
            bssid: DEFAULT_AP_BSSID,
            channel: 6,
            hw_mode: WifiMode::G,
            wpa_passphrase: None,
            beacon_interval: default_beacon_interval(),
            country_code: Some("US".to_string()),
            dtim_period: default_dtim_period(),
            hidden_ssid: false,
            sae: false,
            wmm_enabled: default_wmm_enabled(),
            enterprise_enabled: false,
            mac_acl_mode: 0,
            mac_acl_list: Vec::new(),
            ftm_responder_enabled: default_ftm_responder_enabled(),
            position: Default::default(),
        }
    }
}

impl From<ApConfig> for ApCreate {
    fn from(val: ApConfig) -> Self {
        let bssid = if val.bssid == DEFAULT_AP_BSSID || val.bssid == ZERO_BSSID {
            String::new()
        } else {
            val.bssid.to_string()
        };
        Self {
            ssid: val.ssid,
            bssid,
            channel: val.channel,
            hw_mode: val.hw_mode,
            wpa_passphrase: val.wpa_passphrase,
            beacon_interval: val.beacon_interval,
            country_code: val.country_code,
            dtim_period: val.dtim_period,
            hidden_ssid: val.hidden_ssid,
            sae: val.sae,
            wmm_enabled: val.wmm_enabled,
            enterprise_enabled: val.enterprise_enabled,
            mac_acl_mode: val.mac_acl_mode,
            mac_acl_list: val.mac_acl_list.into_iter().map(|s| s.to_string()).collect(),
            ftm_responder_enabled: val.ftm_responder_enabled,
        }
    }
}

impl TryFrom<ApCreate> for ApConfig {
    type Error = String;

    fn try_from(val: ApCreate) -> Result<Self, Self::Error> {
        let defaults = Self::default();
        let bssid = if val.bssid.is_empty() {
            defaults.bssid
        } else {
            let parsed: netsim_packets::MacAddr =
                val.bssid.parse().map_err(|e| format!("invalid BSSID: {}", e))?;
            if parsed == ZERO_BSSID {
                return Err("invalid BSSID: all zeros not permitted".to_string());
            }
            parsed
        };
        Ok(Self {
            ssid: if val.ssid.is_empty() { defaults.ssid } else { val.ssid },
            bssid,
            channel: if val.channel == 0 { defaults.channel } else { val.channel },
            hw_mode: val.hw_mode,
            wpa_passphrase: val.wpa_passphrase,
            beacon_interval: if val.beacon_interval == 0 {
                defaults.beacon_interval
            } else {
                val.beacon_interval
            },
            country_code: val.country_code,
            dtim_period: if val.dtim_period == 0 { defaults.dtim_period } else { val.dtim_period },
            hidden_ssid: val.hidden_ssid,
            sae: val.sae,
            wmm_enabled: val.wmm_enabled,
            enterprise_enabled: val.enterprise_enabled,
            mac_acl_mode: val.mac_acl_mode,
            mac_acl_list: val
                .mac_acl_list
                .into_iter()
                .map(|s| s.parse().map_err(|e| format!("Invalid MAC in ACL: {}", e)))
                .collect::<Result<Vec<_>, _>>()?,
            ftm_responder_enabled: val.ftm_responder_enabled,
            position: Default::default(),
        })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ApUpdate {
    pub ssid: Option<String>,
    pub channel: Option<u8>,
    #[serde(default)]
    pub force_disconnect: Vec<String>,
}

impl From<ModelApUpdate> for ApUpdate {
    fn from(val: ModelApUpdate) -> Self {
        Self { ssid: val.ssid, channel: val.channel, force_disconnect: val.force_disconnect }
    }
}

/// Consolidated update struct for ApActor.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ApActorUpdate {
    pub position: Option<Position>,
    pub enabled: Option<bool>,
    pub ap_update: Option<ApUpdate>,
}

pub enum ApReq {
    Register {
        stream: std::pin::Pin<Box<dyn tokio_stream::Stream<Item = bytes::Bytes> + Sync + Send>>,
        sink: tokio::sync::mpsc::UnboundedSender<bytes::Bytes>,
        shared_keys: std::sync::Arc<shared::SharedKeyStore>,
        beacon_interval: std::time::Duration,
    },
    Disconnect {
        mac: MacAddr,
    },
    Reset,
}

impl std::fmt::Debug for ApReq {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ApReq::Register { .. } => write!(f, "ApReq::Register {{ ... }}"),
            ApReq::Disconnect { mac } => {
                f.debug_struct("ApReq::Disconnect").field("mac", mac).finish()
            }
            ApReq::Reset => write!(f, "ApReq::Reset"),
        }
    }
}

/// Responses from the ApActor (Output of Action).
#[derive(Debug, Clone)]
pub enum ApResponse {
    Ok,
}

// Internal events removed as we return packets directly now.

#[derive(Clone, Debug)]
pub struct ApActor {
    pub(crate) sink: Option<tokio::sync::mpsc::UnboundedSender<bytes::Bytes>>,
    pub(crate) aps: HashMap<ApId, ApState>,
    pub(crate) manager: Ieee80211Manager,
    pub(crate) shared_keys: std::sync::Arc<shared::SharedKeyStore>,
    pub(crate) next_ap_id: u32,
    pub(crate) beacon_interval: Option<u16>, // In TUs (1024us)
    pub(crate) default_ap_config: Option<ApConfig>,
}

#[derive(Clone, Debug)]
pub struct ApState {
    pub id: ApId,
    pub config: ApConfig,
    pub(crate) wpa: Option<wpa_auth::WpaAuthenticator>,
    pub(crate) sae_sessions: HashMap<MacAddr, crate::sae::SaeStateMachine>,
    pub(crate) eap_sessions: HashMap<MacAddr, crate::eap_auth::EapAuthenticator>,
    pub associations: std::collections::HashSet<MacAddr>,
    pub(crate) delayed_frames: std::collections::VecDeque<(std::time::Instant, bytes::Bytes)>,
    pub(crate) enabled: bool,
    tx_sequence: u16,
    pub(crate) start_time: std::time::Instant,
}

impl ApActor {
    pub fn new(shared_keys: std::sync::Arc<shared::SharedKeyStore>) -> Self {
        Self {
            sink: None,
            aps: HashMap::new(),
            manager: Ieee80211Manager::new(),
            shared_keys,
            next_ap_id: DEFAULT_AP_ID.0 + 1,
            beacon_interval: None,
            default_ap_config: None,
        }
    }
}

impl Default for ApState {
    fn default() -> Self {
        Self {
            id: DEFAULT_AP_ID,
            config: ApConfig { bssid: EXPECTED_DEFAULT_AP_BSSID, ..ApConfig::default() },
            wpa: None,
            sae_sessions: HashMap::new(),
            eap_sessions: HashMap::new(),
            associations: std::collections::HashSet::new(),
            delayed_frames: std::collections::VecDeque::new(),
            enabled: true,
            tx_sequence: 0,
            start_time: std::time::Instant::now(),
        }
    }
}

impl ApState {
    pub(crate) fn new(id: ApId, mut config: ApConfig) -> Self {
        if config.bssid == DEFAULT_AP_BSSID {
            config.bssid = compute_ap_bssid(id);
        }

        Self {
            id,
            config,
            tx_sequence: (std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .subsec_micros()
                % 4096) as u16,
            ..Self::default()
        }
    }

    pub(crate) fn reset(&mut self, config: ApConfig) {
        *self = Self { id: self.id, config, ..Self::default() };
    }

    pub(crate) fn clear_station_state(&mut self, sta_mac: &MacAddr) {
        if self.associations.remove(sta_mac) {
            tracing::info!(
                "ApActor: Implicit deauth of {} from BSSID {}",
                sta_mac,
                self.config.bssid
            );
        }
        if let Some(wpa) = &mut self.wpa {
            wpa.remove_session(sta_mac);
        }
        self.sae_sessions.remove(sta_mac);
        self.eap_sessions.remove(sta_mac);
    }

    /// Fetches the next monotonic sequence control for 802.11 transmissions
    /// and advances the internal counter (wrapping at 4096).
    /// The Fragment Number is always set to 0.
    pub(crate) fn next_seq_control(&mut self) -> netsim_packets::SequenceControl {
        let sc = netsim_packets::SequenceControl::new(self.tx_sequence << 4);
        self.tx_sequence = (self.tx_sequence + 1) % 4096;
        sc
    }
}

impl From<ApState> for Ap {
    fn from(state: ApState) -> Self {
        Self {
            id: state.id.0,
            config: state.config.into(),
            associations: {
                // `ApState::associations` is a `HashSet<MacAddr>`, so format each
                // `MacAddr` and sort to keep the JSON array order stable across calls.
                let mut associations: Vec<String> =
                    state.associations.into_iter().map(|m| m.to_string()).collect();
                associations.sort();
                associations
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ap_state_reset() {
        let initial_config = ApConfig {
            ssid: "InitialSsid".to_string(),
            bssid: expected_default_ap_bssid(),
            channel: 6,
            ..ApConfig::default()
        };
        let mut state = ApState::new(DEFAULT_AP_ID, initial_config.clone());

        // Mutate fields
        state.config.ssid = "MutatedSsid".to_string();
        state.config.channel = 11;
        state.enabled = false;
        let sta_mac = MacAddr::new([0x02, 0x00, 0x00, 0x00, 0x00, 0x10]);
        state.associations.insert(sta_mac);
        state
            .delayed_frames
            .push_back((std::time::Instant::now(), bytes::Bytes::from_static(b"frame")));
        let _ = state.next_seq_control();

        assert_eq!(state.config.ssid, "MutatedSsid");
        assert_eq!(state.config.channel, 11);
        assert!(!state.enabled);
        assert!(!state.associations.is_empty());
        assert!(!state.delayed_frames.is_empty());

        // Reset to initial config
        state.reset(initial_config);

        assert_eq!(state.config.ssid, "InitialSsid");
        assert_eq!(state.config.channel, 6);
        assert!(state.enabled);
        assert!(state.wpa.is_none());
        assert!(state.sae_sessions.is_empty());
        assert!(state.eap_sessions.is_empty());
        assert!(state.associations.is_empty());
        assert!(state.delayed_frames.is_empty());
        let sc = state.next_seq_control();
        assert_eq!(sc.get(), 0);
    }

    #[test]
    fn test_ap_from_state() {
        let config = ApConfig {
            bssid: MacAddr::new([0x02, 0x11, 0x22, 0x33, 0x44, 0x55]),
            ..Default::default()
        };
        let mut state = ApState::new(ApId(42), config);
        state.associations.insert(MacAddr::new([0x00, 0x11, 0x22, 0x33, 0x44, 0xbb]));
        state.associations.insert(MacAddr::new([0x00, 0x11, 0x22, 0x33, 0x44, 0xaa]));

        let ap = Ap::from(state);
        assert_eq!(ap.id, 42);
        assert_eq!(ap.config.bssid, "02:11:22:33:44:55");
        assert_eq!(
            ap.associations,
            vec!["00:11:22:33:44:AA".to_string(), "00:11:22:33:44:BB".to_string()]
        );
    }

    #[test]
    fn test_zero_bssid_validation() {
        let create = ApCreate { bssid: "00:00:00:00:00:00".to_string(), ..Default::default() };
        let err = ApConfig::try_from(create).unwrap_err();
        assert!(err.contains("invalid BSSID: all zeros not permitted"));

        let config = ApConfig { bssid: ZERO_BSSID, ..Default::default() };
        let create: ApCreate = config.into();
        assert_eq!(create.bssid, "");
    }
}
