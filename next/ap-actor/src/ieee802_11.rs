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

use actor_framework::DynContext;
use netsim_model::{ChipId, WifiMode};
use netsim_packets::{
    AssociationResponseFixedFields, AuthenticationFixedFields, BeaconFixedFields,
    BeaconFrameHeader, FrameControl, IeIterator, Ieee80211, LlcSnapHeader, MacHeader3Addr,
    control_field, management_subtype, sap, tags, write_ie, write_wmm_param_element,
};
use tracing::{debug, error, info, warn};
use zerocopy::{FromBytes, IntoBytes, U16};

use crate::{ApActor, ApError, ApState, sae::SaeStateMachine, shared::SharedKeyStore};

/// Handles 802.11 Management Frames
#[derive(Clone, Debug)]
pub(crate) struct Ieee80211Manager {
    // Stateless for now, state passed in methods
}

impl Ieee80211Manager {
    pub(crate) fn new() -> Self {
        Self {}
    }

    pub(crate) fn generate_beacon(
        &self,
        ap: &mut ApState,
        beacon_interval: u16,
    ) -> Result<Vec<bytes::Bytes>, ApError> {
        let frame = self.build_beacon_or_probe_resp(
            ap,
            FrameControl::new(0x0080), /* Mgmt (00), Beacon (1000) -> 0x0080 (LE: 80 00) */
            netsim_packets::MacAddr { bytes: [0xFF; 6] },
            beacon_interval,
        );
        Ok(vec![bytes::Bytes::from(frame)])
    }

    fn build_beacon_or_probe_resp(
        &self,
        ap: &mut ApState,
        frame_control: FrameControl,
        dest: netsim_packets::MacAddr,
        beacon_interval: u16,
    ) -> Vec<u8> {
        let header = BeaconFrameHeader {
            frame_control,
            duration: U16::new(0),
            da: dest,
            sa: ap.config.bssid,
            bssid: ap.config.bssid,
            sequence_control: ap.next_seq_control(),
        };

        let mut frame = header.as_bytes().to_vec();

        // Fixed Fields
        let mut caps = 0x0401; // ESS | Short Slot Time
        if ap.config.wpa_passphrase.is_some() || ap.config.enterprise_enabled {
            caps |= 0x0010; // Privacy
        }
        let fixed = BeaconFixedFields {
            timestamp: [0; 8],
            beacon_interval: U16::new(beacon_interval),
            capabilities: U16::new(caps),
        };
        frame.extend_from_slice(fixed.as_bytes());

        // IEs
        self.append_beacon_ies(&mut frame, ap);

        frame
    }

