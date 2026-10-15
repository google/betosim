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

use actor_framework::{ActorService, DynContext};
use link_api::LinkAction;
use tracing::info;

use crate::{error::LinkError, link_actor::LinkActor};

impl ActorService for LinkActor {
    type Id = link_api::LinkId;
    type Create = link_api::LinkCreate;
    type Update = link_api::LinkUpdate;
    type Action = LinkAction;
    type ActionResult = ();
    type Error = LinkError;
    type Entity = link_api::Link;
    type TypedStream = ();

    async fn handle_create(
        &mut self,
        id: Option<Self::Id>,
        params: Self::Create,
        _ctx: &mut DynContext<Self>,
    ) -> Result<Self::Id, Self::Error> {
        let id = id.unwrap_or_else(|| {
            let id = link_api::LinkId(self.next_id);
            self.next_id += 1;
            id
        });

        // Validate chips exist and have matching kinds
        let sender_kind =
            self.chip_kind_map.get(&params.sender).copied().ok_or(LinkError::InvalidParam(
                format!("Sender chip {} not found", &params.sender),
            ))?;
        let receiver_kind = self.chip_kind_map.get(&params.receiver).copied().ok_or(
            LinkError::InvalidParam(format!("Receiver chip {} not found", &params.receiver)),
        )?;

        if sender_kind != receiver_kind {
            return Err(LinkError::InvalidParam(format!(
                "Chip kind mismatch: sender={sender_kind}, receiver={receiver_kind}",
            )));
        }

        if params.sender == params.receiver {
            return Err(LinkError::InvalidParam(format!(
                "Self-link not allowed: chip={}",
                params.sender
            )));
        }

        if self.chip_pairs.contains_key(&(params.sender, params.receiver)) {
            return Err(LinkError::AlreadyExists);
        }

        // Create Entity (Link)
        let link = link_api::Link {
            id,
            sender: params.sender,
            receiver: params.receiver,
            kind: sender_kind,
            rssi: params.rssi,
        };

        let sender = link.sender;
        let receiver = link.receiver;
        self.chip_pairs.insert((sender, receiver), link.id);

        self.links.insert(id, link);
        self.update_chip_links(sender).await;
        info!(
            "LinkActor: Set RSSI between sender {sender} and receiver {receiver} on {sender_kind}: {}",
            params.rssi
        );
        Ok(id)
    }

    async fn handle_get(
        &self,
        id: Self::Id,
        _ctx: &mut DynContext<Self>,
    ) -> Result<Option<Self::Entity>, Self::Error> {
        Ok(actor_framework::utils::handle_get_default(&self.links, &id))
    }

    async fn handle_update(
        &mut self,
        id: Self::Id,
        update: Self::Update,
        _ctx: &mut DynContext<Self>,
    ) -> Result<Self::Entity, Self::Error> {
        let link_clone = {
            let Some(link) = self.links.get_mut(&id) else {
                return Err(LinkError::NotFound(id.0));
            };

            if let Some(rssi) = update.rssi {
                link.rssi = rssi;
            }
            link.clone()
        };

        if let Some(rssi) = update.rssi {
            info!(
                "LinkActor: Updated RSSI between sender {} and receiver {} on {}: {}",
                link_clone.sender, link_clone.receiver, link_clone.kind, rssi
            );
            self.update_chip_links(link_clone.sender).await;
        }
        Ok(link_clone)
    }

    async fn handle_delete(
        &mut self,
        id: Self::Id,
        _ctx: &mut DynContext<Self>,
    ) -> Result<(), Self::Error> {
        if let Some(link) = self.links.remove(&id) {
            let sender = link.sender;
            let receiver = link.receiver;
            self.chip_pairs.remove(&(sender, receiver));
            self.update_chip_links(sender).await;
            info!(
                "LinkActor: Deleted RSSI between sender {sender} and receiver {receiver} on {}",
                link.kind
            );
            Ok(())
        } else {
            Err(LinkError::NotFound(id.0))
        }
    }

    async fn handle_action(
        &mut self,
        _id: Option<Self::Id>,
        action: Self::Action,
        _ctx: &mut DynContext<Self>,
    ) -> Result<Self::ActionResult, Self::Error> {
        match action {
            LinkAction::NotifyChipAdded(chip_id, chip_kind) => {
                // LinkActor needs to know about all chips to validate link creation requests.
                self.chip_kind_map.insert(chip_id, chip_kind);
            }
            LinkAction::NotifyChipRemoved(chip_id) => {
                self.chip_kind_map.remove(&chip_id);

                let initial_count = self.links.len();
                let mut deleted_senders = std::collections::HashSet::new();
                let chip_pairs = &mut self.chip_pairs;
                // Remove all links associated with the removed chip
                self.links.retain(|_, link| {
                    if link.sender == chip_id || link.receiver == chip_id {
                        chip_pairs.remove(&(link.sender, link.receiver));
                        if link.sender != chip_id {
                            deleted_senders.insert(link.sender);
                        }
                        return false;
                    }
                    true
                });

                let removed_count = initial_count - self.links.len();
                if removed_count > 0 {
                    info!(
                        "LinkActor: Removed {removed_count} RSSI link(s) associated with chip {chip_id}"
                    );
                }

                // Update the remaining chips that were connected to the removed chip
                for sender in deleted_senders {
                    self.update_chip_links(sender).await;
                }
            }
            LinkAction::Reset => {
                info!("LinkActor: Resetting all links");
                self.links.clear();
                self.chip_pairs.clear();
            }
        }
        Ok(())
    }

    async fn handle_list(
        &mut self,
        _ctx: &mut DynContext<Self>,
    ) -> Result<Vec<Self::Entity>, Self::Error> {
        Ok(actor_framework::utils::handle_list_map(&self.links, |e| e.clone()))
    }
}
