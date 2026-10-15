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

use std::collections::BTreeMap;

use modem_rs_derive::CommandParser;
use netsim_model::Quirks;

use crate::{
    parser::QuotedString,
    types::{
        CallForwardCondition, CallForwardDestination, CallForwardNumberType, CallForwardTime,
        CallForwardTon, CallForwardingMode, CallForwardingReason, CallWaitingMode,
        CallWaitingStatus, ClipProvisionStatus, ClirMode, ClirStatus, CmeError, ExecutionResult,
        Parsable, PhoneNumber, ServiceClass, TypeOfAddress, UssdMode, UssdStatus,
    },
};

/// Supplementary service AT commands.
#[derive(Debug, PartialEq, Clone, CommandParser)]
pub enum SupCommand<'a> {
    #[command(tag = "AT+CCFC=")]
    CallForwarding {
        reason: CallForwardingReason,
        mode: CallForwardingMode,
        number: Option<PhoneNumber>,
        toa: Option<TypeOfAddress>,
        class: Option<ServiceClass>,
        subaddr: Option<QuotedString<'a>>,
        satype: Option<u8>,
        time: Option<CallForwardTime>,
    },
    #[command(tag = "AT+CCFCU=")]
    CallForwardUtility {
        reason: CallForwardingReason,
        mode: CallForwardingMode,
        number_type: Option<CallForwardNumberType>,
        ton: Option<CallForwardTon>,
        number: Option<QuotedString<'a>>,
        class: Option<ServiceClass>,
        ruleset: Option<QuotedString<'a>>,
        subaddr: Option<QuotedString<'a>>,
        satype: Option<u8>,
        time: Option<CallForwardTime>,
    },
    #[command(tag = "AT+CLIR?")]
    QueryClir,
    /// Non-standard Goldfish CLIR syntax
    #[command(tag = "AT+CLIR: ")]
    SetClirGoldfish(ClirMode),
    #[command(tag = "AT+CLIR=")]
    SetClir(ClirMode),
    #[command(tag = "AT+CLIP=")]
    SetClip(bool),
    #[command(tag = "AT+CLIP?")]
    QueryClip,
    #[command(tag = "AT+COLP=")]
    SetColp(bool),
    #[command(tag = "AT+CCWA=")]
    SetCallWaiting(bool, Option<CallWaitingMode>, Option<ServiceClass>),
    #[command(tag = "AT+CSSN=")]
    SuppServiceNotification(bool, Option<bool>),
    #[command(tag = "AT+CUSD=")]
    SetUssd { mode: UssdMode, message: Option<QuotedString<'a>>, dcs: Option<u8> },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallForwardLine {
    pub status: bool,
    pub class: ServiceClass,
    pub number_type: CallForwardNumberType,
    pub ton: CallForwardTon,
    pub number: String,
    /// `Some` only for the no-reply reason.
    pub time: Option<CallForwardTime>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SupResponse {
    Clir { n: ClirMode, m: ClirStatus },
    Clip { activation: bool, provision: ClipProvisionStatus },
    CallWaiting(Vec<(CallWaitingStatus, u8)>),
    CallForwarding(Vec<CallForwardLine>),
    CallForwardUtility(Vec<CallForwardLine>),
    Ussd { status: UssdStatus, message: String, dcs: u8 },
}

impl std::fmt::Display for SupResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SupResponse::Clir { n, m } => write!(f, "+CLIR: {n},{m}\r\n"),
            SupResponse::Clip { activation, provision } => {
                write!(f, "+CLIP: {},{provision}\r\n", *activation as u8)
            }
            SupResponse::CallWaiting(infos) => {
                for (status, class) in infos {
                    write!(f, "+CCWA: {status},{class}\r\n")?;
                }
                Ok(())
            }
            SupResponse::CallForwarding(lines) => {
                for line in lines {
                    write!(
                        f,
                        "+CCFC: {},{},\"{}\",{}",
                        u8::from(line.status),
                        line.class,
                        line.number,
                        line.ton.as_address_octet()
                    )?;
                    if let Some(time) = line.time {
                        write!(f, ",,,{time}")?;
                    }
                    write!(f, "\r\n")?;
                }
                Ok(())
            }
            SupResponse::CallForwardUtility(lines) => {
                for line in lines {
                    write!(
                        f,
                        "+CCFCU: {},{},{},{},\"{}\"",
                        u8::from(line.status),
                        line.class,
                        line.number_type.as_u8(),
                        line.ton.as_u8(),
                        line.number
                    )?;
                    if let Some(time) = line.time {
                        write!(f, ",,,{time}")?;
                    }
                    write!(f, "\r\n")?;
                }
                Ok(())
            }
            SupResponse::Ussd { status, message, dcs } => {
                write!(f, "+CUSD: {status},\"{message}\",{dcs}\r\n")
            }
        }
    }
}