    fn append_beacon_ies(&self, body: &mut Vec<u8>, ap: &ApState) {
        // SSID IE
        if ap.config.hidden_ssid {
            write_ie(body, tags::SSID, &[]);
        } else {
            write_ie(body, tags::SSID, ap.config.ssid.as_bytes());
        }

        // Supported Rates
        write_ie(body, tags::SUPPORTED_RATES, tags::SUPPORTED_RATES_DEFAULT);

        // DS Param (Channel)
        write_ie(body, tags::DS_PARAMETER_SET, &[ap.config.channel]);

        // Country IE (Tag 7)
        if let Some(cc) = &ap.config.country_code
            && cc.len() >= 2
        {
            let mut country_body = Vec::new();
            country_body.extend_from_slice(cc.as_bytes());
            // First Channel Number, Number of Channels, Max Transmit Power Level
            // Simple default: Start at 1, cover 13 channels, Max Power 20dBm
            country_body.extend_from_slice(&[1, 13, 20]);
            write_ie(body, tags::COUNTRY, &country_body);
        }

        // TIM IE (Tag 5)
        // DTIM Count (0), DTIM Period (from config), Bitmap Control (0), Partial
        // Virtual Bitmap (0) For now, we claim DTIM count is always 0 (every
        // beacon is DTIM) or just follow period? Let's set DTIM Count = 0
        // (implying this beacon is a DTIM) for simplicity in simulation. Bitmap
        // Control = 0 (No Multicast buffered). Partial Virtual Bitmap = 0 (No unicast
        // buffered).
        write_ie(body, tags::TIM, &[0, ap.config.dtim_period, 0, 0]);

        // RSN IE (WPA2)
        let rsn_ie = crate::rsn::build_rsn_ie(&ap.config);
        if !rsn_ie.is_empty() {
            body.extend_from_slice(&rsn_ie);
        }

        // WiFi 6 (HE) Support
        if ap.config.hw_mode == WifiMode::Ax {
            // HE Capabilities (ID 255, ExtID 35)
            // Body: ExtID(35) + Caps(00 00)
            write_ie(body, tags::EXTENSION, &[tags::HE_CAPABILITIES, 0x00, 0x00]);
        }

        // WMM IE
        // If 802.11n/ac/ax (HT/VHT/HE) is enabled, WMM is typically mandatory.
        // We also check wmm_enabled config.
        let is_ht = ap.config.hw_mode == WifiMode::N
            || ap.config.hw_mode == WifiMode::Ac
            || ap.config.hw_mode == WifiMode::Ax;
        if ap.config.wmm_enabled || is_ht {
            // U-APSD enabled? Default false for now. Param Set Count 0.
            write_wmm_param_element(body, false, 0);
        }

        // Extended Capabilities (Tag 127)
        if ap.config.ftm_responder_enabled {
            let mut ext_cap = Vec::new();
            netsim_packets::set_ext_cap(
                &mut ext_cap,
                netsim_packets::EXTENDED_CAPABILITIES_FTM_RESPONDER_BIT,
            );
            write_ie(body, tags::EXTENDED_CAPABILITIES, &ext_cap);
        }
    }

    /// Handles an incoming Management Frame
    pub(crate) fn handle_frame(
        &mut self,
        ap: &mut ApState,
        frame: &[u8],
        shared_keys: &SharedKeyStore,
        beacon_interval: u16,
        source_id: ChipId,
        _ctx: &mut DynContext<ApActor>,
    ) -> Result<Vec<bytes::Bytes>, ApError> {
        let ieee80211_frame = match Ieee80211::decode(frame) {
            Ok(f) => f,
            Err(error) => {
                warn!(%error, "ApActor: Failed to decode 802.11 frame");
                return Ok(vec![]);
            }
        };

        debug!(
            "ApActor: Handling frame subtype={:?} src={}",
            ieee80211_frame.stype(),
            ieee80211_frame.get_source()
        );

        if !ieee80211_frame.is_mgmt() {
            return self.handle_data_frame(ap, frame, ieee80211_frame, shared_keys);
        }

        match ieee80211_frame.stype() {
            management_subtype::AUTHENTICATION => {
                self.handle_auth(ap, &ieee80211_frame, frame, shared_keys)
            }
            management_subtype::ASSOCIATION_REQUEST => {
                self.handle_assoc(ap, &ieee80211_frame, shared_keys, source_id)
            }
            management_subtype::PROBE_REQUEST => {
                self.handle_probe_req(ap, &ieee80211_frame, frame, beacon_interval)
            }
            management_subtype::DEAUTHENTICATION => {
                self.handle_deauth(ap, &ieee80211_frame, shared_keys)
            }
            management_subtype::ACTION => self.handle_action(ap, &ieee80211_frame, frame),
            _ => Ok(vec![]),
        }
    }

    fn handle_action(
        &mut self,
        _ap: &mut ApState,
        frame: &Ieee80211,
        raw_frame: &[u8],
    ) -> Result<Vec<bytes::Bytes>, ApError> {
        let src = frame.get_source();
        // Action Frame Body starts after Header (24 bytes)
        let Some(&[category, action, ..]) = raw_frame.get(24..) else {
            return Ok(vec![]);
        };

        debug!("ApActor: Action Frame Cat={} Act={} from {}", category, action, src);

        // Public Action (Category 4)
        if category == netsim_packets::category::PUBLIC {
            // FTM Request (Action 32)
            if action == netsim_packets::public_action::FTM_REQUEST {
                info!("ApActor: Received FTM Request from {}", src);
                // FIXME: Parse Dialog Token from action frame body (Trigger field).
                // For now, we assume a standard trigger and generate a fixed response sequence.
                let frames = crate::ftm::FtmResponder::handle_ftm_request(_ap, src, 1);
                return Ok(frames.into_iter().map(bytes::Bytes::from).collect());
            }
        }

        Ok(vec![])
    }

