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

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use actor_framework::{ActorService, DynContext};
use capture_api::{CaptureAction, CaptureActionResult, CaptureCreate, CaptureInfo};
use netsim_model::{ChipError, ChipId, ChipKind};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tokio_stream::wrappers::UnboundedReceiverStream;
use tracing::{info, warn};

use crate::{
    bt_pcap::BluetoothH4Writer,
    capture_actor::CaptureActor,
    error::CaptureError,
    ethernet_pcap::EthernetPcapWriter,
    modem_pcap::ModemPcapWriter,
    nci_pcap::NciPcapWriter,
    uwb_pcap::UwbPcapWriter,
    writer::{CaptureWriter, DataLinkType, PcapWriter},
};

/// Entity representing a packet capture for a specific chip.
///
/// This entity manages the state of a single packet capture session,
/// including whether it is enabled and the associated metadata.
/// It does not hold the actual writer, which is stored in the context
/// to allow shared access.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct InternalCaptureInfo {
    pub(crate) info: CaptureInfo,
    /// Flag to be updated when capture status changes.
    /// This is used by the stream wrappers to check if capture is enabled
    /// without locking the actor.
    #[serde(skip)]
    enabled_flag: Arc<AtomicBool>,
    /// Used to determine whether further error logs while writing captures
    /// should be suppressed.
    #[serde(skip)]
    pub(crate) has_warned_on_write: bool,
    #[serde(skip)]
    sender: Option<
        mpsc::UnboundedSender<(std::time::SystemTime, capture_api::Direction, bytes::Bytes)>,
    >,
}

fn current_epoch_timestamp() -> (i64, i32) {
    let d = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    (
        i64::try_from(d.as_secs()).unwrap_or(i64::MAX),
        i32::try_from(d.subsec_nanos()).expect("subsec_nanos is always < 1_000_000_000"),
    )
}

impl InternalCaptureInfo {
    fn from_create_params(id: ChipId, params: CaptureCreate) -> Result<Self, CaptureError> {
        let enabled = params.enabled_flag.load(Ordering::SeqCst);
        let (seconds, nanos) = if enabled { current_epoch_timestamp() } else { (0, 0) };
        Ok(Self {
            info: CaptureInfo {
                chip_id: id,
                chip_kind: params.chip_kind,
                device_name: params.device_name,
                enabled,
                records_written: 0,
                bytes_written: 0,
                seconds,
                nanos,
                filepath: None,
            },
            enabled_flag: params.enabled_flag,
            has_warned_on_write: false,
            sender: None,
        })
    }
}

impl CaptureActor {
    fn get_filepath(
        &self,
        id: ChipId,
        device_name: &str,
        chip_kind: ChipKind,
    ) -> std::path::PathBuf {
        let safe_device_name: String = device_name
            .chars()
            .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
            .collect();
        let filename = format!("netsim-{}-{safe_device_name}-{chip_kind:?}.pcap", id.0);
        if let Some(dir) = self.capture_dir.as_ref() {
            dir.join(filename)
        } else {
            let mut path = common::system::netsimd_temp_dir();
            path.push("pcaps");
            path.join(filename)
        }
    }

    async fn create_entity(
        &mut self,
        entity: &mut InternalCaptureInfo,
        ctx: &mut DynContext<Self>,
    ) -> Result<(), CaptureError> {
        entity.info.filepath = Some(self.get_filepath(
            entity.info.chip_id,
            &entity.info.device_name,
            entity.info.chip_kind,
        ));
        // If enabled by default (either via params or context), set up the writer
        if entity.info.enabled || self.default_capture_enabled {
            self.update_entity(entity, true, ctx).await?;
        }
        Ok(())
    }