type SupResult = Result<Option<SupResponse>, ExecutionResult>;

#[derive(Debug, Clone, PartialEq, Eq)]
struct CallForwardRule {
    enabled: bool,
    destination: Option<CallForwardDestination>,
    time: CallForwardTime,
}

impl Default for CallForwardRule {
    fn default() -> Self {
        Self { enabled: false, destination: None, time: CallForwardTime(20) }
    }
}

#[derive(Debug)]
pub struct SupService {
    /// Call forwarding rules keyed by reason only per 3GPP TS 27.007 §7.11.
    /// Note: Service class is not stored; the queried class is echoed back.
    call_forward_rules: BTreeMap<CallForwardCondition, CallForwardRule>,
    clip_enabled: bool,
    clir_mode: ClirMode,
    ccwa_presentation: bool,
    ccwa_status: BTreeMap<ServiceClass, CallWaitingStatus>,
    quirks: Quirks,
}

impl SupService {
    pub fn new(quirks: Quirks) -> Self {
        let ccwa_status = BTreeMap::from([
            (ServiceClass::VOICE, CallWaitingStatus::NotActive),
            (ServiceClass::DATA, CallWaitingStatus::NotActive),
            (ServiceClass::FAX, CallWaitingStatus::NotActive),
        ]);
        let call_forward_rules = BTreeMap::from([
            (CallForwardCondition::Unconditional, CallForwardRule::default()),
            (CallForwardCondition::Busy, CallForwardRule::default()),
            (CallForwardCondition::NoReply, CallForwardRule::default()),
            (CallForwardCondition::NotReachable, CallForwardRule::default()),
        ]);
        Self {
            call_forward_rules,
            clip_enabled: false,
            clir_mode: ClirMode::default(),
            ccwa_presentation: false,
            ccwa_status,
            quirks,
        }
    }
}

impl Default for SupService {
    fn default() -> Self {
        Self::new(Quirks::default())
    }
}

#[derive(Debug)]
struct CallForwardRequest {
    reason: CallForwardingReason,
    mode: CallForwardingMode,
    destination: Option<CallForwardDestination>,
    class: Option<ServiceClass>,
    time: Option<CallForwardTime>,
}

impl CallForwardRequest {
    fn from_command<'a>(cmd: &SupCommand<'a>, quirks: Quirks) -> Result<Self, ExecutionResult> {
        match cmd {
            SupCommand::CallForwarding { reason, mode, number, toa, class, time, .. } => {
                let destination = number.as_ref().and_then(|p| {
                    let s = p.as_str();
                    (!s.is_empty()).then(|| {
                        let ton = toa
                            .map(CallForwardTon::from)
                            .unwrap_or_else(|| CallForwardTon::from(TypeOfAddress::from_number(s)));
                        CallForwardDestination::Number { digits: s.to_string(), ton }
                    })
                });
                Ok(Self { reason: *reason, mode: *mode, destination, class: *class, time: *time })
            }
            SupCommand::CallForwardUtility {
                reason,
                mode,
                number_type,
                ton,
                number,
                class,
                time,
                ..
            } => {
                let num_str = number.as_ref().map(|s| s.as_str()).unwrap_or("");
                if *number_type == Some(CallForwardNumberType::NoValidInfo) && !num_str.is_empty() {
                    return Err(CmeError::IncorrectParameters.into());
                }
                let validated_ton =
                    ton.map(|t| CallForwardTon::from_wire(t.as_u8(), quirks)).transpose()?;
                if *number_type == Some(CallForwardNumberType::Uri)
                    && validated_ton.is_some_and(|t| t.ton() != 0)
                {
                    return Err(CmeError::IncorrectParameters.into());
                }
                let destination =
                    if number_type.is_none() && validated_ton.is_none() && num_str.is_empty() {
                        None
                    } else if *number_type == Some(CallForwardNumberType::NoValidInfo) {
                        // NoValidInfo carries no destination type, so do not relabel stored URIs as dialable numbers.
                        None
                    } else {
                        let nt = number_type.unwrap_or(CallForwardNumberType::Number);
                        match nt {
                            CallForwardNumberType::Uri => {
                                Some(CallForwardDestination::Uri(num_str.to_string()))
                            }
                            CallForwardNumberType::Number | CallForwardNumberType::NoValidInfo => {
                                let ton = validated_ton.unwrap_or_else(|| {
                                    if num_str.is_empty() {
                                        CallForwardTon::default()
                                    } else {
                                        CallForwardTon::from(TypeOfAddress::from_number(num_str))
                                    }
                                });
                                Some(CallForwardDestination::Number {
                                    digits: num_str.to_string(),
                                    ton,
                                })
                            }
                        }
                    };
                Ok(Self { reason: *reason, mode: *mode, destination, class: *class, time: *time })
            }
            _ => Err(CmeError::IncorrectParameters.into()),
        }
    }
}

