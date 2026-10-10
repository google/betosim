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

use std::collections::HashMap;

use link_api::Link;
use netsim_model::{ChipId, ChipKind};
use tracing::error;

/// Context for LinkActor.
///
/// Holds the global state shared across all Link entities.
///
/// # State Management
/// - `lookup`: Maps (Sender, Receiver) to `LinkId` for fast upsert/retrieval.
/// - `chip_kind_map`: Maps ChipId to ChipKind for validation.
#[derive(Default)]
pub struct LinkActor {
    /// Index for fast lookup/upsert: (Sender, Receiver) -> LinkId
    pub(crate) chip_pairs: HashMap<(ChipId, ChipId), link_api::LinkId>,
    /// Map of ChipId to ChipKind for validation
    pub(crate) chip_kind_map: HashMap<ChipId, ChipKind>,
    /// Map of ChipKind to ChipClient for forwarding updates
    pub(crate) chip_clients: HashMap<ChipKind, Box<dyn netsim_model::ChipClient>>,
    pub(crate) links: HashMap<link_api::LinkId, Link>,
    pub(crate) next_id: u32,
}

impl LinkActor {
    pub fn new(chip_clients: HashMap<ChipKind, Box<dyn netsim_model::ChipClient>>) -> Self {
        Self {
            chip_pairs: HashMap::new(),
            chip_kind_map: HashMap::new(),
            chip_clients,
            links: HashMap::new(),
            next_id: 1,
        }
    }

    pub(crate) async fn update_chip_links(&self, chip_id: ChipId) {
        let Some(kind) = self.chip_kind_map.get(&chip_id) else {
            return;
        };

        let Some(client) = self.chip_clients.get(kind) else {
            return;
        };

        let links: Vec<_> = self
            .links
            .values()
            .filter(|l| l.sender == chip_id)
            .map(|l| (l.receiver, l.rssi))
            .collect();

        let update = netsim_model::ChipUpdate { links: Some(links), ..Default::default() };

        if let Err(e) = client.update(chip_id, update).await {
            error!("Failed to update links for chip {}: {:?}", chip_id, e);
        }
    }
}
