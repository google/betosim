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
pub enum UwbError {
    #[error("{0}")]
    Chip(#[from] ChipError),

    #[error("Invalid azimuth: {0} (expected -180.0 to 180.0)")]
    InvalidAzimuth(f32),

    #[error("Invalid elevation: {0} (expected -90.0 to 90.0)")]
    InvalidElevation(f32),

    #[error("Packet stream is missing")]
    PacketStreamMissing,

    #[error("Packet sink is missing")]
    PacketSinkMissing,

    #[error("Internal error: {0}")]
    Internal(Box<str>),
}
