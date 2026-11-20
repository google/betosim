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
    collections::{HashMap, HashSet},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use netsim_packets::{CcmpHeader, Ieee80211, MacAddress};
use parking_lot::RwLock;
use tracing::{debug, error};
use zerocopy::IntoBytes;

use crate::ffi::{AesCcmDecrypt, AesCcmEncrypt};

const CCMP_HDR_LEN: usize = 8;
const FC_PROTECTED_BIT: u8 = 0x40;
pub(crate) const CCMP_TK_LEN: usize = 16;
const CCMP_MIC_LEN: usize = 8;

#[derive(Debug)]
struct SessionKeys {
    tk: Vec<u8>,
    tx_pn: AtomicU64,
}

#[derive(Debug, Default)]
struct StationState {
    bssid: Option<MacAddress>,
    associated: bool,
    session: Option<Arc<SessionKeys>>,
}

impl StationState {
    fn session_for(&self, bssid: Option<MacAddress>) -> Option<&SessionKeys> {
        // `bssid: None` only occurs for test-only `add_session` entries.
        self.session.as_deref().filter(|_| self.bssid.is_none_or(|b| bssid == Some(b)))
    }
}

#[derive(Debug, Default)]
struct KeyStoreState {
    stations: HashMap<MacAddress, StationState>,
    gtks: HashMap<MacAddress, [u8; 16]>,
}

#[derive(Clone, Debug)]
pub struct SharedKeyStore {
    // Set of all active AP BSSIDs
    pub bssids: Arc<RwLock<HashSet<MacAddress>>>,
    state: Arc<RwLock<KeyStoreState>>,
    gtk_tx_pn: Arc<AtomicU64>,
}

