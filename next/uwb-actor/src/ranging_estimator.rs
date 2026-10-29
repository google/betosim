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

//! Implementation of [RangingEstimator] for use with [pica].

use std::{collections::HashMap, sync::Arc};

use parking_lot::RwLock;
use pica::{Handle, RangingEstimator, RangingMeasurement};
use tracing::warn;

use crate::{radio_model::RangingDataSet, ranging::Pose, uwb_actor::UwbChipState};

/// UWB Ranging Estimator that uses shared chip state to calculate ranging
/// measurements.
pub(crate) struct UwbRangingEstimator {
    shared_chips: Arc<RwLock<HashMap<Handle, UwbChipState>>>,
    data_set: RangingDataSet,
}

impl UwbRangingEstimator {
    pub(crate) fn new(shared_chips: Arc<RwLock<HashMap<Handle, UwbChipState>>>) -> Self {
        Self { shared_chips, data_set: RangingDataSet::default() }
    }
}

impl RangingEstimator for UwbRangingEstimator {
    fn estimate(&self, a_handle: &Handle, b_handle: &Handle) -> Option<RangingMeasurement> {
        let chips = self.shared_chips.read();
        let a_state = chips.get(a_handle)?;
        let b_state = chips.get(b_handle)?;

        if !a_state.chip.is_uwb_enabled() || !b_state.chip.is_uwb_enabled() {
            return None;
        }

        let a_pose = Pose::from(&a_state.chip.pose);
        let b_pose = Pose::from(&b_state.chip.pose);

        match a_pose.compute_ranging(&b_pose) {
            Ok((range, azimuth, elevation)) => {
                let mut rng = rand::rng();
                let (sampled_range, sampled_az, sampled_el) =
                    self.data_set.sample_ranging(&mut rng, range, azimuth, elevation);
                Some(RangingMeasurement {
                    range: sampled_range,
                    azimuth: sampled_az,
                    elevation: sampled_el,
                })
            }
            Err(error) => {
                warn!(%error, "Failed to estimate ranging measurement");
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::HashMap,
        sync::{Arc, atomic::AtomicU64},
    };

    use netsim_model::{Chip, ChipVariant, Pose as ModelPose, Position, Uwb, UwbMode};
    use parking_lot::RwLock;
    use pica::{Handle, MacAddress, RangingEstimator};

    use super::*;

    fn create_chip_state(x: f32, y: f32, z: f32, mode: UwbMode, mac: [u8; 2]) -> UwbChipState {
        let chip = Chip {
            variant: Some(ChipVariant::Uwb(Uwb { mode, ..Default::default() })),
            pose: ModelPose { position: Position { x, y, z }, ..Default::default() },
            ..Default::default()
        };
        UwbChipState {
            chip,
            tx_count: Arc::new(AtomicU64::new(0)),
            rx_count: Arc::new(AtomicU64::new(0)),
            p2p_tx_count: Arc::new(AtomicU64::new(0)),
            p2p_rx_count: Arc::new(AtomicU64::new(0)),
            mac_address: MacAddress::Short(mac),
        }
    }

    #[test]
    fn test_estimate_missing_handles() {
        let shared_chips = Arc::new(RwLock::new(HashMap::new()));
        let estimator = UwbRangingEstimator::new(shared_chips.clone());

        let handle_a: Handle = 1;
        let handle_b: Handle = 2;

        // Neither exists
        assert!(estimator.estimate(&handle_a, &handle_b).is_none());

        // Only handle_a exists
        shared_chips
            .write()
            .insert(handle_a, create_chip_state(0.0, 0.0, 0.0, UwbMode::Device, [0, 1]));
        assert!(estimator.estimate(&handle_a, &handle_b).is_none());

        // Only handle_b exists
        shared_chips.write().clear();
        shared_chips
            .write()
            .insert(handle_b, create_chip_state(10.0, 0.0, 0.0, UwbMode::Device, [0, 2]));
        assert!(estimator.estimate(&handle_a, &handle_b).is_none());
    }

    #[test]
    fn test_estimate_uwb_disabled() {
        let shared_chips = Arc::new(RwLock::new(HashMap::new()));
        let estimator = UwbRangingEstimator::new(shared_chips.clone());

        let handle_a: Handle = 1;
        let handle_b: Handle = 2;

        // Chip A disabled (empty variant)
        let mut disabled_chip_a = create_chip_state(0.0, 0.0, 0.0, UwbMode::Device, [0, 1]);
        disabled_chip_a.chip.variant = None;

        let active_chip_b = create_chip_state(10.0, 0.0, 0.0, UwbMode::Device, [0, 2]);

        shared_chips.write().insert(handle_a, disabled_chip_a);
        shared_chips.write().insert(handle_b, active_chip_b);
        assert!(estimator.estimate(&handle_a, &handle_b).is_none());

        // Chip B disabled (radio state disabled)
        let active_chip_a = create_chip_state(0.0, 0.0, 0.0, UwbMode::Device, [0, 1]);
        let mut disabled_chip_b = create_chip_state(10.0, 0.0, 0.0, UwbMode::Device, [0, 2]);
        if let Some(ChipVariant::Uwb(ref mut uwb)) = disabled_chip_b.chip.variant {
            uwb.radio.state = Some(false);
        }

        shared_chips.write().insert(handle_a, active_chip_a);
        shared_chips.write().insert(handle_b, disabled_chip_b);
        assert!(estimator.estimate(&handle_a, &handle_b).is_none());
    }

    #[test]
    fn test_estimate_successful_ranging_measurement() {
        let shared_chips = Arc::new(RwLock::new(HashMap::new()));
        let estimator = UwbRangingEstimator::new(shared_chips.clone());

        let handle_a: Handle = 1;
        let handle_b: Handle = 2;

        // Chip A at origin, Chip B at (10m, 0m, 0m) -> 1000 cm range
        let chip_a = create_chip_state(0.0, 0.0, 0.0, UwbMode::Device, [0, 1]);
        let chip_b = create_chip_state(10.0, 0.0, 0.0, UwbMode::Device, [0, 2]);

        shared_chips.write().insert(handle_a, chip_a);
        shared_chips.write().insert(handle_b, chip_b);

        let measurement = estimator.estimate(&handle_a, &handle_b).expect("ranging succeeds");
        assert!((980..=1020).contains(&measurement.range));
        assert!((87..=93).contains(&measurement.azimuth));
        assert!((-3..=3).contains(&measurement.elevation));
    }

    #[test]
    fn test_estimate_anchor_to_device_ranging() {
        let shared_chips = Arc::new(RwLock::new(HashMap::new()));
        let estimator = UwbRangingEstimator::new(shared_chips.clone());

        let handle_anchor: Handle = 10;
        let handle_device: Handle = 20;

        // Anchor at (0m, 5m, 0m), Device at (0m, 0m, 0m) -> 500 cm range
        let chip_anchor = create_chip_state(0.0, 5.0, 0.0, UwbMode::Anchor, [0, 3]);
        let chip_device = create_chip_state(0.0, 0.0, 0.0, UwbMode::Device, [0, 4]);

        shared_chips.write().insert(handle_anchor, chip_anchor);
        shared_chips.write().insert(handle_device, chip_device);

        let measurement =
            estimator.estimate(&handle_anchor, &handle_device).expect("anchor ranging succeeds");
        assert!((480..=520).contains(&measurement.range));
        assert!((-3..=3).contains(&measurement.azimuth));
        assert!((-90..=-87).contains(&measurement.elevation));
    }

    #[test]
    fn test_estimate_symmetric_distance_different_angles() {
        let shared_chips = Arc::new(RwLock::new(HashMap::new()));
        let estimator = UwbRangingEstimator::new(shared_chips.clone());

        let handle_a: Handle = 1;
        let handle_b: Handle = 2;

        let chip_a = create_chip_state(0.0, 0.0, 0.0, UwbMode::Device, [0, 1]);
        let chip_b = create_chip_state(0.0, 0.0, 10.0, UwbMode::Device, [0, 2]);

        shared_chips.write().insert(handle_a, chip_a);
        shared_chips.write().insert(handle_b, chip_b);

        let m_a_to_b = estimator.estimate(&handle_a, &handle_b).expect("a to b succeeds");
        let m_b_to_a = estimator.estimate(&handle_b, &handle_a).expect("b to a succeeds");

        // Distance in cm is symmetric (10m = 1000cm)
        assert!((980..=1020).contains(&m_a_to_b.range));
        assert!((980..=1020).contains(&m_b_to_a.range));

        // Direction vectors are opposite
        assert!((-3..=3).contains(&m_a_to_b.azimuth));
        assert!((-3..=3).contains(&m_a_to_b.elevation));
        assert!(m_b_to_a.azimuth >= 177 || m_b_to_a.azimuth <= -177);
        assert!((-3..=3).contains(&m_b_to_a.elevation));
    }
}