    fn handle_auth(
        &mut self,
        ap: &mut ApState,
        frame: &Ieee80211,
        raw_frame: &[u8],
        shared_keys: &SharedKeyStore,
    ) -> Result<Vec<bytes::Bytes>, ApError> {
        let src = frame.get_source();

        // ACL Check
        // Mode 0: Disable, 1: Deny, 2: Allow
        match ap.config.mac_acl_mode {
            1 if ap.config.mac_acl_list.contains(&src) => {
                // Deny List
                info!("ApActor: ACL Deny {}", src);
                return Ok(vec![bytes::Bytes::from(self.build_auth_frame(ap, src, 0, 2, 1, &[]))]);
            }
            2 if !ap.config.mac_acl_list.contains(&src) => {
                // Allow List
                info!("ApActor: ACL Reject (Not Allowed) {}", src);
                return Ok(vec![bytes::Bytes::from(self.build_auth_frame(ap, src, 0, 2, 1, &[]))]);
            }
            _ => {}
        }

        // Parse Auth Fixed Fields (Alg, Seq, Status)
        // Mgmt Header is usually 24 bytes (FC+Duration+3Addr+SC)
        let Some(body) = raw_frame.get(24..) else {
            warn!("ApActor: Auth frame too short");
            return Ok(vec![]);
        };

        let fixed = match AuthenticationFixedFields::read_from_prefix(body) {
            Ok((f, _)) => f,
            Err(_) => {
                warn!("ApActor: Auth frame fixed fields invalid");
                return Ok(vec![]);
            }
        };
        let alg = fixed.algorithm.get();
        let seq = fixed.sequence.get();
        let status = fixed.status.get();

        info!("ApActor: Received Auth from {} Alg={} Seq={} Status={}", src, alg, seq, status);

        if seq == 1 {
            // An Auth frame with Sequence 1 indicates the station is starting the
            // connection process. We must clear any stale association and
            // session state (e.g., from a previous connection that disconnected
            // without sending a Deauth frame).
            ap.clear_station_state(&src);
            shared_keys.remove_session(&src);
        }

        // SAE (Algorithm 3)
        if alg == 3 {
            if !ap.config.sae {
                warn!("ApActor: SAE requested but not enabled");
                // Should return Auth reject? (Status 13 - not supported alg?)
                // For now, ignore or send error.
                return Ok(vec![]);
            }

            // Get or Create SAE Machine
            let machine = ap.sae_sessions.entry(src).or_insert_with(|| {
                // Password needed. Use config WPA passphrase.
                let pwd = ap.config.wpa_passphrase.clone().unwrap_or_default();
                SaeStateMachine::new(&ap.config.bssid.bytes, &src.bytes, pwd.as_bytes())
            });

            // Handle SAE State
            // Seq 1: Commit (Peer -> AP) or (Simultaneous)
            // Seq 2: Confirm?
            // SAE uses implicit sequence based on content?
            // 802.11-2016 12.4.8.2: SAE Auth frames use Seq 1 for Commit, Seq 2 for
            // Confirm. SAE Standard: Commit is Seq 1, Confirm is Seq 2.

            let Some(sae_payload) = body.get(6..) else {
                warn!("ApActor: Auth frame missing SAE payload");
                return Ok(vec![]);
            };

            let mut resp_frames = Vec::new();

            if seq == 1 {
                // Peer Commit
                if machine.parse_commit(sae_payload).is_some() {
                    // Generate Our Commit (if not already sent?)
                    // AP usually responds with Commit (Seq 1) containing its scalar/element.
                    // Note: Concurrent commit is allowed by SAE spec.
                    // Or responding to commit.

                    if let Some(commit_body) = machine.build_commit() {
                        let resp = self.build_auth_frame(ap, src, 3, 1, 0, &commit_body);
                        resp_frames.push(bytes::Bytes::from(resp));
                    }
                }
            } else if seq == 2 {
                // Peer Confirm
                if machine.parse_confirm(sae_payload).is_some() {
                    // Generate Our Confirm
                    if let Some(confirm_body) = machine.build_confirm() {
                        let resp = self.build_auth_frame(ap, src, 3, 2, 0, &confirm_body);
                        resp_frames.push(bytes::Bytes::from(resp));

                        // SAE Success! Install keys logic?
                        // Valid confirmed state.
                        // PMK is ready.
                        // Wait for Association Request to derive PTK?
                        info!("SAE: Handshake completed for {}", src);
                    }
                }
            }

            return Ok(resp_frames);
        }

        // Open System (Algorithm 0)
        if alg == 0 {
            // ... existing open system logic ...
            // Copied from previous impl
            let mut resp = Vec::new();
            // 802.11 Header
            let header = MacHeader3Addr {
                frame_control: FrameControl::new(0x00B0), // Mgmt(00), Auth(1011) -> 0x00B0
                duration_id: U16::new(0),
                addr1: src,             // DA
                addr2: ap.config.bssid, // SA
                addr3: ap.config.bssid, // BSSID
                sequence_control: ap.next_seq_control(),
            };
            resp.extend_from_slice(header.as_bytes());

            // Auth Body
            let body = AuthenticationFixedFields {
                algorithm: U16::new(0), // Open System
                sequence: U16::new(2),
                status: U16::new(0), // Success
            };
            resp.extend_from_slice(body.as_bytes());

            let msg = bytes::Bytes::from(resp);
            Ok(vec![msg])
        } else {
            Ok(vec![])
        }
    }

