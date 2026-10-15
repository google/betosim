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

use netsim_model::ChipError;

#[derive(thiserror::Error, Debug)]
pub enum NfcError {
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
    #[error("Missing packet stream or sink")]
    MissingStreamSink,
    #[error("Chip error: {0}")]
    Chip(#[from] ChipError),
    #[error("Internal error: {0}")]
    Internal(String),
    #[error("Unknown error")]
    Unknown,
}
