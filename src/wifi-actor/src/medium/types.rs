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
    pub(crate) radio_freq: u32,
    pub(crate) listen_freq: Option<u32>,
    pub(crate) in_p2p_listen: bool,
    pub(crate) in_p2p_action: bool,
}

impl Default for Client {
    fn default() -> Self {
        Self::new()
    }
}

impl Client {
    pub fn new() -> Self {
        Self {
            enabled: true,
            tx_count: 0,
            rx_count: 0,
            p2p_tx_count: 0,
            p2p_rx_count: 0,
            radio_freq: 0,
            listen_freq: None,
            in_p2p_listen: false,
            in_p2p_action: false,
        }
    }

    pub(crate) fn is_reachable_on_freq(&self, tx_freq: Option<u32>) -> bool {
        let Some(tx_freq) = tx_freq else {
            return true;
        };
        if self.in_p2p_action && self.listen_freq == Some(tx_freq) {
            return true;
        }
        let active_freq =
            self.listen_freq.filter(|_| self.in_p2p_listen).unwrap_or(self.radio_freq);
        active_freq == 0 || active_freq == tx_freq
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_is_reachable_on_freq() {
        let mut client = Client::new();
        assert!(client.is_reachable_on_freq(Some(2412)));
        assert!(client.is_reachable_on_freq(None));

        client.radio_freq = 2462;
        assert!(client.is_reachable_on_freq(Some(2462)));
        assert!(!client.is_reachable_on_freq(Some(2412)));

        client.listen_freq = Some(2437);
        client.in_p2p_listen = true;
        assert!(client.is_reachable_on_freq(Some(2437)));
        assert!(!client.is_reachable_on_freq(Some(2462)));

        client.in_p2p_listen = false;
        client.in_p2p_action = true;
        assert!(client.is_reachable_on_freq(Some(2462)));
        assert!(client.is_reachable_on_freq(Some(2437)));
        assert!(!client.is_reachable_on_freq(Some(2412)));
    }
}
