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

use std::{collections::HashMap, path::PathBuf, sync::atomic::Ordering};

use netsim_model::ChipId;
use tracing::warn;

use crate::writer::CaptureWriter;

/// The context for the Capture actor.
///
/// This struct holds the shared state required by the actor, including the
/// `CaptureWriter` instances (file and live streams) for writing packet captures.
pub struct CaptureActor {
    /// Multimap of ChipId to active CaptureWriters (file writer and live stream
    /// writers).
    pub(crate) writers: HashMap<ChipId, Vec<Box<dyn CaptureWriter>>>,
    /// Default capture state for new captures.
    pub(crate) default_capture_enabled: bool,
    /// Default capture directory.
    pub(crate) capture_dir: Option<PathBuf>,
    /// Map of active Capture Entities.
    pub(crate) entities: HashMap<ChipId, crate::service::InternalCaptureInfo>,
}

impl CaptureActor {
    pub fn new(default_capture_enabled: bool, capture_dir: Option<PathBuf>) -> Self {
        Self {
            writers: HashMap::new(),
            default_capture_enabled,
            capture_dir,
            entities: HashMap::new(),
        }
    }

    /// Prunes closed stream writers, resets the chip's fast-path packet-forwarding
    /// flag when no writers remain, and flushes all active writers.
    pub(super) async fn flush_writers(&mut self) {
        let entities = &self.entities;
        self.writers.retain(|chip_id, chip_writers| {
            chip_writers.retain(|w| !w.is_closed());
            let has_active_writers = !chip_writers.is_empty();
            if !has_active_writers && let Some(entity) = entities.get(chip_id) {
                // `enabled_flag` is the fast-path atomic gate checked by radio streams
                // (`true` whenever file capture or any stream subscriber is active),
                // whereas `entity.info.enabled` is the persistent file capture state.
                entity.enabled_flag.store(entity.info.enabled, Ordering::SeqCst);
            }
            has_active_writers
        });
        for (chip_id, chip_writers) in &mut self.writers {
            for writer in chip_writers {
                if let Err(err) = writer.flush().await {
                    warn!("Failed to flush writer for chip {chip_id}: {err}");
                }
            }
        }
    }
}
