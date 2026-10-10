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

// src/network_service.rs

use std::iter::once;

use modem_rs_derive::CommandParser;
use netsim_model::{Quirks, RegistrationStatus};
use tracing::{info, warn};

use crate::{
    CardState, RadioAdmission,
    capabilities::AdmissionDenial,
    constants::{DEFAULT_OPERATOR_NAME_LONG, DEFAULT_OPERATOR_NAME_SHORT},
    parser::QuotedString,
    types::{
        AccessTechnology, AidlSignalStrength, CmeError, CopsFormat, CopsMode, CtecPreferredMask,
        CtecTechnology, ExecutionResult, OperatorStatus, Parsable, Plmn, RadioPowerLevel,
        RegistrationUnsolicitedMode, Response, SignalQuality,
    },
};

/// Network service AT commands.
#[derive(Debug, PartialEq, Clone, Copy, CommandParser)]
pub enum NetworkCommand<'a> {
    #[command(tag = "AT+COPS?")]
    QueryOperator,
    #[command(tag = "AT+COPS=?")]
    QueryAvailableOperators,
    #[command(tag = "AT+COPS=")]
    SetOperator {
        mode: CopsMode,
        format: Option<CopsFormat>,
        oper: Option<QuotedString<'a>>,
        act: Option<u8>,
    },
    #[command(tag = "AT+CREG?")]
    QueryVoiceNetworkRegistration,
    #[command(tag = "AT+CREG=")]
    SetVoiceNetworkRegistration(RegistrationUnsolicitedMode),
    #[command(tag = "AT+CGREG?")]
    QueryDataNetworkRegistration,
    #[command(tag = "AT+CGREG=")]
    SetDataNetworkRegistration(RegistrationUnsolicitedMode),
    #[command(tag = "AT+CEREG?")]
    QueryLteNetworkRegistration,
    #[command(tag = "AT+CEREG=")]
    SetLteNetworkRegistration(RegistrationUnsolicitedMode),
    #[command(tag = "AT+CFUN?")]
    QueryRadioPower,
    #[command(tag = "AT+CFUN=")]
    SetRadioPower(RadioPowerLevel, Option<bool>),
    #[command(tag = "AT+CSQ")]
    QuerySignalQuality,
    #[command(tag = "AT+CESQ")]
    QueryExtendedSignalQuality,
    #[command(tag = "AT+CTEC?")]
    QueryCurrentNetworkTechnology,
    #[command(tag = "AT+CTEC=?")]
    QuerySupportedNetworkTechnology,
    #[command(tag = "AT+CTEC=")]
    SetNetworkTechnology(u8, Option<CtecPreferredMask>),
}

const DUMMY_LAC: &str = "2142";
const DUMMY_CID: &str = "0000B804";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperatorInfo {
    pub status: OperatorStatus,
    pub long_name: String,
    pub short_name: String,
    pub numeric: String,
    pub act: AccessTechnology,
}

impl std::fmt::Display for OperatorInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "({},\"{}\",\"{}\",\"{}\",{})",
            self.status, self.long_name, self.short_name, self.numeric, self.act
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegistrationLocation {
    pub lac: String,
    pub cid: String,
    pub act: AccessTechnology,
}

impl std::fmt::Display for RegistrationLocation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "\"{}\",\"{}\",{}", self.lac, self.cid, self.act)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegistrationType {
    Voice,
    Data,
    Lte,
}

impl RegistrationType {
    pub const fn as_prefix(&self) -> &'static str {
        match self {
            RegistrationType::Voice => "+CREG",
            RegistrationType::Data => "+CGREG",
            RegistrationType::Lte => "+CEREG",
        }
    }
}

impl std::fmt::Display for RegistrationType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_prefix())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NetworkUrc {
    Registration {
        reg_type: RegistrationType,
        status: RegistrationStatus,
        location: Option<RegistrationLocation>,
    },
    SignalQuality(AidlSignalStrength),
}

