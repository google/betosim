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
    collections::{HashMap, HashSet, VecDeque},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use netsim_packets::{CcmpHeader, Ieee80211, MacAddress};
use parking_lot::RwLock;
use tokio::sync::mpsc::UnboundedSender;
use tracing::{debug, error};
use zerocopy::IntoBytes;

use crate::ffi::{AesCcmDecrypt, AesCcmEncrypt};

const CCMP_HDR_LEN: usize = 8;
pub(crate) const FC_PROTECTED_BIT: u8 = 0x40;
pub(crate) const CCMP_TK_LEN: usize = 16;
const CCMP_MIC_LEN: usize = 8;
pub const MAX_BUFFERED_FRAMES_PER_STATION: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PortState {
    Blocked,
    Authorized,
}

/// Decision returned by [`SharedKeyStore::gate_downlink_frame`] for outbound data frames.
#[derive(Clone, Debug)]
pub enum DownlinkPortAction {
    /// Port is open or authorized; contained frames must be sent before the current frame.
    Deliver(VecDeque<Ieee80211>),
    /// Controlled port is blocked or awaiting EAP-Success flush; queued FIFO.
    Buffered,
    /// Drop without buffering (associated to another BSSID, or unassociated on controlled port).
    Drop,
}

#[derive(Debug)]
struct SessionKeys {
    tk: Vec<u8>,
    tx_pn: AtomicU64,
}

impl SessionKeys {
    fn new(tk: Vec<u8>) -> Arc<Self> {
        Arc::new(Self { tk, tx_pn: AtomicU64::new(1) })
    }
}

/// Emitted when an associated station's controlled port transitions to `Authorized`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PortAuthorizedEvent {
    pub sta_addr: MacAddress,
    pub bssid: MacAddress,
    pub epoch: u64,
}

type AuthTx = UnboundedSender<PortAuthorizedEvent>;

fn notify_port_authorized(tx: Option<AuthTx>, event: Option<PortAuthorizedEvent>) {
    if let (Some(tx), Some(event)) = (tx, event)
        && tx.send(event).is_err()
    {
        debug!(sta_addr = %event.sta_addr, "port_authorized_tx receiver closed");
    }
}

fn set_bssid_flag(set: &mut HashSet<MacAddress>, bssid: MacAddress, on: bool) {
    let _ = if on { set.insert(bssid) } else { set.remove(&bssid) };
}