    fn build_auth_frame(
        &self,
        ap: &mut ApState,
        dest: netsim_packets::MacAddr,
        alg: u16,
        seq: u16,
        status: u16,
        payload: &[u8],
    ) -> Vec<u8> {
        let mut frame = Vec::new();
        let header = MacHeader3Addr {
            frame_control: FrameControl::new(0x00B0),
            duration_id: U16::new(0),
            addr1: dest,
            addr2: ap.config.bssid,
            addr3: ap.config.bssid,
            sequence_control: ap.next_seq_control(),
        };
        frame.extend_from_slice(header.as_bytes());

        let fixed = AuthenticationFixedFields {
            algorithm: U16::new(alg),
            sequence: U16::new(seq),
            status: U16::new(status),
        };
        frame.extend_from_slice(fixed.as_bytes());
        frame.extend_from_slice(payload);
        frame
    }

    pub(crate) fn build_deauth_frame(
        &self,
        ap: &mut ApState,
        dest: netsim_packets::MacAddr,
        reason_code: u16,
    ) -> Vec<u8> {
        let mut frame = Vec::new();
        // Deauthentication (Subtype 12 = 1100b) -> 0xC0
        let header = MacHeader3Addr {
            frame_control: FrameControl::new(0x00C0),
            duration_id: U16::new(0),
            addr1: dest,            // DA
            addr2: ap.config.bssid, // SA
            addr3: ap.config.bssid, // BSSID
            sequence_control: ap.next_seq_control(),
        };
        frame.extend_from_slice(header.as_bytes());
        frame.extend_from_slice(&reason_code.to_le_bytes());
        frame
    }