impl std::fmt::Display for NetworkUrc {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NetworkUrc::Registration { reg_type, status, location } => {
                let stat = *status as u8;
                if let Some(loc) = location {
                    write!(f, "{reg_type}: {stat},{loc}\r\n")
                } else {
                    write!(f, "{reg_type}: {stat}\r\n")
                }
            }
            NetworkUrc::SignalQuality(ss) => write!(f, "+CSQ: {ss}\r\n"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NetworkResponse {
    OperatorQuery {
        is_registered: bool,
        mode: CopsMode,
        format: CopsFormat,
        plmn: String,
        quirks: Quirks,
    },
    SignalQuality(AidlSignalStrength),
    RegistrationQuery {
        reg_type: RegistrationType,
        unsol_mode: RegistrationUnsolicitedMode,
        status: RegistrationStatus,
        location: Option<RegistrationLocation>,
    },
    Ctec {
        current: CtecTechnology,
        preferred: u32,
    },
    CtecSupported(Vec<CtecTechnology>),
    CtecSetResult {
        urcs: Vec<NetworkUrc>,
    },
    RadioPower(RadioPowerLevel),
    Urcs(Vec<NetworkUrc>),
    AvailableOperators(Vec<OperatorInfo>),
}

impl std::fmt::Display for NetworkResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NetworkResponse::OperatorQuery { is_registered, mode, format, plmn, quirks } => {
                if *is_registered && *mode != CopsMode::Deregister {
                    match format {
                        CopsFormat::LongAlphanumeric => {
                            write!(f, "+COPS: {mode},0,\"{DEFAULT_OPERATOR_NAME_LONG}\"\r\n")
                        }
                        CopsFormat::ShortAlphanumeric => {
                            write!(f, "+COPS: {mode},1,\"{DEFAULT_OPERATOR_NAME_SHORT}\"\r\n")
                        }
                        CopsFormat::Numeric => write!(f, "+COPS: {mode},2,\"{plmn}\"\r\n"),
                    }
                } else if quirks.goldfish_ril_37_or_earlier {
                    // Legacy Goldfish RIL (SDK <= 37) AtResponse::COPS::parse
                    // expects 3 fields across compound
                    // queries (AT+COPS=3,0;+COPS?;+COPS=3,
                    // 1;+COPS?;+COPS=3,2;+COPS?)
                    // and fails on standard single-field "+COPS: <mode>".
                    // Emitting empty quotes strips to empty
                    // operator strings with RadioError::NONE.
                    write!(f, "+COPS: {mode},{format},\"\"\r\n")
                } else {
                    write!(f, "+COPS: {mode}\r\n")
                }
            }
            NetworkResponse::SignalQuality(ss) => write!(f, "+CSQ: {ss}\r\n"),
            NetworkResponse::RegistrationQuery { reg_type, unsol_mode, status, location } => {
                let stat = *status as u8;
                if let Some(loc) = location {
                    write!(f, "{reg_type}: {unsol_mode},{stat},{loc}\r\n")
                } else {
                    write!(f, "{reg_type}: {unsol_mode},{stat}\r\n")
                }
            }
            NetworkResponse::Ctec { current, preferred } => {
                write!(f, "+CTEC: {current},{preferred:X}\r\n")
            }
            // Goldfish RIL (`radio-service.ranchu`) expects AT+CTEC=? to advertise technology bit
            // indices (indexing into kTechsBitmask) rather than bitmasks.
            NetworkResponse::CtecSupported(techs) => {
                let tech_strs: Vec<String> =
                    techs.iter().map(|t| (*t as u8).trailing_zeros().to_string()).collect();
                write!(f, "+CTEC: {}\r\n", tech_strs.join(","))
            }
            NetworkResponse::CtecSetResult { urcs } => {
                write!(f, "+CTEC: DONE\r\n")?;
                for urc in urcs {
                    write!(f, "{urc}")?;
                }
                Ok(())
            }
            NetworkResponse::RadioPower(power) => {
                write!(f, "+CFUN: {power}\r\n")
            }
            NetworkResponse::Urcs(urcs) => {
                for urc in urcs {
                    write!(f, "{urc}")?;
                }
                Ok(())
            }
            NetworkResponse::AvailableOperators(operators) => {
                write!(f, "+COPS: ")?;
                for (i, op) in operators.iter().enumerate() {
                    if i > 0 {
                        write!(f, ",")?;
                    }
                    write!(f, "{op}")?;
                }
                write!(f, ",,(")?;
                let modes = [
                    CopsMode::Automatic,
                    CopsMode::Manual,
                    CopsMode::Deregister,
                    CopsMode::SetFormatOnly,
                    CopsMode::ManualAutomatic,
                ];
                for (i, m) in modes.iter().enumerate() {
                    if i > 0 {
                        write!(f, ",")?;
                    }
                    write!(f, "{m}")?;
                }
                write!(f, "),(")?;
                let formats = [
                    CopsFormat::LongAlphanumeric,
                    CopsFormat::ShortAlphanumeric,
                    CopsFormat::Numeric,
                ];
                for (i, fmt) in formats.iter().enumerate() {
                    if i > 0 {
                        write!(f, ",")?;
                    }
                    write!(f, "{fmt}")?;
                }
                write!(f, ")\r\n")
            }
        }
    }
}

