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

use std::{
    collections::{HashMap, VecDeque},
    sync::Arc,
};

use device_actor::DeviceClient;
use netsim_model::{Chip, ChipId};
use parking_lot::{Mutex, RwLock};
use rootcanal::{Callbacks as RootcanalCallbacks, Phy, Rootcanal};
use tokio::sync::mpsc;
use tracing::warn;

use crate::{BluetoothEvent, ranging};

const CENTIMETERS_PER_METER: f32 = 100.0;
const DEFAULT_DISTANCE_CM: u32 = 100;

/// A thread-safe map of chip states.
type ChipMap = Arc<RwLock<HashMap<ChipId, Chip>>>;

/// Implementation of `RootcanalCallbacks` for the Bluetooth Actor.
/// We need to wrap this in a Mutex even though Rootcanal runs on
/// the same thread because RootcanalCallbacks is not sync.
struct RootcanalCallbacksImpl {
    chips: ChipMap,
}

impl RootcanalCallbacks for RootcanalCallbacksImpl {
    fn on_send_ll(
        &self,
        source_id: u32,
        destination_id: u32,
        _packet: &[u8],
        phy: Phy,
        tx_power: i32,
    ) -> Option<i32> {
        let src_id = source_id.into();
        let dst_id = destination_id.into();
        let chips = self.chips.read();
        let src_chip = chips.get(&src_id);
        let dst_chip = chips.get(&dst_id);

        if let (Some(src), Some(dst)) = (src_chip, dst_chip) {
            let is_enabled = |chip: &Chip| match phy {
                Phy::LowEnergy => chip.is_le_enabled(),
                _ => chip.is_classic_enabled(),
            };

            if !is_enabled(src) || !is_enabled(dst) {
                return None;
            }

            let dist = src.pose.position.distance(&dst.pose.position);

            // Check for link override
            let rssi = src
                .links
                .iter()
                .find_map(|(id, rssi)| (*id == dst_id).then_some(*rssi as i32))
                .unwrap_or_else(|| ranging::distance_to_rssi(tx_power as i8, dist) as i32);
            Some(rssi)
        } else {
            // If one of the chips is missing, default to tx_power.
            // This can happen during startup/shutdown or if a chip is not yet fully
            // registered.
            if src_chip.is_none() {
                warn!("on_send_ll: Missing src chip {src_id}");
            } else {
                warn!("on_send_ll: Missing dst chip {dst_id}");
            }
            Some(tx_power)
        }
    }

    fn estimate_distance(&self, source_id: u32, destination_id: u32) -> u32 {
        let src_id = source_id.into();
        let dst_id = destination_id.into();
        let chips = self.chips.read();
        let src_chip = chips.get(&src_id);
        let dst_chip = chips.get(&dst_id);
        if let (Some(src), Some(dst)) = (src_chip, dst_chip) {
            let dist = src.pose.position.distance(&dst.pose.position);
            (dist * CENTIMETERS_PER_METER).round() as u32
        } else {
            DEFAULT_DISTANCE_CM
        }
    }
}

/// The context for the Bluetooth actor.
///
/// This struct holds the shared state required by the actor, including the
/// `Rootcanal` instance for simulation, the map of active chips, and the
/// `DeviceClient` for interacting with other devices.
pub struct BluetoothActor {
    /// The Rootcanal simulation instance.
    pub(crate) rootcanal: Arc<Rootcanal>,
    /// A map of active Bluetooth chips, protected by a mutex.
    pub(crate) chips: ChipMap,
    /// A map of initial chip states for reset purposes.
    pub(crate) initial_chips: HashMap<ChipId, Chip>,
    /// Active timeout task abort handles for BLE beacons.
    pub(crate) beacon_timeouts: HashMap<ChipId, tokio::task::AbortHandle>,
    /// The client for interacting with the device actor.
    pub(crate) device_client: DeviceClient,
    /// Unbounded receiver for internal events, registered in on_start.
    pub(crate) event_rx: Mutex<Option<mpsc::UnboundedReceiver<BluetoothEvent>>>,
    /// Shared with HciCallbacks so synchronous Rootcanal callbacks can record invalid packets for GetStatistics.
    pub(crate) invalid_packets: HashMap<ChipId, Arc<Mutex<VecDeque<netsim_model::InvalidPacket>>>>,
}

