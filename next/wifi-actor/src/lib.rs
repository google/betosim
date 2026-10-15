// Copyright 2025 The Android Open Source Project
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

pub(crate) mod client;
pub(crate) mod error;
pub(crate) mod ftm;
pub(crate) mod gateway;
pub(crate) mod lifecycle;
pub(crate) mod mdns_forwarder;
pub(crate) mod medium;
pub(crate) mod pmsr;
pub(crate) mod service;
pub(crate) mod slirp_gateway;
pub(crate) mod stats;
#[cfg(target_os = "linux")]
pub(crate) mod tap_gateway;
pub(crate) mod wifi_actor;

use actor_framework::ResourceActor;
pub use client::WifiClient;
pub use error::WifiError;
pub use ftm::handle_ftm_request;
pub use gateway::GatewayTrait;
pub use medium::{Medium, types::Station, utils::create_hwsim_msg_from_frame};
pub use slirp_gateway::SlirpGateway;
pub use stats::{Clock, MockClock, SystemClock, WifiStats};
#[cfg(target_os = "linux")]
pub use tap_gateway::TapGateway;
pub use wifi_actor::{WifiActor, WifiReq, WifiResponse};

#[derive(Default, Debug, Clone)]
pub struct DebugArgs {
    pub debug_no_traffic: bool,
    pub debug_no_network: bool,
    pub debug_no_wmedium: bool,
    pub debug_no_mdns_wmedium: bool,
    pub debug_no_guest_to_host_mdns: bool,
}

/// Creates a new WifiActor runner and client.
pub fn new() -> (ResourceActor<WifiActor>, WifiClient) {
    let (runner, client) = ResourceActor::new(32);
    (runner, WifiClient::new(client))
}
// Touch to force clippy rerun
