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

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum UwbMode {
    #[default]
    Device,
    Anchor,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Uwb {
    #[serde(flatten)]
    pub radio: crate::chip::Radio,
    #[serde(default)]
    pub address: String,
    #[serde(default)]
    pub mode: UwbMode,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct UwbUpdate {
    #[serde(flatten)]
    pub radio: crate::chip::RadioUpdate,
}

impl UwbUpdate {
    pub fn apply(&self, uwb: &mut Uwb) {
        self.radio.apply(&mut uwb.radio);
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub enum UwbCreate {
    #[default]
    Device,
    Anchor {
        #[serde(default)]
        address: Option<String>,
    },
}

impl From<UwbCreate> for Uwb {
    fn from(create: UwbCreate) -> Self {
        match create {
            UwbCreate::Device => {
                Self { radio: Default::default(), address: String::new(), mode: UwbMode::Device }
            }
            UwbCreate::Anchor { address } => Self {
                radio: Default::default(),
                address: address.unwrap_or_default(),
                mode: UwbMode::Anchor,
            },
        }
    }
}
