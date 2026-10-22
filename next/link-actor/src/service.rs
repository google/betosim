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
        // Validate custom ID collision if explicitly provided
        if id.as_ref().is_some_and(|custom_id| self.get_link(custom_id).is_some()) {
            return Err(LinkError::AlreadyExists);
        }

        // Validate chips exist and have matching kinds
        let sender_kind = self
            .chip_kind(&params.sender)
            .ok_or(LinkError::InvalidParam(format!("Sender chip {} not found", params.sender)))?;
        let receiver_kind = self.chip_kind(&params.receiver).ok_or(LinkError::InvalidParam(
            format!("Receiver chip {} not found", params.receiver),
        ))?;

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

        if self.has_link_pair(params.sender, params.receiver) {
            return Err(LinkError::AlreadyExists);
        }

        // Allocate ID only after all parameter validations pass
        let id = match id {
            Some(custom_id) => custom_id,
            None => self.allocate_link_id()?,
        };

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
        self.insert_link(link);
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
        Ok(self.get_link(&id).cloned())
    }

    async fn handle_update(
        &mut self,
        id: Self::Id,
        update: Self::Update,
        _ctx: &mut DynContext<Self>,
    ) -> Result<Self::Entity, Self::Error> {
        let link_clone = {
            let Some(link) = self.get_link_mut(&id) else {
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
        if let Some(link) = self.remove_link(&id) {
            let sender = link.sender;
            let receiver = link.receiver;
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
                self.register_chip(chip_id, chip_kind);
            }
            LinkAction::NotifyChipRemoved(chip_id) => {
                self.unregister_chip(&chip_id);
                let (removed_count, deleted_senders) = self.remove_chip_links(chip_id);
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
                let senders = self.unique_senders();
                self.clear();
                for sender in senders {
                    self.update_chip_links(sender).await;
                }
            }
        }
        Ok(())
    }

    async fn handle_list(
        &mut self,
        _ctx: &mut DynContext<Self>,
    ) -> Result<Vec<Self::Entity>, Self::Error> {
        Ok(self.all_links())
    }
}
