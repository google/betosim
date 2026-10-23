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
    collections::{HashMap, HashSet},
    sync::Arc,
};

use ap_actor::SharedKeyStore;
use netsim_packets::{HwsimFrame, MacAddress};
use tracing::{info, warn};

use crate::{
    DebugArgs,
    error::WifiError,
    medium::types::{Client, Station, WifiResult},
    stats::WifiStats,
};

pub struct Medium {
    pub stations: HashMap<MacAddress, Station>,
    pub clients: HashMap<u32, Client>,
    /// If true, the Medium reflects `ToDS` frames as `FromDS` frames when
    /// deliverying to other stations. This allows peer-to-peer
    /// communication via the AP without passing through a full AP stack.
    pub(crate) simulate_ap_reflection: bool,
    pub(crate) key_store: Arc<SharedKeyStore>,
    pub wifi_stats: WifiStats,
    pub(crate) debug: Arc<DebugArgs>,
    pub(crate) seq: std::sync::atomic::AtomicU16,
    pub(crate) active_p2p_groups: HashSet<MacAddress>,
}

impl std::fmt::Debug for Medium {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Medium")
            .field("stations", &self.stations)
            .field("clients", &self.clients)
            .field("simulate_ap_reflection", &self.simulate_ap_reflection)
            .field("key_store", &self.key_store)
            .field("wifi_stats", &self.wifi_stats)
            .field("debug", &self.debug)
            .finish()
    }
}

impl Medium {
    // Data Flow Documentation:
    // ...
    pub fn new(
        key_store: Arc<SharedKeyStore>,
        wifi_stats: WifiStats,
        debug: Arc<DebugArgs>,
    ) -> Medium {
        Self {
            stations: HashMap::new(),
            clients: HashMap::new(),
            simulate_ap_reflection: true,
            key_store,
            wifi_stats,
            debug,
            seq: std::sync::atomic::AtomicU16::new(100),
            active_p2p_groups: HashSet::new(),
        }
    }

    pub fn add(&mut self, client_id: u32) {
        let _ = self.clients.entry(client_id).or_insert_with(|| {
            info!("Insert client {client_id}");
            Client::new()
        });
    }

    pub fn remove(&mut self, client_id: u32) {
        self.stations.retain(|_, s| s.client_id != client_id);
        self.clients.remove(&client_id);
    }

    pub fn reset(&mut self, client_id: u32) {
        self.stations.retain(|_, s| s.client_id != client_id);
        if let Some(client) = self.clients.get_mut(&client_id) {
            *client = Client::new();
        }
    }

    pub fn contains_client(&self, client_id: u32) -> bool {
        self.clients.contains_key(&client_id)
    }

    pub fn contains_station(&self, addr: &MacAddress) -> bool {
        self.stations.contains_key(addr)
    }

    pub fn get_station_chip_id(&self, addr: &MacAddress) -> Option<u32> {
        self.stations.get(addr).map(|s| s.client_id)
    }

    pub(crate) fn get_station(&self, addr: &MacAddress) -> WifiResult<&Station> {
        self.stations.get(addr).ok_or_else(|| {
            WifiError::Internal(Box::from(format!("Station not found for address: {addr}")))
        })
    }

    pub(crate) fn get_station_mut(&mut self, addr: &MacAddress) -> WifiResult<&mut Station> {
        self.stations.get_mut(addr).ok_or_else(|| {
            WifiError::Internal(Box::from(format!("Station not found for address: {addr}")))
        })
    }

    pub(crate) fn upsert_station(&mut self, client_id: u32, frame: &HwsimFrame) -> WifiResult<()> {
        let src_addr = frame.ieee80211.get_source();
        let hwsim_addr = frame.transmitter.ok_or(WifiError::Internal(Box::from(format!(
            "Missing transmitter attribute in frame for client: {client_id}"
        ))))?;
        self.stations.entry(src_addr).or_insert_with(|| {
            info!(
                "Insert station with client id {client_id}, hwsimaddr: {hwsim_addr}, \
                Ieee80211 addr: {src_addr}"
            );
            Station::new(client_id, src_addr, hwsim_addr)
        });
        if !self.contains_client(client_id) {
            warn!("Client {client_id} is missing");
            self.add(client_id);
        }
        Ok(())
    }

    pub fn set_enabled(&mut self, client_id: u32, enabled: bool) {
        if let Some(client) = self.clients.get_mut(&client_id) {
            client.enabled = enabled;
        }
    }

    pub(crate) fn enabled(&self, client_id: u32) -> WifiResult<bool> {
        self.clients
            .get(&client_id)
            .map(|c| c.enabled)
            .ok_or_else(|| WifiError::Internal(Box::from(format!("client {client_id} is missing"))))
    }

    pub(crate) fn incr_tx(&mut self, client_id: u32) -> WifiResult<()> {
        self.clients.get_mut(&client_id).map_or(
            Err(WifiError::Internal(Box::from(format!(
                "client {client_id} is missing for incr_tx"
            )))),
            |c| {
                c.tx_count += 1;
                Ok(())
            },
        )
    }