    fn build_assoc_resp(
        &self,
        ap: &mut ApState,
        dest: netsim_packets::MacAddr,
        status: u16,
    ) -> Vec<u8> {
        let mut resp = Vec::new();
        let header = MacHeader3Addr {
            frame_control: FrameControl::new(0x0010), // Mgmt, Assoc Resp
            duration_id: U16::new(0),
            addr1: dest,
            addr2: ap.config.bssid,
            addr3: ap.config.bssid,
            sequence_control: ap.next_seq_control(),
        };
        resp.extend_from_slice(header.as_bytes());

        let body = AssociationResponseFixedFields {
            capabilities: U16::new(0x0001), // ESS
            status: U16::new(status),
            aid: U16::new(0xC001),
        };
        resp.extend_from_slice(body.as_bytes());

        // Only append IEs on Success (Status 0)
        if status == 0 {
            write_ie(&mut resp, tags::SUPPORTED_RATES, tags::SUPPORTED_RATES_DEFAULT);
            if ap.config.wmm_enabled
                || ap.config.hw_mode == WifiMode::N
                || ap.config.hw_mode == WifiMode::Ax
            {
                write_wmm_param_element(&mut resp, false, 0);
            }
        }
        resp
    }

    fn handle_assoc(
        &mut self,
        ap: &mut ApState,
        frame: &Ieee80211,
        shared_keys: &SharedKeyStore,
        _source_id: ChipId,
    ) -> Result<Vec<bytes::Bytes>, ApError> {
        let src = frame.get_source();
        info!("ApActor: Received Assoc Req from {}", src);

        // ACL Check
        match ap.config.mac_acl_mode {
            1 if ap.config.mac_acl_list.contains(&src) => {
                info!("ApActor: ACL Deny Assoc {}", src);
                let resp = self.build_assoc_resp(ap, src, 1);
                return Ok(vec![bytes::Bytes::from(resp)]);
            }
            2 if !ap.config.mac_acl_list.contains(&src) => {
                info!("ApActor: ACL Reject Assoc (Not Allowed) {}", src);
                let resp = self.build_assoc_resp(ap, src, 1);
                return Ok(vec![bytes::Bytes::from(resp)]);
            }
            _ => {}
        }

        let mut msgs = Vec::new();
        let resp = self.build_assoc_resp(ap, src, 0);
        msgs.push(bytes::Bytes::from(resp));

        // Init WPA if configured
        if let Some(passphrase) = &ap.config.wpa_passphrase {
            let rsn_ie = crate::rsn::build_rsn_ie(&ap.config);
            let bssid = ap.config.bssid;
            let global_gtk = shared_keys.get_gtk(&bssid).unwrap_or_else(|| {
                let gtk_bytes = crate::ffi::RandBytes(16);
                let mut new_gtk = [0u8; 16];
                new_gtk.copy_from_slice(&gtk_bytes);
                shared_keys.set_gtk(bssid, new_gtk);
                new_gtk
            });

            let mut authenticator = crate::wpa_auth::WpaAuthenticator::new(
                ap.config.bssid,
                src,
                ap.config.ssid.as_bytes(),
                passphrase.as_bytes(),
                &rsn_ie,
                global_gtk,
            );

            if let Ok(m1) = authenticator.initiate_handshake() {
                let m1_frame = self.wrap_eapol(ap, src, &m1);
                ap.wpa = Some(authenticator);

                // 150ms buffer prevents EAPOL M1 from racing via AF_PACKET against the
                // asynchronous Netlink Association event. Without it,
                // wpa_supplicant drops M1 before reaching ASSOCIATED state.
                let delay = std::time::Instant::now() + std::time::Duration::from_millis(150);
                ap.delayed_frames.push_back((delay, bytes::Bytes::from(m1_frame)));
                info!("ApActor: Triggered 150ms buffer for EAPOL M1 handshake to {}", src);
            }
        }

        // Track Association
        ap.associations.insert(src);
        info!("ApActor: Associated {}", src);

        Ok(msgs)
    }

