// Copyright 2025-2026 The Android Open Source Project
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
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use netsim_packets::{CcmpHeader, Ieee80211, MacAddress};
use parking_lot::RwLock;
use tracing::error;
use zerocopy::IntoBytes;

use crate::ffi::{AesCcmDecrypt, AesCcmEncrypt};

const CCMP_HDR_LEN: usize = 8;

#[derive(Debug)]
pub(crate) struct SessionKeys {
    tk: Vec<u8>,
    tx_pn: AtomicU64,
}

impl Default for SessionKeys {
    fn default() -> Self {
        Self { tk: Vec::new(), tx_pn: AtomicU64::new(1) }
    }
}

#[derive(Clone, Debug)]
pub struct SharedKeyStore {
    // Set of all active AP BSSIDs
    pub bssids: Arc<RwLock<std::collections::HashSet<MacAddress>>>,
    // Map of Station Address -> Connected BSSID
    pub(crate) station_bssids: Arc<RwLock<HashMap<MacAddress, MacAddress>>>,
    // Map of Station Address -> SessionKeys
    pub(crate) sessions: Arc<RwLock<HashMap<MacAddress, Arc<SessionKeys>>>>,
    pub(crate) gtks: Arc<RwLock<HashMap<MacAddress, [u8; 16]>>>,
    gtk_tx_pn: Arc<AtomicU64>,
}

impl Default for SharedKeyStore {
    fn default() -> Self {
        Self {
            bssids: Arc::new(RwLock::new(std::collections::HashSet::new())),
            station_bssids: Arc::new(RwLock::new(HashMap::new())),
            sessions: Arc::new(RwLock::new(HashMap::new())),
            gtks: Arc::new(RwLock::new(HashMap::new())),
            gtk_tx_pn: Arc::new(AtomicU64::new(1)),
        }
    }
}