    async fn update_entity(
        &mut self,
        entity: &mut InternalCaptureInfo,
        enabled: bool,
        _ctx: &mut DynContext<Self>,
    ) -> Result<(), CaptureError> {
        let state_changed = entity.info.enabled != enabled;
        if state_changed {
            entity.info.enabled = enabled;
            entity.enabled_flag.store(enabled, Ordering::SeqCst);
        }

        if entity.info.enabled {
            if state_changed || (entity.info.seconds == 0 && entity.info.nanos == 0) {
                (entity.info.seconds, entity.info.nanos) = current_epoch_timestamp();
            }
            // If enabling, create a writer if one does not exist.
            if !self.writers.contains_key(&entity.info.chip_id) {
                let writer = self.create_writer(entity).await?;
                self.writers.insert(entity.info.chip_id, writer);
            }
        } else {
            // Disable capture: remove the writer to close the file
            if let Some(mut writer) = self.writers.remove(&entity.info.chip_id)
                && let Err(err) = writer.flush().await
            {
                warn!("Failed to flush writer for chip {}: {err}", entity.info.chip_id);
            }
        }
        Ok(())
    }

    async fn delete_entity(
        &mut self,
        entity: &InternalCaptureInfo,
        _ctx: &mut DynContext<Self>,
    ) -> Result<(), CaptureError> {
        if let Some(mut writer) = self.writers.remove(&entity.info.chip_id)
            && let Err(err) = writer.flush().await
        {
            warn!("Failed to flush writer for chip {}: {err}", entity.info.chip_id);
        }
        Ok(())
    }

    async fn create_writer(
        &self,
        entity: &InternalCaptureInfo,
    ) -> Result<Box<dyn CaptureWriter>, CaptureError> {
        let filepath = entity.info.filepath.clone().unwrap_or_else(|| {
            self.get_filepath(entity.info.chip_id, &entity.info.device_name, entity.info.chip_kind)
        });
        if let Some(pcap_dir) = filepath.parent()
            && let Err(e) = std::fs::create_dir_all(pcap_dir)
        {
            warn!("Failed to create pcap directory {}: {}", pcap_dir.display(), e);
        }
        info!("Creating capture file: {}", filepath.display());
        let writer: Box<dyn CaptureWriter> = match entity.info.chip_kind {
            ChipKind::BLUETOOTH => BluetoothH4Writer::new(&filepath).await?,
            ChipKind::UWB => UwbPcapWriter::new(&filepath).await?,
            ChipKind::WIFI => crate::wifi_pcap::WifiPcapWriter::new(&filepath).await?,
            ChipKind::ETHERNET | ChipKind::CELLULAR_DATA => {
                EthernetPcapWriter::new(&filepath).await?
            }
            ChipKind::NFC => NciPcapWriter::new(&filepath).await?,
            ChipKind::CELLULAR => ModemPcapWriter::new(&filepath).await?,
            // Fallback for custom/unregistered protocols (use DataLinkType::User0)
            ChipKind::UNSPECIFIED => {
                Box::new(PcapWriter::new(&filepath, DataLinkType::User0).await?)
            }
        };
        Ok(writer)
    }

    async fn list_entities(&self) -> Vec<CaptureInfo> {
        let mut infos = Vec::new();
        for e in self.entities.values() {
            let (records_written, bytes_written) =
                self.writers.get(&e.info.chip_id).map(|w| w.get_stats()).unwrap_or_default();
            let mut info = e.info.clone();
            info.records_written = records_written;
            info.bytes_written = bytes_written;
            infos.push(info);
        }
        infos
    }
    async fn handle_entity_action(
        &mut self,
        entity: &mut InternalCaptureInfo,
        action: CaptureAction,
        ctx: &mut DynContext<Self>,
    ) -> Result<CaptureActionResult, CaptureError> {
        match action {
            CaptureAction::GetPacketSender => {
                let sender = entity
                    .sender
                    .get_or_insert_with(|| {
                        let (tx, rx) = mpsc::unbounded_channel();

                        ctx.add_typed_stream(
                            entity.info.chip_id.0 as usize,
                            Box::pin(UnboundedReceiverStream::new(rx)),
                        );

                        tx
                    })
                    .clone();
                Ok(CaptureActionResult::PacketSender(sender))
            }

            CaptureAction::Patch { chip_id, enabled } => {
                if entity.info.chip_id == chip_id {
                    self.update_entity(entity, enabled, ctx).await?;
                    let (records_written, bytes_written) =
                        self.writers.get(&chip_id).map(|w| w.get_stats()).unwrap_or_default();
                    let mut info = entity.info.clone();
                    info.records_written = records_written;
                    info.bytes_written = bytes_written;
                    Ok(CaptureActionResult::Updated(info))
                } else {
                    Ok(CaptureActionResult::Success)
                }
            }
            CaptureAction::Delete { chip_id } => {
                if entity.info.chip_id == chip_id {
                    self.delete_entity(entity, ctx).await?;
                }
                Ok(CaptureActionResult::Success)
            }
            CaptureAction::SetCaptureDirectory { ref path } => {
                self.capture_dir = Some(path.clone());
                Ok(CaptureActionResult::Success)
            }
            CaptureAction::Create { .. } => Ok(CaptureActionResult::Success),
        }
    }
}

