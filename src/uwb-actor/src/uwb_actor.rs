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
    sync::{Arc, atomic::AtomicU64},
};

use actor_framework::TimerKey;
use bytes::Bytes;
use device_actor::DeviceClient;
use netsim_model::{Chip, ChipId, ChipVariant, Uwb, UwbMode};
use parking_lot::RwLock;
use pica::{Handle, Pica};
use tokio::sync::mpsc;

/// State associated with a single UWB chip.
pub(crate) struct UwbChipState {
    /// The chip model.
    pub(crate) chip: Chip,
    pub(crate) tx_count: AtomicU64,
    pub(crate) rx_count: AtomicU64,
    pub(crate) p2p_tx_count: AtomicU64,
    pub(crate) p2p_rx_count: AtomicU64,
    pub(crate) packet_tx: Option<mpsc::UnboundedSender<Bytes>>,
}

impl UwbChipState {
    pub(crate) fn is_anchor(&self) -> bool {
        matches!(self.chip.variant, Some(ChipVariant::Uwb(Uwb { mode: UwbMode::Anchor, .. })))
    }
}

/// The UWB Actor responsible for managing UWB chips and their state.
pub struct UwbActor {
    /// Map of active chips shared with
    /// [UwbRangingEstimator](super::ranging_estimator::UwbRangingEstimator).
    pub(super) chip_states: Arc<RwLock<HashMap<Handle, UwbChipState>>>,
    /// Map from chip ID to Pica handle, used for actor lookups.
    pub(super) chip_to_handle: HashMap<ChipId, Handle>,
    /// Map from chip ID to initial chip state for resets.
    pub(super) initial_chips: HashMap<ChipId, Chip>,
    /// Client for interacting with the device actor.
    pub(super) device_client: DeviceClient,
    /// Synchronous UWB state machine managing virtual devices, anchors, and UCI sessions.
    pub(super) pica: Pica,
    /// Active actor timer scheduled for the next Pica ranging tick.
    pub(super) ranging_timer: Option<TimerKey>,
    pub(crate) uwb_stats: crate::stats::UwbStats,
    pub(super) next_anchor_mac: u16,
}

pub(super) const MAX_UNICAST_SHORT_MAC: u16 = 0xFFFE;

impl UwbActor {
    pub fn new(device_client: DeviceClient) -> Self {
        let chip_states = Arc::new(RwLock::new(HashMap::new()));
        let pica = Pica::new(Box::new(crate::ranging_estimator::UwbRangingEstimator::new(
            chip_states.clone(),
        )));

        UwbActor {
            chip_states,
            chip_to_handle: HashMap::new(),
            initial_chips: HashMap::new(),
            device_client,
            pica,
            ranging_timer: None,
            uwb_stats: crate::stats::UwbStats::new(),
            next_anchor_mac: MAX_UNICAST_SHORT_MAC,
        }
    }
}
