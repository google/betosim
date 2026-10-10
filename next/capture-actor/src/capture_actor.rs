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

use std::{collections::HashMap, path::PathBuf};

use netsim_model::ChipId;
use tracing::warn;

use crate::writer::CaptureWriter;

/// The context for the Capture actor.
///
/// This struct holds the shared state required by the actor, including the
/// `CaptureWriter` instance for writing packet captures.
/// Since multiple components need access to the writers and flags,
/// they are stored in the context and protected by mutexes.
pub struct CaptureActor {
    /// Map of ChipId to CaptureWriter.
    /// Writers are created when capture is enabled for a chip.
    pub(crate) writers: HashMap<ChipId, Box<dyn CaptureWriter>>,
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

    /// Calls [CaptureWriter::flush] on all active writers.
    pub(super) async fn flush_writers(&mut self) {
        for (id, writer) in &mut self.writers {
            if let Err(err) = writer.flush().await {
                warn!("Failed to flush writer for chip {id}: {err}");
            }
        }
    }
}
