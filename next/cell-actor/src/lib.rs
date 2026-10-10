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
mod cell_actor;
mod error;
mod lifecycle;
mod service;

mod client;
// Re-export core types
pub use actions::{CellAction, CellActionResult};
pub use actor_framework::ResourceActor;
pub use cell_actor::CellActor;
/// A type alias for the client used to interact with the Cell actor.
pub use client::CellClient;
pub use error::CellError;

/// Creates a new Cell actor framework instance and client.
///
/// Returns:
/// - `ResourceActor`: The actor runner that drives the service.
/// - `CellClient`: The client for sending requests to the actor.
///
/// The caller is responsible for creating the `CellActor` service and spawning
/// the runner: `tokio::spawn(actor.run(service));`
pub fn new() -> (ResourceActor<CellActor>, CellClient) {
    let (runner, client) = ResourceActor::new(32);
    (runner, CellClient(client))
}
