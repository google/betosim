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

use crate::world::World;

// Feature: LibSlirp Lifecycle
//
//   As a developer
//   I want to initialize and shutdown the LibSlirp network stack
//   So that resources and worker threads are cleanly released

// Scenario: Clean stack shutdown disconnects packet channel
//   Given a running LibSlirp stack
//   When the network stack is shut down
//   Then the packet receiver channel is disconnected
#[test]
fn test_shutdown() {
    // Given
    let mut world = World::new();
    world.given_default_slirp();

    // When
    world.when_shutdown_slirp();

    // Then
    world.then_receiver_disconnected();
}
