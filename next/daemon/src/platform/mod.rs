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

//! Compile-time platform initialization policy for `NetsimDaemon`.

#[cfg(feature = "cuttlefish")]
mod cuttlefish;
#[cfg(not(feature = "cuttlefish"))]
mod standard;

#[cfg(feature = "cuttlefish")]
pub(crate) use cuttlefish::{
    PlatformTasks, default_test_beacons, init_transport_listeners, preflight_check,
    run_connector_mode_if_requested, should_start_tcp_listener, start_servers,
};
#[cfg(not(feature = "cuttlefish"))]
pub(crate) use standard::{
    PlatformTasks, default_test_beacons, init_transport_listeners, preflight_check,
    run_connector_mode_if_requested, should_start_tcp_listener, start_servers,
};