impl BluetoothActor {
    /// Creates a new BluetoothActor context.
    pub fn new(device_client: DeviceClient, disable_address_reuse: bool) -> Self {
        let chips = Arc::new(RwLock::new(HashMap::new()));
        let initial_chips = HashMap::new();
        let beacon_timeouts = HashMap::new();
        let (event_tx, event_rx) = mpsc::unbounded_channel();
        let rootcanal = Rootcanal::new(
            Box::new(RootcanalCallbacksImpl { chips: chips.clone() }),
            disable_address_reuse,
            Box::new(move |receiver_id, packet, phy, rssi| {
                let _ = event_tx.send(BluetoothEvent::DeliverPacket {
                    receiver_id: ChipId(receiver_id),
                    packet,
                    phy,
                    rssi,
                });
            }),
        );
        Self {
            rootcanal,
            chips,
            initial_chips,
            beacon_timeouts,
            device_client,
            event_rx: Mutex::new(Some(event_rx)),
            invalid_packets: HashMap::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use netsim_model::{Chip, ChipId, ChipVariant};
    use rootcanal::Phy;

    use super::*;

    #[test]
    fn test_on_send_ll_link_override() {
        let chips = Arc::new(RwLock::new(HashMap::new()));
        let callbacks = RootcanalCallbacksImpl { chips: chips.clone() };

        let chip1_id = ChipId(1);
        let chip2_id = ChipId(2);

        let mut chip1 = Chip::default();
        chip1.id = 1;
        // Position at (0,0,0)

        let mut chip2 = Chip::default();
        chip2.id = 2;
        // Position at (0,0,0) - distance 0

        chips.write().insert(chip1_id, chip1.clone());
        chips.write().insert(chip2_id, chip2.clone());

        // Test without link (should use distance-based RSSI).
        // Distance 0 should result in a valid RSSI value.
        let rssi_default = callbacks.on_send_ll(1, 2, &[], Phy::LowEnergy, 0);
        assert!(rssi_default.is_some());

        // Add link override
        chip1.links.push((chip2_id, -50));
        chips.write().insert(chip1_id, chip1);

        // Test with link
        let rssi_override = callbacks.on_send_ll(1, 2, &[], Phy::LowEnergy, 0);
        assert_eq!(rssi_override, Some(-50));
    }

    // Scenario: Block traffic when destination radio is disabled
    //   Given a source chip with enabled radio
    //   And a destination chip with disabled LE radio
    //   When the source sends a packet
    //   Then the packet is blocked (returns None)
    #[test]
    fn test_on_send_ll_disabled_destination() {
        // Given a source chip with enabled radio
        let chips = Arc::new(RwLock::new(HashMap::new()));
        let callbacks = RootcanalCallbacksImpl { chips: chips.clone() };

        let chip1_id = ChipId(1);
        let chip2_id = ChipId(2);

        let mut chip1 = Chip::default();
        chip1.id = 1;

        // And a destination chip with disabled LE radio
        let mut chip2 = Chip::default();
        chip2.id = 2;
        chip2.variant = Some(ChipVariant::Bluetooth(Box::new(netsim_model::Bluetooth {
            low_energy: netsim_model::Radio { state: Some(false), ..Default::default() },
            classic: Default::default(),
            ..Default::default()
        })));

        chips.write().insert(chip1_id, chip1.clone());
        chips.write().insert(chip2_id, chip2.clone());

        // When the source sends an LE packet
        let rssi = callbacks.on_send_ll(1, 2, &[], Phy::LowEnergy, 0);

        // Then the packet is blocked (returns None)
        assert!(rssi.is_none());
    }
}