type NetworkResult = Result<Option<NetworkResponse>, ExecutionResult>;

// Holds all state related to the network.
pub struct NetworkService {
    voice_registration: RegistrationStatus,
    data_registration: RegistrationStatus,
    signal_quality: SignalQuality,
    voice_unsol_mode: RegistrationUnsolicitedMode,
    data_unsol_mode: RegistrationUnsolicitedMode,
    lte_unsol_mode: RegistrationUnsolicitedMode,
    radio_power: RadioPowerLevel,
    plmn: Option<Plmn>,
    cops_mode: CopsMode,
    cops_format: CopsFormat,
    current_network_mode: CtecTechnology,
    preferred_network_mode: CtecPreferredMask,
    is_attached: bool,
    act: AccessTechnology,
    pub(crate) quirks: Quirks,
}

impl NetworkService {
    pub fn new(quirks: Quirks, home_plmn: Option<Plmn>) -> Self {
        Self {
            voice_registration: RegistrationStatus::NotRegistered,
            data_registration: RegistrationStatus::NotRegistered,
            signal_quality: SignalQuality::default(),
            voice_unsol_mode: RegistrationUnsolicitedMode::default(),
            data_unsol_mode: RegistrationUnsolicitedMode::default(),
            lte_unsol_mode: RegistrationUnsolicitedMode::default(),
            radio_power: RadioPowerLevel::default(),
            plmn: home_plmn,
            cops_mode: CopsMode::Automatic,
            // Non-standard default: While 3GPP TS 27.007 § 7.3 specifies format 0 (long format
            // alphanumeric) as the default when AT+COPS is queried without setting
            // format, Goldfish RIL (`reference-ril.c`) in `requestOperator`
            // (`RIL_REQUEST_OPERATOR`) expects `AT+COPS?` to return `response[2]` (`<oper>`)
            // as a 6-digit numeric MCC/MNC string (`310260` or `311740`). If an alphanumeric
            // operator string is returned, Goldfish RIL logs `requestOperator expected
            // mccmnc to be 6 decimal digits` and returns an error. Therefore, format 2
            // (numeric MCC/MNC) must be the default for Goldfish compatibility.
            cops_format: CopsFormat::Numeric,
            current_network_mode: CtecTechnology::Lte,
            preferred_network_mode: CtecPreferredMask(CtecTechnology::Nr as u32),
            is_attached: false,
            act: AccessTechnology::Lte,
            quirks,
        }
    }
}

impl NetworkService {
    pub fn is_attached(&self) -> bool {
        self.is_attached
    }
    pub fn is_radio_on(&self) -> bool {
        self.radio_power == RadioPowerLevel::Full
    }
    pub fn voice_registration(&self) -> RegistrationStatus {
        self.voice_registration
    }
    pub fn data_registration(&self) -> RegistrationStatus {
        self.data_registration
    }
    pub fn signal_quality(&self) -> SignalQuality {
        self.signal_quality
    }

    pub fn detach_network(&mut self) {
        self.is_attached = false;
    }

    pub fn set_home_plmn(&mut self, plmn: Option<Plmn>) {
        self.plmn = plmn;
    }

    pub(crate) fn set_operator_manual(
        &mut self,
        mode: CopsMode,
        oper: Option<&str>,
        sim: &impl CardState,
    ) -> NetworkResult {
        self.handle_set_operator(mode, Some(CopsFormat::LongAlphanumeric), oper, None, sim)
    }

    pub(crate) fn attach_network(&mut self, sim: &impl CardState) -> Vec<String> {
        self.attach_network_urcs(sim).iter().map(|u| u.to_string()).collect()
    }

