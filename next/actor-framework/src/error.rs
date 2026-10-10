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

//! # Framework Errors
//!
//! This module defines the common error types used throughout the actor
//! framework. By centralizing error definitions, we ensure consistent error
//! handling across all actors and clients.

use std::error::Error;

/// Errors that can occur within the actor framework itself.
#[derive(Debug, thiserror::Error)]
pub enum FrameworkError<E: Error + Send + Sync> {
    #[error("Actor send channel closed")]
    ActorClosed(#[source] Box<dyn std::error::Error + Send + Sync>),
    #[error("Actor dropped response channel")]
    ActorDropped(#[source] tokio::sync::oneshot::error::RecvError),
    #[error("Service error: {0}")]
    ServiceError(#[source] E),
}
