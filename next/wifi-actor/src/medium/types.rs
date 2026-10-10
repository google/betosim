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

use netsim_packets::MacAddress;

use crate::error::WifiError;

pub(crate) type WifiResult<T> = Result<T, WifiError>;

#[derive(Clone, Debug)]
pub struct Station {
    pub client_id: u32,
    pub addr: MacAddress,
    pub hwsim_addr: MacAddress,
    pub freq: u32,
}

impl Station {
    pub fn new(client_id: u32, addr: MacAddress, hwsim_addr: MacAddress) -> Self {
        Self { client_id, addr, hwsim_addr, freq: 0 }
    }

    pub fn update_freq(&mut self, freq: u32) {
        self.freq = freq;
    }
}

#[derive(Clone, Debug)]
pub struct Client {
    pub enabled: bool,
    pub tx_count: u32,
    pub rx_count: u32,
    pub p2p_tx_count: u64,
    pub p2p_rx_count: u64,
}

impl Default for Client {
    fn default() -> Self {
        Self::new()
    }
}

impl Client {
    pub fn new() -> Self {
        Self { enabled: true, tx_count: 0, rx_count: 0, p2p_tx_count: 0, p2p_rx_count: 0 }
    }
}