    fn current_signal_strength(&self) -> AidlSignalStrength {
        AidlSignalStrength::from_quality(self.signal_quality, self.act)
    }

    fn attach_network_urcs(&mut self, sim: &impl CardState) -> Vec<NetworkUrc> {
        info!(
            "attach_network called! is_attached: {}, radio_power: {}",
            self.is_attached, self.radio_power
        );
        if !sim.is_present()
            || self.is_attached
            || self.radio_power == RadioPowerLevel::Minimum
            || self.radio_power == RadioPowerLevel::DisableRf
        {
            return Vec::new();
        }
        self.is_attached = true;
        // 3GPP TS 24.008 / TS 24.301: a card that is present but not yet
        // authenticated attaches for emergency bearer services only.
        if sim.is_ready() {
            self.voice_registration = RegistrationStatus::RegisteredHome;
            self.data_registration = RegistrationStatus::RegisteredHome;
        } else {
            self.voice_registration = RegistrationStatus::Emergency;
            self.data_registration = RegistrationStatus::NotRegistered;
        }

        self.tech_change_urcs()
    }

    pub fn set_registration(
        &mut self,
        reg_type: RegistrationType,
        status: RegistrationStatus,
    ) -> Option<String> {
        match reg_type {
            RegistrationType::Voice => {
                let old_status = self.voice_registration;
                self.voice_registration = status;
                if self.wire_registration_status(old_status)
                    != self.wire_registration_status(status)
                {
                    self.format_registration_urc(RegistrationType::Voice, status)
                        .map(|u| u.to_string())
                } else {
                    None
                }
            }
            RegistrationType::Data | RegistrationType::Lte => {
                let old_status = self.data_registration;
                self.data_registration = status;
                if self.wire_registration_status(old_status)
                    != self.wire_registration_status(status)
                {
                    let mut urcs = String::new();
                    if let Some(cgreg) =
                        self.format_registration_urc(RegistrationType::Data, status)
                    {
                        urcs.push_str(&cgreg.to_string());
                    }
                    if let Some(cereg) = self.format_registration_urc(RegistrationType::Lte, status)
                    {
                        urcs.push_str(&cereg.to_string());
                    }
                    if urcs.is_empty() { None } else { Some(urcs) }
                } else {
                    None
                }
            }
        }
    }

    pub fn set_network_technology(
        &mut self,
        tech: netsim_model::RadioTechnology,
    ) -> Option<String> {
        info!(
            "set_network_technology: tech={tech:?}, current_mode={}, act={}, is_attached={}",
            self.current_network_mode, self.act, self.is_attached
        );
        let (new_mode, new_act) = match tech {
            netsim_model::RadioTechnology::Gsm => (CtecTechnology::Gsm, AccessTechnology::Gsm),
            netsim_model::RadioTechnology::Lte => (CtecTechnology::Lte, AccessTechnology::Lte),
            netsim_model::RadioTechnology::Nr => (CtecTechnology::Nr, AccessTechnology::Nr),
            netsim_model::RadioTechnology::Unknown => {
                warn!("Unknown radio technology {:?}, falling back to LTE", tech);
                (CtecTechnology::Lte, AccessTechnology::Lte)
            }
        };

        if self.current_network_mode != new_mode || self.act != new_act {
            self.current_network_mode = new_mode;
            self.act = new_act;
            if self.is_attached {
                let urcs: String =
                    self.tech_change_urcs().into_iter().map(|u| u.to_string()).collect();
                if urcs.is_empty() { None } else { Some(urcs) }
            } else {
                None
            }
        } else {
            None
        }
    }

    /// Sets the signal strength and bit error rate.
    pub fn set_signal_strength(&mut self, rssi: u8, ber: u8) -> Option<String> {
        self.signal_quality = SignalQuality::new(rssi, ber);
        if self.is_radio_on() {
            Some(NetworkUrc::SignalQuality(self.current_signal_strength()).to_string())
        } else {
            None
        }
    }