    pub(crate) fn incr_rx(&mut self, client_id: u32) -> WifiResult<()> {
        self.clients.get_mut(&client_id).map_or(
            Err(WifiError::Internal(Box::from(format!(
                "client {client_id} is missing for incr_rx"
            )))),
            |c| {
                c.rx_count += 1;
                Ok(())
            },
        )
    }

    pub fn get_tx_count(&self, client_id: u32) -> u32 {
        self.clients.get(&client_id).map(|c| c.tx_count).unwrap_or(0)
    }

    pub fn get_rx_count(&self, client_id: u32) -> u32 {
        self.clients.get(&client_id).map(|c| c.rx_count).unwrap_or(0)
    }

    pub(crate) fn incr_p2p_tx(&mut self, client_id: u32) {
        if let Some(c) = self.clients.get_mut(&client_id) {
            c.p2p_tx_count += 1;
        } else {
            warn!("client {client_id} is missing for incr_p2p_tx");
        }
    }

    pub(crate) fn incr_p2p_rx(&mut self, client_id: u32) {
        if let Some(c) = self.clients.get_mut(&client_id) {
            c.p2p_rx_count += 1;
        } else {
            warn!("client {client_id} is missing for incr_p2p_rx");
        }
    }

    pub fn get_p2p_tx_count(&self, client_id: u32) -> u64 {
        self.clients.get(&client_id).map(|c| c.p2p_tx_count).unwrap_or(0)
    }

    pub fn get_p2p_rx_count(&self, client_id: u32) -> u64 {
        self.clients.get(&client_id).map(|c| c.p2p_rx_count).unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_p2p_telemetry_counters() {
        use std::sync::Arc;

        use ap_actor::SharedKeyStore;

        use crate::{
            DebugArgs,
            stats::{SystemClock, WifiStats},
        };

        let key_store = Arc::new(SharedKeyStore::new());
        let clock = Arc::new(SystemClock);
        let wifi_stats = WifiStats::new(clock);
        let debug = Arc::new(DebugArgs::default());

        let mut medium = Medium::new(key_store, wifi_stats, debug);
        let client_id = 1;
        medium.add(client_id);

        assert_eq!(medium.get_p2p_tx_count(client_id), 0);
        assert_eq!(medium.get_p2p_rx_count(client_id), 0);

        medium.incr_p2p_tx(client_id);
        assert_eq!(medium.get_p2p_tx_count(client_id), 1);
        assert_eq!(medium.get_p2p_rx_count(client_id), 0);

        medium.incr_p2p_rx(client_id);
        assert_eq!(medium.get_p2p_tx_count(client_id), 1);
        assert_eq!(medium.get_p2p_rx_count(client_id), 1);

        medium.reset(client_id);
        assert_eq!(medium.get_p2p_tx_count(client_id), 0);
        assert_eq!(medium.get_p2p_rx_count(client_id), 0);
    }

    #[test]
    fn test_parse_action_frame() {
        use std::sync::Arc;

        use netsim_packets::{
            FrameControl, Ieee80211, MacAddr, MacHeader3Addr, SequenceControl, category,
            public_action,
        };
        use zerocopy::IntoBytes;

        use crate::{
            DebugArgs,
            stats::{SystemClock, WifiStats},
        };

        let key_store = Arc::new(ap_actor::SharedKeyStore::new());
        let clock = Arc::new(SystemClock);
        let wifi_stats = WifiStats::new(clock);
        let debug = Arc::new(DebugArgs::default());

        let mut medium = Medium::new(key_store, wifi_stats, debug);

        // Construct FTM Request Frame (Public Action 32)
        let mut req_frame = Vec::new();

        // Header
        let header = MacHeader3Addr {
            frame_control: FrameControl::new(0x00D0), // Action
            duration_id: zerocopy::U16::new(0),
            addr1: MacAddr::new([0x02, 0x00, 0x00, 0x00, 0x00, 0x01]),
            addr2: MacAddr::new([0x02, 0x00, 0x00, 0x00, 0x00, 0x02]),
            addr3: MacAddr::new([0x02, 0x00, 0x00, 0x00, 0x00, 0x01]),
            sequence_control: SequenceControl::new(0),
        };
        req_frame.extend_from_slice(header.as_bytes());

        // Body
        req_frame.push(category::PUBLIC);
        req_frame.push(public_action::FTM_REQUEST);
        req_frame.push(1); // Trigger = 1

        let ieee80211 = Ieee80211::decode(&req_frame).unwrap();

        medium.parse_action_frame(&ieee80211);

        let proto_stats = medium.wifi_stats.to_proto();
        let wifi_api_stats = proto_stats.wifi_api_stats.unwrap();
        let rtt_stats = wifi_api_stats.wifi_rtt_manager.unwrap();
        assert_eq!(rtt_stats.start_ranging, Some(1));
    }
}
