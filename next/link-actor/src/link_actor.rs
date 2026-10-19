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

use std::collections::{HashMap, HashSet};

use link_api::Link;
use netsim_model::{ChipId, ChipKind};
use tracing::error;

use crate::error::LinkError;

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
    chip_pairs: HashMap<(ChipId, ChipId), link_api::LinkId>,
    /// Map of ChipId to ChipKind for validation
    chip_kind_map: HashMap<ChipId, ChipKind>,
    /// Map of ChipKind to ChipClient for forwarding updates
    chip_clients: HashMap<ChipKind, Box<dyn netsim_model::ChipClient>>,
    links: HashMap<link_api::LinkId, Link>,
    next_id: u32,
}

impl LinkActor {
    const INITIAL_LINK_ID: u32 = 1;

    pub fn new(chip_clients: HashMap<ChipKind, Box<dyn netsim_model::ChipClient>>) -> Self {
        Self {
            chip_pairs: HashMap::new(),
            chip_kind_map: HashMap::new(),
            chip_clients,
            links: HashMap::new(),
            next_id: Self::INITIAL_LINK_ID,
        }
    }

    fn wrapping_next_id(id: u32) -> u32 {
        id.checked_add(1).unwrap_or(Self::INITIAL_LINK_ID)
    }

    pub(crate) fn allocate_link_id(&mut self) -> Result<link_api::LinkId, LinkError> {
        let start = self.next_id;
        while self.links.contains_key(&link_api::LinkId(self.next_id)) {
            self.next_id = Self::wrapping_next_id(self.next_id);
            if self.next_id == start {
                return Err(LinkError::from("LinkId space exhausted".to_string()));
            }
        }
        let id = link_api::LinkId(self.next_id);
        self.next_id = Self::wrapping_next_id(self.next_id);
        Ok(id)
    }

    pub(crate) fn chip_kind(&self, chip_id: &ChipId) -> Option<ChipKind> {
        self.chip_kind_map.get(chip_id).copied()
    }

    pub(crate) fn register_chip(&mut self, chip_id: ChipId, kind: ChipKind) {
        self.chip_kind_map.insert(chip_id, kind);
    }

    pub(crate) fn unregister_chip(&mut self, chip_id: &ChipId) {
        self.chip_kind_map.remove(chip_id);
    }

    pub(crate) fn has_link_pair(&self, sender: ChipId, receiver: ChipId) -> bool {
        self.chip_pairs.contains_key(&(sender, receiver))
    }

    pub(crate) fn insert_link(&mut self, link: Link) {
        self.chip_pairs.insert((link.sender, link.receiver), link.id);
        self.links.insert(link.id, link);
    }

    pub(crate) fn get_link(&self, id: &link_api::LinkId) -> Option<&Link> {
        self.links.get(id)
    }

    pub(crate) fn get_link_mut(&mut self, id: &link_api::LinkId) -> Option<&mut Link> {
        self.links.get_mut(id)
    }

    pub(crate) fn remove_link(&mut self, id: &link_api::LinkId) -> Option<Link> {
        self.links.remove(id).inspect(|link| {
            self.chip_pairs.remove(&(link.sender, link.receiver));
        })
    }

    pub(crate) fn remove_chip_links(&mut self, chip_id: ChipId) -> (usize, HashSet<ChipId>) {
        let initial_count = self.links.len();
        let mut deleted_senders = HashSet::new();
        let chip_pairs = &mut self.chip_pairs;
        self.links.retain(|_, link| {
            let should_remove = link.sender == chip_id || link.receiver == chip_id;
            if should_remove {
                chip_pairs.remove(&(link.sender, link.receiver));
                if link.sender != chip_id {
                    deleted_senders.insert(link.sender);
                }
            }
            !should_remove
        });
        (initial_count - self.links.len(), deleted_senders)
    }

    pub(crate) fn clear(&mut self) {
        self.links.clear();
        self.chip_pairs.clear();
        self.next_id = Self::INITIAL_LINK_ID;
    }

    pub(crate) fn all_links(&self) -> Vec<Link> {
        self.links.values().cloned().collect()
    }

    pub(crate) fn unique_senders(&self) -> HashSet<ChipId> {
        self.links.values().map(|l| l.sender).collect()
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

        if let Err(error) = client.update(chip_id, update).await {
            error!(?error, "Failed to update links for chip {}", chip_id);
        }
    }
}
