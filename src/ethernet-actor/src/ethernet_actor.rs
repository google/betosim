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

use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use actor_framework::{ActorService, DynContext};
use futures::sink::SinkExt;
use netsim_model::{Chip, ChipId, ChipKind, ChipVariant};
use slirp_actor::SlirpClient;
use tokio_stream::StreamExt;

use crate::error::EthernetError;

fn is_chip_radio_enabled(chip: &Chip) -> bool {
    if !chip.enabled {
        return false;
    }
    match &chip.variant {
        Some(ChipVariant::Ethernet(ethernet)) => ethernet.radio.state != Some(false),
        Some(ChipVariant::CellularData(cellular)) => cellular.radio.state != Some(false),
        _ => true,
    }
}

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
    pub(crate) chip_enabled: HashMap<ChipId, Arc<AtomicBool>>,
    pub(crate) device_client: device_actor::DeviceClient,
}

impl EthernetActor {
    pub fn new(slirp_client: SlirpClient, device_client: device_actor::DeviceClient) -> Self {
        Self {
            slirp_client,
            active_chips: HashMap::new(),
            chip_enabled: HashMap::new(),
            device_client,
        }
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
        let enabled_flag = Arc::new(AtomicBool::new(is_chip_radio_enabled(&chip)));
        self.chip_enabled.insert(id, Arc::clone(&enabled_flag));
        self.active_chips.insert(id, chip);

        if let (Some(stream), Some(sink)) = (packet_stream, packet_sink) {
            let (notifier_tx, notifier_rx) =
                tokio::sync::mpsc::unbounded_channel::<netsim_model::ChipId>();

            let uplink_enabled = Arc::clone(&enabled_flag);
            let gated_stream = stream.filter(move |_| uplink_enabled.load(Ordering::Relaxed));
            let gated_sink: netsim_model::PacketSink = Box::pin(futures::sink::unfold(
                (sink, enabled_flag),
                |(mut sink, downlink_enabled), frame: bytes::Bytes| async move {
                    if downlink_enabled.load(Ordering::Relaxed) {
                        sink.send(frame).await?;
                    }
                    Ok::<_, std::io::Error>((sink, downlink_enabled))
                },
            ));

            let isolated = chip_kind == ChipKind::CELLULAR_DATA;
            if let Err(error) = self
                .slirp_client
                .register(id.0, Box::pin(gated_stream), gated_sink, Some(notifier_tx), isolated)
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
        if let Some(flag) = self.chip_enabled.get(&id) {
            flag.store(is_chip_radio_enabled(chip), Ordering::Relaxed);
        }
        Ok(chip.clone())
    }

    async fn handle_delete(
        &mut self,
        id: Self::Id,
        _ctx: &mut DynContext<Self>,
    ) -> Result<(), Self::Error> {
        self.chip_enabled.remove(&id);
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
                    .get_mut(&id)
                    .ok_or_else(|| EthernetError::Internal(format!("Chip {id} not found")))?;
                chip.enabled = true;
                match &mut chip.variant {
                    Some(ChipVariant::Ethernet(ethernet)) => {
                        ethernet.radio = Default::default();
                    }
                    Some(ChipVariant::CellularData(cellular)) => {
                        cellular.radio = Default::default();
                    }
                    _ => {}
                }
                if let Some(flag) = self.chip_enabled.get(&id) {
                    flag.store(true, Ordering::Relaxed);
                }
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

#[cfg(test)]
mod tests {
    use actor_framework::{FrameworkError, ResourceActor};
    use netsim_model::{CellularData, ChipCreate, Ethernet, Radio};

    use super::*;

    fn ethernet_variant(state: Option<bool>) -> Option<ChipVariant> {
        Some(ChipVariant::Ethernet(Ethernet { radio: Radio { state, ..Default::default() } }))
    }

    fn cellular_data_variant(state: Option<bool>) -> Option<ChipVariant> {
        Some(ChipVariant::CellularData(CellularData {
            radio: Radio { state, ..Default::default() },
        }))
    }

    fn chip_with_variant(enabled: bool, variant: Option<ChipVariant>) -> Chip {
        Chip { enabled, variant, ..Default::default() }
    }

    #[test]
    fn test_is_chip_radio_enabled_returns_false_when_ethernet_chip_disabled() {
        let chip = chip_with_variant(false, ethernet_variant(Some(true)));
        assert!(!is_chip_radio_enabled(&chip));
    }

    #[test]
    fn test_is_chip_radio_enabled_returns_false_when_cellular_data_chip_disabled() {
        let chip = chip_with_variant(false, cellular_data_variant(Some(true)));
        assert!(!is_chip_radio_enabled(&chip));
    }

    #[test]
    fn test_is_chip_radio_enabled_returns_true_when_ethernet_radio_state_unspecified() {
        let chip = chip_with_variant(true, ethernet_variant(None));
        assert!(is_chip_radio_enabled(&chip));
    }

    #[test]
    fn test_is_chip_radio_enabled_returns_true_when_ethernet_radio_enabled() {
        let chip = chip_with_variant(true, ethernet_variant(Some(true)));
        assert!(is_chip_radio_enabled(&chip));
    }

    #[test]
    fn test_is_chip_radio_enabled_returns_true_when_cellular_data_radio_state_unspecified() {
        let chip = chip_with_variant(true, cellular_data_variant(None));
        assert!(is_chip_radio_enabled(&chip));
    }

    #[test]
    fn test_is_chip_radio_enabled_returns_true_when_cellular_data_radio_enabled() {
        let chip = chip_with_variant(true, cellular_data_variant(Some(true)));
        assert!(is_chip_radio_enabled(&chip));
    }

    #[test]
    fn test_is_chip_radio_enabled_returns_true_when_variant_absent() {
        let chip = chip_with_variant(true, None);
        assert!(is_chip_radio_enabled(&chip));
    }

    #[test]
    fn test_is_chip_radio_enabled_returns_false_when_ethernet_radio_disabled() {
        let chip = chip_with_variant(true, ethernet_variant(Some(false)));
        assert!(!is_chip_radio_enabled(&chip));
    }

    #[test]
    fn test_is_chip_radio_enabled_returns_false_when_cellular_data_radio_disabled() {
        let chip = chip_with_variant(true, cellular_data_variant(Some(false)));
        assert!(!is_chip_radio_enabled(&chip));
    }

    #[tokio::test]
    async fn test_handle_create_rejects_missing_chip_id() {
        let (_, slirp_client) = slirp_actor::new();
        let (_, device_client) = device_actor::new();
        let (runner, raw_client) = ResourceActor::new(1);
        let handle = tokio::spawn(runner.run(EthernetActor::new(slirp_client, device_client)));

        let params = ChipCreate {
            packet_stream: None,
            packet_sink: None,
            chip: Chip { kind: ChipKind::ETHERNET, ..Default::default() },
        };
        let err = raw_client.create(params).await.expect_err("expected missing chip id error");
        assert!(
            matches!(
                err,
                FrameworkError::ServiceError(EthernetError::Internal(ref msg))
                    if msg == "missing chip id"
            ),
            "unexpected error: {err:?}"
        );
        handle.abort();
    }
}
