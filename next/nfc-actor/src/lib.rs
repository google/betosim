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

mod error;
mod lifecycle;
mod nfc_actor;
mod scene;
pub mod service;
mod stats;

mod client;

// Re-export core types
pub use ::casimir;
pub use actor_framework::ResourceActor;
pub use client::NfcClient;
pub use error::NfcError;
pub use nfc_actor::{
    ChipState, NFC_MODE_IDLE, NFC_MODE_LISTEN, NFC_MODE_POLL, NfcAction, NfcActionResult, NfcActor,
    NfcMode,
};
pub use scene::SceneClient;
pub use service::NciCodec;
pub use stats::{NfcApi, NfcServiceStats, NfcStats};

/// Creates a new NFC actor framework instance and client.
pub fn new() -> (ResourceActor<NfcActor>, NfcClient) {
    let (runner, client) = ResourceActor::new(32);
    let stats = std::sync::Arc::new(stats::NfcStats::new());
    let service_stats = std::sync::Arc::new(stats::NfcServiceStats::new());
    (runner, NfcClient { client, stats, service_stats })
}