fn update_destination(
    existing: &CallForwardDestination,
    dest: &CallForwardDestination,
) -> CallForwardDestination {
    let digits = if dest.as_str().is_empty() {
        existing.as_str().to_string()
    } else {
        dest.as_str().to_string()
    };
    match dest {
        CallForwardDestination::Uri(_) => CallForwardDestination::Uri(digits),
        CallForwardDestination::Number { ton, .. } => {
            CallForwardDestination::Number { digits, ton: *ton }
        }
    }
}

impl SupService {
    pub fn clip_enabled(&self) -> bool {
        self.clip_enabled
    }

    pub fn clir_mode(&self) -> ClirMode {
        self.clir_mode
    }

    // --- Pure command handlers ---

    fn validate_registered_numbers(
        &self,
        conditions: &[CallForwardCondition],
    ) -> Result<(), ExecutionResult> {
        for c in conditions {
            match self.call_forward_rules.get(c) {
                Some(rule) if rule.destination.is_some() => {}
                _ => return Err(CmeError::IncorrectParameters.into()),
            }
        }
        Ok(())
    }

    fn handle_call_forward(
        &mut self,
        req: CallForwardRequest,
    ) -> Result<Option<Vec<CallForwardLine>>, ExecutionResult> {
        let target_conditions = req.reason.conditions();
        let class = req.class.unwrap_or(ServiceClass::VOICE);

        match req.mode {
            CallForwardingMode::Disable | CallForwardingMode::Enable => {
                let enabled = req.mode == CallForwardingMode::Enable;
                let has_digits = req.destination.as_ref().is_some_and(|d| !d.as_str().is_empty());
                if enabled && !has_digits {
                    self.validate_registered_numbers(target_conditions)?;
                }
                for &c in target_conditions {
                    if let Some(rule) = self.call_forward_rules.get_mut(&c) {
                        rule.enabled = enabled;
                        if has_digits {
                            rule.destination = req.destination.clone();
                        }
                    }
                }
                Ok(None)
            }
            CallForwardingMode::Query => {
                let mut lines = Vec::new();
                for &c in target_conditions {
                    if let Some(rule) = self.call_forward_rules.get(&c) {
                        let (number_type, ton, number) = match &rule.destination {
                            Some(dest) => {
                                (dest.number_type(), dest.ton(), dest.as_str().to_string())
                            }
                            None => (
                                CallForwardNumberType::Number,
                                CallForwardTon::default(),
                                String::new(),
                            ),
                        };
                        lines.push(CallForwardLine {
                            status: rule.enabled,
                            class,
                            number_type,
                            ton,
                            number,
                            time: if c == CallForwardCondition::NoReply {
                                Some(rule.time)
                            } else {
                                None
                            },
                        });
                    }
                }
                Ok(Some(lines))
            }
            CallForwardingMode::Registration => {
                let has_digits = req.destination.as_ref().is_some_and(|d| !d.as_str().is_empty());
                if !has_digits {
                    self.validate_registered_numbers(target_conditions)?;
                }
                for &c in target_conditions {
                    if let Some(rule) = self.call_forward_rules.get_mut(&c) {
                        rule.enabled = true;
                        if has_digits {
                            rule.destination = req.destination.clone();
                        } else if let Some(ref dest) = req.destination
                            && let Some(ref existing) = rule.destination
                        {
                            rule.destination = Some(update_destination(existing, dest));
                        }
                        if let Some(t) = req.time {
                            rule.time = t;
                        }
                    }
                }
                Ok(None)
            }
            CallForwardingMode::Erasure => {
                for &c in target_conditions {
                    if let Some(rule) = self.call_forward_rules.get_mut(&c) {
                        *rule = CallForwardRule::default();
                    }
                }
                Ok(None)
            }
        }
    }

