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

//! # Step Definitions
//!
//! This module defines the core traits and types for Gherkin steps.

use std::{future::Future, pin::Pin};

/// A table of data from a Gherkin step.
pub type DataTable = Vec<Vec<String>>;

/// Context passed to every step execution.
#[derive(Default)]
pub struct StepContext {
    /// Optional data table attached to the step.
    pub table: Option<DataTable>,
}

/// A trait for the test world that holds state.
pub trait World: Send + Sync {
    /// Resets the world state before a new scenario.
    fn reset(&mut self) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        Box::pin(async {})
    }

    /// Fetches all observables from guest and host subsystems and populates
    /// variables.
    fn fetch_observables(
        &mut self,
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + '_>> {
        Box::pin(async { Ok(()) })
    }
}

/// A trait for async steps that can be executed by the Features engine.
///
/// This trait is automatically implemented for any function that matches the
/// signature: `fn(&mut W, Vec<String>, StepContext) -> Pin<Box<dyn
/// Future<Output = Result<(), String>> + Send>>`.
pub trait AsyncStep<W: ?Sized>: Send + Sync {
    fn call<'a>(
        &'a self,
        world: &'a mut W,
        args: Vec<String>,
        ctx: StepContext,
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>>;
}

impl<W, F> AsyncStep<W> for F
where
    F: for<'a> Fn(
            &'a mut W,
            Vec<String>,
            StepContext,
        ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>>
        + Send
        + Sync,
{
    fn call<'a>(
        &'a self,
        world: &'a mut W,
        args: Vec<String>,
        ctx: StepContext,
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>> {
        self(world, args, ctx)
    }
}
