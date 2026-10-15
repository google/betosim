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

mod ap_actor;
mod ap_client;
mod eap_auth;
mod error;
#[doc(hidden)]
pub mod ffi;
mod ftm;
mod ieee802_11;
mod lifecycle;
mod rsn;
mod sae;
mod service;
mod shared;
mod wpa_auth;

use actor_framework::ResourceActor;

pub use crate::{
    ap_actor::{
        ApActor, ApActorUpdate, ApConfig, ApReq, ApResponse, ApState, ApUpdate, DEFAULT_AP_BSSID,
        EXPECTED_DEFAULT_AP_BSSID, expected_default_ap_bssid,
    },
    ap_client::ApClient,
    error::ApError,
    sae::SaeStateMachine,
    shared::SharedKeyStore,
};

/// Creates a new ApActor and returns the runner and a client.
///
/// The runner must be spawned on a runtime.
pub fn new() -> (ResourceActor<ApActor>, ApClient) {
    let (runner, client) = ResourceActor::new(32);
    (runner, ApClient::new(client))
}
