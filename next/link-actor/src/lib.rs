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

mod error;
mod lifecycle;
mod link_actor;
mod service;

use actor_framework::ResourceActor;
pub use link_actor::LinkActor;

// # Link Actor
//
// This crate implements the Link Actor for Netsim.
// It manages the lifecycle of links between chips and handles link-related
// actions.
//
// ## Structure
//
// - [`entity`] - [`ActorEntity`](actor_framework::ActorEntity) implementation
//   for [`Link`]
// - [`error`] - [`LinkError`] type for type-safe error handling
// - [`handlers`] - Implements the logic for processing [`LinkAction`]s
// - [`new()`] - Factory function that creates the actor and client
//
// ## Design
//
// The Link Actor follows the `ResourceActor` pattern from the
// `actor-framework`. It maintains a collection of `Link` objects, each
// representing a connection between two chips.
//
// ## Key Components
//
// - `Link`: Represents a single link.
// - `LinkActor`: Shared state, including chip mappings and lookup tables.
//
// ## Lifecycle & Code Paths
//
// ### 1. Initialization (Startup)
// * **Where**: `netsimd.rs`
// * **Flow**:
//     1. `netsimd` creates `LinkActor` using `link_actor::new()`.
//     2. `netsimd` collects `radio_clients` (Bluetooth, WiFi, etc.).
//     3. `netsimd` injects `radio_clients` into `LinkActor`.
//     4. `LinkActor` is spawned and waits for messages.
//
// ### 2. Creating a Link (`create`)
// * **Scenario**: Frontend client (e.g., CLI, Python) requests a new link.
// * **Flow**:
//     1. **Frontend**: Calls `LinkClient::create(LinkCreate { sender, receiver,
//        rssi })`.
//     2. **Actor**: `LinkActor::handle_create` is called.
//         * Validates chips exist in `chip_kind_map`.
//         * **Duplicate Check**: Checks `lookup`. If found, returns
//           `AlreadyExists`.
//         * **Success**: Inserts into `lookup`.
//
// ### 3. Patching a Link (`patch_link`)
// * **Scenario**: User runs `netsim-cli` to set link attenuation (RSSI).
// * **Flow**:
//     1. **Frontend**: Receives `PatchLink` RPC.
//     2. **Frontend**: Implements **Look-then-Act** logic (NOT the Actor):
//         * Calls `LinkClient::list()` to find existing link ID (if any).
//     4. **Actor**: `LinkActor::handle_delete` is called.
//         * Removes entry from `lookup`.
//
// ### 4. Chip Lifecycle Events
// * **Flow**:
//     1. **Event**: A new chip is added (e.g., a device connects).
//     2. **Action**: `LinkAction::NotifyChipAdded(id, kind)` is sent.
//     3. **Actor**: `handle_action` updates `ctx.chip_kind_map`.
//     4. *Result*: Subsequent `create` calls for this chip will pass
//        validation.

mod client;
pub use client::LinkClient;

/// Creates a new LinkActor and returns the runner and a client.
///
/// The runner must be spawned on a runtime.
pub fn new() -> (ResourceActor<LinkActor>, LinkClient) {
    let (runner, client) = ResourceActor::new(32);
    (runner, LinkClient::new(client))
}
