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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Wifi {
    #[serde(flatten)]
    pub radio: crate::chip::Radio,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct WifiUpdate {
    #[serde(flatten)]
    pub radio: crate::chip::RadioUpdate,
}

impl WifiUpdate {
    pub fn apply(&self, wifi: &mut Wifi) {
        self.radio.apply(&mut wifi.radio);
    }
}

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
pub struct WifiCreate {
    // Future Wi-Fi specific properties.
}

impl From<WifiCreate> for Wifi {
    fn from(_create: WifiCreate) -> Self {
        Wifi { radio: crate::chip::Radio::default() }
    }
}
