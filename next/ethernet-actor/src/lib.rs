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

mod client;
mod error;
mod ethernet_actor;
mod lifecycle;

use actor_framework::ResourceActor;
pub use client::EthernetClient;
pub use ethernet_actor::EthernetActor;

/// Creates a new EthernetActor runner and client.
pub fn new() -> (ResourceActor<EthernetActor>, EthernetClient) {
    let (runner, client) = ResourceActor::new(32);
    (runner, EthernetClient::new(client))
}
