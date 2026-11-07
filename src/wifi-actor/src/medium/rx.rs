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
        let has_p2p_ie = frame.ieee80211.is_mgmt()
            && frame.ieee80211.stype() == netsim_packets::management_subtype::PROBE_REQUEST
            && frame.ieee80211.get_destination().is_multicast()
            && frame.attrs.freq == Some(2462)
            && self.has_p2p_ie(&frame.ieee80211);
        if let Some(client) = self.clients.get_mut(&client_id) {
            if let Some(freq) = frame.attrs.freq {
                client.radio_freq = freq;
            }
            let is_mgmt = frame.ieee80211.is_mgmt();
            let stype = frame.ieee80211.stype();
            let is_multicast = frame.ieee80211.get_destination().is_multicast();
            client.in_p2p_listen = has_p2p_ie;
            client.in_p2p_action = is_mgmt
                && ((stype == netsim_packets::management_subtype::ACTION
                    && frame.ieee80211.get_payload().first()
                        == Some(&netsim_packets::category::PUBLIC))
                    || ((stype == netsim_packets::management_subtype::PROBE_REQUEST
                        || stype == netsim_packets::management_subtype::PROBE_RESPONSE)
                        && !is_multicast));
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

        let routing_frame = tx_state.get_ieee80211();
        let is_hosted_to_ds_nodata = routing_frame.is_data()
            && routing_frame.is_to_ap()
            && routing_frame.stype() == u8::from(DataSubType::Nodata)
            && !self.debug.debug_no_network
            && self.enabled(client_id).unwrap_or(false)
            && bssid.is_some_and(|b| self.key_store.has_bssid(&b));
        // Remember which AP this station is sending infrastructure frames to
        #[allow(clippy::collapsible_if)]
        if tx_state.infra_target != InfraTarget::None || is_hosted_to_ds_nodata {
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
            if !dest_addr.is_multicast()
                && dest_addr.bytes != [0; 6]
                && !self.debug.debug_no_wmedium
            {
                stations = true;
            }
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

        if infra_target == InfraTarget::None
            && !dest_addr.is_multicast()
            && dest_addr.bytes != [0; 6]
            && !self.debug.debug_no_wmedium
            && !ieee80211.get_bssid().is_some_and(|b| self.key_store.has_bssid(&b))
        {
            stations = true;
        }

        (infra_target, stations)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use ap_actor::SharedKeyStore;
    use bytes::Bytes;
    use netsim_packets::{DataSubType, FrameType, Ieee80211, Ieee80211ToAp, MacAddress};

    use crate::{
        DebugArgs,
        medium::{core::Medium, utils::create_hwsim_msg_from_frame},
        stats::{SystemClock, WifiStats},
    };

    const FREQ_2412: u32 = 2412;

    fn test_medium_with_debug(debug: DebugArgs) -> (Medium, Arc<SharedKeyStore>) {
        let key_store = Arc::new(SharedKeyStore::new());
        let wifi_stats = WifiStats::new(Arc::new(SystemClock));
        let medium = Medium::new(key_store.clone(), wifi_stats, Arc::new(debug));
        (medium, key_store)
    }

    fn build_nodata_bytes(
        sta_mac: &MacAddress,
        target_bssid: &MacAddress,
        hwsim_addr: &MacAddress,
    ) -> Bytes {
        let null_frame = Ieee80211ToAp {
            version: 0,
            ftype: FrameType::Data,
            stype: u8::from(DataSubType::Nodata),
            destination: *target_bssid,
            source: *sta_mac,
            bssid: *target_bssid,
            duration_id: 0,
            seq_ctrl: 0,
            qos_ctrl: None,
            protected: 0,
            order: 0,
            more_frags: 0,
            retry: 0,
            pm: 0,
            more_data: 0,
            payload: vec![],
        };
        let ieee: Ieee80211 = null_frame.try_into().unwrap();
        let msg =
            create_hwsim_msg_from_frame(&ieee, target_bssid, FREQ_2412, Some(hwsim_addr)).unwrap();
        Bytes::from(msg.encode_to_vec().unwrap())
    }

    #[test]
    fn test_resolve_tx_packet_hosted_to_ds_nodata_registers_bssid() {
        let hosted_bssid = MacAddress::new([0x00, 0x13, 0x10, 0x85, 0xfe, 0x01]);
        let sta_mac = MacAddress::new([0x02, 0x15, 0xb2, 0x00, 0x01, 0x00]);
        let hwsim_addr = MacAddress::new([0x02, 0x00, 0x00, 0x00, 0x00, 0x00]);

        let (mut medium, key_store) = test_medium_with_debug(DebugArgs::default());
        key_store.add_bssid(hosted_bssid);
        medium.add(1);

        let bytes = build_nodata_bytes(&sta_mac, &hosted_bssid, &hwsim_addr);
        medium.resolve_tx_packet(1, &bytes).unwrap();

        assert_eq!(key_store.get_station_bssid(&sta_mac), Some(hosted_bssid));
        assert_eq!(key_store.get_station_bssid(&hwsim_addr), Some(hosted_bssid));
    }

    #[test]
    fn test_resolve_tx_packet_unhosted_to_ds_nodata_ignores_bssid() {
        let hosted_bssid = MacAddress::new([0x00, 0x13, 0x10, 0x85, 0xfe, 0x01]);
        let unhosted_bssid = MacAddress::new([0x00, 0x13, 0x10, 0x85, 0xfe, 0x02]);
        let sta_mac = MacAddress::new([0x02, 0x15, 0xb2, 0x00, 0x01, 0x00]);
        let hwsim_addr = MacAddress::new([0x02, 0x00, 0x00, 0x00, 0x00, 0x00]);

        let (mut medium, key_store) = test_medium_with_debug(DebugArgs::default());
        key_store.add_bssid(hosted_bssid);
        medium.add(1);

        let bytes = build_nodata_bytes(&sta_mac, &unhosted_bssid, &hwsim_addr);
        medium.resolve_tx_packet(1, &bytes).unwrap();

        assert_eq!(key_store.get_station_bssid(&sta_mac), None);
        assert_eq!(key_store.get_station_bssid(&hwsim_addr), None);
    }

    #[test]
    fn test_resolve_tx_packet_disabled_client_nodata_ignores_bssid() {
        let hosted_bssid = MacAddress::new([0x00, 0x13, 0x10, 0x85, 0xfe, 0x01]);
        let sta_mac = MacAddress::new([0x02, 0x15, 0xb2, 0x00, 0x01, 0x00]);
        let hwsim_addr = MacAddress::new([0x02, 0x00, 0x00, 0x00, 0x00, 0x00]);

        let (mut medium, key_store) = test_medium_with_debug(DebugArgs::default());
        key_store.add_bssid(hosted_bssid);
        medium.add(1);
        medium.set_enabled(1, false);

        let bytes = build_nodata_bytes(&sta_mac, &hosted_bssid, &hwsim_addr);
        medium.resolve_tx_packet(1, &bytes).unwrap();

        assert_eq!(key_store.get_station_bssid(&sta_mac), None);
        assert_eq!(key_store.get_station_bssid(&hwsim_addr), None);
    }

    #[test]
    fn test_resolve_tx_packet_no_network_nodata_ignores_bssid() {
        let hosted_bssid = MacAddress::new([0x00, 0x13, 0x10, 0x85, 0xfe, 0x01]);
        let sta_mac = MacAddress::new([0x02, 0x15, 0xb2, 0x00, 0x01, 0x00]);
        let hwsim_addr = MacAddress::new([0x02, 0x00, 0x00, 0x00, 0x00, 0x00]);

        let (mut medium, key_store) =
            test_medium_with_debug(DebugArgs { debug_no_network: true, ..Default::default() });
        key_store.add_bssid(hosted_bssid);
        medium.add(1);

        let bytes = build_nodata_bytes(&sta_mac, &hosted_bssid, &hwsim_addr);
        medium.resolve_tx_packet(1, &bytes).unwrap();

        assert_eq!(key_store.get_station_bssid(&sta_mac), None);
        assert_eq!(key_store.get_station_bssid(&hwsim_addr), None);
    }

    #[test]
    fn test_resolve_tx_packet_dropped_non_nodata_to_ds_ignores_bssid() {
        let hosted_bssid = MacAddress::new([0x00, 0x13, 0x10, 0x85, 0xfe, 0x01]);
        let sta_mac = MacAddress::new([0x02, 0x15, 0xb2, 0x00, 0x01, 0x00]);
        let hwsim_addr = MacAddress::new([0x02, 0x00, 0x00, 0x00, 0x00, 0x00]);
        let mdns_mac = MacAddress::new([0x01, 0x00, 0x5e, 0x00, 0x00, 0xfb]);

        let (mut medium, key_store) = test_medium_with_debug(DebugArgs {
            debug_no_guest_to_host_mdns: true,
            ..Default::default()
        });
        key_store.add_bssid(hosted_bssid);
        medium.add(1);

        let to_ap = Ieee80211ToAp {
            version: 0,
            ftype: FrameType::Data,
            stype: u8::from(DataSubType::Data),
            destination: mdns_mac,
            source: sta_mac,
            bssid: hosted_bssid,
            duration_id: 0,
            seq_ctrl: 0,
            qos_ctrl: None,
            protected: 0,
            order: 0,
            more_frags: 0,
            retry: 0,
            pm: 0,
            more_data: 0,
            payload: vec![0x12, 0x34],
        };
        let ieee: Ieee80211 = to_ap.try_into().unwrap();
        let msg = create_hwsim_msg_from_frame(&ieee, &hosted_bssid, FREQ_2412, Some(&hwsim_addr))
            .unwrap();
        let bytes = Bytes::from(msg.encode_to_vec().unwrap());
        medium.resolve_tx_packet(1, &bytes).unwrap();

        assert_eq!(key_store.get_station_bssid(&sta_mac), None);
        assert_eq!(key_store.get_station_bssid(&hwsim_addr), None);
    }
}
