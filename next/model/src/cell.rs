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

use std::net::IpAddr;

use serde::{Deserialize, Serialize};

use crate::chip::ChipId;

pub const MODEM_STATE_DOWN: &str = "down";
pub const MODEM_STATE_RINGING: &str = "ringing";
pub const MODEM_STATE_IDLE: &str = "idle";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CellNetworkConfig {
    pub ip_address: IpAddr,
    pub prefixlen: u8,
    pub gateway: IpAddr,
    pub dns: IpAddr,
}

/// Parameters for creating a Cellular chip.
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
pub struct CellCreate {
    pub sim_type: Option<i32>,
    pub sim_profile: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub network_configs: Vec<CellNetworkConfig>,
}

/// Quirks for compatibility with different guest-side implementations.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Quirks {
    /// Flag for compatibility with Goldfish RIL in SDK 37 and earlier.
    pub goldfish_ril_37_or_earlier: bool,
    /// Flag indicating if the guest device is Cuttlefish.
    pub is_cuttlefish: bool,
    /// Flag indicating whether CTZV (NITZ time zone reporting) should be
    /// enabled by default.
    pub auto_ctzv: bool,
    /// Flag indicating whether IPv6 PDP contexts should fall back to IPv4
    /// when only IPv4 network configurations are available.
    pub allow_ipv4_for_ipv6: bool,
    /// Flag indicating whether to dynamically allocate a SLIRP IP lease
    /// when network configuration is not pre-provisioned by the host.
    #[serde(default)]
    pub dynamic_slirp_lease: bool,
}

/// Cellular technology specific chip information.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Cell {
    #[serde(default)]
    pub radio: crate::chip::Radio,
    /// A string representing the current state of the cellular modem.
    pub state: String,
    /// SIM card type (0 = No SIM, 1 = Normal SIM, etc.).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sim_type: Option<i32>,
    /// XML SIM ICC profile content.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sim_profile: Option<String>,
    /// Quirks for compatibility with different guest-side implementations.
    #[serde(default)]
    pub quirks: Quirks,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub network_configs: Vec<CellNetworkConfig>,
    #[serde(default)]
    pub sms_count: u32,
    #[serde(default)]
    pub rssi: u32,
    #[serde(default)]
    pub ber: u32,
    #[serde(default)]
    pub voice_registration: RegistrationStatus,
    #[serde(default)]
    pub data_registration: RegistrationStatus,
    #[serde(default)]
    pub active_calls: Vec<Call>,
}

impl Default for Cell {
    fn default() -> Self {
        Self {
            radio: crate::chip::Radio::default(),
            state: "idle".to_string(),
            sim_type: None,
            sim_profile: None,
            quirks: Quirks::default(),
            network_configs: Vec::new(),
            sms_count: 0,
            rssi: 0,
            ber: 0,
            voice_registration: RegistrationStatus::default(),
            data_registration: RegistrationStatus::default(),
            active_calls: Vec::new(),
        }
    }
}

#[derive(Debug, PartialEq, Eq, Clone, Serialize, Deserialize)]
pub struct Call {
    pub number: String,
    pub state: CallState,
    pub direction: CallDirection,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy, Serialize, Deserialize, Default)]
pub enum CallState {
    #[default]
    Unknown = 0,
    Active = 1,
    Holding = 2,
    Dialing = 3,
    Alerting = 4,
    Incoming = 5,
    Waiting = 6,
}

/// Standard 3GPP terms for Outgoing (Mobile Originated) and Incoming (Mobile
/// Terminated) calls
#[derive(Debug, PartialEq, Eq, Clone, Copy, Serialize, Deserialize, Default)]
pub enum CallDirection {
    #[default]
    Unknown = 0,
    MobileOriginated = 1,
    MobileTerminated = 2,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy, Serialize, Deserialize, Default)]
pub enum RegistrationStatus {
    #[default]
    NotRegistered = 0,
    RegisteredHome = 1,
    Searching = 2,
    Denied = 3,
    Unknown = 4,
    Roaming = 5,
    RegisteredSmsOnlyHome = 6,
    RegisteredSmsOnlyRoaming = 7,
    Emergency = 8,
}

impl RegistrationStatus {
    /// Returns true if registered for normal voice/data service (Home or
    /// Roaming).
    pub fn is_registered(self) -> bool {
        matches!(self, Self::RegisteredHome | Self::Roaming)
    }

    /// Returns true if attached for emergency bearer services only.
    pub fn is_emergency_only(self) -> bool {
        matches!(self, Self::Emergency)
    }
}

#[derive(Debug, PartialEq, Eq, Clone, Copy, Serialize, Deserialize)]
pub enum RadioTechnology {
    Unknown = 0,
    Gsm = 1,
    Lte = 2,
    Nr = 3,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum ModemAction {
    ProcessAtCommand { id: ChipId, command: Vec<u8> },
    SetSignalStrength { id: ChipId, rssi: u8, ber: u8 },
    SetVoiceRegistration { id: ChipId, status: RegistrationStatus },
    SetDataRegistration { id: ChipId, status: RegistrationStatus },
    IncomingCall { target_id: ChipId, number: String },
    RemoteAnswer { id: ChipId },
    RemoteHold { id: ChipId, on_hold: bool },
    RemoteHangup { id: ChipId },
    IncomingSms { id: ChipId, sender: String, text: String },
    IncomingPdu { id: ChipId, pdu: String },
    InjectCellBroadcast { id: ChipId, serial_number: u16, message_id: u16, dcs: u8, text: String },
    UpdatePhysicalChannelConfigs { id: ChipId },
    UpdateNetworkTime { id: ChipId },
    SetSimStatus { id: ChipId, present: bool },
    SetNetworkTechnology { id: ChipId, tech: RadioTechnology },
    SetOperator { id: ChipId, operator: String },
}

/// Cellular specific chip updates.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CellUpdate {
    #[serde(default)]
    pub radio: crate::chip::RadioUpdate,
    pub state: Option<String>,
    /// Cellular network configurations for future data calls.
    ///
    /// Note: do not signal changes via `+CGEV` (b/562098771).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub network_configs: Option<Vec<CellNetworkConfig>>,
}

impl CellUpdate {
    pub fn apply(&self, cell: &mut Cell) {
        self.radio.apply(&mut cell.radio);
        if let Some(state) = &self.state {
            cell.state.clone_from(state);
        }
        if let Some(cfgs) = &self.network_configs {
            cell.network_configs.clone_from(cfgs);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chip::{Radio, RadioUpdate};

    #[test]
    fn test_cell_serde_json_roundtrip() {
        let cell = Cell {
            radio: Radio { state: Some(true), range: 0.0, tx_count: 10, rx_count: 20 },
            state: MODEM_STATE_IDLE.to_string(),
            rssi: 15,
            ber: 1,
            sms_count: 2,
            voice_registration: RegistrationStatus::RegisteredHome,
            data_registration: RegistrationStatus::Roaming,
            active_calls: vec![Call {
                number: "12345".to_string(),
                state: CallState::Active,
                direction: CallDirection::MobileOriginated,
            }],
            ..Default::default()
        };
        let json = serde_json::to_string(&cell).unwrap();
        let decoded: Cell = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, cell);
    }

    #[test]
    fn test_cell_update_serde_json_roundtrip() {
        let update = CellUpdate {
            radio: RadioUpdate { state: Some(false) },
            state: Some(MODEM_STATE_DOWN.to_string()),
            network_configs: None,
        };
        let json = serde_json::to_string(&update).unwrap();
        let decoded: CellUpdate = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, update);
    }
}
