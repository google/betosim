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

use thiserror::Error;

use crate::chip_error::ChipError;

/// The error type for client-side operations.
#[derive(Error, Debug)]
pub enum ClientError {
    /// Error sending a command to a service.
    #[error("Failed to send command: {0}")]
    Send(String),

    /// Error receiving a response from a service.
    #[error("Failed to receive response: {0}")]
    Recv(String),

    /// An operation-specific error occurred from the chip service.
    #[error("Chip error: {0}")]
    Chip(#[from] ChipError),

    /// A generic error from the actor framework.
    #[error("Framework error: {0}")]
    Framework(#[source] Box<dyn std::error::Error + Send + Sync>),
}

#[cfg(any(feature = "testing", test))]
impl ClientError {
    pub fn as_chip_error(&self) -> Option<&ChipError> {
        match self {
            Self::Chip(chip) => Some(chip),
            Self::Framework(framework) => {
                let mut chip_error = framework.downcast_ref::<ChipError>();
                let mut dyn_error_opt = framework.source();

                while let Some(dyn_error) = dyn_error_opt {
                    if chip_error.is_some() {
                        break;
                    }
                    chip_error = dyn_error.downcast_ref();
                    dyn_error_opt = dyn_error.source();
                }

                chip_error
            }
            _other => None,
        }
    }
}

impl From<tokio::sync::oneshot::error::RecvError> for ClientError {
    fn from(err: tokio::sync::oneshot::error::RecvError) -> Self {
        ClientError::Recv(err.to_string())
    }
}

impl<E: 'static + std::error::Error + Send + Sync> From<actor_framework::FrameworkError<E>>
    for ClientError
{
    fn from(val: actor_framework::FrameworkError<E>) -> Self {
        ClientError::Framework(Box::new(val))
    }
}