    fn handle_probe_req(
        &mut self,
        ap: &mut ApState,
        frame: &Ieee80211,
        raw_frame: &[u8],
        beacon_interval: u16,
    ) -> Result<Vec<bytes::Bytes>, ApError> {
        // Filter by Destination Address (DA)
        // Must be Broadcast (FF:...) or match our BSSID.
        let da = frame.get_destination();
        if !da.is_broadcast() && da != ap.config.bssid {
            return Ok(vec![]);
        }

        // Parse SSID from Probe Req
        // Frame: Header (24) + IEs.
        // Ieee80211 doesn't have `payload()`, but `decode` validates it.
        // We'll operate on `raw_frame` slice for IE parsing. Header is usually 24 bytes
        // for Mgmt.
        let Some(ies) = raw_frame.get(24..) else {
            return Ok(vec![]);
        };

        let mut requested_ssid = None;

        // Use IeIterator for safe parsing
        for ie in IeIterator::new(ies) {
            if ie.id == tags::SSID {
                let val = String::from_utf8_lossy(ie.body);
                requested_ssid = Some(val.to_string());
                break; // Found it
            }
        }

        // Filter: Respond if SSID matches or is Wildcard (empty)
        let respond = match requested_ssid {
            Some(s) if s.is_empty() => !ap.config.hidden_ssid, // Wildcard
            Some(s) if s == ap.config.ssid => true,            // Direct Match
            None => !ap.config.hidden_ssid,                    // No SSID IE? Assume wildcard
            _ => false,                                        // Mismatch
        };

        if !respond {
            return Ok(vec![]);
        }

        let src = frame.get_source();

        let resp = self.build_beacon_or_probe_resp(
            ap,
            FrameControl::new(0x0050), // Mgmt(00), ProbeResp(0101) -> 0x0050
            src,
            beacon_interval,
        );

        Ok(vec![bytes::Bytes::from(resp)])
    }

    fn handle_deauth(
        &self,
        ap: &mut ApState,
        frame: &Ieee80211,
        shared_keys: &SharedKeyStore,
    ) -> Result<Vec<bytes::Bytes>, ApError> {
        let src = frame.get_source();
        info!("ApActor: Received Deauth from {}", src);

        // Check if we have a session for this source
        if let Some(_wpa) = &ap.wpa {
            // Since ApState currently supports only a single session/authenticator,
            // we clear it indiscriminately. Future multi-station support will need keyed
            // lookup.
            ap.wpa = None;
        }

        shared_keys.remove_session(&src);
        ap.associations.remove(&src);

        Ok(vec![])
    }

    // Helper to wrap EAPOL in Data Frame
    fn wrap_eapol(
        &self,
        ap: &mut ApState,
        dest: netsim_packets::MacAddr,
        payload: &[u8],
    ) -> Vec<u8> {
        let mut frame = Vec::new();

        // 802.11 Header
        // ToDS=0, FromDS=1 (AP to STA) -> FC 0x0208
        // Type=Data(10) Subtype=Data(0000)
        let header = MacHeader3Addr {
            frame_control: FrameControl::new(0x0208),
            duration_id: U16::new(0),
            addr1: dest,            // DA
            addr2: ap.config.bssid, // BSSID
            addr3: ap.config.bssid, // SA
            sequence_control: ap.next_seq_control(),
        };
        frame.extend_from_slice(header.as_bytes());

        // LLC Header
        let llc = LlcSnapHeader::new(
            sap::SNAP,
            sap::SNAP,
            control_field::UI,
            [0x00, 0x00, 0x00],
            netsim_packets::ether_type::EAPOL,
        );
        frame.extend_from_slice(llc.as_bytes());

        frame.extend_from_slice(payload);
        frame
    }