    /// Applies `<AcT>`, returning whether the active technology changed.
    fn apply_resolved_act(&mut self, new_act: AccessTechnology) -> bool {
        let new_mode = match new_act {
            AccessTechnology::Gsm => CtecTechnology::Gsm,
            AccessTechnology::Wcdma => CtecTechnology::Wcdma,
            AccessTechnology::Lte => CtecTechnology::Lte,
            AccessTechnology::Nr => CtecTechnology::Nr,
        };

        if self.current_network_mode == new_mode && self.act == new_act {
            return false;
        }

        info!(
            "apply_resolved_act: mode={new_mode}, act={new_act} (was mode={}, act={})",
            self.current_network_mode, self.act
        );
        self.current_network_mode = new_mode;
        self.act = new_act;
        true
    }

    fn tech_change_urcs(&self) -> Vec<NetworkUrc> {
        self.all_registration_urcs()
            .into_iter()
            .chain(once(NetworkUrc::SignalQuality(self.current_signal_strength())))
            .collect()
    }

    // --- Pure command handlers ---

    fn handle_query_operator(&self) -> NetworkResult {
        let is_registered = matches!(
            self.voice_registration,
            RegistrationStatus::RegisteredHome | RegistrationStatus::Roaming
        ) || matches!(
            self.data_registration,
            RegistrationStatus::RegisteredHome | RegistrationStatus::Roaming
        );

        Ok(Some(NetworkResponse::OperatorQuery {
            is_registered,
            mode: self.cops_mode,
            format: self.cops_format,
            plmn: self.plmn.as_ref().map(|p| p.to_string()).unwrap_or_default(),
            quirks: self.quirks,
        }))
    }

    fn is_valid_operator(&self, op_str: &str) -> bool {
        self.plmn.as_ref().is_some_and(|p| op_str == p.as_str())
            || op_str == crate::constants::DEFAULT_OPERATOR_NAME_LONG
            || op_str == crate::constants::DEFAULT_OPERATOR_NAME_SHORT
    }

    fn select_operator(
        &mut self,
        act: Option<AccessTechnology>,
        sim: &impl CardState,
    ) -> Vec<NetworkUrc> {
        let tech_changed = act.is_some_and(|a| self.apply_resolved_act(a));
        if self.is_attached {
            if tech_changed { self.tech_change_urcs() } else { Vec::new() }
        } else if self.radio_power == RadioPowerLevel::Full {
            self.attach_network_urcs(sim)
        } else {
            Vec::new()
        }
    }

    fn handle_set_operator(
        &mut self,
        mode: CopsMode,
        format: Option<CopsFormat>,
        oper: Option<&str>,
        act: Option<u8>,
        sim: &impl CardState,
    ) -> NetworkResult {
        info!("handle_set_operator: mode={mode}, format={format:?}, oper={oper:?}, act={act:?}");

        let act = act.map(|w| AccessTechnology::from_wire(w, self.quirks)).transpose()?;

        let mut urcs = Vec::new();

        match mode {
            CopsMode::Automatic => {
                self.cops_mode = CopsMode::Automatic;
                if let Some(fmt) = format {
                    self.cops_format = fmt;
                }
                urcs.extend(self.select_operator(act, sim));
                if urcs.is_empty() { Ok(None) } else { Ok(Some(NetworkResponse::Urcs(urcs))) }
            }
            CopsMode::Manual => {
                let Some(op_str) = oper else {
                    return Err(ExecutionResult::error());
                };
                self.cops_mode = CopsMode::Manual;
                if let Some(fmt) = format {
                    self.cops_format = fmt;
                }

                if self.is_valid_operator(op_str) {
                    urcs.extend(self.select_operator(act, sim));
                    if urcs.is_empty() { Ok(None) } else { Ok(Some(NetworkResponse::Urcs(urcs))) }
                } else {
                    self.is_attached = false;
                    self.voice_registration = RegistrationStatus::Denied;
                    self.data_registration = RegistrationStatus::Denied;
                    urcs = self.all_registration_urcs();

                    // Legacy Goldfish RIL treats CME 30 as an invalid argument
                    // (b/495634151).
                    if self.quirks.goldfish_ril_37_or_earlier {
                        if urcs.is_empty() {
                            Ok(None)
                        } else {
                            Ok(Some(NetworkResponse::Urcs(urcs)))
                        }
                    } else {
                        Err(ExecutionResult::cme_error(CmeError::NoNetworkService)
                            .with_urcs(vec![Response::Network(NetworkResponse::Urcs(urcs))]))
                    }
                }
            }
            CopsMode::Deregister => {
                self.cops_mode = CopsMode::Deregister;
                if let Some(fmt) = format {
                    self.cops_format = fmt;
                }
                if self.is_attached {
                    self.is_attached = false;
                    self.voice_registration = RegistrationStatus::NotRegistered;
                    self.data_registration = RegistrationStatus::NotRegistered;
                    urcs = self.all_registration_urcs();
                }
                if urcs.is_empty() { Ok(None) } else { Ok(Some(NetworkResponse::Urcs(urcs))) }
            }
            CopsMode::SetFormatOnly => {
                let Some(fmt) = format else {
                    return Err(ExecutionResult::error());
                };
                self.cops_format = fmt;
                if act.is_some_and(|a| self.apply_resolved_act(a)) && self.is_attached {
                    urcs.extend(self.tech_change_urcs());
                }
                if urcs.is_empty() { Ok(None) } else { Ok(Some(NetworkResponse::Urcs(urcs))) }
            }
            CopsMode::ManualAutomatic => {
                let Some(op_str) = oper else {
                    return Err(ExecutionResult::error());
                };
                self.cops_mode = CopsMode::ManualAutomatic;
                if let Some(fmt) = format {
                    self.cops_format = fmt;
                }

                if !self.is_valid_operator(op_str) {
                    self.cops_mode = CopsMode::Automatic;
                }

                urcs.extend(self.select_operator(act, sim));
                if urcs.is_empty() { Ok(None) } else { Ok(Some(NetworkResponse::Urcs(urcs))) }
            }
        }
    }

