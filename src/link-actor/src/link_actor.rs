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

use link_api::{Link, LinkId};
use netsim_model::{ChipClient, ChipId, ChipKind, ChipUpdate};
use tracing::error;

use crate::error::LinkError;

/// Context for LinkActor.
///
/// Holds the global state shared across all Link entities.
pub struct LinkActor {
    /// Index for fast lookup/upsert: (Sender, Receiver) -> LinkId
    chip_pairs: HashMap<(ChipId, ChipId), LinkId>,
    /// Map of ChipId to ChipKind for validation
    chip_kind_map: HashMap<ChipId, ChipKind>,
    /// Map of ChipKind to ChipClient for forwarding updates
    chip_clients: HashMap<ChipKind, Box<dyn ChipClient>>,
    links: HashMap<LinkId, Link>,
    next_id: u32,
}

impl Default for LinkActor {
    fn default() -> Self {
        Self::new(HashMap::new())
    }
}

impl LinkActor {
    const INITIAL_LINK_ID: u32 = 1;

    pub fn new(chip_clients: HashMap<ChipKind, Box<dyn ChipClient>>) -> Self {
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

    pub(crate) fn allocate_link_id(&mut self) -> Result<LinkId, LinkError> {
        let start = self.next_id;
        while self.links.contains_key(&LinkId(self.next_id)) {
            self.next_id = Self::wrapping_next_id(self.next_id);
            if self.next_id == start {
                return Err(LinkError::Internal("link ID space exhausted".into()));
            }
        }
        let id = LinkId(self.next_id);
        self.next_id = Self::wrapping_next_id(self.next_id);
        Ok(id)
    }

    pub(crate) fn chip_kind(&self, chip_id: ChipId) -> Option<ChipKind> {
        self.chip_kind_map.get(&chip_id).copied()
    }

    pub(crate) fn register_chip(&mut self, chip_id: ChipId, kind: ChipKind) {
        self.chip_kind_map.insert(chip_id, kind);
    }

    pub(crate) fn unregister_chip(&mut self, chip_id: ChipId) {
        self.chip_kind_map.remove(&chip_id);
    }

    pub(crate) fn has_link_pair(&self, sender: ChipId, receiver: ChipId) -> bool {
        self.chip_pairs.contains_key(&(sender, receiver))
    }

    pub(crate) fn insert_link(&mut self, link: Link) {
        self.chip_pairs.insert((link.sender, link.receiver), link.id);
        self.links.insert(link.id, link);
    }

    pub(crate) fn get_link(&self, id: LinkId) -> Option<&Link> {
        self.links.get(&id)
    }

    pub(crate) fn get_link_mut(&mut self, id: LinkId) -> Option<&mut Link> {
        self.links.get_mut(&id)
    }

    pub(crate) fn remove_link(&mut self, id: LinkId) -> Option<Link> {
        self.links.remove(&id).inspect(|link| {
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
        let Some(kind) = self.chip_kind(chip_id) else {
            return;
        };

        let Some(client) = self.chip_clients.get(&kind) else {
            return;
        };

        let links: Vec<_> = self
            .links
            .values()
            .filter_map(|l| (l.sender == chip_id).then_some((l.receiver, l.rssi)))
            .collect();

        let update = ChipUpdate { links: Some(links), ..Default::default() };

        if let Err(error) = client.update(chip_id, update).await {
            error!(%error, %chip_id, "Failed to update links for chip");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn link(id: u32, sender: u32, receiver: u32, rssi: i8) -> Link {
        Link {
            id: LinkId(id),
            sender: ChipId(sender),
            receiver: ChipId(receiver),
            rssi,
            kind: ChipKind::BLUETOOTH,
        }
    }

    #[test]
    fn test_default_initializes_next_id_to_initial_link_id() {
        let mut actor = LinkActor::default();
        assert_eq!(actor.allocate_link_id().expect("first allocation succeeds"), LinkId(1));
    }

    #[test]
    fn test_allocate_link_id_wraparound_to_initial_id() {
        let mut actor = LinkActor::new(HashMap::new());
        actor.next_id = u32::MAX;

        let first = actor.allocate_link_id().expect("first allocation succeeds");
        assert_eq!(first, LinkId(u32::MAX));

        let second = actor.allocate_link_id().expect("second allocation wraps to initial id");
        assert_eq!(second, LinkId(LinkActor::INITIAL_LINK_ID));
    }

    #[test]
    fn test_allocate_link_id_skips_occupied_ids() {
        let mut actor = LinkActor::new(HashMap::new());
        actor.insert_link(link(1, 10, 20, -50));
        actor.insert_link(link(2, 10, 21, -50));
        actor.insert_link(link(4, 10, 22, -50));
        actor.next_id = 1;

        let first = actor.allocate_link_id().expect("allocation skips 1 and 2");
        assert_eq!(first, LinkId(3));
        assert_eq!(actor.next_id, 4);

        let second = actor.allocate_link_id().expect("allocation skips 4");
        assert_eq!(second, LinkId(5));
        assert_eq!(actor.next_id, 6);
    }

    #[test]
    fn test_remove_chip_links_bidirectional_and_deleted_senders() {
        let mut actor = LinkActor::new(HashMap::new());
        actor.insert_link(link(1, 1, 2, -50));
        actor.insert_link(link(2, 2, 1, -60));
        actor.insert_link(link(3, 3, 1, -70));
        actor.insert_link(link(4, 3, 4, -80));

        let (removed_count, deleted_senders) = actor.remove_chip_links(ChipId(1));
        assert_eq!(removed_count, 3);
        assert_eq!(deleted_senders, HashSet::from([ChipId(2), ChipId(3)]));

        assert!(actor.has_link_pair(ChipId(3), ChipId(4)));
        assert_eq!(actor.links.len(), 1);
        assert!(actor.links.contains_key(&LinkId(4)));
        assert!(!actor.has_link_pair(ChipId(1), ChipId(2)));
        assert!(!actor.has_link_pair(ChipId(2), ChipId(1)));
        assert!(!actor.has_link_pair(ChipId(3), ChipId(1)));
    }

    #[test]
    fn test_remove_link_purges_chip_pairs() {
        let mut actor = LinkActor::new(HashMap::new());
        let link_id = LinkId(42);
        actor.insert_link(link(42, 10, 20, -50));
        assert!(actor.has_link_pair(ChipId(10), ChipId(20)));
        let removed = actor.remove_link(link_id);
        assert_eq!(removed.map(|l| l.id), Some(link_id));
        assert!(!actor.has_link_pair(ChipId(10), ChipId(20)));
        assert_eq!(actor.get_link(link_id), None);
    }
}