impl Default for SharedKeyStore {
    fn default() -> Self {
        Self {
            bssids: Arc::default(),
            state: Arc::default(),
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

    /// Records a candidate BSSID for `sta_addr` before association; an associated entry
    /// keeps its associated BSSID.
    pub fn set_station_bssid(&self, sta_addr: MacAddress, bssid: MacAddress) {
        let mut s = self.state.write();
        let e = s.stations.entry(sta_addr).or_default();
        if !e.associated {
            e.bssid = Some(bssid);
        }
    }

    #[cfg(any(test, feature = "testing"))]
    pub fn has_station(&self, addr: &MacAddress) -> bool {
        self.state.read().stations.contains_key(addr)
    }

    pub fn get_station_bssid(&self, sta_addr: &MacAddress) -> Option<MacAddress> {
        self.state.read().stations.get(sta_addr).and_then(|e| e.bssid)
    }

    pub(crate) fn associate_station(&self, sta: MacAddress, bssid: MacAddress) {
        let st = StationState { bssid: Some(bssid), associated: true, ..Default::default() };
        self.state.write().stations.insert(sta, st);
    }

    pub(crate) fn authorize_station(&self, sta: MacAddress, bssid: MacAddress, tk: Vec<u8>) {
        let mut s = self.state.write();
        let Some(entry) =
            s.stations.get_mut(&sta).filter(|e| e.associated && e.bssid == Some(bssid))
        else {
            debug!(%sta, %bssid, "ignoring authorize_station for unassociated/mismatched");
            return;
        };
        entry.session = Some(Arc::new(SessionKeys { tk, tx_pn: AtomicU64::new(1) }));
    }

    #[cfg(any(test, feature = "testing"))]
    pub fn add_session(&self, sta_addr: MacAddress, tk: Vec<u8>) {
        let mut s = self.state.write();
        let entry = s.stations.entry(sta_addr).or_default();
        entry.session = Some(Arc::new(SessionKeys { tk, tx_pn: AtomicU64::new(1) }));
    }

    pub fn get_gtk(&self, bssid: &MacAddress) -> Option<[u8; 16]> {
        self.state.read().gtks.get(bssid).copied()
    }

    pub(crate) fn set_gtk(&self, bssid: MacAddress, gtk: [u8; 16]) {
        self.state.write().gtks.insert(bssid, gtk);
    }

    pub(crate) fn remove_station_for_bssid(&self, sta_addr: &MacAddress, bssid: &MacAddress) {
        let mut state = self.state.write();
        if state.stations.get(sta_addr).is_some_and(|s| s.bssid == Some(*bssid)) {
            state.stations.remove(sta_addr);
        }
    }

    pub(crate) fn remove_bssid(&self, bssid: &MacAddress) {
        self.bssids.write().remove(bssid);
        let state = &mut *self.state.write();
        state.gtks.remove(bssid);
        state.stations.retain(|_, s| s.bssid != Some(*bssid));
    }

    pub(crate) fn reset(&self, default_bssid: MacAddress) {
        *self.bssids.write() = HashSet::from([default_bssid]);
        self.gtk_tx_pn.store(1, Ordering::SeqCst);
        *self.state.write() = KeyStoreState::default();
    }

    pub fn try_encrypt(&self, ieee80211: &Ieee80211) -> Option<Vec<u8>> {
        if !ieee80211.is_payload_bearing_data() || ieee80211.is_eapol().unwrap_or(false) {
            return None;
        }
        let dest = ieee80211.get_destination();
        let is_group = dest.is_multicast() || dest.is_broadcast();
        let state = self.state.read();
        let (tk, pn, key_id) = if is_group {
            let gtk = state.gtks.get(&ieee80211.get_bssid()?)?;
            (gtk.to_vec(), self.gtk_tx_pn.fetch_add(1, Ordering::SeqCst), 1)
        } else {
            let session = state.stations.get(&dest)?.session_for(ieee80211.get_bssid())?;
            (session.tk.clone(), session.tx_pn.fetch_add(1, Ordering::SeqCst), 0)
        };
        drop(state);

        let tk = tk.get(..CCMP_TK_LEN)?;

        // CCMP Nonce (13 bytes): Priority(1: 0) || A2(6: Src/BSSID) || PN(6: MSB-first)
        // Note: pn.to_le_bytes() yields LSB at pn0; Nonce maps PN in big-endian order
        // [pn5..pn0] matching IEEE 802.11 CCMP / hostapd logic.
        let [pn0, pn1, pn2, pn3, pn4, pn5, ..] = pn.to_le_bytes();
        let nonce = ieee80211.get_nonce(&[pn5, pn4, pn3, pn2, pn1, pn0]);

        // The CCMP AAD explicitly requires the 'Protected' frame control bit to be
        // active. We must calculate the AAD against the final physical MAC byte
        // sequence!
        let mut final_fc_bytes = ieee80211.as_bytes().to_vec();
        *final_fc_bytes.get_mut(1)? |= FC_PROTECTED_BIT;
        let aad = Ieee80211::decode(&final_fc_bytes).ok()?.get_aad();

        let payload = ieee80211.get_payload();
        let mut ciphertext = Vec::with_capacity(payload.len() + CCMP_MIC_LEN);
        if !AesCcmEncrypt(tk, &nonce, &aad, &payload, &mut ciphertext, CCMP_MIC_LEN) {
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
        *new_packet.get_mut(1)? |= FC_PROTECTED_BIT;

        Some(new_packet)
    }

    pub fn try_decrypt(&self, ieee80211: &Ieee80211) -> Option<Vec<u8>> {
        if !ieee80211.needs_decryption() {
            return None;
        }
        let src = ieee80211.get_source();
        let state = self.state.read();
        let session = state.stations.get(&src)?.session_for(ieee80211.get_bssid())?;
        let tk = session.tk.get(..CCMP_TK_LEN)?.to_vec();
        drop(state);

        let hdr_len = ieee80211.hdr_length();
        // Extract Nonce from Frame (CCMP Header: PN0, PN1, Rsvd, KeyID, PN2..PN5)
        // Frame: Header || CCMP Header || Data || MIC
        let data = ieee80211.as_bytes().get(hdr_len + CCMP_HDR_LEN..)?;
        if data.len() < CCMP_MIC_LEN {
            return None;
        }
        let nonce = ieee80211.get_nonce(&ieee80211.get_packet_number());
        let aad = ieee80211.get_aad();

        let mut out_plain = Vec::with_capacity(data.len() - CCMP_MIC_LEN);
        if !AesCcmDecrypt(&tk, &nonce, &aad, data, &mut out_plain, CCMP_MIC_LEN) {
            error!(
                code = crate::err::Code::CcmpDecryptFailed as u32,
                "CCMP DECRYPT FAILED! hdr_len: {}, msg_len: {}, aad_len: {}",
                hdr_len,
                data.len(),
                aad.len()
            );
            debug!("RAW FRAME TO DECRYPT (Hex): {:02X?}", ieee80211.as_bytes());
            return None;
        }

        let mut new_packet = Vec::with_capacity(hdr_len + out_plain.len());
        new_packet.extend_from_slice(ieee80211.as_bytes().get(..hdr_len)?);
        new_packet.extend_from_slice(&out_plain);

        *new_packet.get_mut(1)? &= !FC_PROTECTED_BIT;
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
        assert!(store.state.read().stations.is_empty());
        assert_eq!(store.gtk_tx_pn.load(Ordering::SeqCst), 1);
    }

    use netsim_packets::{
        BeaconFixedFields, DataSubType, FrameControl, FrameType, MacHeader3Addr, SequenceControl,
        management_subtype,
    };

    const MIN_ETHERNET_LEN: usize = 60;

    fn create_dummy_ethernet(src: &[u8; 6], dst: &[u8; 6], payload: &[u8]) -> Vec<u8> {
        let dst_mac = netsim_packets::MacAddr::new(*dst);
        let src_mac = netsim_packets::MacAddr::new(*src);
        let frame =
            netsim_packets::EthernetFrame::new(dst_mac, src_mac, netsim_packets::ether_type::IPV4);
        let mut bytes = frame.as_bytes().to_vec();
        bytes.extend_from_slice(payload);
        if bytes.len() < MIN_ETHERNET_LEN {
            bytes.resize(MIN_ETHERNET_LEN, 0);
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

    fn try_decrypt_with_error_stats(
        store: &SharedKeyStore,
        frame: &Ieee80211,
    ) -> (Option<Vec<u8>>, Vec<(u32, u32)>) {
        use common::util::netsim_logger::{ErrorStatsLayer, ErrorStatsTable};
        use tracing_subscriber::layer::SubscriberExt;

        let table = Arc::new(ErrorStatsTable::new());
        let subscriber =
            tracing_subscriber::registry().with(ErrorStatsLayer::with_table(Arc::clone(&table)));
        let _multi_dispatch_guard = tracing::Dispatch::new(tracing_subscriber::registry());
        let decrypted = tracing::subscriber::with_default(subscriber, || store.try_decrypt(frame));
        (decrypted, table.snapshot())
    }

    #[test]
    fn test_try_decrypt_corrupted_ciphertext_records_error_stats() {
        use crate::err::Code;

        // Given
        let store = SharedKeyStore::new();
        let bssid = MacAddress::new([0x02, 0x00, 0x00, 0x00, 0x00, 0x01]);
        let sta_mac = MacAddress::new([0x02, 0x00, 0x00, 0x00, 0x00, 0x55]);
        store.add_bssid(bssid);
        store.add_session(sta_mac, vec![0x42; 16]);

        let eth = create_dummy_ethernet(&sta_mac.bytes, &bssid.bytes, b"CorruptedToApPayload");
        let to_ap_data = netsim_packets::Ieee80211::from_ieee8023(
            &bytes::Bytes::from(eth),
            bssid,
            netsim_packets::FrameDirection::ToAp,
            102,
        )
        .expect("create ToAp 802.11 data");
        let mut raw = to_ap_data.as_bytes().to_vec();
        raw[1] |= FC_PROTECTED_BIT;
        let corrupted = Ieee80211::decode(&raw).expect("decode forged protected frame");

        // When
        let (decrypted, error_stats) = try_decrypt_with_error_stats(&store, &corrupted);

        // Then
        assert!(decrypted.is_none());
        assert_eq!(error_stats, vec![(Code::CcmpDecryptFailed as u32, 1)]);
    }

    const TEST_BSSID_1: MacAddress = MacAddress::new([0x02, 0x00, 0x00, 0x00, 0x00, 0x01]);
    const TEST_BSSID_2: MacAddress = MacAddress::new([0x02, 0x00, 0x00, 0x00, 0x00, 0x02]);
    const TEST_STA_MAC: MacAddress = MacAddress::new([0x02, 0x00, 0x00, 0x00, 0x00, 0x55]);
    const TEST_STA_MAC_2: MacAddress = MacAddress::new([0x02, 0x00, 0x00, 0x00, 0x00, 0x66]);

    fn dummy_from_ap_frame(bssid: MacAddress, dst: MacAddress, seq: u16, body: &[u8]) -> Ieee80211 {
        let eth = bytes::Bytes::from(create_dummy_ethernet(&bssid.bytes, &dst.bytes, body));
        Ieee80211::from_ieee8023(&eth, bssid, netsim_packets::FrameDirection::FromAp, seq).unwrap()
    }

    fn associated_store() -> SharedKeyStore {
        let store = SharedKeyStore::new();
        store.add_bssid(TEST_BSSID_1);
        store.add_bssid(TEST_BSSID_2);
        store.associate_station(TEST_STA_MAC, TEST_BSSID_1);
        store
    }

    #[test]
    fn test_remove_station_for_bssid_removes_only_matching_bssid() {
        // Given
        let s = associated_store();

        // When
        s.remove_station_for_bssid(&TEST_STA_MAC, &TEST_BSSID_2);

        // Then
        assert_eq!(s.get_station_bssid(&TEST_STA_MAC), Some(TEST_BSSID_1));

        // When
        s.remove_station_for_bssid(&TEST_STA_MAC, &TEST_BSSID_1);

        // Then
        assert!(!s.has_station(&TEST_STA_MAC));
        assert_eq!(s.get_station_bssid(&TEST_STA_MAC), None);
    }

    #[test]
    fn test_authorize_station_ignores_unassociated_station() {
        // Given
        let store = SharedKeyStore::new();
        store.add_bssid(TEST_BSSID_1);

        // When
        store.authorize_station(TEST_STA_MAC, TEST_BSSID_1, vec![0x42; CCMP_TK_LEN]);

        // Then
        assert!(
            store.try_encrypt(&dummy_from_ap_frame(TEST_BSSID_1, TEST_STA_MAC, 1, b"x")).is_none()
        );

        // When
        store.set_station_bssid(TEST_STA_MAC, TEST_BSSID_1);
        store.authorize_station(TEST_STA_MAC, TEST_BSSID_1, vec![0x42; CCMP_TK_LEN]);

        // Then
        assert!(
            store.try_encrypt(&dummy_from_ap_frame(TEST_BSSID_1, TEST_STA_MAC, 1, b"x")).is_none()
        );
    }

    #[test]
    fn test_authorize_station_ignores_mismatched_bssid() {
        // Given
        let store = associated_store();

        // When
        store.authorize_station(TEST_STA_MAC, TEST_BSSID_2, vec![0x42; CCMP_TK_LEN]);

        // Then
        assert!(
            store.try_encrypt(&dummy_from_ap_frame(TEST_BSSID_1, TEST_STA_MAC, 1, b"x")).is_none()
        );

        // When
        store.authorize_station(TEST_STA_MAC, TEST_BSSID_1, vec![0x11; CCMP_TK_LEN]);

        // Then
        assert!(
            store.try_encrypt(&dummy_from_ap_frame(TEST_BSSID_1, TEST_STA_MAC, 1, b"x")).is_some()
        );
    }

    // Guards the wifi-actor test seam (`add_session` installs `bssid: None`).
    #[test]
    fn test_add_session_supports_unassociated_decrypt() {
        // Given
        let enc_store = associated_store();
        enc_store.authorize_station(TEST_STA_MAC, TEST_BSSID_1, vec![0x42; CCMP_TK_LEN]);
        let plain = dummy_from_ap_frame(TEST_BSSID_1, TEST_STA_MAC, 1, b"StandAlone");
        let enc = Ieee80211::decode(&enc_store.try_encrypt(&plain).expect("encrypt")).unwrap();
        let dec = SharedKeyStore::new();

        // When
        dec.add_session(enc.get_source(), vec![0x42; CCMP_TK_LEN]);

        // Then
        assert!(dec.get_station_bssid(&enc.get_source()).is_none());
        assert!(dec.try_decrypt(&enc).is_some());
    }

    #[test]
    fn test_set_station_bssid_does_not_clobber_associated_station() {
        // Given
        let s = associated_store();

        // When
        s.set_station_bssid(TEST_STA_MAC_2, TEST_BSSID_2);

        // Then
        assert_eq!(s.get_station_bssid(&TEST_STA_MAC_2), Some(TEST_BSSID_2));

        // When
        s.set_station_bssid(TEST_STA_MAC, TEST_BSSID_2);

        // Then
        assert_eq!(s.get_station_bssid(&TEST_STA_MAC), Some(TEST_BSSID_1));
    }

    #[test]
    fn test_remove_bssid_clears_only_its_stations_and_gtk() {
        // Given
        let s = SharedKeyStore::new();
        s.add_bssid(TEST_BSSID_1);
        s.add_bssid(TEST_BSSID_2);
        s.associate_station(TEST_STA_MAC, TEST_BSSID_2);
        s.associate_station(TEST_STA_MAC_2, TEST_BSSID_1);
        s.set_gtk(TEST_BSSID_2, [0x22; 16]);

        // When
        s.remove_bssid(&TEST_BSSID_2);

        // Then
        assert!(!s.has_station(&TEST_STA_MAC));
        assert!(s.has_station(&TEST_STA_MAC_2));
        assert!(s.get_gtk(&TEST_BSSID_2).is_none());
    }

    #[test]
    fn test_session_for_rejects_mismatched_bssid_on_encrypt_and_decrypt() {
        use crate::err::Code;

        // Given
        let store = associated_store();
        store.authorize_station(TEST_STA_MAC, TEST_BSSID_1, vec![0x11; CCMP_TK_LEN]);
        let eth = create_dummy_ethernet(&TEST_STA_MAC.bytes, &TEST_BSSID_2.bytes, b"ToApPayload");
        let make_protected_to_ap = |bssid: MacAddress, seq: u16| {
            let frame = Ieee80211::from_ieee8023(
                &bytes::Bytes::from(eth.clone()),
                bssid,
                netsim_packets::FrameDirection::ToAp,
                seq,
            )
            .expect("create ToAp 802.11");
            let mut raw = frame.as_bytes().to_vec();
            raw[1] |= FC_PROTECTED_BIT;
            Ieee80211::decode(&raw).expect("decode protected ToAp frame")
        };
        let to_ap_bssid1 = make_protected_to_ap(TEST_BSSID_1, 3);
        let to_ap_bssid2 = make_protected_to_ap(TEST_BSSID_2, 4);

        // When
        let enc_match =
            store.try_encrypt(&dummy_from_ap_frame(TEST_BSSID_1, TEST_STA_MAC, 1, b"ok"));
        let enc_mismatch =
            store.try_encrypt(&dummy_from_ap_frame(TEST_BSSID_2, TEST_STA_MAC, 2, b"no"));
        let (dec_match, stats_match) = try_decrypt_with_error_stats(&store, &to_ap_bssid1);
        let (dec_mismatch, stats_mismatch) = try_decrypt_with_error_stats(&store, &to_ap_bssid2);

        // Then
        assert!(enc_match.is_some());
        assert!(enc_mismatch.is_none());
        assert!(dec_match.is_none());
        assert_eq!(stats_match, vec![(Code::CcmpDecryptFailed as u32, 1)]);
        assert!(dec_mismatch.is_none());
        assert!(stats_mismatch.is_empty());
    }

    #[test]
    fn test_try_encrypt_skips_eapol_frames() {
        let store = SharedKeyStore::new();
        store.add_bssid(TEST_BSSID_1);
        store.set_station_bssid(TEST_STA_MAC, TEST_BSSID_1);
        store.add_session(TEST_STA_MAC, vec![0x42; CCMP_TK_LEN]);

        let eth_type = netsim_packets::ether_type::EAPOL;
        let mut eapol_bytes =
            netsim_packets::EthernetFrame::new(TEST_STA_MAC, TEST_BSSID_1, eth_type)
                .as_bytes()
                .to_vec();
        eapol_bytes.resize(MIN_ETHERNET_LEN, 0);
        let dir = netsim_packets::FrameDirection::FromAp;
        let eapol_frame = Ieee80211::from_ieee8023(&eapol_bytes, TEST_BSSID_1, dir, 1).unwrap();
        assert!(store.try_encrypt(&eapol_frame).is_none());
    }

    #[test]
    fn test_try_decrypt_skips_unprotected_frames() {
        // Given
        let store = SharedKeyStore::new();
        store.add_bssid(TEST_BSSID_1);
        store.set_station_bssid(TEST_STA_MAC, TEST_BSSID_1);
        store.add_session(TEST_STA_MAC, vec![0x42; CCMP_TK_LEN]);

        let eth_plain =
            create_dummy_ethernet(&TEST_STA_MAC.bytes, &TEST_BSSID_1.bytes, b"PlaintextUplink");
        let dir = netsim_packets::FrameDirection::ToAp;
        let plain_to_ap = Ieee80211::from_ieee8023(&eth_plain, TEST_BSSID_1, dir, 1).unwrap();

        // When
        let (decrypted, error_stats) = try_decrypt_with_error_stats(&store, &plain_to_ap);

        // Then
        assert!(decrypted.is_none());
        assert!(error_stats.is_empty());
    }
}