    fn handle_query_available_operators(&self) -> NetworkResult {
        let status =
            if self.is_attached { OperatorStatus::Current } else { OperatorStatus::Available };
        let default_op = OperatorInfo {
            status,
            long_name: DEFAULT_OPERATOR_NAME_LONG.to_string(),
            short_name: DEFAULT_OPERATOR_NAME_SHORT.to_string(),
            numeric: self.plmn.as_ref().map(|p| p.to_string()).unwrap_or_default(),
            act: self.act,
        };
        Ok(Some(NetworkResponse::AvailableOperators(vec![default_op])))
    }

    fn handle_query_signal_quality(&self) -> NetworkResult {
        Ok(Some(NetworkResponse::SignalQuality(self.current_signal_strength())))
    }

    fn handle_query_extended_signal_quality(&self) -> NetworkResult {
        Ok(None)
    }

    fn unsol_mode(&self, reg_type: RegistrationType) -> RegistrationUnsolicitedMode {
        match reg_type {
            RegistrationType::Voice => self.voice_unsol_mode,
            RegistrationType::Data => self.data_unsol_mode,
            RegistrationType::Lte => self.lte_unsol_mode,
        }
    }

    fn unsol_mode_mut(&mut self, reg_type: RegistrationType) -> &mut RegistrationUnsolicitedMode {
        match reg_type {
            RegistrationType::Voice => &mut self.voice_unsol_mode,
            RegistrationType::Data => &mut self.data_unsol_mode,
            RegistrationType::Lte => &mut self.lte_unsol_mode,
        }
    }

    pub fn registration_status(&self, reg_type: RegistrationType) -> RegistrationStatus {
        match reg_type {
            RegistrationType::Voice => self.voice_registration,
            RegistrationType::Data | RegistrationType::Lte => self.data_registration,
        }
    }

    fn wire_registration_status(&self, status: RegistrationStatus) -> RegistrationStatus {
        if self.quirks.goldfish_ril_37_or_earlier && status == RegistrationStatus::Emergency {
            RegistrationStatus::NotRegistered
        } else {
            status
        }
    }

    fn should_include_location(
        &self,
        unsol_mode: RegistrationUnsolicitedMode,
        status: RegistrationStatus,
    ) -> bool {
        (unsol_mode == RegistrationUnsolicitedMode::EnableWithLocation
            || self.quirks.goldfish_ril_37_or_earlier)
            && status.is_registered()
    }

    fn registration_location(
        &self,
        unsol_mode: RegistrationUnsolicitedMode,
        status: RegistrationStatus,
    ) -> Option<RegistrationLocation> {
        self.should_include_location(unsol_mode, status).then(|| RegistrationLocation {
            lac: DUMMY_LAC.to_string(),
            cid: DUMMY_CID.to_string(),
            act: self.act,
        })
    }

