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
    sync::{Arc, atomic::AtomicU32},
};

use actor_framework::TimerKey;
use capture_api::CaptureSender;
use netsim_model::{ChipClient, ChipKind};

use crate::DeviceClient;

pub struct DeviceActor {
    pub chip_clients: HashMap<ChipKind, Box<dyn ChipClient>>,
    pub next_chip_id: Arc<AtomicU32>,
    pub capture_client: Option<Arc<dyn CaptureSender>>,
    pub(crate) devices: HashMap<device_api::DeviceId, crate::service::InternalDevice>,
    pub next_device_id: u32,
    pub link_client: Box<dyn link_api::LinkClient>,
    pub startup_timeout: Option<std::time::Duration>,
    pub idle_timeout: Option<std::time::Duration>,

    pub startup_timer: Option<TimerKey>,
    pub(crate) idle_timer: Option<TimerKey>,
    pub has_seen_device: bool,
    pub guid_to_id: HashMap<String, device_api::DeviceId>,
    pub stats_write_task: Option<tokio::task::JoinHandle<()>>,
    pub(crate) stats: crate::stats::Stats,
    pub stats_interval: std::time::Duration,
    // Client to send messages to self (e.g. for periodic stats)
    pub(crate) self_client: Option<DeviceClient>,
}

impl DeviceActor {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        chip_clients: HashMap<ChipKind, Box<dyn ChipClient>>,
        next_chip_id: Arc<AtomicU32>,
        capture_client: Option<Arc<dyn CaptureSender>>,
        link_client: Box<dyn link_api::LinkClient>,
        startup_timeout: Option<std::time::Duration>,
        idle_timeout: Option<std::time::Duration>,
        version: String,
        stats_path: Option<std::path::PathBuf>,
        stats_interval: Option<std::time::Duration>,
        frontend_stats: std::sync::Arc<netsim_model::FrontendStats>,
    ) -> Self {
        Self {
            chip_clients,
            next_chip_id,
            capture_client,
            devices: HashMap::new(),
            next_device_id: 1,
            link_client,
            startup_timeout,
            idle_timeout,

            startup_timer: None,
            idle_timer: None,
            has_seen_device: false,
            guid_to_id: HashMap::new(),
            stats_write_task: None,
            stats: crate::stats::Stats::new(version.clone(), stats_path, frontend_stats),
            stats_interval: stats_interval.unwrap_or(std::time::Duration::from_secs(10)),
            self_client: None,
        }
    }

    pub fn set_self_client(&mut self, client: DeviceClient) {
        self.self_client = Some(client);
    }
}

impl std::fmt::Debug for DeviceActor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeviceActor")
            .field("next_chip_id", &self.next_chip_id)
            .field("devices", &self.devices)
            .field("startup_timeout", &self.startup_timeout)
            .field("idle_timeout", &self.idle_timeout)
            .field("startup_timer", &self.startup_timer)
            .field("idle_timer", &self.idle_timer)
            .field("has_seen_device", &self.has_seen_device)
            .field("stats_interval", &self.stats_interval)
            .finish_non_exhaustive()
    }
}