fn push_bounded<T>(queue: &mut VecDeque<T>, item: T) {
    if queue.len() >= MAX_BUFFERED_FRAMES_PER_STATION {
        queue.pop_front();
    }
    queue.push_back(item);
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum EapFlushState {
    #[default]
    Idle,
    AwaitingTx(u16),
    ReadyToDrain,
}

#[derive(Debug, Default)]
struct StationState {
    bssid: Option<MacAddress>,
    port_state: Option<PortState>,
    epoch: u64,
    eap_flush: EapFlushState,
    session: Option<Arc<SessionKeys>>,
    pending_downlink: VecDeque<Ieee80211>,
}

impl StationState {
    fn is_blocked(&self, requires_cp: bool) -> bool {
        self.port_state.map_or(requires_cp, |port_state| port_state == PortState::Blocked)
    }

    fn is_associated_to_other_bssid(&self, bssid: &MacAddress) -> bool {
        self.bssid != Some(*bssid) && self.port_state.is_some()
    }

    fn session_for(&self, bssid: Option<MacAddress>) -> Option<&SessionKeys> {
        // `bssid: None` only occurs for test-only `add_session` entries.
        self.session
            .as_deref()
            .filter(|_| self.bssid.is_none_or(|sta_bssid| bssid == Some(sta_bssid)))
    }
}

#[derive(Clone, Copy, Debug)]
struct PendingHwsimAlias {
    bssid: MacAddress,
    hwsim_addr: MacAddress,
}

#[derive(Debug, Default)]
struct KeyStoreState {
    secure_bssids: HashSet<MacAddress>,
    controlled_port_bssids: HashSet<MacAddress>,
    stations: HashMap<MacAddress, StationState>,
    hwsim_aliases: HashMap<MacAddress, MacAddress>,
    pending_hwsim_aliases: HashMap<MacAddress, PendingHwsimAlias>,
    port_authorized_tx: Option<AuthTx>,
    gtks: HashMap<MacAddress, [u8; 16]>,
    next_epoch: u64,
}

impl KeyStoreState {
    fn requires_controlled_port(&self, bssid: &MacAddress) -> bool {
        self.controlled_port_bssids.contains(bssid)
    }

    fn alloc_epoch(&mut self) -> u64 {
        self.next_epoch += 1;
        self.next_epoch
    }

    fn apply_bssid_mode(&mut self, bssid: MacAddress, is_secure: bool, enterprise: bool) {
        set_bssid_flag(&mut self.secure_bssids, bssid, is_secure);
        set_bssid_flag(&mut self.controlled_port_bssids, bssid, is_secure || enterprise);
    }

    fn is_managed(&self, addr: &MacAddress) -> bool {
        self.stations.get(addr).is_some_and(|station| station.port_state.is_some())
    }
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

    #[cfg(any(test, feature = "testing"))]
    pub fn add_bssid(&self, bssid: MacAddress) {
        self.register_bssid(bssid, false, false);
    }

    pub fn has_bssid(&self, bssid: &MacAddress) -> bool {
        self.bssids.read().contains(bssid)
    }

    /// Records a candidate BSSID before association, promoting a pre-installed test session.
    pub fn set_station_bssid(&self, sta_addr: MacAddress, bssid: MacAddress) {
        let mut state = self.state.write();
        let entry = state.stations.entry(sta_addr).or_default();
        let unassociated = entry.port_state.is_none();
        if unassociated {
            entry.bssid = Some(bssid);
        }
        #[cfg(any(test, feature = "testing"))]
        if unassociated && entry.session.is_some() {
            let epoch = state.alloc_epoch();
            let tx = state.port_authorized_tx.clone();
            let entry = state.stations.get_mut(&sta_addr).expect("inserted above");
            entry.port_state = Some(PortState::Authorized);
            entry.eap_flush = EapFlushState::Idle;
            entry.epoch = epoch;
            drop(state);
            notify_port_authorized(tx, Some(PortAuthorizedEvent { sta_addr, bssid, epoch }));
        }
    }

    /// Stages a pending `hwsim_addr -> sta_addr` alias until association completes.
    pub fn stage_hwsim_alias(
        &self,
        hwsim_addr: MacAddress,
        sta_addr: MacAddress,
        bssid: MacAddress,
    ) {
        let mut state = self.state.write();
        if hwsim_addr == sta_addr {
            state.pending_hwsim_aliases.remove(&sta_addr);
        } else {
            state.pending_hwsim_aliases.insert(sta_addr, PendingHwsimAlias { bssid, hwsim_addr });
        }
    }

    /// Records `hwsim_addr -> sta_addr` for `bssid` while protecting associated bindings.
    pub fn set_hwsim_alias(&self, hwsim_addr: MacAddress, sta_addr: MacAddress, bssid: MacAddress) {
        let mut state = self.state.write();
        let sta_on_bssid =
            state.stations.get(&sta_addr).is_some_and(|station| station.bssid == Some(bssid));
        if !sta_on_bssid {
            return;
        }
        let current_is_managed =
            state.hwsim_aliases.get(&hwsim_addr).is_some_and(|current| state.is_managed(current));
        if sta_addr == hwsim_addr {
            if !current_is_managed {
                state.hwsim_aliases.remove(&hwsim_addr);
            }
            return;
        }
        if !state.is_managed(&hwsim_addr) && (state.is_managed(&sta_addr) || !current_is_managed) {
            state.hwsim_aliases.insert(hwsim_addr, sta_addr);
        }
    }

    #[cfg(any(test, feature = "testing"))]
    pub fn has_station(&self, addr: &MacAddress) -> bool {
        self.state.read().stations.contains_key(addr)
    }

    #[cfg(any(test, feature = "testing"))]
    pub fn resolve_hwsim_alias(&self, hwsim_addr: &MacAddress) -> Option<MacAddress> {
        self.state.read().hwsim_aliases.get(hwsim_addr).copied()
    }

    /// Returns the BSSID for `addr`, giving an active `hwsim_aliases` target precedence.
    pub fn get_station_bssid(&self, addr: &MacAddress) -> Option<MacAddress> {
        let state = self.state.read();
        let aliased = state.hwsim_aliases.get(addr).and_then(|target| state.stations.get(target));
        aliased.or_else(|| state.stations.get(addr)).and_then(|entry| entry.bssid)
    }

    pub fn set_port_authorized_sender(&self, sender: AuthTx) {
        self.state.write().port_authorized_tx = Some(sender);
    }

    pub(crate) fn register_bssid(&self, bssid: MacAddress, is_secure: bool, enterprise: bool) {
        let mut bssids = self.bssids.write();
        let mut state = self.state.write();
        bssids.insert(bssid);
        state.apply_bssid_mode(bssid, is_secure, enterprise);
    }

    #[cfg(test)]
    fn is_secure_bssid(&self, bssid: &MacAddress) -> bool {
        self.state.read().secure_bssids.contains(bssid)
    }

    pub fn requires_controlled_port(&self, bssid: &MacAddress) -> bool {
        self.state.read().requires_controlled_port(bssid)
    }

    pub(crate) fn associate_station(&self, sta_addr: MacAddress, bssid: MacAddress) {
        let mut state = self.state.write();
        let requires_cp = state.requires_controlled_port(&bssid);
        let epoch = state.alloc_epoch();
        let port_state = Some(if requires_cp { PortState::Blocked } else { PortState::Authorized });
        let station = StationState { bssid: Some(bssid), port_state, epoch, ..Default::default() };
        state.stations.insert(sta_addr, station);
        state.hwsim_aliases.retain(|hwsim, sta| hwsim != &sta_addr && sta != &sta_addr);
        if let Some(pending) = state.pending_hwsim_aliases.remove(&sta_addr)
            && pending.bssid == bssid
            && !state.is_managed(&pending.hwsim_addr)
        {
            state.hwsim_aliases.insert(pending.hwsim_addr, sta_addr);
        }
    }

    pub(crate) fn authorize_station(&self, sta_addr: MacAddress, bssid: MacAddress, tk: Vec<u8>) {
        self.authorize(sta_addr, bssid, Some(tk), None);
    }

    pub(crate) fn authorize_eap_station(
        &self,
        sta_addr: MacAddress,
        bssid: MacAddress,
        seq_ctrl: u16,
    ) {
        self.authorize(sta_addr, bssid, None, Some(seq_ctrl));
    }

    fn authorize(
        &self,
        sta_addr: MacAddress,
        bssid: MacAddress,
        tk: Option<Vec<u8>>,
        seq_ctrl: Option<u16>,
    ) {
        let mut state = self.state.write();
        let Some(entry) = state
            .stations
            .get_mut(&sta_addr)
            .filter(|entry| entry.port_state.is_some() && entry.bssid == Some(bssid))
        else {
            debug!(%sta_addr, %bssid, "ignoring authorize for unassociated/mismatched");
            return;
        };
        let has_tk = tk.is_some();
        let epoch = entry.epoch;
        entry.port_state = Some(PortState::Authorized);
        entry.eap_flush = match seq_ctrl {
            Some(seq_ctrl) if !has_tk => EapFlushState::AwaitingTx(seq_ctrl),
            _ => EapFlushState::Idle,
        };
        if let Some(tk) = tk {
            entry.session = Some(SessionKeys::new(tk));
        }
        let event = has_tk.then_some(PortAuthorizedEvent { sta_addr, bssid, epoch });
        let tx = has_tk.then(|| state.port_authorized_tx.clone()).flatten();
        drop(state);
        notify_port_authorized(tx, event);
    }

    /// Returns `true` if non-EAPOL uplink data from `sta_addr` toward `frame_bssid` must drop.
    pub fn is_uplink_port_blocked(&self, sta_addr: &MacAddress, frame_bssid: &MacAddress) -> bool {
        let state = self.state.read();
        let requires_cp = state.requires_controlled_port(frame_bssid);
        state.stations.get(sta_addr).map_or(requires_cp, |entry| {
            entry.is_associated_to_other_bssid(frame_bssid) || entry.is_blocked(requires_cp)
        })
    }

    /// Gates a downlink frame: buffers while blocked/pending, delivers, or drops.
    pub fn gate_downlink_frame(
        &self,
        sta_addr: MacAddress,
        bssid: &MacAddress,
        frame: &Ieee80211,
    ) -> DownlinkPortAction {
        use DownlinkPortAction::{Buffered, Deliver, Drop};
        if !frame.is_payload_bearing_data() {
            return Deliver(VecDeque::new());
        }
        let mut state = self.state.write();
        let requires_cp = state.requires_controlled_port(bssid);
        let Some(entry) = state.stations.get_mut(&sta_addr) else {
            return if requires_cp { Drop } else { Deliver(VecDeque::new()) };
        };
        if entry.is_associated_to_other_bssid(bssid) || (entry.port_state.is_none() && requires_cp)
        {
            return Drop;
        }
        if !frame.needs_decryption()
            && frame.is_from_ds()
            && !frame.is_to_ds()
            && frame.get_bssid() == Some(*bssid)
            && frame.get_source() == *bssid
            && frame.get_destination() == sta_addr
            && frame.is_eapol().unwrap_or(false)
        {
            return Deliver(VecDeque::new());
        }
        if !entry.is_blocked(requires_cp) && entry.eap_flush == EapFlushState::Idle {
            return Deliver(std::mem::take(&mut entry.pending_downlink));
        }
        push_bounded(&mut entry.pending_downlink, frame.clone());
        Buffered
    }

    /// Marks a pending `EAP-Success` matching `seq_ctrl` ready to drain and returns its epoch.
    pub fn take_eap_success_epoch(
        &self,
        sta_addr: &MacAddress,
        bssid: &MacAddress,
        seq_ctrl: u16,
    ) -> Option<u64> {
        let mut state = self.state.write();
        let entry = state.stations.get_mut(sta_addr)?;
        if entry.port_state == Some(PortState::Authorized)
            && entry.bssid == Some(*bssid)
            && entry.eap_flush == EapFlushState::AwaitingTx(seq_ctrl)
        {
            entry.eap_flush = EapFlushState::ReadyToDrain;
            Some(entry.epoch)
        } else {
            None
        }
    }

    /// Drains queued downlink frames for `sta_addr` on `bssid` matching `epoch`.
    pub fn take_authorized_buffered_frames(
        &self,
        sta_addr: &MacAddress,
        bssid: &MacAddress,
        epoch: u64,
    ) -> VecDeque<Ieee80211> {
        let mut state = self.state.write();
        let Some(entry) = state.stations.get_mut(sta_addr) else { return VecDeque::new() };
        if entry.port_state != Some(PortState::Authorized)
            || entry.bssid != Some(*bssid)
            || entry.epoch != epoch
            || matches!(entry.eap_flush, EapFlushState::AwaitingTx(_))
        {
            return VecDeque::new();
        }
        entry.eap_flush = EapFlushState::Idle;
        std::mem::take(&mut entry.pending_downlink)
    }

    /// Discards queued downlink frames for `sta_addr` without changing port state.
    pub fn clear_station_buffer(&self, sta_addr: &MacAddress) {
        if let Some(entry) = self.state.write().stations.get_mut(sta_addr) {
            entry.pending_downlink.clear();
        }
    }

    #[cfg(any(test, feature = "testing"))]
    pub fn buffered_frame_count(&self, sta_addr: &MacAddress) -> usize {
        self.state.read().stations.get(sta_addr).map_or(0, |entry| entry.pending_downlink.len())
    }

    #[cfg(any(test, feature = "testing"))]
    pub fn add_session(&self, sta_addr: MacAddress, tk: Vec<u8>) {
        let mut state = self.state.write();
        let promote = state.stations.get(&sta_addr).is_some_and(|entry| {
            entry.bssid.is_some() && entry.port_state != Some(PortState::Authorized)
        });
        let epoch = promote.then(|| state.alloc_epoch());
        let tx = epoch.and_then(|_| state.port_authorized_tx.clone());
        let entry = state.stations.entry(sta_addr).or_default();
        entry.session = Some(SessionKeys::new(tk));
        if entry.bssid.is_some() {
            entry.eap_flush = EapFlushState::Idle;
        }
        let event = entry.bssid.zip(epoch).map(|(bssid, epoch)| {
            entry.port_state = Some(PortState::Authorized);
            entry.epoch = epoch;
            PortAuthorizedEvent { sta_addr, bssid, epoch }
        });
        drop(state);
        notify_port_authorized(tx, event);
    }

    pub fn get_gtk(&self, bssid: &MacAddress) -> Option<[u8; 16]> {
        self.state.read().gtks.get(bssid).copied()
    }

    pub(crate) fn set_gtk(&self, bssid: MacAddress, gtk: [u8; 16]) {
        self.state.write().gtks.insert(bssid, gtk);
    }

    pub(crate) fn remove_station_for_bssid(&self, sta_addr: &MacAddress, bssid: &MacAddress) {
        let mut state = self.state.write();
        if state.pending_hwsim_aliases.get(sta_addr).is_some_and(|pending| pending.bssid == *bssid)
        {
            state.pending_hwsim_aliases.remove(sta_addr);
        }
        if state.stations.get(sta_addr).is_some_and(|station| station.bssid == Some(*bssid)) {
            state.stations.remove(sta_addr);
            state.hwsim_aliases.retain(|_, sta| sta != sta_addr);
        }
    }

    pub(crate) fn remove_bssid(&self, bssid: &MacAddress) {
        let mut bssids = self.bssids.write();
        let mut state_guard = self.state.write();
        let state = &mut *state_guard;
        bssids.remove(bssid);
        state.secure_bssids.remove(bssid);
        state.controlled_port_bssids.remove(bssid);
        state.gtks.remove(bssid);
        state.stations.retain(|_, station| station.bssid != Some(*bssid));
        state.hwsim_aliases.retain(|_, sta| state.stations.contains_key(sta));
        state.pending_hwsim_aliases.retain(|_, pending| pending.bssid != *bssid);
    }

    pub(crate) fn reset_with_mode(
        &self,
        default_bssid: MacAddress,
        is_secure: bool,
        enterprise: bool,
    ) {
        let mut bssids = self.bssids.write();
        let mut state = self.state.write();
        self.gtk_tx_pn.store(1, Ordering::SeqCst);
        *bssids = HashSet::from([default_bssid]);
        let port_authorized_tx = state.port_authorized_tx.take();
        let next_epoch = state.next_epoch;
        *state = KeyStoreState { port_authorized_tx, next_epoch, ..Default::default() };
        state.apply_bssid_mode(default_bssid, is_secure, enterprise);
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
        let post_reset_sta = MacAddress::new([0x02, 0x00, 0x00, 0x00, 0x00, 0x56]);
        let mut rx = with_auth_rx(&store);

        store.add_bssid(default_bssid);
        store.register_bssid(other_bssid, true, true);
        store.set_station_bssid(sta, other_bssid);
        store.add_session(sta, vec![0x42; 16]);
        let prior_epoch = assert_auth_event(&mut rx, sta, other_bssid);
        store.set_gtk(default_bssid, [0x11; 16]);
        store.set_gtk(other_bssid, [0x24; 16]);

        store.gtk_tx_pn.store(42, Ordering::SeqCst);
        store.reset_with_mode(default_bssid, true, false);

        assert!(store.has_bssid(&default_bssid));
        assert!(!store.has_bssid(&other_bssid));
        assert!(store.get_station_bssid(&sta).is_none());
        assert!(store.get_gtk(&default_bssid).is_none());
        assert!(store.get_gtk(&other_bssid).is_none());
        assert!(store.state.read().stations.is_empty());
        assert_eq!(store.gtk_tx_pn.load(Ordering::SeqCst), 1);
        assert!(!store.is_secure_bssid(&other_bssid));
        assert!(!store.requires_controlled_port(&other_bssid));
        assert!(store.is_secure_bssid(&default_bssid));
        assert!(store.requires_controlled_port(&default_bssid));
        store.associate_station(post_reset_sta, default_bssid);
        store.authorize_station(post_reset_sta, default_bssid, vec![0x99; 16]);
        assert!(assert_auth_event(&mut rx, post_reset_sta, default_bssid) > prior_epoch);
    }

    use netsim_packets::{
        BeaconFixedFields, DataSubType, FrameControl, FrameType, LlcSnapHeader, MacHeader3Addr,
        SequenceControl, management_subtype,
    };
    use zerocopy::FromBytes;

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
    const TEST_BSSID_3: MacAddress = MacAddress::new([0x02, 0x00, 0x00, 0x00, 0x00, 0x03]);
    const TEST_STA_MAC: MacAddress = MacAddress::new([0x02, 0x00, 0x00, 0x00, 0x00, 0x55]);
    const TEST_STA_MAC_2: MacAddress = MacAddress::new([0x02, 0x00, 0x00, 0x00, 0x00, 0x66]);
    const TEST_HWSIM_MAC: MacAddress = MacAddress::new([0x02, 0x00, 0x00, 0x00, 0x00, 0x77]);
    const TEST_HWSIM_MAC_2: MacAddress = MacAddress::new([0x02, 0x00, 0x00, 0x00, 0x00, 0x88]);
    const TEST_UNASSOCIATED_STA: MacAddress = MacAddress::new([0x02, 0xAA, 0xBB, 0xCC, 0xDD, 0x02]);

    fn dummy_from_ap_frame(bssid: MacAddress, dst: MacAddress, seq: u16, body: &[u8]) -> Ieee80211 {
        let eth = bytes::Bytes::from(create_dummy_ethernet(&bssid.bytes, &dst.bytes, body));
        Ieee80211::from_ieee8023(&eth, bssid, netsim_packets::FrameDirection::FromAp, seq).unwrap()
    }

    use netsim_packets::{
        EthernetFrame,
        FrameDirection::{self, FromAp, ToAp},
        ether_type,
    };

    fn dummy_eapol_to(
        dst: MacAddress,
        src: MacAddress,
        bssid: MacAddress,
        dir: FrameDirection,
    ) -> Ieee80211 {
        let eth = EthernetFrame::new(dst, src, ether_type::EAPOL);
        let mut bytes = [0u8; MIN_ETHERNET_LEN];
        bytes[..eth.as_bytes().len()].copy_from_slice(eth.as_bytes());
        Ieee80211::from_ieee8023(&bytes, bssid, dir, 1).unwrap()
    }

    fn dummy_eapol(src: MacAddress, bssid: MacAddress, dir: FrameDirection) -> Ieee80211 {
        dummy_eapol_to(TEST_STA_MAC, src, bssid, dir)
    }

    fn dummy_from_ap_eapol(src: MacAddress, bssid: MacAddress) -> Ieee80211 {
        dummy_eapol(src, bssid, FromAp)
    }

    fn associated_store() -> SharedKeyStore {
        let store = SharedKeyStore::new();
        store.register_bssid(TEST_BSSID_1, true, false);
        store.add_bssid(TEST_BSSID_2);
        store.associate_station(TEST_STA_MAC, TEST_BSSID_1);
        store
    }

    #[test]
    fn test_remove_station_for_bssid_removes_only_matching_bssid() {
        // Given
        let s = associated_store();
        gate_and_expect_buffered(&s, TEST_BSSID_1, 1, b"queued");
        s.associate_station(TEST_STA_MAC_2, TEST_BSSID_1);
        s.set_hwsim_alias(TEST_HWSIM_MAC, TEST_STA_MAC, TEST_BSSID_1);
        s.set_hwsim_alias(TEST_HWSIM_MAC_2, TEST_STA_MAC_2, TEST_BSSID_1);

        // When
        s.remove_station_for_bssid(&TEST_STA_MAC, &TEST_BSSID_2);

        // Then
        assert_eq!(s.get_station_bssid(&TEST_STA_MAC), Some(TEST_BSSID_1));
        assert_eq!(s.resolve_hwsim_alias(&TEST_HWSIM_MAC), Some(TEST_STA_MAC));

        // When
        s.remove_station_for_bssid(&TEST_STA_MAC, &TEST_BSSID_1);

        // Then
        assert!(!s.has_station(&TEST_STA_MAC));
        assert_eq!(s.get_station_bssid(&TEST_STA_MAC), None);
        assert_eq!(s.buffered_frame_count(&TEST_STA_MAC), 0);
        assert!(matches!(gate(&s, TEST_BSSID_1, b"x"), DownlinkPortAction::Drop));
        assert_eq!(s.resolve_hwsim_alias(&TEST_HWSIM_MAC), None);
        assert_eq!(s.resolve_hwsim_alias(&TEST_HWSIM_MAC_2), Some(TEST_STA_MAC_2));
    }

    #[test]
    fn test_authorize_station_ignores_unassociated_station() {
        // Given
        let store = SharedKeyStore::new();
        store.add_bssid(TEST_BSSID_1);
        let mut rx = with_auth_rx(&store);

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
        assert!(rx.try_recv().is_err());
        assert!(
            store.try_encrypt(&dummy_from_ap_frame(TEST_BSSID_1, TEST_STA_MAC, 1, b"x")).is_none()
        );
    }

    #[test]
    fn test_authorize_station_ignores_mismatched_bssid() {
        // Given
        let store = associated_store();
        let mut rx = with_auth_rx(&store);

        // When
        store.authorize_station(TEST_STA_MAC, TEST_BSSID_2, vec![0x42; CCMP_TK_LEN]);

        // Then
        assert!(rx.try_recv().is_err());
        assert!(store.is_uplink_port_blocked(&TEST_STA_MAC, &TEST_BSSID_1));
        assert!(
            store.try_encrypt(&dummy_from_ap_frame(TEST_BSSID_1, TEST_STA_MAC, 1, b"x")).is_none()
        );

        // When
        store.authorize_station(TEST_STA_MAC, TEST_BSSID_1, vec![0x11; CCMP_TK_LEN]);

        // Then
        assert!(!store.is_uplink_port_blocked(&TEST_STA_MAC, &TEST_BSSID_1));
        assert_auth_event(&mut rx, TEST_STA_MAC, TEST_BSSID_1);
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
        let src = enc.get_source();
        dec.add_session(src, vec![0x42; CCMP_TK_LEN]);

        // Then
        assert!(dec.get_station_bssid(&src).is_none());
        assert!(!dec.is_uplink_port_blocked(&src, &TEST_BSSID_1));
        assert!(dec.try_decrypt(&enc).is_some());
    }

    #[test]
    fn test_set_station_bssid_does_not_clobber_associated_station() {
        // Given
        let s = associated_store();

        // When
        s.set_station_bssid(TEST_STA_MAC_2, TEST_BSSID_1);
        s.set_station_bssid(TEST_STA_MAC_2, TEST_BSSID_2);

        // Then
        assert_eq!(s.get_station_bssid(&TEST_STA_MAC_2), Some(TEST_BSSID_2));
        gate_and_expect_buffered(&s, TEST_BSSID_1, 1, b"queued");

        // When
        s.set_station_bssid(TEST_STA_MAC, TEST_BSSID_2);

        // Then
        assert_eq!(s.get_station_bssid(&TEST_STA_MAC), Some(TEST_BSSID_1));
        assert!(s.is_uplink_port_blocked(&TEST_STA_MAC, &TEST_BSSID_1));
        assert_eq!(s.buffered_frame_count(&TEST_STA_MAC), 1);
    }

    #[test]
    fn test_remove_bssid_clears_stations_gtk_and_aliases() {
        // Given
        let s = SharedKeyStore::new();
        s.register_bssid(TEST_BSSID_1, false, true);
        s.register_bssid(TEST_BSSID_2, true, true);
        s.associate_station(TEST_STA_MAC, TEST_BSSID_2);
        s.authorize_eap_station(TEST_STA_MAC, TEST_BSSID_2, 1);
        s.associate_station(TEST_STA_MAC_2, TEST_BSSID_1);
        s.set_gtk(TEST_BSSID_2, [0x22; 16]);
        s.set_hwsim_alias(TEST_HWSIM_MAC, TEST_STA_MAC, TEST_BSSID_2);
        s.set_hwsim_alias(TEST_HWSIM_MAC_2, TEST_STA_MAC_2, TEST_BSSID_1);

        // When
        s.remove_bssid(&TEST_BSSID_2);

        // Then
        assert!(!s.has_station(&TEST_STA_MAC));
        assert!(!s.is_secure_bssid(&TEST_BSSID_2));
        assert!(!s.requires_controlled_port(&TEST_BSSID_2));
        assert!(s.requires_controlled_port(&TEST_BSSID_1));
        assert!(s.has_station(&TEST_STA_MAC_2));
        assert!(s.get_gtk(&TEST_BSSID_2).is_none());
        assert!(s.take_eap_success_epoch(&TEST_STA_MAC, &TEST_BSSID_2, 1).is_none());
        assert!(s.resolve_hwsim_alias(&TEST_HWSIM_MAC).is_none());
        assert_eq!(s.resolve_hwsim_alias(&TEST_HWSIM_MAC_2), Some(TEST_STA_MAC_2));
    }

    fn aliased_store() -> SharedKeyStore {
        let store = associated_store();
        store.set_hwsim_alias(TEST_HWSIM_MAC, TEST_STA_MAC, TEST_BSSID_1);
        store
    }

    fn staged_store() -> SharedKeyStore {
        let store = bssid_store();
        store.stage_hwsim_alias(TEST_HWSIM_MAC, TEST_STA_MAC, TEST_BSSID_1);
        store
    }

    type Alias = Option<MacAddress>;

    fn assoc_alias(store: &SharedKeyStore, sta: MacAddress, bssid: MacAddress) -> Alias {
        store.associate_station(sta, bssid);
        store.resolve_hwsim_alias(&TEST_HWSIM_MAC)
    }

    fn set_alias(store: &SharedKeyStore, sta: MacAddress, bssid: MacAddress) -> Alias {
        store.set_hwsim_alias(TEST_HWSIM_MAC, sta, bssid);
        store.resolve_hwsim_alias(&TEST_HWSIM_MAC)
    }

    #[test]
    fn test_get_station_bssid_prefers_hwsim_alias_over_stale_direct_entry() {
        let store = aliased_store();
        store.set_station_bssid(TEST_HWSIM_MAC, TEST_BSSID_2);
        assert_eq!(store.get_station_bssid(&TEST_HWSIM_MAC), Some(TEST_BSSID_1));
    }

    #[test]
    fn test_associate_station_clears_stale_hwsim_alias_for_reassociated_sta() {
        assert_eq!(assoc_alias(&aliased_store(), TEST_STA_MAC, TEST_BSSID_2), None);
    }

    #[test]
    fn test_associate_station_clears_stale_hwsim_alias_when_hwsim_addr_associates() {
        let store = aliased_store();
        assert_eq!(assoc_alias(&store, TEST_HWSIM_MAC, TEST_BSSID_2), None);
        assert_eq!(store.get_station_bssid(&TEST_HWSIM_MAC), Some(TEST_BSSID_2));
    }

    #[test]
    fn test_set_hwsim_alias_self_clears_stale_alias_when_unprotected() {
        let store = SharedKeyStore::new();
        store.set_station_bssid(TEST_STA_MAC, TEST_BSSID_1);
        store.set_station_bssid(TEST_HWSIM_MAC, TEST_BSSID_1);
        assert_eq!(set_alias(&store, TEST_STA_MAC, TEST_BSSID_1), Some(TEST_STA_MAC));
        assert_eq!(set_alias(&store, TEST_HWSIM_MAC, TEST_BSSID_1), None);
    }

    #[test]
    fn test_set_hwsim_alias_ignores_self_clear_when_hwsim_not_on_bssid() {
        let store = SharedKeyStore::new();
        store.set_station_bssid(TEST_STA_MAC, TEST_BSSID_1);
        store.set_station_bssid(TEST_HWSIM_MAC, TEST_BSSID_1);
        assert_eq!(set_alias(&store, TEST_STA_MAC, TEST_BSSID_1), Some(TEST_STA_MAC));
        assert_eq!(set_alias(&store, TEST_HWSIM_MAC, TEST_BSSID_2), Some(TEST_STA_MAC));
    }

    #[test]
    fn test_reset_clears_hwsim_alias() {
        let store = aliased_store();
        store.reset_with_mode(TEST_BSSID_1, false, false);
        assert!(store.resolve_hwsim_alias(&TEST_HWSIM_MAC).is_none());
    }

    #[test]
    fn test_set_hwsim_alias_ignores_unassociated_candidate_over_associated_alias() {
        let store = aliased_store();
        store.set_station_bssid(TEST_UNASSOCIATED_STA, TEST_BSSID_2);
        assert_eq!(set_alias(&store, TEST_UNASSOCIATED_STA, TEST_BSSID_2), Some(TEST_STA_MAC));
    }

    #[test]
    fn test_set_hwsim_alias_allows_associated_peer_overwrite_on_own_bssid() {
        let store = aliased_store();
        store.associate_station(TEST_STA_MAC_2, TEST_BSSID_2);
        assert_eq!(set_alias(&store, TEST_STA_MAC_2, TEST_BSSID_2), Some(TEST_STA_MAC_2));
    }

    #[test]
    fn test_set_hwsim_alias_ignores_station_associated_on_other_bssid() {
        let store = aliased_store();
        store.associate_station(TEST_STA_MAC_2, TEST_BSSID_2);
        assert_eq!(set_alias(&store, TEST_STA_MAC_2, TEST_BSSID_1), Some(TEST_STA_MAC));
    }

    #[test]
    fn test_set_hwsim_alias_ignores_self_clear_when_target_managed() {
        let store = aliased_store();
        store.set_station_bssid(TEST_HWSIM_MAC, TEST_BSSID_1);
        assert_eq!(set_alias(&store, TEST_HWSIM_MAC, TEST_BSSID_1), Some(TEST_STA_MAC));
    }

    #[test]
    fn test_stage_hwsim_alias_promotes_once_and_consumes_pending() {
        let store = staged_store();
        assert_eq!(store.resolve_hwsim_alias(&TEST_HWSIM_MAC), None);
        assert_eq!(assoc_alias(&store, TEST_STA_MAC, TEST_BSSID_1), Some(TEST_STA_MAC));
        assert_eq!(assoc_alias(&store, TEST_STA_MAC, TEST_BSSID_1), None);
    }

    #[test]
    fn test_stage_hwsim_alias_mismatched_bssid_consumes_without_promoting() {
        let store = staged_store();
        assert_eq!(assoc_alias(&store, TEST_STA_MAC, TEST_BSSID_2), None);
        assert_eq!(assoc_alias(&store, TEST_STA_MAC, TEST_BSSID_1), None);
    }

    #[test]
    fn test_stage_hwsim_alias_matching_peer_association_replaces_active_alias() {
        let store = aliased_store();
        store.stage_hwsim_alias(TEST_HWSIM_MAC, TEST_STA_MAC_2, TEST_BSSID_2);
        assert_eq!(assoc_alias(&store, TEST_STA_MAC_2, TEST_BSSID_2), Some(TEST_STA_MAC_2));
    }

    #[test]
    fn test_stage_hwsim_alias_self_stage_clears_pending() {
        let store = staged_store();
        store.stage_hwsim_alias(TEST_STA_MAC, TEST_STA_MAC, TEST_BSSID_1);
        assert_eq!(assoc_alias(&store, TEST_STA_MAC, TEST_BSSID_1), None);
    }

    #[test]
    fn test_stage_hwsim_alias_blocked_when_hwsim_addr_managed() {
        let store = staged_store();
        store.associate_station(TEST_HWSIM_MAC, TEST_BSSID_1);
        assert_eq!(assoc_alias(&store, TEST_STA_MAC, TEST_BSSID_1), None);
    }

    #[test]
    fn test_stage_hwsim_alias_cleared_by_remove_station_for_bssid() {
        let store = staged_store();
        store.remove_station_for_bssid(&TEST_STA_MAC, &TEST_BSSID_1);
        assert_eq!(assoc_alias(&store, TEST_STA_MAC, TEST_BSSID_1), None);
    }

    #[test]
    fn test_remove_station_for_bssid_preserves_other_station_pending_alias_on_same_bssid() {
        let store = staged_store();
        store.stage_hwsim_alias(TEST_HWSIM_MAC_2, TEST_STA_MAC_2, TEST_BSSID_1);
        store.remove_station_for_bssid(&TEST_STA_MAC, &TEST_BSSID_1);
        store.associate_station(TEST_STA_MAC_2, TEST_BSSID_1);
        assert_eq!(store.resolve_hwsim_alias(&TEST_HWSIM_MAC_2), Some(TEST_STA_MAC_2));
    }

    #[test]
    fn test_remove_station_for_bssid_preserves_cross_bssid_pending_alias() {
        let store = bssid_store();
        store.associate_station(TEST_STA_MAC, TEST_BSSID_1);
        store.stage_hwsim_alias(TEST_HWSIM_MAC, TEST_STA_MAC, TEST_BSSID_2);
        store.remove_station_for_bssid(&TEST_STA_MAC, &TEST_BSSID_1);
        assert_eq!(assoc_alias(&store, TEST_STA_MAC, TEST_BSSID_2), Some(TEST_STA_MAC));
    }

    #[test]
    fn test_stage_hwsim_alias_cleared_by_remove_bssid() {
        let store = staged_store();
        store.stage_hwsim_alias(TEST_HWSIM_MAC, TEST_STA_MAC_2, TEST_BSSID_2);
        store.remove_bssid(&TEST_BSSID_1);
        assert_eq!(assoc_alias(&store, TEST_STA_MAC, TEST_BSSID_1), None);
        assert_eq!(assoc_alias(&store, TEST_STA_MAC_2, TEST_BSSID_2), Some(TEST_STA_MAC_2));
    }

    #[test]
    fn test_stage_hwsim_alias_cleared_by_reset() {
        let store = staged_store();
        store.reset_with_mode(TEST_BSSID_1, false, false);
        assert_eq!(assoc_alias(&store, TEST_STA_MAC, TEST_BSSID_1), None);
    }

    #[test]
    fn test_set_hwsim_alias_ignores_unknown_sta_when_unmanaged() {
        assert_eq!(set_alias(&bssid_store(), TEST_UNASSOCIATED_STA, TEST_BSSID_1), None);
    }

    #[test]
    fn test_set_hwsim_alias_protects_managed_hwsim_against_associated_peer() {
        let store = bssid_store();
        store.associate_station(TEST_HWSIM_MAC, TEST_BSSID_1);
        store.associate_station(TEST_STA_MAC_2, TEST_BSSID_2);
        assert_eq!(set_alias(&store, TEST_STA_MAC_2, TEST_BSSID_2), None);
        assert_eq!(store.get_station_bssid(&TEST_HWSIM_MAC), Some(TEST_BSSID_1));
    }

    #[test]
    fn test_remove_station_on_hwsim_addr_preserves_active_alias() {
        let store = aliased_store();
        store.set_station_bssid(TEST_HWSIM_MAC, TEST_BSSID_2);
        store.remove_station_for_bssid(&TEST_HWSIM_MAC, &TEST_BSSID_2);
        assert!(!store.has_station(&TEST_HWSIM_MAC));
        assert_eq!(store.resolve_hwsim_alias(&TEST_HWSIM_MAC), Some(TEST_STA_MAC));
        assert_eq!(store.get_station_bssid(&TEST_HWSIM_MAC), Some(TEST_BSSID_1));
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

    fn gate(store: &SharedKeyStore, bssid: MacAddress, payload: &[u8]) -> DownlinkPortAction {
        let frame = dummy_from_ap_frame(bssid, TEST_STA_MAC, 1, payload);
        store.gate_downlink_frame(TEST_STA_MAC, &bssid, &frame)
    }

    fn gate_and_expect_buffered(store: &SharedKeyStore, bssid: MacAddress, seq: u16, body: &[u8]) {
        let frame = dummy_from_ap_frame(bssid, TEST_STA_MAC, seq, body);
        let action = store.gate_downlink_frame(TEST_STA_MAC, &bssid, &frame);
        assert!(matches!(action, DownlinkPortAction::Buffered));
    }

    fn drain(store: &SharedKeyStore, bssid: MacAddress, epoch: u64) -> VecDeque<Ieee80211> {
        store.take_authorized_buffered_frames(&TEST_STA_MAC, &bssid, epoch)
    }

    fn deliver(action: DownlinkPortAction) -> VecDeque<Ieee80211> {
        let DownlinkPortAction::Deliver(frames) = action else { panic!("expected Deliver") };
        frames
    }

    type AuthRx = tokio::sync::mpsc::UnboundedReceiver<PortAuthorizedEvent>;

    fn assert_auth_event(rx: &mut AuthRx, sta_addr: MacAddress, bssid: MacAddress) -> u64 {
        let event = rx.try_recv().expect("port authorized event");
        assert_eq!(event.sta_addr, sta_addr);
        assert_eq!(event.bssid, bssid);
        assert!(event.epoch > 0);
        event.epoch
    }

    fn with_auth_rx(store: &SharedKeyStore) -> AuthRx {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        store.set_port_authorized_sender(tx);
        rx
    }

    fn snap_body(frame: &Ieee80211) -> Vec<u8> {
        LlcSnapHeader::read_from_prefix(&frame.get_payload()).expect("LLC/SNAP header").1.to_vec()
    }

    fn bssid_store() -> SharedKeyStore {
        let store = SharedKeyStore::new();
        store.add_bssid(TEST_BSSID_1);
        store.register_bssid(TEST_BSSID_2, true, false);
        store.register_bssid(TEST_BSSID_3, false, true);
        store
    }

    #[test]
    fn test_uplink_port_isolated_per_bssid() {
        let store = bssid_store();
        assert!(store.is_uplink_port_blocked(&TEST_STA_MAC, &TEST_BSSID_2));
        assert!(!store.is_uplink_port_blocked(&TEST_STA_MAC, &TEST_BSSID_1));
        store.associate_station(TEST_STA_MAC, TEST_BSSID_2);
        store.authorize_eap_station(TEST_STA_MAC, TEST_BSSID_2, 1);
        assert!(!store.is_uplink_port_blocked(&TEST_STA_MAC, &TEST_BSSID_2));
        assert!(store.is_uplink_port_blocked(&TEST_STA_MAC, &TEST_BSSID_1));
    }

    #[test]
    fn test_bssid_security_and_controlled_port_flags() {
        let store = bssid_store();
        let check_bssid = |bssid, is_secure, requires_cp| {
            assert_eq!(store.is_secure_bssid(&bssid), is_secure);
            assert_eq!(store.requires_controlled_port(&bssid), requires_cp);
        };
        check_bssid(TEST_BSSID_1, false, false);
        check_bssid(TEST_BSSID_2, true, true);
        check_bssid(TEST_BSSID_3, false, true);
        check_bssid(MacAddress::new([2, 0, 0, 0, 0, 0xFF]), false, false);
    }

    #[test]
    fn test_register_bssid_downgrade_to_open_clears_flags() {
        let store = bssid_store();
        for bssid in [TEST_BSSID_2, TEST_BSSID_3] {
            store.register_bssid(bssid, false, false);
            assert!(!store.is_secure_bssid(&bssid));
            assert!(!store.requires_controlled_port(&bssid));
        }
    }

    #[test]
    fn test_add_bssid_on_secure_or_enterprise_bssid_clears_flags() {
        let store = bssid_store();
        for bssid in [TEST_BSSID_2, TEST_BSSID_3] {
            store.add_bssid(bssid);
            assert!(!store.is_secure_bssid(&bssid));
            assert!(!store.requires_controlled_port(&bssid));
        }
    }

    #[test]
    fn test_buffered_frame_count_tracks_queued_and_cleared_frames() {
        let store = associated_store();
        assert_eq!(store.buffered_frame_count(&TEST_STA_MAC), 0);
        gate_and_expect_buffered(&store, TEST_BSSID_1, 1, b"one");
        gate_and_expect_buffered(&store, TEST_BSSID_1, 2, b"two");
        assert_eq!(store.buffered_frame_count(&TEST_STA_MAC), 2);
        store.clear_station_buffer(&TEST_STA_MAC);
        assert_eq!(store.buffered_frame_count(&TEST_STA_MAC), 0);
        store.authorize_eap_station(TEST_STA_MAC, TEST_BSSID_1, 1);
        let epoch = store.take_eap_success_epoch(&TEST_STA_MAC, &TEST_BSSID_1, 1).unwrap();
        assert!(drain(&store, TEST_BSSID_1, epoch).is_empty());
    }

    #[test]
    fn test_downlink_gate_drops_unassociated_frames_on_controlled_port_bssid() {
        let store = bssid_store();
        for bssid in [TEST_BSSID_2, TEST_BSSID_3] {
            assert!(matches!(gate(&store, bssid, b"x"), DownlinkPortAction::Drop));
            let eapol = dummy_from_ap_eapol(bssid, bssid);
            let action = store.gate_downlink_frame(TEST_STA_MAC, &bssid, &eapol);
            assert!(matches!(action, DownlinkPortAction::Drop));
        }
    }

    #[test]
    fn test_downlink_gate_drops_unassociated_station_entry_on_controlled_port_bssid() {
        for (is_secure, enterprise) in [(true, false), (false, true)] {
            let store = bssid_store();
            store.set_station_bssid(TEST_STA_MAC, TEST_BSSID_1);
            store.register_bssid(TEST_BSSID_1, is_secure, enterprise);
            assert!(matches!(gate(&store, TEST_BSSID_1, b"x"), DownlinkPortAction::Drop));
            assert_eq!(store.buffered_frame_count(&TEST_STA_MAC), 0);
        }
    }

    #[test]
    fn test_downlink_gate_associated_station_on_other_bssid_drops() {
        let store = associated_store();
        store.add_bssid(TEST_BSSID_3);
        assert!(matches!(gate(&store, TEST_BSSID_2, b"x"), DownlinkPortAction::Drop));
        assert!(matches!(gate(&store, TEST_BSSID_3, b"x"), DownlinkPortAction::Drop));
    }

    #[test]
    fn test_downlink_gate_absent_station_on_open_bssid_delivers() {
        let store = SharedKeyStore::new();
        store.add_bssid(TEST_BSSID_3);
        assert!(deliver(gate(&store, TEST_BSSID_3, b"absent-open")).is_empty());
    }

    #[test]
    fn test_downlink_gate_unassociated_candidate_on_open_bssid_delivers() {
        let store = SharedKeyStore::new();
        store.add_bssid(TEST_BSSID_2);
        store.add_bssid(TEST_BSSID_3);
        store.set_station_bssid(TEST_STA_MAC, TEST_BSSID_2);
        assert!(store.has_station(&TEST_STA_MAC));
        assert!(deliver(gate(&store, TEST_BSSID_3, b"open")).is_empty());
    }

    #[test]
    fn test_take_authorized_buffered_frames_returns_empty_while_blocked() {
        let store = associated_store();
        gate_and_expect_buffered(&store, TEST_BSSID_1, 1, b"blocked");
        let epoch = store.state.read().stations[&TEST_STA_MAC].epoch;
        assert!(drain(&store, TEST_BSSID_1, epoch).is_empty());
        assert_eq!(store.buffered_frame_count(&TEST_STA_MAC), 1);
    }

    #[test]
    fn test_downlink_gate_buffers_bounded_fifo_evicting_oldest() {
        let store = associated_store();
        let total = MAX_BUFFERED_FRAMES_PER_STATION as u16 + 5;
        for seq in 0..total {
            gate_and_expect_buffered(&store, TEST_BSSID_1, seq, &seq.to_be_bytes());
        }
        assert_eq!(store.buffered_frame_count(&TEST_STA_MAC), MAX_BUFFERED_FRAMES_PER_STATION);
        store.authorize_eap_station(TEST_STA_MAC, TEST_BSSID_1, 1);
        let epoch = store.take_eap_success_epoch(&TEST_STA_MAC, &TEST_BSSID_1, 1).unwrap();
        let to_u16 =
            |frame: Ieee80211| u16::from_be_bytes(snap_body(&frame)[..2].try_into().unwrap());
        let drained = drain(&store, TEST_BSSID_1, epoch);
        assert!(drained.into_iter().map(to_u16).eq(5..total));
        assert_eq!(store.buffered_frame_count(&TEST_STA_MAC), 0);
    }

    #[test]
    fn test_downlink_gate_deliver_drains_pending_frames_once_authorized() {
        let store = associated_store();
        gate_and_expect_buffered(&store, TEST_BSSID_1, 1, b"first");
        store.authorize_station(TEST_STA_MAC, TEST_BSSID_1, vec![0x11; CCMP_TK_LEN]);
        let drained = deliver(gate(&store, TEST_BSSID_1, b"second"));
        assert_eq!(drained.len(), 1);
        assert!(snap_body(&drained[0]).starts_with(b"first"));
    }

    #[test]
    fn test_downlink_gate_buffers_after_eap_success_until_flush_drains() {
        let store = associated_store();
        let mut rx = with_auth_rx(&store);
        gate_and_expect_buffered(&store, TEST_BSSID_1, 1, b"early");
        store.authorize_eap_station(TEST_STA_MAC, TEST_BSSID_1, 1);
        assert!(rx.try_recv().is_err());
        let pre_tx_epoch = store.state.read().stations[&TEST_STA_MAC].epoch;
        assert!(drain(&store, TEST_BSSID_1, pre_tx_epoch).is_empty());
        gate_and_expect_buffered(&store, TEST_BSSID_1, 2, b"during");
        let epoch = store.take_eap_success_epoch(&TEST_STA_MAC, &TEST_BSSID_1, 1).unwrap();
        gate_and_expect_buffered(&store, TEST_BSSID_1, 3, b"ready");
        let drained = drain(&store, TEST_BSSID_1, epoch);
        assert_eq!(drained.len(), 3);
        assert!(snap_body(&drained[0]).starts_with(b"early"));
        assert!(snap_body(&drained[1]).starts_with(b"during"));
        assert!(snap_body(&drained[2]).starts_with(b"ready"));
        assert!(deliver(gate(&store, TEST_BSSID_1, b"after")).is_empty());
    }

    #[test]
    fn test_take_authorized_buffered_frames_ignores_stale_epoch_after_roam() {
        let store = associated_store();
        store.authorize_station(TEST_STA_MAC, TEST_BSSID_1, vec![0x11; CCMP_TK_LEN]);
        let prior_epoch = store.state.read().stations[&TEST_STA_MAC].epoch;
        store.register_bssid(TEST_BSSID_3, false, true);
        store.associate_station(TEST_STA_MAC, TEST_BSSID_3);
        gate_and_expect_buffered(&store, TEST_BSSID_3, 1, b"ent");
        store.authorize_eap_station(TEST_STA_MAC, TEST_BSSID_3, 10);
        assert!(drain(&store, TEST_BSSID_1, prior_epoch).is_empty());
        assert!(drain(&store, TEST_BSSID_3, prior_epoch).is_empty());
        gate_and_expect_buffered(&store, TEST_BSSID_3, 2, b"still-buffered");
        let eap_epoch = store.take_eap_success_epoch(&TEST_STA_MAC, &TEST_BSSID_3, 10).unwrap();
        assert_eq!(drain(&store, TEST_BSSID_3, eap_epoch).len(), 2);
    }

    #[test]
    fn test_remove_station_for_bssid_clears_eap_flush_state() {
        let store = associated_store();
        store.authorize_eap_station(TEST_STA_MAC, TEST_BSSID_1, 10);
        store.remove_station_for_bssid(&TEST_STA_MAC, &TEST_BSSID_1);
        assert!(store.take_eap_success_epoch(&TEST_STA_MAC, &TEST_BSSID_1, 10).is_none());
    }

    #[test]
    fn test_reassociation_clears_stale_eap_seq_ctrl() {
        let store = associated_store();
        store.authorize_eap_station(TEST_STA_MAC, TEST_BSSID_1, 10);
        store.associate_station(TEST_STA_MAC, TEST_BSSID_1);
        store.authorize_eap_station(TEST_STA_MAC, TEST_BSSID_1, 20);
        assert!(store.take_eap_success_epoch(&TEST_STA_MAC, &TEST_BSSID_1, 10).is_none());
    }

    #[test]
    fn test_reassociation_allows_seq_ctrl_reuse_without_wedging_downlink() {
        let store = associated_store();
        store.authorize_eap_station(TEST_STA_MAC, TEST_BSSID_1, 10);
        store.associate_station(TEST_STA_MAC, TEST_BSSID_1);
        gate_and_expect_buffered(&store, TEST_BSSID_1, 1, b"reused");
        store.authorize_eap_station(TEST_STA_MAC, TEST_BSSID_1, 10);
        let epoch = store.take_eap_success_epoch(&TEST_STA_MAC, &TEST_BSSID_1, 10).unwrap();
        let drained = drain(&store, TEST_BSSID_1, epoch);
        assert_eq!(drained.len(), 1);
        assert!(snap_body(&drained[0]).starts_with(b"reused"));
        assert!(deliver(gate(&store, TEST_BSSID_1, b"after")).is_empty());
    }

    #[test]
    fn test_take_eap_success_epoch_is_one_shot_per_association() {
        let store = associated_store();
        store.authorize_eap_station(TEST_STA_MAC, TEST_BSSID_1, 10);
        assert!(store.take_eap_success_epoch(&TEST_STA_MAC, &TEST_BSSID_1, 10).is_some());
        assert!(store.take_eap_success_epoch(&TEST_STA_MAC, &TEST_BSSID_1, 10).is_none());
    }

    #[test]
    fn test_downlink_gate_delivers_eapol_and_mgmt_while_preserving_blocked_data_buffer() {
        let store = associated_store();
        let bssid = TEST_BSSID_1;
        gate_and_expect_buffered(&store, bssid, 1, b"blocked");
        let eapol = dummy_from_ap_eapol(bssid, bssid);
        let frame_ctrl = FrameControl::new((management_subtype::PROBE_RESPONSE as u16) << 4);
        let header =
            MacHeader3Addr::new(frame_ctrl, 0, TEST_STA_MAC, bssid, bssid, SequenceControl::new(3));
        let mgmt = Ieee80211::decode(header.as_bytes()).unwrap();
        assert!(eapol.is_eapol().unwrap_or(false));
        assert!(!mgmt.is_payload_bearing_data());
        assert!(deliver(store.gate_downlink_frame(TEST_STA_MAC, &bssid, &eapol)).is_empty());
        assert!(deliver(store.gate_downlink_frame(TEST_STA_MAC, &bssid, &mgmt)).is_empty());
        store.authorize_station(TEST_STA_MAC, bssid, vec![0x11; CCMP_TK_LEN]);
        let drained = deliver(gate(&store, bssid, b"after"));
        assert_eq!(drained.len(), 1);
        assert!(snap_body(&drained[0]).starts_with(b"blocked"));
    }

    fn assert_non_authentic_eapol_buffered(frame: &Ieee80211) {
        assert!(frame.is_eapol().unwrap_or(false));
        let store = associated_store();
        let action = store.gate_downlink_frame(TEST_STA_MAC, &TEST_BSSID_1, frame);
        assert!(matches!(action, DownlinkPortAction::Buffered));
        assert_eq!(store.buffered_frame_count(&TEST_STA_MAC), 1);
    }

    #[test]
    fn test_downlink_gate_buffers_non_authentic_eapol_while_blocked() {
        let mut raw = dummy_from_ap_eapol(TEST_BSSID_1, TEST_BSSID_1).as_bytes().to_vec();
        raw[1] |= FC_PROTECTED_BIT;
        let protected_eapol = Ieee80211::decode(&raw).unwrap();
        for frame in [
            dummy_eapol(TEST_STA_MAC_2, TEST_BSSID_1, FromAp),
            dummy_eapol(TEST_BSSID_1, TEST_BSSID_1, ToAp),
            dummy_eapol(TEST_BSSID_1, TEST_BSSID_2, FromAp),
            protected_eapol,
        ] {
            assert_non_authentic_eapol_buffered(&frame);
        }
    }

    #[test]
    fn test_downlink_gate_buffers_wrong_destination_eapol_while_blocked() {
        for dst in [TEST_STA_MAC_2, MacAddress::BROADCAST] {
            let frame = dummy_eapol_to(dst, TEST_BSSID_1, TEST_BSSID_1, FromAp);
            assert_non_authentic_eapol_buffered(&frame);
        }
    }

    #[test]
    fn test_set_station_bssid_promotes_existing_session_to_authorized() {
        let store = bssid_store();
        let mut rx = with_auth_rx(&store);
        store.add_session(TEST_STA_MAC, vec![0x42; CCMP_TK_LEN]);
        assert!(rx.try_recv().is_err());
        store.set_station_bssid(TEST_STA_MAC, TEST_BSSID_2);
        assert_auth_event(&mut rx, TEST_STA_MAC, TEST_BSSID_2);
        assert!(!store.is_uplink_port_blocked(&TEST_STA_MAC, &TEST_BSSID_2));
        assert!(deliver(gate(&store, TEST_BSSID_2, b"ok")).is_empty());
    }
}