    fn handle_query_registration(&self, reg_type: RegistrationType) -> NetworkResult {
        let unsol_mode = self.unsol_mode(reg_type);
        let status = self.wire_registration_status(self.registration_status(reg_type));
        let location = self.registration_location(unsol_mode, status);
        Ok(Some(NetworkResponse::RegistrationQuery { reg_type, unsol_mode, status, location }))
    }

    fn handle_set_registration(
        &mut self,
        reg_type: RegistrationType,
        mode: RegistrationUnsolicitedMode,
    ) -> NetworkResult {
        info!(
            "handle_set_registration({reg_type:?}): mode={mode}, current_reg={:?}",
            self.registration_status(reg_type)
        );
        *self.unsol_mode_mut(reg_type) = mode;
        let mut urcs = Vec::new();
        let status = self.registration_status(reg_type);
        if self.wire_registration_status(status) != RegistrationStatus::NotRegistered
            && mode != RegistrationUnsolicitedMode::Disable
            && let Some(urc) = self.format_registration_urc(reg_type, status)
        {
            urcs.push(urc);
        }
        if urcs.is_empty() { Ok(None) } else { Ok(Some(NetworkResponse::Urcs(urcs))) }
    }

    fn handle_query_current_ctec(&self) -> NetworkResult {
        Ok(Some(NetworkResponse::Ctec {
            current: self.current_network_mode,
            preferred: self.preferred_network_mode.0,
        }))
    }

    fn handle_query_supported_ctec(&self) -> NetworkResult {
        Ok(Some(NetworkResponse::CtecSupported(crate::constants::SUPPORTED_CTEC_TECHS.to_vec())))
    }

    fn handle_set_ctec(
        &mut self,
        current: CtecTechnology,
        preferred: Option<CtecPreferredMask>,
    ) -> NetworkResult {
        let preferred_mask = preferred.unwrap_or(CtecPreferredMask(0));
        info!("handle_set_ctec: current={current}, preferred_mask={:#X}", preferred_mask.0);

        let new_act = match current {
            CtecTechnology::Gsm => AccessTechnology::Gsm,
            CtecTechnology::Wcdma => AccessTechnology::Wcdma,
            CtecTechnology::Lte => AccessTechnology::Lte,
            // TODO(b/564626830): Clamp to LTE until %CGFPCCFG physical channel configs are
            // supported.
            CtecTechnology::Nr => AccessTechnology::Lte,
            CtecTechnology::Cdma | CtecTechnology::Evdo | CtecTechnology::Tdscdma => {
                return Err(ExecutionResult::cme_error(CmeError::IncorrectParameters));
            }
        };

        self.current_network_mode = current;
        self.preferred_network_mode = preferred_mask;
        self.act = new_act;
        info!("handle_set_ctec: updated self.act to {}", self.act);

        let urcs = if self.is_attached { self.tech_change_urcs() } else { Vec::new() };

        Ok(Some(NetworkResponse::CtecSetResult { urcs }))
    }

    fn handle_query_radio_power(&self) -> NetworkResult {
        Ok(Some(NetworkResponse::RadioPower(self.radio_power)))
    }

    fn handle_set_radio_power(
        &mut self,
        power: RadioPowerLevel,
        reset: Option<bool>,
        enable_unsolicited_urcs: bool,
    ) -> NetworkResult {
        let old_power = self.radio_power;
        self.radio_power = power;

        let mut urcs = Vec::new();

        let was_on = old_power == RadioPowerLevel::Full;
        let is_on = self.radio_power == RadioPowerLevel::Full;
        let should_reset = reset == Some(true);

        if (was_on && !is_on) || should_reset {
            self.is_attached = false;
            self.voice_registration = RegistrationStatus::NotRegistered;
            self.data_registration = RegistrationStatus::NotRegistered;

            if enable_unsolicited_urcs {
                urcs = self.all_registration_urcs();
            }
        }

        if urcs.is_empty() { Ok(None) } else { Ok(Some(NetworkResponse::Urcs(urcs))) }
    }

    fn format_registration_urc(
        &self,
        reg_type: RegistrationType,
        status: RegistrationStatus,
    ) -> Option<NetworkUrc> {
        let unsol_mode = self.unsol_mode(reg_type);
        if unsol_mode == RegistrationUnsolicitedMode::Disable {
            None
        } else {
            let status = self.wire_registration_status(status);
            let location = self.registration_location(unsol_mode, status);
            Some(NetworkUrc::Registration { reg_type, status, location })
        }
    }

