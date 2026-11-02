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

mod actions;
mod client;
mod error;
mod lifecycle;
mod radio_model;
mod ranging;
mod ranging_estimator;
mod service;
mod stats;
mod uwb_actor;

pub(crate) use actions::{UwbAction, UwbActionResult};
use actor_framework::ResourceActor;
pub use client::UwbClient;
pub(crate) use error::UwbError;
pub use uwb_actor::UwbActor;

/// Creates a new UWB actor runner and its client.
pub fn new() -> (ResourceActor<UwbActor>, UwbClient) {
    let (actor, resource_client) = ResourceActor::new(32);
    (actor, UwbClient(resource_client))
}
