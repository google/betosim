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

//! Decoupled Integration Tests: Backend Lifecycle & Initialization
//! (b/560172253).

use actor_framework::ActorService;
use slirp_actor::{SlirpActor, SlirpBackend, SlirpReq};

use super::test_driver::{MockContext, SlirpTestDriver};

async fn run_actor_initialization_test(backend: SlirpBackend) {
    // Given a Slirp driver initialized with the specified backend
    let mut driver = SlirpTestDriver::new(backend, None).await;

    // When we query driver status
    let initialized = driver.is_initialized().await;

    // Then driver is ready
    assert!(initialized);
}

#[tokio::test]
async fn test_actor_initialization_parity() {
    for backend in [SlirpBackend::CFfi, SlirpBackend::Native] {
        run_actor_initialization_test(backend).await;
    }
}

#[tokio::test]
async fn test_dynamic_backend_switch() {
    // Given a SlirpActor running default backend
    let mut actor = SlirpActor::new(Default::default(), None, None).await;
    let mut ctx = MockContext::default();

    // When we switch backend to Native
    let result =
        actor.handle_action(None, SlirpReq::SwitchBackend(SlirpBackend::Native), &mut ctx).await;

    // Then backend transition succeeds
    assert!(result.is_ok());
    assert_eq!(actor.backend(), SlirpBackend::Native);
}
