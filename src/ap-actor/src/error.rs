// Copyright 2025-2026 The Android Open Source Project
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

use netsim_model::ChipError;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ApError {
    #[error("AP not found: {0}")]
    ApNotFound(u32),
    #[error("AP with ID {0} already exists")]
    ApAlreadyExists(u32),
    #[error("Invalid Frame")]
    InvalidFrame,
    #[error("Chip error: {0}")]
    Chip(#[from] ChipError),
    #[error("Internal error: {0}")]
    Internal(Box<dyn std::error::Error + Send + Sync>),
}
