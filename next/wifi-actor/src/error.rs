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
use thiserror::Error;

#[derive(Error, Debug)]
pub enum WifiError {
    #[error("Hostapd error: {0}")]
    Hostapd(Box<dyn std::error::Error + Send + Sync>),
    #[error("Network error: {0}")]
    Network(Box<dyn std::error::Error + Send + Sync>),
    #[error("Client error: {0}")]
    Client(Box<dyn std::error::Error + Send + Sync>),
    #[error("Frame error: {0}")]
    Frame(Box<dyn std::error::Error + Send + Sync>),
    #[error("Transmission error: {0}")]
    Transmission(Box<dyn std::error::Error + Send + Sync>),
    #[error("Chip error: {0}")]
    Chip(#[from] ChipError),
    #[error("Other error: {0}")]
    Other(Box<dyn std::error::Error + Send + Sync>),
    #[error("Internal error: {0}")]
    Internal(Box<dyn std::error::Error + Send + Sync>),
}