    fn handle_query_clir(&self) -> SupResult {
        Ok(Some(SupResponse::Clir { n: self.clir_mode, m: ClirStatus::Active }))
    }

    fn handle_query_clip(&self) -> SupResult {
        Ok(Some(SupResponse::Clip {
            activation: self.clip_enabled,
            provision: ClipProvisionStatus::Provisioned,
        }))
    }

    fn handle_set_call_waiting(
        &mut self,
        presentation: bool,
        mode: Option<CallWaitingMode>,
        class: Option<ServiceClass>,
    ) -> SupResult {
        self.ccwa_presentation = presentation;
        let class = class.unwrap_or_default();
        match mode {
            Some(CallWaitingMode::Disable) => {
                self.set_ccwa_status(class, CallWaitingStatus::NotActive);
                Ok(None)
            }
            Some(CallWaitingMode::Enable) => {
                self.set_ccwa_status(class, CallWaitingStatus::Active);
                Ok(None)
            }
            Some(CallWaitingMode::Query) => {
                // Find all active basic classes that are subset of the queried class.
                let mut active_classes = Vec::new();
                for (&bc, &status) in &self.ccwa_status {
                    if class.contains(bc) && status == CallWaitingStatus::Active {
                        active_classes.push(bc.as_u8());
                    }
                }
                if active_classes.is_empty() {
                    // None are active, return single line indicating disabled for the queried class
                    Ok(Some(SupResponse::CallWaiting(vec![(
                        CallWaitingStatus::NotActive,
                        class.as_u8(),
                    )])))
                } else {
                    // Return active classes
                    let infos = active_classes
                        .into_iter()
                        .map(|bc| (CallWaitingStatus::Active, bc))
                        .collect();
                    Ok(Some(SupResponse::CallWaiting(infos)))
                }
            }
            None => Ok(None),
        }
    }

    fn set_ccwa_status(&mut self, class: ServiceClass, status: CallWaitingStatus) {
        for (&bc, val) in &mut self.ccwa_status {
            if class.contains(bc) {
                *val = status;
            }
        }
    }

    fn handle_set_ussd(
        &self,
        mode: UssdMode,
        message: Option<QuotedString>,
        _dcs: Option<u8>,
    ) -> SupResult {
        if mode == UssdMode::EnableUrc && message.is_some() {
            Ok(Some(SupResponse::Ussd {
                status: UssdStatus::NoActionRequired,
                message: "OK".to_string(),
                dcs: 15,
            }))
        } else {
            Ok(None)
        }
    }

    fn handle_command<'a>(&mut self, command: &SupCommand<'a>) -> SupResult {
        let resp = match command {
            SupCommand::CallForwarding { .. } => {
                let req = CallForwardRequest::from_command(command, self.quirks)?;
                self.handle_call_forward(req)?.map(SupResponse::CallForwarding)
            }
            SupCommand::CallForwardUtility { .. } => {
                let req = CallForwardRequest::from_command(command, self.quirks)?;
                self.handle_call_forward(req)?.map(SupResponse::CallForwardUtility)
            }
            SupCommand::QueryClir => return self.handle_query_clir(),
            SupCommand::SetClir(clir) | SupCommand::SetClirGoldfish(clir) => {
                self.clir_mode = *clir;
                None
            }
            SupCommand::SetClip(enabled) => {
                self.clip_enabled = *enabled;
                None
            }
            SupCommand::QueryClip => return self.handle_query_clip(),
            SupCommand::SetColp(_) | SupCommand::SuppServiceNotification(_, _) => None,
            SupCommand::SetCallWaiting(presentation, mode, class) => {
                return self.handle_set_call_waiting(*presentation, *mode, *class);
            }
            SupCommand::SetUssd { mode, message, dcs } => {
                return self.handle_set_ussd(*mode, *message, *dcs);
            }
        };
        Ok(resp)
    }

    pub(crate) fn execute<'a>(&mut self, command: &SupCommand<'a>) -> ExecutionResult {
        self.handle_command(command).into()
    }
}