    fn all_registration_urcs(&self) -> Vec<NetworkUrc> {
        let mut urcs = Vec::new();
        if let Some(urc) =
            self.format_registration_urc(RegistrationType::Voice, self.voice_registration)
        {
            urcs.push(urc);
        }
        if let Some(urc) =
            self.format_registration_urc(RegistrationType::Data, self.data_registration)
        {
            urcs.push(urc);
        }
        if let Some(urc) =
            self.format_registration_urc(RegistrationType::Lte, self.data_registration)
        {
            urcs.push(urc);
        }
        urcs
    }

    pub(crate) fn execute<'a>(
        &mut self,
        command: &NetworkCommand<'a>,
        sim_service: &impl CardState,
        enable_unsolicited_urcs: bool,
    ) -> ExecutionResult {
        let res = match command {
            NetworkCommand::QueryOperator => self.handle_query_operator(),
            NetworkCommand::QueryAvailableOperators => self.handle_query_available_operators(),
            NetworkCommand::SetOperator { mode, format, oper, act } => self.handle_set_operator(
                *mode,
                *format,
                oper.as_ref().map(|s| s.as_str()),
                *act,
                sim_service,
            ),
            NetworkCommand::QuerySignalQuality => self.handle_query_signal_quality(),
            NetworkCommand::QueryExtendedSignalQuality => {
                self.handle_query_extended_signal_quality()
            }
            NetworkCommand::QueryVoiceNetworkRegistration => {
                self.handle_query_registration(RegistrationType::Voice)
            }
            NetworkCommand::SetVoiceNetworkRegistration(mode) => {
                self.handle_set_registration(RegistrationType::Voice, *mode)
            }
            NetworkCommand::QueryDataNetworkRegistration => {
                self.handle_query_registration(RegistrationType::Data)
            }
            NetworkCommand::SetDataNetworkRegistration(mode) => {
                self.handle_set_registration(RegistrationType::Data, *mode)
            }
            NetworkCommand::QueryLteNetworkRegistration => {
                self.handle_query_registration(RegistrationType::Lte)
            }
            NetworkCommand::SetLteNetworkRegistration(mode) => {
                self.handle_set_registration(RegistrationType::Lte, *mode)
            }
            NetworkCommand::QueryRadioPower => self.handle_query_radio_power(),
            NetworkCommand::SetRadioPower(power, reset) => {
                self.handle_set_radio_power(*power, *reset, enable_unsolicited_urcs)
            }
            NetworkCommand::QueryCurrentNetworkTechnology => self.handle_query_current_ctec(),
            NetworkCommand::QuerySupportedNetworkTechnology => self.handle_query_supported_ctec(),
            NetworkCommand::SetNetworkTechnology(raw_current, preferred) => {
                CtecTechnology::from_wire(*raw_current)
                    .and_then(|current| self.handle_set_ctec(current, *preferred))
            }
        };
        res.into()
    }
}

impl RadioAdmission for NetworkService {
    fn can_originate_voice_call(&self, is_emergency: bool) -> Result<(), AdmissionDenial> {
        if !self.is_radio_on() {
            return Err(AdmissionDenial::NoNetworkService);
        }
        if is_emergency {
            return Ok(());
        }
        AdmissionDenial::check_registration(self.voice_registration)
    }

    fn can_originate_data(&self) -> Result<(), AdmissionDenial> {
        if !self.is_radio_on() {
            return Err(AdmissionDenial::NoNetworkService);
        }
        AdmissionDenial::check_registration(self.data_registration)
    }

    fn can_terminate_voice_call(&self) -> bool {
        self.is_radio_on() && self.voice_registration.is_registered()
    }

    fn can_originate_sms(&self) -> Result<(), AdmissionDenial> {
        if !self.is_radio_on() || !self.voice_registration.is_registered() {
            Err(AdmissionDenial::NoNetworkService)
        } else {
            Ok(())
        }
    }

    fn can_terminate_sms(&self) -> bool {
        self.is_radio_on() && self.voice_registration.is_registered()
    }
}

#[cfg(test)]
mod tests;