impl SharedKeyStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_bssid(&self, bssid: MacAddress) {
        self.bssids.write().insert(bssid);
    }

    pub fn has_bssid(&self, bssid: &MacAddress) -> bool {
        self.bssids.read().contains(bssid)
    }

    pub fn set_station_bssid(&self, sta_addr: MacAddress, bssid: MacAddress) {
        self.station_bssids.write().insert(sta_addr, bssid);
    }

    pub fn get_station_bssid(&self, sta_addr: &MacAddress) -> Option<MacAddress> {
        self.station_bssids.read().get(sta_addr).copied()
    }

    pub fn add_session(&self, sta_addr: MacAddress, tk: Vec<u8>) {
        let mut sessions = self.sessions.write();
        sessions.insert(sta_addr, Arc::new(SessionKeys { tk, tx_pn: AtomicU64::new(1) }));
    }

    pub fn get_gtk(&self, bssid: &MacAddress) -> Option<[u8; 16]> {
        self.gtks.read().get(bssid).copied()
    }

    pub(crate) fn set_gtk(&self, bssid: MacAddress, gtk: [u8; 16]) {
        self.gtks.write().insert(bssid, gtk);
    }

    pub(crate) fn remove_session(&self, sta_addr: &MacAddress) {
        let mut sessions = self.sessions.write();
        sessions.remove(sta_addr);
    }

    pub(crate) fn remove_station_bssid(&self, sta_addr: &MacAddress) {
        self.station_bssids.write().remove(sta_addr);
    }

    pub(crate) fn reset(&self, default_bssid: MacAddress) {
        self.station_bssids.write().clear();
        self.sessions.write().clear();
        {
            let mut gtks = self.gtks.write();
            gtks.clear();
            self.gtk_tx_pn.store(1, Ordering::SeqCst);
        }
        let mut bssids = self.bssids.write();
        bssids.clear();
        bssids.insert(default_bssid);
    }

    pub fn try_encrypt(&self, ieee80211: &Ieee80211) -> Option<Vec<u8>> {
        if !ieee80211.is_payload_bearing_data() {
            return None;
        }
        let dest = ieee80211.get_destination();
        let is_group = dest.is_multicast() || dest.is_broadcast();

        let (tk, pn, key_id) = if is_group {
            let bssid = ieee80211.get_bssid()?;
            let gtks = self.gtks.read();
            let gtk = gtks.get(&bssid)?;
            let pn = self.gtk_tx_pn.fetch_add(1, Ordering::SeqCst);
            (gtk.to_vec(), pn, 1) // KeyID 1
        } else {
            let sessions = self.sessions.read();
            let session = sessions.get(&dest)?;
            let pn = session.tx_pn.fetch_add(1, Ordering::SeqCst);
            (session.tk.clone(), pn, 0) // KeyID 0
        };

        if tk.len() < 16 {
            return None;
        }

        // CCMP Nonce (13 bytes): Priority(1: 0) || A2(6: Src/BSSID) || PN(6: MSB-first)
        // Note: pn.to_le_bytes() yields LSB at pn0; Nonce maps PN in big-endian order
        // [pn5..pn0] matching IEEE 802.11 CCMP / hostapd logic.
        let [pn0, pn1, pn2, pn3, pn4, pn5, ..] = pn.to_le_bytes();
        let [a0, a1, a2, a3, a4, a5] = ieee80211.get_addr2().bytes; // A2 (Src/BSSID)
        let nonce = [0 /* Priority */, a0, a1, a2, a3, a4, a5, pn5, pn4, pn3, pn2, pn1, pn0];

        // The CCMP AAD explicitly requires the 'Protected' frame control bit to be
        // active. We must calculate the AAD against the final physical MAC byte
        // sequence!
        let mut final_fc_bytes = ieee80211.as_bytes().to_vec();
        let fc_byte = final_fc_bytes.get_mut(1)?;
        *fc_byte |= 0x40; // Force Protected Bit inside the temporary buffer
        let final_ieee = Ieee80211::decode(&final_fc_bytes).ok()?;
        let aad = final_ieee.get_aad();

        let payload = ieee80211.get_payload();

        let mut ciphertext = Vec::with_capacity(payload.len() + 8);
        let tk_slice = tk.get(..16)?;
        let success = AesCcmEncrypt(tk_slice, &nonce, &aad, &payload, &mut ciphertext, 8);
        if !success {
            return None;
        }

        // Reconstruct Frame
        let mut new_packet =
            Vec::with_capacity(ieee80211.hdr_length() + CCMP_HDR_LEN + ciphertext.len());
        new_packet.extend_from_slice(ieee80211.as_bytes().get(..ieee80211.hdr_length())?);

        // CCMP Header
        // PN0, PN1, Rsvd, KeyID_ExtIV, PN2, PN3, PN4, PN5
        let ccmp_key_id = 0x20 | (key_id << 6);
        let ccmp_header = CcmpHeader { pn0, pn1, rsvd: 0, key_id: ccmp_key_id, pn2, pn3, pn4, pn5 };
        new_packet.extend_from_slice(ccmp_header.as_bytes());

        new_packet.extend_from_slice(&ciphertext);

        // Set Protected Bit
        let prot_byte = new_packet.get_mut(1)?;
        *prot_byte |= 0x40;

        Some(new_packet)
    }

    pub fn try_decrypt(&self, ieee80211: &Ieee80211) -> Option<Vec<u8>> {
        // Decrypt if source is in sessions (Unicast from Station)
        let src = ieee80211.get_source();
        let sessions = self.sessions.read();
        let session = sessions.get(&src)?;
        if session.tk.len() < 16 {
            return None;
        }

        if ieee80211.as_bytes().len() < ieee80211.hdr_length() + CCMP_HDR_LEN {
            return None;
        }

        // Extract Nonce from Frame (CCMP Header: PN0, PN1, Rsvd, KeyID, PN2..PN5)
        // Frame: Header || CCMP Header || Data || MIC
        let hdr_len = ieee80211.hdr_length();
        let &[pn0, pn1, _, _, pn2, pn3, pn4, pn5, ..] = ieee80211.as_bytes().get(hdr_len..)? else {
            return None;
        };

        // CCMP Nonce: Priority(0) || A2(Src/STA) || PN(pn5..pn0)
        let [a0, a1, a2, a3, a4, a5] = ieee80211.get_addr2().bytes; // A2 (Src/STA)
        let nonce = [0 /* Priority */, a0, a1, a2, a3, a4, a5, pn5, pn4, pn3, pn2, pn1, pn0];

        // Payload
        let data = ieee80211.as_bytes().get(hdr_len + CCMP_HDR_LEN..)?;
        let aad = ieee80211.get_aad();

        if data.len() < 8 {
            return None;
        }
        let mut out_plain = Vec::with_capacity(data.len() - 8);
        let tk_slice = session.tk.get(..16)?;
        let success = AesCcmDecrypt(tk_slice, &nonce, &aad, data, &mut out_plain, 8);
        if !success {
            error!(
                "CCMP DECRYPT FAILED! hdr_len: {}, msg_len: {}, aad_len: {}",
                hdr_len,
                data.len(),
                aad.len()
            );
            error!("RAW FRAME TO DECRYPT (Hex): {:02X?}", ieee80211.as_bytes());
            return None;
        }
        let plaintext = out_plain;

        let mut new_packet = Vec::new();
        new_packet.extend_from_slice(ieee80211.as_bytes().get(..hdr_len)?);
        new_packet.extend_from_slice(&plaintext);

        let prot_byte = new_packet.get_mut(1)?;
        *prot_byte &= !0x40; // Clear protected bit
        Some(new_packet)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shared_key_store_reset() {
        let store = SharedKeyStore::new();
        let default_bssid = MacAddress::new([0x02, 0x00, 0x00, 0x00, 0x00, 0x01]);
        let other_bssid = MacAddress::new([0x02, 0x00, 0x00, 0x00, 0x00, 0x02]);
        let sta = MacAddress::new([0x02, 0x00, 0x00, 0x00, 0x00, 0x55]);

        store.add_bssid(default_bssid);
        store.add_bssid(other_bssid);
        store.set_station_bssid(sta, other_bssid);
        store.add_session(sta, vec![0x42; 16]);
        store.set_gtk(default_bssid, [0x11; 16]);
        store.set_gtk(other_bssid, [0x24; 16]);

        store.gtk_tx_pn.store(42, Ordering::SeqCst);
        store.reset(default_bssid);

        assert!(store.has_bssid(&default_bssid));
        assert!(!store.has_bssid(&other_bssid));
        assert!(store.get_station_bssid(&sta).is_none());
        assert!(store.get_gtk(&default_bssid).is_none());
        assert!(store.get_gtk(&other_bssid).is_none());
        assert!(store.sessions.read().is_empty());
        assert_eq!(store.gtk_tx_pn.load(Ordering::SeqCst), 1);
    }

    use netsim_packets::{
        BeaconFixedFields, DataSubType, FrameControl, FrameType, MacHeader3Addr, SequenceControl,
        management_subtype,
    };

    fn create_dummy_ethernet(src: &[u8; 6], dst: &[u8; 6], payload: &[u8]) -> Vec<u8> {
        let dst_mac = netsim_packets::MacAddr::new(*dst);
        let src_mac = netsim_packets::MacAddr::new(*src);
        let frame =
            netsim_packets::EthernetFrame::new(dst_mac, src_mac, netsim_packets::ether_type::IPV4);
        let mut bytes = frame.as_bytes().to_vec();
        bytes.extend_from_slice(payload);
        if bytes.len() < 60 {
            bytes.resize(60, 0);
        }
        bytes
    }

    #[test]
    fn test_try_encrypt_skips_management_frames() {
        let store = SharedKeyStore::new();
        let bssid = MacAddress::new([0x02, 0x00, 0x00, 0x00, 0x00, 0x01]);
        let sta_mac = MacAddress::new([0x02, 0x00, 0x00, 0x00, 0x00, 0x55]);

        store.add_bssid(bssid);
        store.set_gtk(bssid, [0x11; 16]);
        store.add_session(sta_mac, vec![0x42; 16]);

        let mut beacon_bytes = Vec::new();
        let beacon_hdr = MacHeader3Addr {
            frame_control: FrameControl::new(
                ((management_subtype::BEACON as u16) << 4) | ((FrameType::Management as u16) << 2),
            ),
            duration_id: zerocopy::U16::ZERO,
            addr1: MacAddress::new([0xFF; 6]),
            addr2: bssid,
            addr3: bssid,
            sequence_control: SequenceControl::new(0),
        };
        beacon_bytes.extend_from_slice(beacon_hdr.as_bytes());
        let fixed = BeaconFixedFields {
            timestamp: [0; 8],
            beacon_interval: zerocopy::U16::new(100),
            capabilities: zerocopy::U16::ZERO,
        };
        beacon_bytes.extend_from_slice(fixed.as_bytes());
        let beacon = Ieee80211::decode(&beacon_bytes).expect("decode beacon");

        let mut probe_resp_bytes = Vec::new();
        let probe_hdr = MacHeader3Addr {
            frame_control: FrameControl::new(
                ((management_subtype::PROBE_RESPONSE as u16) << 4)
                    | ((FrameType::Management as u16) << 2),
            ),
            duration_id: zerocopy::U16::ZERO,
            addr1: sta_mac,
            addr2: bssid,
            addr3: bssid,
            sequence_control: SequenceControl::new(0),
        };
        probe_resp_bytes.extend_from_slice(probe_hdr.as_bytes());
        probe_resp_bytes.extend_from_slice(fixed.as_bytes());
        let probe_resp = Ieee80211::decode(&probe_resp_bytes).expect("decode probe response");

        assert!(store.try_encrypt(&beacon).is_none());
        assert!(store.try_encrypt(&probe_resp).is_none());

        let eth_bcast = create_dummy_ethernet(&bssid.bytes, &[0xFF; 6], b"BroadcastPayload");
        let bcast_data = netsim_packets::Ieee80211::from_ieee8023(
            &bytes::Bytes::from(eth_bcast),
            bssid,
            netsim_packets::FrameDirection::FromAp,
            100,
        )
        .expect("create broadcast 802.11");
        let encrypted_bcast = store.try_encrypt(&bcast_data).expect("encrypt broadcast data");
        let decoded_bcast =
            Ieee80211::decode(&encrypted_bcast).expect("decode encrypted broadcast");
        assert_eq!(decoded_bcast.get_packet_number(), [0, 0, 0, 0, 0, 1]);

        let eth_unicast = create_dummy_ethernet(&bssid.bytes, &sta_mac.bytes, b"UnicastPayload");
        let unicast_data = netsim_packets::Ieee80211::from_ieee8023(
            &bytes::Bytes::from(eth_unicast),
            bssid,
            netsim_packets::FrameDirection::FromAp,
            101,
        )
        .expect("create unicast 802.11");
        let encrypted_unicast = store.try_encrypt(&unicast_data).expect("encrypt unicast data");
        let decoded_unicast =
            Ieee80211::decode(&encrypted_unicast).expect("decode encrypted unicast");
        assert_eq!(decoded_unicast.get_packet_number(), [0, 0, 0, 0, 0, 1]);
    }

    #[test]
    fn test_try_encrypt_skips_null_data() {
        let store = SharedKeyStore::new();
        let bssid = MacAddress::new([0x02, 0x00, 0x00, 0x00, 0x00, 0x01]);
        let sta_mac = MacAddress::new([0x02, 0x00, 0x00, 0x00, 0x00, 0x55]);

        store.add_bssid(bssid);
        store.add_session(sta_mac, vec![0x42; 16]);

        let null_hdr = MacHeader3Addr {
            frame_control: FrameControl::new(
                ((DataSubType::Nodata as u16) << 4) | ((FrameType::Data as u16) << 2) | 0x0200,
            ),
            duration_id: zerocopy::U16::ZERO,
            addr1: sta_mac,
            addr2: bssid,
            addr3: bssid,
            sequence_control: SequenceControl::new(0),
        };
        let null_data = Ieee80211::decode(null_hdr.as_bytes()).expect("decode null data");

        assert!(store.try_encrypt(&null_data).is_none());

        let eth_unicast = create_dummy_ethernet(&bssid.bytes, &sta_mac.bytes, b"UnicastPayload");
        let unicast_data = netsim_packets::Ieee80211::from_ieee8023(
            &bytes::Bytes::from(eth_unicast),
            bssid,
            netsim_packets::FrameDirection::FromAp,
            101,
        )
        .expect("create unicast 802.11");
        let encrypted = store.try_encrypt(&unicast_data).expect("encrypt unicast data");
        let decoded = Ieee80211::decode(&encrypted).expect("decode encrypted unicast");
        assert_eq!(decoded.get_packet_number(), [0, 0, 0, 0, 0, 1]);
    }

    #[test]
    fn test_try_encrypt_encrypts_payload_bearing_data() {
        let store = SharedKeyStore::new();
        let bssid = MacAddress::new([0x02, 0x00, 0x00, 0x00, 0x00, 0x01]);
        let sta_mac = MacAddress::new([0x02, 0x00, 0x00, 0x00, 0x00, 0x55]);

        store.add_bssid(bssid);
        store.set_gtk(bssid, [0x11; 16]);
        store.add_session(sta_mac, vec![0x42; 16]);

        let eth_bcast = create_dummy_ethernet(&bssid.bytes, &[0xFF; 6], b"BroadcastPayload");
        let bcast_data = netsim_packets::Ieee80211::from_ieee8023(
            &bytes::Bytes::from(eth_bcast),
            bssid,
            netsim_packets::FrameDirection::FromAp,
            100,
        )
        .expect("create broadcast 802.11");
        let encrypted_bcast = store.try_encrypt(&bcast_data).expect("encrypt broadcast data");
        let decoded_bcast =
            Ieee80211::decode(&encrypted_bcast).expect("decode encrypted broadcast");
        assert!(decoded_bcast.needs_decryption());

        let eth_unicast = create_dummy_ethernet(&bssid.bytes, &sta_mac.bytes, b"UnicastPayload");
        let unicast_data = netsim_packets::Ieee80211::from_ieee8023(
            &bytes::Bytes::from(eth_unicast),
            bssid,
            netsim_packets::FrameDirection::FromAp,
            101,
        )
        .expect("create unicast 802.11");
        let encrypted_unicast = store.try_encrypt(&unicast_data).expect("encrypt unicast data");
        let decoded_unicast =
            Ieee80211::decode(&encrypted_unicast).expect("decode encrypted unicast");
        assert!(decoded_unicast.needs_decryption());
    }
}
