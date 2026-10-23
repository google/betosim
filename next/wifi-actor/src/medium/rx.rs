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

use bytes::Bytes;
use netsim_packets::{DataSubType, Ieee80211};
use tracing::debug;

use crate::{
    error::WifiError,
    medium::{
        WifiResult,
        core::Medium,
        tx_packet_state::{InfraTarget, TxPacketState},
        utils,
    },
};

// Packets flowing from Guest (Source) to Medium
impl Medium {
    /// Main entry point for processing RX packets from a guest (radio).
    ///
    /// # Flow
    /// 1. **Parse & Stats**: Validates `HwsimFrame` and updates RX statistics.
    /// 2. **Station Tracking**: Updates the known state (frequency, etc.) of
    ///    the source station.
    /// 3. **Decryption**: Attempts to decrypt the frame if a key session
    ///    exists.
    /// 4. **Routing**: Determines where the packet should go (`ap`, `slirp`,
    ///    `stations`).
    pub fn resolve_tx_packet(
        &mut self,
        client_id: u32,
        packet: &Bytes,
    ) -> WifiResult<TxPacketState> {
        // 1. Parse & Stats
        let frame = utils::parse_hwsim_frame(packet, client_id).map_err(|e| {
            WifiError::Client(Box::from(format!("error validate for client {client_id}: {e}")))
        })?;

        if self.debug.debug_no_traffic {
            return Ok(TxPacketState {
                infra_target: InfraTarget::None,
                stations: false,
                frame,
                plaintext_ieee80211: None,
                plaintext_bytes: None,
            });
        }

        self.wifi_stats.incr_hwsim_frames_rx();

        // 2. Station Tracking
        self.upsert_station(client_id, &frame).map_err(|e| {
            WifiError::Client(Box::from(format!(
                "error upsert station for client {client_id}: {e}"
            )))
        })?;

        if let Some(freq) = frame.attrs.freq {
            self.get_station_mut(&frame.ieee80211.get_source())
                .map(|sta| sta.update_freq(freq))
                .map_err(|e| self.wifi_stats.log_and_incr_err_count(&e))
                .ok();
        }

        // 3. Decryption
        let (plaintext_bytes, plaintext_ieee80211) = self.try_decrypt_frame(&frame.ieee80211);

        // 4. Routing
        let mut tx_state = TxPacketState {
            infra_target: InfraTarget::None,
            stations: false,
            frame,
            plaintext_ieee80211,
            plaintext_bytes,
        };

        let (infra_target, stations, bssid, src_mac) = {
            // If we have plaintext, use it for routing decisions. Otherwise use the
            // original frame (likely Mgmt/Open).
            let routing_frame = tx_state.get_ieee80211();

            if routing_frame.is_mgmt() {
                self.wifi_stats.incr_mgmt_frames_rx();
            }

            let (infra, stas) = self.determine_routes(client_id, routing_frame);
            (infra, stas, routing_frame.get_bssid(), routing_frame.get_source())
        };

        tx_state.infra_target = infra_target;
        tx_state.stations = stations;

        // Remember which AP this station is sending infrastructure frames to
        #[allow(clippy::collapsible_if)]
        if tx_state.infra_target != InfraTarget::None {
            if let Some(b) = bssid {
                if !b.is_multicast() {
                    self.key_store.set_station_bssid(src_mac, b);
                    // Also map hardware MAC (hwsim_addr) to ensure Slirp responses can be routed
                    if let Some(hwsim_addr) = tx_state.frame.transmitter {
                        self.key_store.set_station_bssid(hwsim_addr, b);
                    }
                }
            }
        }

        Ok(tx_state)
    }

    /// Attempts to decrypt the 802.11 frame using the shared key store.
    fn try_decrypt_frame(&self, ieee80211: &Ieee80211) -> (Option<Bytes>, Option<Ieee80211>) {
        if let Some(bytes) = self.key_store.try_decrypt(ieee80211) {
            match Ieee80211::decode(&bytes) {
                Ok(parsed) => (Some(Bytes::from(bytes)), Some(parsed)),
                Err(_) => (None, None),
            }
        } else {
            (None, None)
        }
    }

    /// Routes the packet to the appropriate destinations (AP, Slirp, Stations).
    ///
    /// Returns `(InfraTarget, stations)`.
    fn determine_routes(&mut self, client_id: u32, ieee80211: &Ieee80211) -> (InfraTarget, bool) {
        let dest_addr = ieee80211.get_destination();
        let mut infra_target = InfraTarget::None;
        let mut stations = false;

        // --- Stations (Loopback / Peer-to-Peer) ---
        if self.contains_station(&dest_addr) {
            if !self.debug.debug_no_wmedium {
                stations = true;
            }
            return (infra_target, stations);
        }

        if dest_addr.is_multicast()
            && !(self.debug.debug_no_wmedium
                || (self.debug.debug_no_mdns_wmedium && dest_addr.is_mdns()))
        {
            stations = true;
        }

        // --- AP & Slirp ---

        // Check BSSID match for Infrastructure frames against ALL active APs
        if let Some(bssid) = ieee80211.get_bssid()
            && !bssid.is_multicast()
            && !self.key_store.has_bssid(&bssid)
        {
            return (infra_target, stations);
        }

        if ieee80211.is_data() {
            // Data Frames
            let is_eapol = ieee80211.is_eapol().unwrap_or_else(|e| {
                debug!("Failed to get ether type for is_eapol(): {e}");
                false
            });

            if is_eapol {
                infra_target = InfraTarget::Ap;
            } else if ieee80211.is_to_ap() {
                // ToDS frames (Station -> AP)
                if ieee80211.stype() != u8::from(DataSubType::Nodata) {
                    // Check if should go to Slirp
                    if self.debug.debug_no_network
                        || (self.debug.debug_no_guest_to_host_mdns && dest_addr.is_mdns())
                    {
                        infra_target = InfraTarget::None;
                    } else if self.enabled(client_id).unwrap_or(false) {
                        infra_target = InfraTarget::Slirp;
                    } else {
                        // If client disabled, only multicast or own-BSSID traffic?
                        if dest_addr.is_multicast() || self.key_store.has_bssid(&dest_addr) {
                            infra_target = InfraTarget::Slirp;
                        }
                    };
                }
            }
        } else {
            // Management or Control Frames
            let addr1 = ieee80211.get_addr1(); // Destination/RA
            if addr1.is_multicast() || addr1.is_broadcast() || self.key_store.has_bssid(&addr1) {
                infra_target = InfraTarget::Ap;
            }
        }

        (infra_target, stations)
    }
}
