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

impl ClientError {
    /// Returns the underlying `ChipError`, if any, by walking the error source chain.
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

#[cfg(test)]
mod tests {
    use thiserror::Error;

    use super::*;
    use crate::ChipId;

    #[test]
    fn test_as_chip_error_direct_chip() {
        let error = ClientError::Chip(ChipError::ChipNotFound(ChipId(99)));
        assert!(matches!(
            error.as_chip_error(),
            Some(ChipError::ChipNotFound(id)) if *id == ChipId(99)
        ));
    }

    #[test]
    fn test_as_chip_error_direct_framework() {
        let error = ClientError::Framework(Box::new(ChipError::ChipNotFound(ChipId(42))));
        assert!(matches!(
            error.as_chip_error(),
            Some(ChipError::ChipNotFound(id)) if *id == ChipId(42)
        ));
    }

    #[test]
    fn test_as_chip_error_nested_source() {
        #[derive(Error, Debug)]
        #[error("inner: {0}")]
        struct InnerError(#[source] ChipError);

        #[derive(Error, Debug)]
        #[error("outer: {0}")]
        struct OuterError(#[source] InnerError);

        let error = ClientError::Framework(Box::new(OuterError(InnerError(
            ChipError::ChipNotFound(ChipId(7)),
        ))));
        assert!(matches!(
            error.as_chip_error(),
            Some(ChipError::ChipNotFound(id)) if *id == ChipId(7)
        ));
    }

    #[test]
    fn test_as_chip_error_non_chip_returns_none() {
        assert!(ClientError::Send("send".to_string()).as_chip_error().is_none());
        assert!(ClientError::Recv("recv".to_string()).as_chip_error().is_none());
        let framework_io = ClientError::Framework(Box::new(std::io::Error::other("io")));
        assert!(framework_io.as_chip_error().is_none());
    }
}
