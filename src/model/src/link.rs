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

use netsim_types::ChipKind;
use serde::{Deserialize, Serialize};

use crate::chip::ChipId;

/// A unique identifier for a link, represented as a u32.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub struct LinkId(pub u32);

impl std::fmt::Display for LinkId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<u32> for LinkId {
    fn from(id: u32) -> Self {
        LinkId(id)
    }
}

impl From<LinkId> for u32 {
    fn from(id: LinkId) -> Self {
        id.0
    }
}

/// Internal representation of a Link.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Link {
    pub id: LinkId,
    pub sender: ChipId,
    pub receiver: ChipId,
    pub kind: ChipKind,
    pub rssi: i8,
}

/// Parameters for creating an RF Link.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LinkCreate {
    pub sender: ChipId,
    pub receiver: ChipId,
    pub rssi: i8,
}

/// Update for a Link.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct LinkUpdate {
    pub rssi: Option<i8>,
}

// impl Default for Link removed as ChipKind::UNSPECIFIED is removed.
