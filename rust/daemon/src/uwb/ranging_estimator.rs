// Copyright 2024 The Android Open Source Project
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

#![allow(mismatched_lifetime_syntaxes)]

use log::{debug, error};
use netsim_proto::model::Device as ProtoDevice;
use pica::{Handle, RangingEstimator, RangingMeasurement};

use crate::devices::{chip::ChipIdentifier, devices_handler::get_device};
use crate::ranging::{Pose, compute_range_azimuth_elevation};
use crate::uwb::ranging_data::RangingDataSet;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};

// Initially a single HashMap might grow into a Struct to allow
// for more fields.
type State = HashMap<Handle, ChipIdentifier>;

#[derive(Clone)]
pub struct SharedState(Arc<Mutex<State>>);

impl SharedState {
    pub fn new() -> Self {
        SharedState(Arc::new(Mutex::new(State::new())))
    }

    fn lock(&self) -> MutexGuard<State> {
        self.0.lock().expect("Poisoned SharedState lock")
    }

    pub fn get_chip_id(&self, pica_id: &Handle) -> anyhow::Result<ChipIdentifier> {
        self.lock().get(pica_id).ok_or(anyhow::anyhow!("pica_id: {pica_id} not in State")).cloned()
    }

    pub fn insert(&self, pica_id: Handle, chip_id: ChipIdentifier) {
        self.lock().insert(pica_id, chip_id);
    }

    pub fn remove(&self, pica_id: &Handle) {
        self.lock().remove(pica_id);
    }
}

// Netsim's UwbRangingEstimator
pub struct UwbRangingEstimator {
    shared_state: SharedState,
    data_set: RangingDataSet,
}

impl UwbRangingEstimator {
    pub fn new(shared_state: SharedState) -> Self {
        UwbRangingEstimator { shared_state, data_set: RangingDataSet::new(None) }
    }

    // Utility to convert the UWB Chip handle into the device and chip_id.
    fn handle_to_netsim_model(&self, handle: &Handle) -> Option<(ChipIdentifier, ProtoDevice)> {
        let chip_id = self.shared_state.get_chip_id(handle).map_err(|e| debug!("{e:?}")).ok()?;
        let device = get_device(&chip_id).map_err(|e| debug!("{e:?}")).ok()?;
        Some((chip_id, device))
    }
}

impl RangingEstimator for UwbRangingEstimator {
    fn estimate(&self, a: &Handle, b: &Handle) -> Option<RangingMeasurement> {
        // Use the Handle to obtain the positions and orientation information in netsim
        // and perform compute_range_azimuth_elevation
        let (a_chip_id, a_device) = self.handle_to_netsim_model(a)?;
        let (b_chip_id, b_device) = self.handle_to_netsim_model(b)?;
        // Chips are invisible at the PHY layer when uwb is disabled so test here.
        // Note, chips must always process Host-Controller messages even when
        // state-off to avoid protocol stack timeouts and other glitches.
        if !is_uwb_state_on(&a_device, &a_chip_id) || !is_uwb_state_on(&b_device, &b_chip_id) {
            return None;
        }
        let (a_p, a_o) = (a_device.position, a_device.orientation);
        let (b_p, b_o) = (b_device.position, b_device.orientation);
        let a_pose = Pose::new(a_p.x, a_p.y, a_p.z, a_o.yaw, a_o.pitch, a_o.roll);
        let b_pose = Pose::new(b_p.x, b_p.y, b_p.z, b_o.yaw, b_o.pitch, b_o.roll);
        compute_range_azimuth_elevation(&a_pose, &b_pose)
            .map(|(range, azimuth, elevation)| RangingMeasurement {
                range: self.data_set.sample(range, None).round() as u16,
                azimuth,
                elevation,
            })
            .map_err(|e| error!("{e:?}"))
            .ok()
    }
}

/// With given ProtoDevice and ChipIdentifier, return true if state of a UWB chip
/// with ChipIdentifier inside ProtoDevice is ON. Otherwise, return false.
fn is_uwb_state_on(device: &ProtoDevice, chip_id: &ChipIdentifier) -> bool {
    for chip in &device.chips {
        if chip.has_uwb() && chip.id == chip_id.0 {
            return chip.uwb().state.unwrap_or(false);
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    use netsim_proto::model::Chip as ProtoChip;
    use netsim_proto::model::chip::Radio as ProtoRadio;

    fn create_proto_device_with_uwb_chip(uwb_state: bool) -> (ProtoDevice, ChipIdentifier) {
        // Setting Radio State to true
        let mut proto_radio = ProtoRadio::new();
        proto_radio.state = Some(uwb_state);

        // Setting UWB ProtoChip
        let mut proto_chip = ProtoChip::new();
        proto_chip.set_uwb(proto_radio);
        let chip_id = proto_chip.id;

        // Adding chip to proto_device
        let mut proto_device = ProtoDevice::new();
        proto_device.chips.push(proto_chip);

        // Return proto_device
        (proto_device, ChipIdentifier(chip_id))
    }

    #[test]
    fn test_is_uwb_state_on() {
        // False when no uwb chip is present in ProtoDevice
        assert!(!is_uwb_state_on(&ProtoDevice::new(), &ChipIdentifier(0)));

        // Check if UWB State is on
        let (proto_device, chip_id) = create_proto_device_with_uwb_chip(true);
        assert!(is_uwb_state_on(&proto_device, &chip_id));

        // Check if UWB State is off
        let (proto_device, chip_id) = create_proto_device_with_uwb_chip(false);
        assert!(!is_uwb_state_on(&proto_device, &chip_id));
    }
}
