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
mod native;
mod service;
mod slirp_actor;

mod slirp_client;

use actor_framework::ResourceActor;
pub use error::SlirpError;
pub use slirp_actor::{
    SlirpActor, SlirpBackend, SlirpConfig, SlirpCreate, SlirpIpv4Lease, SlirpIpv6Lease, SlirpLease,
    SlirpReq, SlirpStatus,
};
pub use slirp_client::SlirpClient;

/// Creates a new SlirpActor and returns the runner and a client.
pub fn new() -> (ResourceActor<SlirpActor>, SlirpClient) {
    let (runner, client) = ResourceActor::new(32);
    (runner, SlirpClient::new(client))
}
