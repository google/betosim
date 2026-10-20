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

/// Mode of the NFC controller.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum NfcMode {
    /// Controller is idle.
    #[default]
    Idle = 0,
    /// Controller is polling for remote tags.
    Poll = 1,
    /// Controller is in listen mode (card emulation).
    Listen = 2,
}

impl NfcMode {
    /// Returns the raw u8 representation of the mode.
    pub const fn as_u8(self) -> u8 {
        self as u8
    }

    /// Returns true if the controller is in listen mode.
    pub const fn is_listen(self) -> bool {
        matches!(self, Self::Listen)
    }

    /// Returns true if the controller is in poll mode.
    pub const fn is_poll(self) -> bool {
        matches!(self, Self::Poll)
    }
}

impl From<NfcMode> for u8 {
    fn from(mode: NfcMode) -> Self {
        mode.as_u8()
    }
}

impl TryFrom<u8> for NfcMode {
    type Error = u8;

    fn try_from(val: u8) -> Result<Self, Self::Error> {
        match val {
            0 => Ok(Self::Idle),
            1 => Ok(Self::Poll),
            2 => Ok(Self::Listen),
            other => Err(other),
        }
    }
}

/// Backwards compatibility constants for `NFC_MODE_*`.
pub const NFC_MODE_IDLE: u8 = NfcMode::Idle.as_u8();
pub const NFC_MODE_POLL: u8 = NfcMode::Poll.as_u8();
pub const NFC_MODE_LISTEN: u8 = NfcMode::Listen.as_u8();

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

impl ChipState {
    /// Loads the current NFC operating mode.
    pub fn mode(&self) -> NfcMode {
        NfcMode::try_from(self.mode.load(std::sync::atomic::Ordering::Relaxed))
            .unwrap_or(NfcMode::Idle)
    }

    /// Stores a new NFC operating mode.
    pub fn set_mode(&self, mode: NfcMode) {
        self.mode.store(mode.as_u8(), std::sync::atomic::Ordering::Relaxed);
    }

    /// Returns whether the controller is currently in listen mode.
    pub fn is_listen(&self) -> bool {
        self.mode() == NfcMode::Listen
    }
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

    /// Returns the number of currently active chips.
    pub fn chip_count(&self) -> usize {
        self.active_chips.len()
    }

    /// Returns true if a chip with the given ID is active.
    pub(crate) fn is_chip_active(&self, id: &ChipId) -> bool {
        self.active_chips.contains_key(id)
    }

    /// Returns a reference to the active [`ChipState`] for the given chip ID.
    pub(crate) fn get_chip(&self, id: &ChipId) -> Option<&ChipState> {
        self.active_chips.get(id)
    }

    /// Returns a mutable reference to the active [`ChipState`] for the given chip ID.
    pub fn get_chip_mut(&mut self, id: &ChipId) -> Option<&mut ChipState> {
        self.active_chips.get_mut(id)
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

    #[test]
    fn test_nfc_mode_repr_and_conversions() {
        assert_eq!(NfcMode::Idle.as_u8(), 0);
        assert_eq!(NfcMode::Poll.as_u8(), 1);
        assert_eq!(NfcMode::Listen.as_u8(), 2);

        assert_eq!(u8::from(NfcMode::Idle), 0);
        assert_eq!(u8::from(NfcMode::Poll), 1);
        assert_eq!(u8::from(NfcMode::Listen), 2);

        assert_eq!(NfcMode::try_from(0), Ok(NfcMode::Idle));
        assert_eq!(NfcMode::try_from(1), Ok(NfcMode::Poll));
        assert_eq!(NfcMode::try_from(2), Ok(NfcMode::Listen));
        assert_eq!(NfcMode::try_from(3), Err(3));

        assert!(NfcMode::Poll.is_poll());
        assert!(!NfcMode::Poll.is_listen());
        assert!(NfcMode::Listen.is_listen());
        assert!(!NfcMode::Listen.is_poll());

        assert_eq!(NfcMode::default(), NfcMode::Idle);
    }

    #[test]
    fn test_chip_state_mode_helpers() {
        let (nfc_io, _peer) = tokio::io::duplex(64);
        let (_rx, nfc_writer) = tokio::io::split(nfc_io);
        let state = ChipState {
            id: ChipId(42),
            device_id: device_api::DeviceId(1),
            enabled: Arc::new(AtomicBool::new(true)),
            casimir_device_id: 1,
            nfc_writer,
            tx_count: Arc::new(AtomicU64::new(0)),
            rx_count: Arc::new(AtomicU64::new(0)),
            mode: Arc::new(AtomicU8::new(NfcMode::Idle.as_u8())),
        };

        assert_eq!(state.mode(), NfcMode::Idle);
        assert!(!state.is_listen());

        state.set_mode(NfcMode::Poll);
        assert_eq!(state.mode(), NfcMode::Poll);
        assert!(!state.is_listen());

        state.set_mode(NfcMode::Listen);
        assert_eq!(state.mode(), NfcMode::Listen);
        assert!(state.is_listen());
    }

    #[tokio::test]
    async fn test_nfc_actor_domain_helpers() {
        let (_runner, device_client) = device_actor::new();
        let mut actor = NfcActor::new(device_client, Arc::new(crate::stats::NfcStats::new()));
        assert_eq!(actor.chip_count(), 0);
        assert!(!actor.is_chip_active(&ChipId(10)));
        assert!(actor.get_chip(&ChipId(10)).is_none());
        assert!(actor.get_chip_mut(&ChipId(10)).is_none());
    }
}
