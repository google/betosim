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

use std::collections::HashMap;

use device_actor::DeviceClient;
use modem_rs::{ModemNetworkSimulator, modem_network::ModemNetworkInterface};
use netsim_model::ChipId;
use slirp_actor::SlirpClient;
use tokio::sync::mpsc::UnboundedReceiver;

pub struct ChipState {
    pub device_id: device_api::DeviceId,
    pub enabled: bool,
}

pub struct CellActor {
    pub device_client: DeviceClient,
    pub slirp_client: SlirpClient,
    pub controller: Box<dyn ModemNetworkInterface>,
    pub active_chips: HashMap<ChipId, ChipState>,
    pub event_receiver: Option<UnboundedReceiver<modem_rs::HostEvent>>,
}

impl CellActor {
    pub fn new(device_client: DeviceClient, slirp_client: SlirpClient) -> Self {
        // Create channel for internal events loopback
        let (event_tx, event_rx) = tokio::sync::mpsc::unbounded_channel::<modem_rs::HostEvent>();
        let controller = Box::new(ModemNetworkSimulator::new(event_tx));

        Self {
            device_client,
            slirp_client,
            controller,
            active_chips: HashMap::new(),
            event_receiver: Some(event_rx),
        }
    }
}
