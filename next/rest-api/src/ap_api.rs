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

//! Wi-Fi Access Point (AP) REST API endpoints and data transfer models.

pub use netsim_model::{Ap, ApCreate, ApUpdate};

pub use crate::ListApResponse;

impl From<Vec<(u32, ap_actor::ApState)>> for ListApResponse {
    fn from(aps: Vec<(u32, ap_actor::ApState)>) -> Self {
        let mut aps: Vec<Ap> = aps
            .into_iter()
            .map(|(id, state)| Ap {
                id,
                config: state.config.into(),
                associations: {
                    // `ApState::associations` is a `HashSet<MacAddr>`, so format each
                    // `MacAddr` and sort to keep the JSON array order stable across calls.
                    let mut associations: Vec<String> =
                        state.associations.into_iter().map(|m| m.to_string()).collect();
                    associations.sort();
                    associations
                },
            })
            .collect();
        aps.sort_by_key(|ap| ap.id);
        Self { aps }
    }
}
