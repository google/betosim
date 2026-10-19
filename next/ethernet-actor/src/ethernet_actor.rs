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

use std::collections::HashMap;

use actor_framework::{ActorService, DynContext};
use netsim_model::{Chip, ChipId, ChipKind};
use slirp_actor::SlirpClient;

use crate::error::EthernetError;

#[derive(Debug)]
pub enum EthernetReq {
    GetStatistics,
    Reset { id: ChipId },
}

#[derive(Debug)]
pub enum EthernetResponse {
    Statistics(Box<[netsim_model::NetsimRadioStats]>),
    Chip(Box<Chip>),
}

pub struct EthernetActor {
    slirp_client: SlirpClient,
    pub(crate) active_chips: HashMap<ChipId, Chip>,
    pub(crate) device_client: device_actor::DeviceClient,
}

impl EthernetActor {
    pub fn new(slirp_client: SlirpClient, device_client: device_actor::DeviceClient) -> Self {
        Self { slirp_client, active_chips: HashMap::new(), device_client }
    }
}

impl ActorService for EthernetActor {
    type Id = ChipId;
    type Create = netsim_model::ChipCreate;
    type Update = netsim_model::ChipUpdate;
    type Action = EthernetReq;
    type ActionResult = EthernetResponse;
    type Error = EthernetError;
    type Entity = Chip;
    type TypedStream = ChipId;

    async fn handle_create(
        &mut self,
        id: Option<Self::Id>,
        params: Self::Create,
        ctx: &mut DynContext<Self>,
    ) -> Result<Self::Id, Self::Error> {
        let id = id.ok_or_else(|| EthernetError::Internal("missing chip id".into()))?;
        let chip_kind = params.chip.kind;

        match chip_kind {
            ChipKind::CELLULAR_DATA => {
                tracing::info!("Creating CELLULAR_DATA interface for chip {id}");
            }
            ChipKind::ETHERNET => {
                tracing::info!("Creating ETHERNET interface for chip {id}");
            }
            _ => return Err(EthernetError::InvalidChipKind),
        }

        let packet_stream = params.packet_stream;
        let packet_sink = params.packet_sink;
        let mut chip = params.chip;
        chip.id = id.0;
        self.active_chips.insert(id, chip);

        if let (Some(stream), Some(sink)) = (packet_stream, packet_sink) {
            let (notifier_tx, notifier_rx) =
                tokio::sync::mpsc::unbounded_channel::<netsim_model::ChipId>();

            let isolated = chip_kind == ChipKind::CELLULAR_DATA;
            if let Err(error) = self
                .slirp_client
                .register(id.0, Box::pin(stream), sink, Some(notifier_tx), isolated)
                .await
            {
                tracing::warn!(%error, "EthernetActor: Failed to register with SlirpActor");
            }

            ctx.add_typed_stream(
                id.0 as usize,
                Box::pin(tokio_stream::wrappers::UnboundedReceiverStream::new(notifier_rx)),
            );
        }

        Ok(id)
    }

    async fn handle_get(
        &self,
        id: Self::Id,
        _ctx: &mut DynContext<Self>,
    ) -> Result<Option<Self::Entity>, Self::Error> {
        Ok(self.active_chips.get(&id).cloned())
    }

    async fn handle_update(
        &mut self,
        id: Self::Id,
        update: Self::Update,
        _ctx: &mut DynContext<Self>,
    ) -> Result<Self::Entity, Self::Error> {
        let chip = self
            .active_chips
            .get_mut(&id)
            .ok_or_else(|| EthernetError::Internal(format!("Chip {id} not found")))?;
        update.apply(chip);
        Ok(chip.clone())
    }

    async fn handle_delete(
        &mut self,
        id: Self::Id,
        _ctx: &mut DynContext<Self>,
    ) -> Result<(), Self::Error> {
        if let Some(chip) = self.active_chips.remove(&id) {
            let _ = self.slirp_client.unregister(id.0).await;
            let dc = self.device_client.clone();
            let device_id = chip.device_id;
            tokio::spawn(async move {
                let _ = dc.notify_chip_removed(device_id, id).await;
            });
        }
        Ok(())
    }

    async fn handle_action(
        &mut self,
        _id: Option<Self::Id>,
        action: Self::Action,
        _ctx: &mut DynContext<Self>,
    ) -> Result<Self::ActionResult, Self::Error> {
        match action {
            EthernetReq::GetStatistics => {
                let stats = Vec::new();
                Ok(EthernetResponse::Statistics(stats.into_boxed_slice()))
            }
            EthernetReq::Reset { id } => {
                let chip = self
                    .active_chips
                    .get(&id)
                    .ok_or_else(|| EthernetError::Internal(format!("Chip {id} not found")))?;
                Ok(EthernetResponse::Chip(Box::new(chip.clone())))
            }
        }
    }

    async fn handle_list(
        &mut self,
        _ctx: &mut DynContext<Self>,
    ) -> Result<Vec<Self::Entity>, Self::Error> {
        Ok(self.active_chips.values().cloned().collect())
    }
}
