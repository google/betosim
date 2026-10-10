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

//! BDD Step Definitions for Modem-rs Tests.
//!
//! This module contains free functions that implement the Given/When/Then steps
//! for the integration tests. These functions abstract the interaction with the
//! `World` state and the `ModemNetworkSimulator`.
//!
//! # Organization
//!
//! - `setup`: Steps for initializing modems and state (Given).
//! - `action`: Steps for performing actions like sending commands (When).
//! - `check`: Steps for verifying state and responses (Then).
//! - `prelude`: Re-exports commonly used steps for convenient import.

pub mod action;
pub mod check;
pub mod setup;

/// Prelude for easy import of all steps.
pub mod prelude {
    pub use super::{action::*, check::*, setup::*};
}

// Retain legacy re-exports for backward compatibility with existing tests
pub use action::*;
pub use check::*;
pub use setup::*;
