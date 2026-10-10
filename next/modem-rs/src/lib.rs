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

#![allow(unreachable_pub)]

pub mod apdu;
mod call_service;
mod capabilities;
pub mod config;

pub(crate) use capabilities::{CardState, RadioAdmission, SmsStore};
pub mod constants;
mod data_service;
mod metrics;
mod misc_service;
mod modem;
pub mod modem_network;
mod modem_network_simulator;
mod network_service;
mod parser;
mod pdu;
pub mod profiles;
mod sim_service;
mod sms_service;
mod stk_service;
mod sup_service;
pub mod time;

pub mod xml_profile;

pub mod test_utils;
#[cfg(test)]
mod tests;
mod types; // Test utils might need to be public for integration tests

// The Public API
// Configuration types needed for setup
pub use config::{
    DedicatedFile, ElementaryFile, FileSystem, PinProfile, PinState, ProfileMetadata, SimFile,
    SimIo, SimProfile,
};
pub use metrics::MetricsSnapshot;
pub use misc_service::NitzTime;
pub use modem::ModemEvent;
pub use modem_network_simulator::{ModemNetworkSimulator, NetworkEvent, ScheduledEvent};
pub use netsim_model::RegistrationStatus;
pub use pdu::{EncodeError, ParseError, SubmitPdu};
pub use profiles::{
    SIM_TYPE_CTS, SIM_TYPE_DEFAULT, SIM_TYPE_TEL_ALASKA, XmlProfileError, get_builtin_profile,
    parse_xml_profile,
};
pub use types::{
    AT_ERROR, AT_OK, HostEvent, ModemError, ModemId, ModemSink, PhoneNumber, Plmn, PlmnError,
};