impl ActorService for CaptureActor {
    type Id = ChipId;
    type Create = CaptureCreate;
    type Update = bool; // Enabled status
    type Action = CaptureAction;
    type ActionResult = CaptureActionResult;
    type Error = CaptureError;
    type Entity = CaptureInfo;
    type TypedStream = (std::time::SystemTime, capture_api::Direction, bytes::Bytes);

    async fn handle_create(
        &mut self,
        id: Option<Self::Id>,
        params: Self::Create,
        _ctx: &mut DynContext<Self>,
    ) -> Result<Self::Id, Self::Error> {
        let id = id.ok_or_else(|| ChipError::InvalidArguments(Box::from("chip id required")))?;
        let mut entity = InternalCaptureInfo::from_create_params(id, params)?;
        self.create_entity(&mut entity, _ctx).await?;

        self.entities.insert(id, entity);
        Ok(id)
    }

    async fn handle_get(
        &self,
        id: Self::Id,
        _ctx: &mut DynContext<Self>,
    ) -> Result<Option<Self::Entity>, Self::Error> {
        if let Some(entity) = self.entities.get(&id) {
            let (records_written, bytes_written) =
                self.writers.get(&id).map(|w| w.get_stats()).unwrap_or_default();
            let mut info = entity.info.clone();
            info.records_written = records_written;
            info.bytes_written = bytes_written;
            Ok(Some(info))
        } else {
            Ok(None)
        }
    }

    async fn handle_update(
        &mut self,
        id: Self::Id,
        update: Self::Update,
        _ctx: &mut DynContext<Self>,
    ) -> Result<Self::Entity, Self::Error> {
        let mut entity = self.entities.remove(&id).ok_or_else(|| ChipError::ChipNotFound(id))?;
        self.update_entity(&mut entity, update, _ctx).await?;
        let (records_written, bytes_written) =
            self.writers.get(&id).map(|w| w.get_stats()).unwrap_or_default();
        let mut info = entity.info.clone();
        info.records_written = records_written;
        info.bytes_written = bytes_written;
        self.entities.insert(id, entity);
        Ok(info)
    }

    async fn handle_delete(
        &mut self,
        id: Self::Id,
        ctx: &mut DynContext<Self>,
    ) -> Result<(), Self::Error> {
        let entity = self.entities.remove(&id).ok_or_else(|| ChipError::ChipNotFound(id))?;
        self.delete_entity(&entity, ctx).await
    }

    async fn handle_action(
        &mut self,
        id: Option<Self::Id>,
        action: Self::Action,
        ctx: &mut DynContext<Self>,
    ) -> Result<Self::ActionResult, Self::Error> {
        let Some(id) = id else {
            // Global action
            match action {
                CaptureAction::SetCaptureDirectory { path } => {
                    self.capture_dir = Some(path);
                    return Ok(CaptureActionResult::Success);
                }
                _ => {
                    // Other actions require Entity, so they are invalid here (or no-op).
                    return Ok(CaptureActionResult::Success);
                }
            }
        };

        let Some(mut entity) = self.entities.remove(&id) else {
            return Ok(CaptureActionResult::Success);
        };

        let result = self.handle_entity_action(&mut entity, action, ctx).await;

        self.entities.insert(id, entity);
        result
    }

    async fn handle_list(
        &mut self,
        _ctx: &mut DynContext<Self>,
    ) -> Result<Vec<Self::Entity>, Self::Error> {
        Ok(self.list_entities().await)
    }
}