    fn handle_data_frame(
        &self,
        ap: &mut ApState,
        frame: &[u8],
        ieee80211_frame: Ieee80211,
        shared_keys: &SharedKeyStore,
    ) -> Result<Vec<bytes::Bytes>, ApError> {
        let dump = frame.get(..48).unwrap_or(frame);
        warn!(
            "ApActor: Data Frame Debug [{} bytes] (stype: {:?}) hex: {:02x?}",
            frame.len(),
            ieee80211_frame.stype(),
            dump
        );

        if !ieee80211_frame.is_eapol().unwrap_or(false) {
            return Ok(vec![]);
        }

        info!("ApActor: Received EAPOL frame from src={}", ieee80211_frame.get_source());
        let frame_payload = ieee80211_frame.get_payload();
        let Some(payload) = frame_payload.get(8..) else {
            return Ok(vec![]);
        };

        let &[_, eapol_type, _, _, ref eap_packet @ ..] = payload else {
            return Ok(vec![]);
        };

        // EAPOL-Start (Type 1)
        if eapol_type == 1 {
            if !ap.config.enterprise_enabled {
                warn!("ApActor: Received EAPOL-Start but enterprise not enabled");
                return Ok(vec![]);
            }
            // Start EAP
            let mut auth = crate::eap_auth::EapAuthenticator::new(
                ap.config.bssid,
                ieee80211_frame.get_source(),
            );
            match auth.start() {
                Ok(outputs) => {
                    let mut frames = Vec::new();
                    for out in outputs {
                        if let crate::eap_auth::EapOutput::Frame(data) = out {
                            frames.push(bytes::Bytes::from(self.wrap_eapol(
                                ap,
                                ieee80211_frame.get_source(),
                                &data,
                            )));
                        }
                    }
                    ap.eap_sessions.insert(ieee80211_frame.get_source(), auth);
                    return Ok(frames);
                }
                Err(error) => {
                    error!(?error, "ApActor: Failed to start EAP");
                    return Ok(vec![]);
                }
            }
        }

        // EAP-Packet (Type 0)
        if eapol_type == 0 {
            if let Some(auth) = ap.eap_sessions.get_mut(&ieee80211_frame.get_source()) {
                match auth.handle_eap(eap_packet) {
                    Ok(outputs) => {
                        let mut frames = Vec::new();
                        for out in outputs {
                            match out {
                                crate::eap_auth::EapOutput::Frame(data) => {
                                    frames.push(bytes::Bytes::from(self.wrap_eapol(
                                        ap,
                                        ieee80211_frame.get_source(),
                                        &data,
                                    )));
                                }
                                crate::eap_auth::EapOutput::Success => {
                                    info!("EAP: Success for {}", ieee80211_frame.get_source());
                                    // TODO: Key Derivation / PMK setting?
                                    // For now, we are Authenticated.
                                }
                                _ => {}
                            }
                        }
                        return Ok(frames);
                    }
                    Err(error) => {
                        warn!(?error, "EAP Error");
                        return Ok(vec![]);
                    }
                }
            } else {
                debug!("EAP Packet from unknown session {}", ieee80211_frame.get_source());
                return Ok(vec![]);
            }
        }

        // Key (Type 3) - WPA
        if eapol_type == 3 {
            let wpa = match &mut ap.wpa {
                Some(wpa) => wpa,
                None => {
                    debug!("ApActor: Received EAPOL-Key but WPA not configured");
                    return Ok(vec![]);
                }
            };

            let outputs = match wpa.handle_eapol(payload) {
                Ok(o) => o,
                Err(_) => {
                    warn!("ApActor: WPA handle_eapol failed");
                    return Ok(vec![]);
                }
            };

            let mut frames = Vec::new();
            let src = ieee80211_frame.get_source();
            info!("ApActor: EAPOL produced {} outputs for {}", outputs.len(), src);

            for out in outputs {
                match out {
                    crate::wpa_auth::WpaOutput::Frame(data) => {
                        debug!("ApActor: Sending EAPOL response (len={})", data.len());
                        let wrapped = self.wrap_eapol(ap, src, &data);
                        frames.push(bytes::Bytes::from(wrapped));
                    }
                    crate::wpa_auth::WpaOutput::InstallKey { key_index, key, cipher } => {
                        // Key Install Logic
                        info!("ApActor: Installing PTK.");
                        if key_index == 0 {
                            info!("ApActor: Installing PTK for {} cipher={}", src, cipher);
                            shared_keys.add_session(src, key);
                        }
                    }
                }
            }
            return Ok(frames);
        }

        Ok(vec![])
    }
}
