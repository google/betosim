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
        atomic::{AtomicBool, AtomicU8, AtomicU64},
    },
};

use device_actor::DeviceClient;
use netsim_model::ChipId;
use parking_lot::Mutex;
use tokio::io::{DuplexStream, WriteHalf};
use tracing::info;

pub const NFC_MODE_IDLE: u8 = 0;
pub const NFC_MODE_POLL: u8 = 1;
pub const NFC_MODE_LISTEN: u8 = 2;

pub struct ChipState {
    pub id: ChipId,
    pub device_id: device_api::DeviceId,
    pub enabled: Arc<AtomicBool>,
    pub casimir_device_id: u16,
    pub nfc_writer: WriteHalf<DuplexStream>,
    pub tx_count: Arc<AtomicU64>,
    pub rx_count: Arc<AtomicU64>,
    pub mode: Arc<AtomicU8>,
}

use crate::stats::NfcStats;

pub struct NfcActor {
    pub device_client: DeviceClient,
    pub active_chips: HashMap<ChipId, ChipState>,
    pub casimir_to_device: Arc<Mutex<HashMap<u16, device_api::DeviceId>>>,
    pub scene_client: Option<crate::scene::SceneClient>,
    pub scene_task: Option<tokio::task::JoinHandle<()>>,
    pub nfc_stats: Arc<NfcStats>,
}

impl NfcActor {
    pub fn new(device_client: DeviceClient, nfc_stats: Arc<NfcStats>) -> Self {
        Self {
            device_client,
            active_chips: HashMap::new(),
            casimir_to_device: Arc::new(Mutex::new(HashMap::new())),
            scene_client: None,
            scene_task: None,
            nfc_stats,
        }
    }

    pub fn start_casimir(&mut self) {
        if self.scene_task.is_some() {
            return;
        }
        let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel();
        let scene = crate::scene::NetsimScene::new(cmd_rx);
        let scene_client = crate::scene::SceneClient::new(cmd_tx);

        let scene_task = tokio::spawn(async move {
            scene.await;
        });

        self.scene_task = Some(scene_task);
        self.scene_client = Some(scene_client);
        info!("In-process Casimir scene started");
    }
}

#[derive(Debug)]
pub enum NfcAction {
    Generic(netsim_model::ChipRequest),
    GetStatistics,
    CreateControlChannel {
        respond_to: tokio::sync::oneshot::Sender<
            Result<(tokio::io::DuplexStream, u16), crate::error::NfcError>,
        >,
    },
}

#[derive(Debug)]
pub enum NfcActionResult {
    Ok,
    Statistics(Box<[netsim_model::NetsimRadioStats]>),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_start_casimir() {
        let (_runner, device_client) = device_actor::new();
        let mut actor = NfcActor::new(device_client, Arc::new(crate::stats::NfcStats::new()));
        actor.start_casimir();
        assert!(actor.scene_task.is_some());
        assert!(actor.scene_client.is_some());
    }

    #[tokio::test]
    async fn test_add_device_to_scene() {
        let (_runner, device_client) = device_actor::new();
        let mut actor = NfcActor::new(device_client, Arc::new(crate::stats::NfcStats::new()));
        actor.start_casimir();

        let scene_client = actor.scene_client.as_ref().unwrap().clone();

        // Create a mock duplex stream for the device
        let (_nfc_io, casimir_io) = tokio::io::duplex(1024);

        // Add device to scene
        let add_result = scene_client
            .add_device(move |id, rf_tx| {
                let (rx, tx) = tokio::io::split(casimir_io);
                casimir::Device::nci(id, rx, tx, rf_tx)
            })
            .await;

        assert!(add_result.is_ok());
        let device_id = add_result.unwrap();
        assert_eq!(device_id, 0); // First device should be ID 0 (slot index)
    }
}
