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

use netsim_model::{CellNetworkConfig, ModemAction, Quirks};

// ...
use crate::modem_network_simulator::NetworkEvent;
pub use crate::types::ModemError;
use crate::types::{ModemId, ModemInfo, ModemSink};

pub trait ModemNetworkInterface: Send + Sync {
    fn add_modem(
        &mut self,
        chip_id: ModemId,
        sink: ModemSink,
        sim_type: Option<i32>,
        sim_profile: Option<String>,
        quirks: Quirks,
        network_configs: Vec<CellNetworkConfig>,
    ) -> Result<(), ModemError>;
    fn update_network_configs(
        &mut self,
        chip_id: ModemId,
        network_configs: Vec<CellNetworkConfig>,
    ) -> Result<(), ModemError>;
    fn remove_modem(&mut self, chip_id: ModemId) -> Result<(), ModemError>;
    fn send_data(&mut self, chip_id: ModemId, data: &[u8]) -> Result<(), ModemError>;
    fn get_modem_info(&self, chip_id: ModemId) -> Result<ModemInfo, ModemError>;
    fn on_timer(&mut self, chip_id: ModemId) -> Result<(), ModemError>;
    fn perform_action(&mut self, action: ModemAction) -> Result<Vec<NetworkEvent>, ModemError>;
}

use crate::modem_network_simulator::ModemNetworkSimulator;

impl ModemNetworkInterface for ModemNetworkSimulator {
    fn add_modem(
        &mut self,
        chip_id: ModemId,
        sink: ModemSink,
        sim_type: Option<i32>,
        sim_profile: Option<String>,
        quirks: Quirks,
        network_configs: Vec<CellNetworkConfig>,
    ) -> Result<(), ModemError> {
        self.new_modem(chip_id, sink, sim_type, sim_profile, quirks, network_configs)
    }

    fn update_network_configs(
        &mut self,
        chip_id: ModemId,
        network_configs: Vec<CellNetworkConfig>,
    ) -> Result<(), ModemError> {
        self.update_network_configs(chip_id, network_configs)
    }

    fn remove_modem(&mut self, chip_id: ModemId) -> Result<(), ModemError> {
        self.remove_modem(chip_id);
        Ok(())
    }

    fn send_data(&mut self, chip_id: ModemId, data: &[u8]) -> Result<(), ModemError> {
        self.send_at_command(chip_id, data);
        Ok(())
    }

    fn get_modem_info(&self, chip_id: ModemId) -> Result<ModemInfo, ModemError> {
        if let Some(modem) = self.get_modem(chip_id) {
            let signal = modem.signal_quality();
            let calls = modem
                .calls()
                .iter()
                .map(|c| netsim_model::Call {
                    number: c.number.as_ref().map(|n| n.as_str().to_string()).unwrap_or_default(),
                    state: match c.state {
                        crate::call_service::CallState::Active => netsim_model::CallState::Active,
                        crate::call_service::CallState::Held => netsim_model::CallState::Holding,
                        crate::call_service::CallState::Dialing => netsim_model::CallState::Dialing,
                        crate::call_service::CallState::Alerting => {
                            netsim_model::CallState::Alerting
                        }
                        crate::call_service::CallState::Incoming => {
                            netsim_model::CallState::Incoming
                        }
                        crate::call_service::CallState::Waiting => netsim_model::CallState::Waiting,
                    },
                    direction: match c.direction {
                        crate::call_service::CallDirection::Outgoing => {
                            netsim_model::CallDirection::MobileOriginated
                        }
                        crate::call_service::CallDirection::Incoming => {
                            netsim_model::CallDirection::MobileTerminated
                        }
                    },
                })
                .collect();
            Ok(ModemInfo {
                id: chip_id,
                calls,
                ringing: modem.is_ringing(),
                sms_count: modem.get_sms_count(),
                quirks: modem.quirks,
                rssi: signal.rssi as u32,
                ber: signal.ber as u32,
                voice_registration: modem.voice_registration(),
                data_registration: modem.data_registration(),
                network_configs: modem.network_configs().to_vec(),
            })
        } else {
            Err(ModemError::NotFound)
        }
    }

    fn on_timer(&mut self, chip_id: ModemId) -> Result<(), ModemError> {
        self.on_timer(chip_id);
        Ok(())
    }

    fn perform_action(&mut self, action: ModemAction) -> Result<Vec<NetworkEvent>, ModemError> {
        Ok(self.dispatch(action))
    }
}
