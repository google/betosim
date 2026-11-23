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
    BeaconFrameHeader, EAPOL_TYPE_KEY, EAPOL_TYPE_PACKET, EAPOL_TYPE_START, FrameControl,
    IeIterator, Ieee80211, LlcSnapHeader, MacHeader3Addr, control_field, management_subtype, sap,
    tags, write_ie, write_wmm_param_element,
};
use tracing::{debug, error, info, warn};
use zerocopy::{FromBytes, IntoBytes, U16};

use crate::{
    ApActor, ApError, ApState,
    eap_auth::{EapAuthenticator, EapOutput},
    sae::SaeStateMachine,
    shared::SharedKeyStore,
    wpa_auth::{WpaAuthenticator, WpaOutput},
};

const GTK_LEN: usize = 16;

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
            timestamp: (ap.start_time.elapsed().as_micros() as u64).to_le_bytes(),
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
        if ap.config.channel <= 14 {
            write_ie(body, tags::DS_PARAMETER_SET, &[ap.config.channel]);
        }

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
                warn!(
                    code = crate::err::Code::FrameDecodeFailed as u32,
                    %error,
                    "ApActor: Failed to decode 802.11 frame"
                );
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
            warn!(
                code = crate::err::Code::AuthFrameTooShort as u32,
                "ApActor: Auth frame too short"
            );
            return Ok(vec![]);
        };

        let fixed = match AuthenticationFixedFields::read_from_prefix(body) {
            Ok((f, _)) => f,
            Err(_) => {
                warn!(
                    code = crate::err::Code::AuthFixedFieldsInvalid as u32,
                    "ApActor: Auth frame fixed fields invalid"
                );
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
            shared_keys.remove_station_for_bssid(&src, &ap.config.bssid);
        }

        // SAE (Algorithm 3)
        if alg == 3 {
            if !ap.config.sae {
                warn!(
                    code = crate::err::Code::AuthSaeNotEnabled as u32,
                    "ApActor: SAE requested but not enabled"
                );
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
                warn!(
                    code = crate::err::Code::AuthSaePayloadMissing as u32,
                    "ApActor: Auth frame missing SAE payload"
                );
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

        let msgs = vec![bytes::Bytes::from(self.build_assoc_resp(ap, src, 0))];

        ap.wpa_sessions.remove(&src);
        ap.eap_sessions.remove(&src);
        ap.purge_delayed_frames(&src);
        shared_keys.associate_station(src, ap.config.bssid);

        // Init WPA if configured
        if let Some(passphrase) = &ap.config.wpa_passphrase {
            let rsn_ie = crate::rsn::build_rsn_ie(&ap.config);
            let global_gtk = shared_keys.get_gtk(&ap.config.bssid).unwrap_or_else(|| {
                let mut new_gtk = [0u8; GTK_LEN];
                new_gtk.copy_from_slice(&crate::ffi::RandBytes(GTK_LEN));
                shared_keys.set_gtk(ap.config.bssid, new_gtk);
                new_gtk
            });

            let mut authenticator = WpaAuthenticator::new(
                ap.config.bssid,
                src,
                ap.config.ssid.as_bytes(),
                passphrase.as_bytes(),
                &rsn_ie,
                global_gtk,
            );

            if let Ok(m1) = authenticator.initiate_handshake() {
                let m1_frame = self.wrap_eapol(ap, src, &m1);
                ap.wpa_sessions.insert(src, authenticator);

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
        info!("ApActor: Received Deauth/Disassoc from {}", src);
        ap.clear_station_state(&src);
        shared_keys.remove_station_for_bssid(&src, &ap.config.bssid);
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
        debug!(
            "ApActor: Data Frame Debug [{} bytes] (stype: {:?}) hex: {:02x?}",
            frame.len(),
            ieee80211_frame.stype(),
            dump
        );

        if !ieee80211_frame.is_eapol().unwrap_or(false) {
            return Ok(vec![]);
        }

        let src = ieee80211_frame.get_source();
        if ieee80211_frame.needs_decryption()
            || !ieee80211_frame.is_to_ap()
            || ieee80211_frame.get_bssid() != Some(ap.config.bssid)
            || !ap.associations.contains(&src)
        {
            return Ok(vec![]);
        }
        info!("ApActor: Received EAPOL frame from src={}", src);
        let frame_payload = ieee80211_frame.get_payload();
        let Some(payload) = frame_payload.get(std::mem::size_of::<LlcSnapHeader>()..) else {
            return Ok(vec![]);
        };

        let &[_, eapol_type, _, _, ref eap_packet @ ..] = payload else {
            return Ok(vec![]);
        };

        // EAPOL-Start (Type 1)
        if eapol_type == EAPOL_TYPE_START {
            if !ap.config.enterprise_enabled {
                warn!(
                    code = crate::err::Code::EapolStartEnterpriseDisabled as u32,
                    "ApActor: Received EAPOL-Start but enterprise not enabled"
                );
                return Ok(vec![]);
            }
            // Start EAP
            let mut auth = EapAuthenticator::new(ap.config.bssid, src);
            match auth.start() {
                Ok(outputs) => {
                    let mut frames = Vec::new();
                    for out in outputs {
                        if let EapOutput::Frame(data) = out {
                            frames.push(bytes::Bytes::from(self.wrap_eapol(ap, src, &data)));
                        }
                    }
                    ap.eap_sessions.insert(src, auth);
                    return Ok(frames);
                }
                Err(error) => {
                    error!(
                        code = crate::err::Code::EapStartFailed as u32,
                        ?error,
                        "ApActor: Failed to start EAP"
                    );
                    return Ok(vec![]);
                }
            }
        }

        // EAP-Packet (Type 0)
        if eapol_type == EAPOL_TYPE_PACKET {
            if let Some(auth) = ap.eap_sessions.get_mut(&src) {
                match auth.handle_eap(eap_packet) {
                    Ok(outputs) => {
                        let mut frames = Vec::new();
                        let mut last_seq_ctrl = None;
                        for out in outputs {
                            match out {
                                EapOutput::Frame(data) => {
                                    let raw = self.wrap_eapol(ap, src, &data);
                                    last_seq_ctrl = MacHeader3Addr::read_from_prefix(&raw)
                                        .ok()
                                        .map(|(header, _)| header.sequence_control.get());
                                    frames.push(bytes::Bytes::from(raw));
                                }
                                EapOutput::Success => {
                                    info!("EAP: Success for {}", src);
                                    // TODO: Key Derivation / PMK setting?
                                    // For now, we are Authenticated.
                                    if ap.config.wpa_passphrase.is_none()
                                        && let Some(seq_ctrl) = last_seq_ctrl
                                    {
                                        shared_keys.authorize_eap_station(
                                            src,
                                            ap.config.bssid,
                                            seq_ctrl,
                                        );
                                    }
                                }
                                _ => {}
                            }
                        }
                        return Ok(frames);
                    }
                    Err(error) => {
                        warn!(code = crate::err::Code::EapHandleFailed as u32, ?error, "EAP Error");
                        return Ok(vec![]);
                    }
                }
            } else {
                debug!("EAP Packet from unknown session {}", src);
                return Ok(vec![]);
            }
        }

        // Key (Type 3) - WPA
        if eapol_type == EAPOL_TYPE_KEY {
            let Some(wpa) = ap.wpa_sessions.get_mut(&src) else {
                debug!("ApActor: Received EAPOL-Key without active WPA session for {}", src);
                return Ok(vec![]);
            };

            let outputs = match wpa.handle_eapol(payload) {
                Ok(o) => o,
                Err(_) => {
                    warn!(
                        code = crate::err::Code::WpaHandleEapolFailed as u32,
                        "ApActor: WPA handle_eapol failed"
                    );
                    return Ok(vec![]);
                }
            };

            let mut frames = Vec::new();
            info!("ApActor: EAPOL produced {} outputs for {}", outputs.len(), src);

            for out in outputs {
                match out {
                    WpaOutput::Frame(data) => {
                        debug!("ApActor: Sending EAPOL response (len={})", data.len());
                        frames.push(bytes::Bytes::from(self.wrap_eapol(ap, src, &data)));
                    }
                    WpaOutput::InstallKey { key_index, key, cipher } => {
                        // Key Install Logic
                        info!("ApActor: Installing PTK.");
                        if key_index == 0 {
                            info!("ApActor: Installing PTK for {} cipher={}", src, cipher);
                            shared_keys.authorize_station(src, ap.config.bssid, key);
                        }
                    }
                }
            }
            return Ok(frames);
        }

        Ok(vec![])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::{CCMP_TK_LEN, FC_PROTECTED_BIT};

    const AUTH_FC: u16 = 0x00B0;
    const MIN_ETHERNET_LEN: usize = 60;

    fn extract_tsf(frame: &[u8]) -> u64 {
        let (fixed, _) = BeaconFixedFields::read_from_prefix(&frame[24..])
            .expect("frame should contain valid BeaconFixedFields at offset 24");
        u64::from_le_bytes(fixed.timestamp)
    }

    #[test]
    fn test_beacon_monotonic_tsf_timestamp() {
        let manager = Ieee80211Manager::new();
        let mut ap = ApState::default();

        let b1 = manager.generate_beacon(&mut ap, 100).expect("first beacon should build");
        std::thread::sleep(std::time::Duration::from_millis(2));
        let b2 = manager.generate_beacon(&mut ap, 100).expect("second beacon should build");

        let ts1 = extract_tsf(&b1[0]);
        let ts2 = extract_tsf(&b2[0]);
        assert!(ts2 > ts1, "expected second beacon TSF ({ts2}) > first beacon TSF ({ts1})");
    }

    #[test]
    fn test_probe_resp_monotonic_tsf_timestamp() {
        let mut manager = Ieee80211Manager::new();
        let mut ap = ApState::default();

        let header = MacHeader3Addr {
            frame_control: FrameControl::new(0x0040),
            duration_id: U16::new(0),
            addr1: netsim_packets::MacAddr { bytes: [0xFF; 6] },
            addr2: netsim_packets::MacAddr { bytes: [0x02, 0x00, 0x00, 0x00, 0x01, 0x00] },
            addr3: netsim_packets::MacAddr { bytes: [0xFF; 6] },
            sequence_control: ap.next_seq_control(),
        };
        let mut raw_req = header.as_bytes().to_vec();
        write_ie(&mut raw_req, tags::SSID, &[]);
        let parsed_req = Ieee80211::decode(&raw_req).expect("probe request should decode");

        let r1 = manager
            .handle_probe_req(&mut ap, &parsed_req, &raw_req, 100)
            .expect("first probe response should build");
        std::thread::sleep(std::time::Duration::from_millis(2));
        let r2 = manager
            .handle_probe_req(&mut ap, &parsed_req, &raw_req, 100)
            .expect("second probe response should build");

        let ts1 = extract_tsf(&r1[0]);
        let ts2 = extract_tsf(&r2[0]);
        assert!(ts2 > ts1, "expected second probe resp TSF ({ts2}) > first probe resp TSF ({ts1})");
    }

    #[test]
    fn test_ds_parameter_set_ie_channel_filtering() {
        let manager = Ieee80211Manager::new();

        let mut ap_2g = ApState::default();
        ap_2g.config.channel = 6;
        let beacon_2g =
            manager.generate_beacon(&mut ap_2g, 100).expect("2.4 GHz beacon should build");
        let ds_ie_2g = IeIterator::new(&beacon_2g[0][36..])
            .find(|ie| ie.id == tags::DS_PARAMETER_SET)
            .map(|ie| ie.body);
        assert_eq!(ds_ie_2g, Some(&[6u8][..]));

        let mut ap_5g = ApState::default();
        ap_5g.config.channel = 36;
        let beacon_5g =
            manager.generate_beacon(&mut ap_5g, 100).expect("5 GHz beacon should build");
        let has_ds_ie_5g =
            IeIterator::new(&beacon_5g[0][36..]).any(|ie| ie.id == tags::DS_PARAMETER_SET);
        assert!(!has_ds_ie_5g, "5 GHz channel 36 must omit DS_PARAMETER_SET IE");
    }

    struct NoopContext;

    impl actor_framework::Context<ApActor> for NoopContext {
        fn set_interval(&mut self, _duration: std::time::Duration) {}
        fn add_stream(&mut self, _id: crate::ap_actor::ApId, _stream: actor_framework::BoxStream) {}
        fn remove_stream(&mut self, _id: crate::ap_actor::ApId) {}
        fn add_typed_stream(&mut self, _id: usize, _stream: actor_framework::BoxTypedStream<()>) {}
        fn remove_typed_stream(&mut self, _id: usize) {}
        fn spawn(
            &mut self,
            _id: crate::ap_actor::ApId,
            _task: futures::future::BoxFuture<'static, crate::ap_actor::ApId>,
        ) {
        }
        fn abort(&mut self, _id: crate::ap_actor::ApId) {}
        fn shutdown(&mut self) {}
        fn run_later(
            &mut self,
            _duration: std::time::Duration,
            _f: actor_framework::TimerCallback<ApActor>,
        ) -> actor_framework::TimerKey {
            unimplemented!()
        }
        fn cancel_timer(&mut self, _key: actor_framework::TimerKey) {}
    }

    #[test]
    fn test_handle_frame_decode_failed_records_error_stat() {
        use std::sync::Arc;

        use common::util::netsim_logger::{ErrorStatsLayer, ErrorStatsTable};
        use tracing_subscriber::layer::SubscriberExt;

        use crate::err::Code;

        let table = Arc::new(ErrorStatsTable::new());
        let subscriber =
            tracing_subscriber::registry().with(ErrorStatsLayer::with_table(Arc::clone(&table)));
        let mut manager = Ieee80211Manager::new();
        let mut ap = ApState::default();
        let shared_keys = SharedKeyStore::new();
        let mut ctx = NoopContext;

        let _multi_dispatch_guard = tracing::Dispatch::new(tracing_subscriber::registry());
        tracing::subscriber::with_default(subscriber, || {
            let out = manager
                .handle_frame(&mut ap, &[0x01, 0x02], &shared_keys, 100, ChipId(0), &mut ctx)
                .expect("short frame returns empty Ok");
            assert!(out.is_empty());
        });

        assert_eq!(table.snapshot(), vec![(Code::FrameDecodeFailed as u32, 1)]);
    }

    #[test]
    fn test_handle_non_eapol_data_frame_emits_no_warn_or_error() {
        use std::sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        };

        use tracing_subscriber::{Layer, layer::SubscriberExt};

        struct WarnOrErrorCounter(Arc<AtomicUsize>);

        impl<S: tracing::Subscriber> Layer<S> for WarnOrErrorCounter {
            fn on_event(
                &self,
                event: &tracing::Event<'_>,
                _ctx: tracing_subscriber::layer::Context<'_, S>,
            ) {
                let meta = event.metadata();
                if meta.target().starts_with("ap_actor") && *meta.level() <= tracing::Level::WARN {
                    self.0.fetch_add(1, Ordering::Relaxed);
                }
            }
        }

        const DATA_TO_DS_FC: u16 = 0x0108;
        const STA_MAC: [u8; 6] = [0x02, 0x00, 0x00, 0x00, 0x00, 0x55];

        // Given
        let count = Arc::new(AtomicUsize::new(0));
        let subscriber =
            tracing_subscriber::registry().with(WarnOrErrorCounter(Arc::clone(&count)));
        let manager = Ieee80211Manager::new();
        let mut ap = ApState::default();
        let shared_keys = SharedKeyStore::new();
        let header = MacHeader3Addr {
            frame_control: FrameControl::new(DATA_TO_DS_FC),
            duration_id: U16::new(0),
            addr1: ap.config.bssid,
            addr2: netsim_packets::MacAddr { bytes: STA_MAC },
            addr3: ap.config.bssid,
            sequence_control: ap.next_seq_control(),
        };
        let mut raw_data = header.as_bytes().to_vec();
        raw_data.extend_from_slice(&[0u8; 8]);
        let ieee80211_frame = Ieee80211::decode(&raw_data).expect("decode valid 802.11 data frame");

        // When
        let _multi_dispatch_guard = tracing::Dispatch::new(tracing_subscriber::registry());
        let out = tracing::subscriber::with_default(subscriber, || {
            manager
                .handle_data_frame(&mut ap, &raw_data, ieee80211_frame, &shared_keys)
                .expect("non-EAPOL data frame returns empty Ok")
        });

        // Then
        assert!(out.is_empty());
        assert_eq!(count.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn test_handle_auth_seq1_clears_only_matching_bssid_session() {
        use netsim_packets::{EthernetFrame, FrameDirection, MacAddr};
        let b1 = MacAddr::new([2, 0, 0, 0, 0, 1]);
        let b2 = MacAddr::new([2, 0, 0, 0, 0, 2]);
        let sta1 = MacAddr::new([2, 0, 0, 0, 0, 0x55]);
        let sta2 = MacAddr::new([2, 0, 0, 0, 0, 0x56]);
        let auth_seq1 = |bssid, sta, seq| {
            let hdr = MacHeader3Addr::new(FrameControl::new(AUTH_FC), 0, bssid, sta, bssid, seq);
            let fix = AuthenticationFixedFields {
                algorithm: U16::ZERO,
                sequence: 1.into(),
                status: U16::ZERO,
            };
            [hdr.as_bytes(), fix.as_bytes()].concat()
        };
        let downlink_data = |bssid, sta| {
            let mut eth = EthernetFrame::new(sta, bssid, netsim_packets::ether_type::IPV4)
                .as_bytes()
                .to_vec();
            eth.resize(MIN_ETHERNET_LEN, 0);
            Ieee80211::from_ieee8023(&eth, bssid, FrameDirection::FromAp, 1).unwrap()
        };

        // Given sta1 authorized on b1 and sta2 authorized on b2
        let mut mgr = Ieee80211Manager::new();
        let mut ap = ApState::default();
        ap.config.bssid = b2;
        let keys = SharedKeyStore::new();
        keys.register_bssid(b1, true, false);
        keys.register_bssid(b2, true, false);
        keys.associate_station(sta1, b1);
        keys.authorize_station(sta1, b1, vec![0x42; CCMP_TK_LEN]);
        keys.associate_station(sta2, b2);
        keys.authorize_station(sta2, b2, vec![0x22; CCMP_TK_LEN]);

        // When AP b2 handles Auth seq 1 from sta1 (mismatched BSSID) and sta2 (matching BSSID)
        let raw1 = auth_seq1(b2, sta1, ap.next_seq_control());
        mgr.handle_auth(&mut ap, &Ieee80211::decode(&raw1).unwrap(), &raw1, &keys).unwrap();
        let raw2 = auth_seq1(b2, sta2, ap.next_seq_control());
        mgr.handle_auth(&mut ap, &Ieee80211::decode(&raw2).unwrap(), &raw2, &keys).unwrap();

        // Then sta1's session on b1 stays intact while sta2's matching b2 session is cleared
        assert_eq!(keys.get_station_bssid(&sta1), Some(b1));
        assert!(!keys.is_uplink_port_blocked(&sta1, &b1));
        assert!(keys.try_encrypt(&downlink_data(b1, sta1)).is_some());
        assert!(keys.get_station_bssid(&sta2).is_none());
        assert!(keys.try_encrypt(&downlink_data(b2, sta2)).is_none());
    }

    #[test]
    fn test_handle_data_frame_ignores_protected_uplink_eapol() {
        use netsim_packets::{
            EAPOL_VERSION, EapolHeader, EthernetFrame, FrameDirection, MacAddr, ether_type,
        };
        use zerocopy::IntoBytes;
        let bssid = MacAddr::new([2, 0, 0, 0, 0, 1]);
        let sta = MacAddr::new([2, 0, 0, 0, 0, 0x55]);
        let mgr = Ieee80211Manager::new();
        let mut ap = ApState::default();
        let keys = SharedKeyStore::new();
        ap.config.enterprise_enabled = true;
        ap.config.bssid = bssid;
        ap.associations.insert(sta);
        keys.register_bssid(bssid, false, true);
        keys.associate_station(sta, bssid);
        let mut eth = EthernetFrame::new(bssid, sta, ether_type::EAPOL).as_bytes().to_vec();
        let eapol = EapolHeader::new(EAPOL_VERSION, EAPOL_TYPE_START, 0);
        eth.extend_from_slice(eapol.as_bytes());
        let frame = Ieee80211::from_ieee8023(&eth, bssid, FrameDirection::ToAp, 1).unwrap();
        let mut raw = frame.as_bytes().to_vec();
        raw[1] |= FC_PROTECTED_BIT;
        let out =
            mgr.handle_data_frame(&mut ap, &raw, Ieee80211::decode(&raw).unwrap(), &keys).unwrap();
        assert!(out.is_empty());
        assert!(ap.eap_sessions.is_empty());
        assert!(keys.is_uplink_port_blocked(&sta, &bssid));
    }
}
