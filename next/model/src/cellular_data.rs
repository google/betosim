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
pub struct CellularData {
    #[serde(flatten)]
    pub radio: crate::chip::Radio,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CellularDataUpdate {
    #[serde(flatten)]
    pub radio: crate::chip::RadioUpdate,
}

impl CellularDataUpdate {
    pub fn apply(&self, cellular_data: &mut CellularData) {
        self.radio.apply(&mut cellular_data.radio);
    }
}

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
pub struct CellularDataCreate {
    // Future Cellular Data specific properties.
}

impl From<CellularDataCreate> for CellularData {
    fn from(_create: CellularDataCreate) -> Self {
        CellularData { radio: crate::chip::Radio::default() }
    }
}
