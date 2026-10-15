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

use thiserror::Error;

#[derive(Debug, Error)]
pub enum LinkError {
    #[error("Link not found: {0}")]
    NotFound(u32),
    #[error("Invalid parameter: {0}")]
    InvalidParam(String),
    #[error("Internal error: {0}")]
    Internal(Box<dyn std::error::Error + Send + Sync>),
    #[error("Link already exists")]
    AlreadyExists,
    #[error("Actor communication error: {0}")]
    ActorCommunicationError(Box<dyn std::error::Error + Send + Sync>),
}

impl From<String> for LinkError {
    fn from(s: String) -> Self {
        LinkError::Internal(Box::from(s))
    }
}
