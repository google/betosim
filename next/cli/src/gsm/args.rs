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
pub(crate) enum GsmCommand {
    /// Show registration status and signal strength (gsm status)
    Status(GsmStatus),
    /// List all simulated cellular devices (netsim specific)
    List,
    /// Trigger an incoming call (gsm call <number>)
    Call(GsmCall),
    /// Simulate remote party answering an outbound call (gsm accept <number>)
    Accept(GsmAccept),
    /// Reject incoming call or terminate active call (gsm cancel <number>)
    Cancel(GsmCancel),
    /// Place a call on hold or take it off hold (gsm hold <number>)
    Hold(GsmHold),
    /// Set signal strength (gsm signal <rssi> [ber])
    Signal(GsmSignal),
    /// Set voice registration status (gsm voice <state>)
    Voice(GsmVoice),
    /// Set data registration status (gsm data <state>)
    Data(GsmData),
    /// Set SIM card status (gsm sim <present/absent>)
    Sim(GsmSim),
    /// Set network technology (gsm tech <gsm/lte/nr>)
    Tech(GsmTech),
    /// Set network operator (gsm operator <operator_name_or_plmn>)
    Operator(GsmOperator),
}

#[derive(Debug, Args, PartialEq)]
pub(crate) struct GsmStatus {
    /// ID of the cellular device
    #[arg(long)]
    pub id: Option<u32>,
}

#[derive(Debug, Args, PartialEq)]
pub(crate) struct GsmCall {
    /// Phone number of the incoming call (digits, optional leading '+', max 15
    /// digits)
    pub number: String,
    /// ID of the cellular device
    #[arg(long)]
    pub id: Option<u32>,
}

#[derive(Debug, Args, PartialEq)]
pub(crate) struct GsmAccept {
    /// Phone number of the call to accept (digits, optional leading '+', max 15
    /// digits)
    pub number: String,
    /// ID of the cellular device
    #[arg(long)]
    pub id: Option<u32>,
}

#[derive(Debug, Args, PartialEq)]
pub(crate) struct GsmCancel {
    /// Phone number of the call to cancel (digits, optional leading '+', max 15
    /// digits)
    pub number: String,
    /// ID of the cellular device
    #[arg(long)]
    pub id: Option<u32>,
}

#[derive(Debug, Args, PartialEq)]
pub(crate) struct GsmHold {
    /// Phone number of the call (digits, optional leading '+', max 15 digits)
    pub number: String,
    /// Hold state (on/off). If not specified, toggles hold state.
    #[arg(value_enum, ignore_case = true)]
    pub state: Option<HoldStateOption>,
    /// ID of the cellular device
    #[arg(long)]
    pub id: Option<u32>,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum HoldStateOption {
    On,
    Off,
}

#[derive(Debug, Args, PartialEq)]
pub(crate) struct GsmSignal {
    /// RSSI value (0-31, or 99 for unknown)
    pub rssi: u32,
    /// BER value (0-7, or 99 for unknown)
    #[arg(default_value = "99")]
    pub ber: u32,
    /// ID of the cellular device
    #[arg(long)]
    pub id: Option<u32>,
}

#[derive(Debug, Args, PartialEq)]
pub(crate) struct GsmVoice {
    /// Registration status
    #[arg(value_enum, ignore_case = true)]
    pub status: RegistrationStatusOption,
    /// ID of the cellular device
    #[arg(long)]
    pub id: Option<u32>,
}

#[derive(Debug, Args, PartialEq)]
pub(crate) struct GsmData {
    /// Registration status
    #[arg(value_enum, ignore_case = true)]
    pub status: RegistrationStatusOption,
    /// ID of the cellular device
    #[arg(long)]
    pub id: Option<u32>,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum RegistrationStatusOption {
    Unregistered,
    Home,
    Searching,
    Denied,
    Unknown,
    Roaming,
    SmsOnlyHome,
    SmsOnlyRoaming,
    Emergency,
}

#[derive(Debug, Args, PartialEq)]
pub(crate) struct GsmSim {
    /// SIM status (present/absent)
    #[arg(value_enum, ignore_case = true)]
    pub state: SimStateOption,
    /// ID of the cellular device
    #[arg(long)]
    pub id: Option<u32>,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum SimStateOption {
    Present,
    Absent,
}

#[derive(Debug, Args, PartialEq)]
pub(crate) struct GsmTech {
    /// Radio technology
    #[arg(value_enum, ignore_case = true)]
    pub tech: RadioTechnologyOption,
    /// ID of the cellular device
    #[arg(long)]
    pub id: Option<u32>,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum RadioTechnologyOption {
    Gsm,
    Lte,
    Nr,
}

#[derive(Debug, Args, PartialEq)]
pub(crate) struct GsmOperator {
    /// Operator name or numeric PLMN. Use empty string to reset to automatic.
    pub operator: String,
    /// ID of the cellular device
    #[arg(long)]
    pub id: Option<u32>,
}
