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
use futures::{SinkExt, StreamExt};
use netsim_model::{Chip, ChipId, ChipVariant, ChipVariantUpdate, RadioUpdate, WifiUpdate};
use tokio::sync::mpsc;

use crate::{
    error::WifiError,
    wifi_actor::{WifiActor, WifiReq, WifiResponse},
};

impl ActorService for WifiActor {
    type Id = ChipId;
    type Create = netsim_model::ChipCreate;
    type Update = netsim_model::ChipUpdate;
    type Action = WifiReq;
    type ActionResult = WifiResponse;
    type Error = WifiError;
    type Entity = Chip;
    type TypedStream = bytes::Bytes;

    async fn handle_create(
        &mut self,
        id: Option<Self::Id>,
        mut params: Self::Create,
        ctx: &mut DynContext<Self>,
    ) -> Result<Self::Id, Self::Error> {
        let id = id.ok_or_else(|| WifiError::Internal("missing chip id".into()))?;
        if self.active_chips.contains_key(&id) {
            return Err(WifiError::Internal(Box::from(format!("Chip {} already exists", id))));
        }

        let stream = params
            .packet_stream
            .take()
            .ok_or(WifiError::Internal(Box::from("Missing PacketStream")))?;
        let sink = params
            .packet_sink
            .take()
            .ok_or(WifiError::Internal(Box::from("Missing PacketSink")))?;

        // Spawn sink task
        let (tx, mut rx) = mpsc::unbounded_channel::<bytes::Bytes>();
        self.senders.insert(id, tx);

        // Define sink task: forwards bytes from channel to PacketSink
        let sink_id = id;
        ctx.spawn(
            sink_id,
            Box::pin(async move {
                let mut sink = sink;
                while let Some(packet) = rx.recv().await {
                    if sink.send(packet).await.is_err() {
                        break;
                    }
                }
                let _ = sink.close().await;
                sink_id
            }),
        );

        // Register stream with context for polling
        let mapped_stream = stream.map(move |packet| packet);
        ctx.add_stream(id, Box::pin(mapped_stream));

        let mut chip = params.chip;
        chip.id = id.0;
        self.active_chips.insert(id, chip.clone());
        self.initial_chips.insert(id, chip);

        // Notify Medium about new chip
        self.medium.add(id.0);

        // Notify Gateway about new chip (e.g. attach TAP)
        self.gateway.on_chip_create(id, ctx).await;

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
            .ok_or_else(|| WifiError::Internal(Box::from(format!("Chip {} not found", id))))?;

        // Apply update to chip
        update.apply(chip);

        // Enable medium if either radio state or chip enabled state is set to true in
        // the update.
        if let Some(ChipVariantUpdate::Wifi(WifiUpdate {
            radio: RadioUpdate { state: Some(state) },
        })) = &update.variant
        {
            self.medium.set_enabled(id.0, *state);
        }
        if let Some(enabled) = update.enabled {
            self.medium.set_enabled(id.0, enabled);
        }

        // Synchronize the chip struct's state with the actual medium state.
        if let Ok(enabled) = self.medium.enabled(id.0) {
            if let Some(ChipVariant::Wifi(ref mut radio)) = chip.variant {
                radio.radio.state = Some(enabled);
            }
            chip.enabled = enabled;
        }

        Ok(chip.clone())
    }

    async fn handle_delete(
        &mut self,
        id: Self::Id,
        ctx: &mut DynContext<Self>,
    ) -> Result<(), Self::Error> {
        self.handle_delete_impl(id, ctx).await
    }

    async fn handle_action(
        &mut self,
        _id: Option<Self::Id>,
        action: Self::Action,
        _ctx: &mut DynContext<Self>,
    ) -> Result<Self::ActionResult, Self::Error> {
        match action {
            WifiReq::GetStatistics => {
                let mut stats = Vec::new();
                for (id, chip) in &self.active_chips {
                    let rx_count = self.medium.get_rx_count(id.0);
                    let tx_count = self.medium.get_tx_count(id.0);
                    let p2p_tx_count = self.medium.get_p2p_tx_count(id.0);
                    let p2p_rx_count = self.medium.get_p2p_rx_count(id.0);
                    let mut radio_stats = netsim_model::NetsimRadioStats::default();
                    radio_stats.id = id.0;
                    radio_stats.name = chip.name.clone();
                    radio_stats.kind = netsim_model::RadioKind::Wifi;
                    radio_stats.tx_count = tx_count as u64;
                    radio_stats.rx_count = rx_count as u64;
                    radio_stats.p2p_tx_count = p2p_tx_count;
                    radio_stats.p2p_rx_count = p2p_rx_count;
                    stats.push(radio_stats);
                }
                Ok(WifiResponse::Statistics(stats.into_boxed_slice()))
            }
            WifiReq::GetGlobalStats => {
                let stats = self.medium.wifi_stats.to_proto();
                Ok(WifiResponse::GlobalStats(Box::new(stats)))
            }
            WifiReq::Reset { id } => {
                self.medium.reset(id.0);
                let initial_chip = self.initial_chips.get(&id).ok_or_else(|| {
                    WifiError::Internal(Box::from(format!("Initial chip not found: {}", id)))
                })?;
                let chip = self.active_chips.get_mut(&id).ok_or_else(|| {
                    WifiError::Internal(Box::from(format!("Chip not found: {}", id)))
                })?;
                *chip = initial_chip.clone();
                let chip = chip.clone();
                Ok(WifiResponse::Chip(Box::new(chip)))
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
