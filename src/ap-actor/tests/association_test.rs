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

use ap_actor::{
    ApConfig,
    ffi::{DigestType, Hmac, Pbkdf2HmacSha1},
};
use netsim_packets::{
    EAPOL_KEY_DESC_TYPE_RSN, EAPOL_TYPE_KEY, EAPOL_TYPE_START, EAPOL_VERSION, EapolHeader,
    EapolKeyFrame, FrameControl, Ieee80211, LlcSnapHeader, MacAddr, MacHeader3Addr,
    SequenceControl,
};
use tracing::info;
use zerocopy::{FromBytes, IntoBytes};

use crate::world::{
    ApWorld, DEAUTH_REASON_STA_LEAVING, FC_DATA_TO_DS, TestApId, create_downlink_data_frame,
};

// Helper: Calculate MIC (from wpa_auth.rs logic reversed/reused)
fn calc_mic(kck: &[u8], frame: &[u8]) -> Vec<u8> {
    let mic = Hmac(DigestType::SHA1, kck, frame);
    mic[0..16].to_vec()
}

// Helper: PRF (from wpa_auth.rs)
fn prf(key: &[u8], label: &[u8], data: &[u8], bit_len: usize) -> Vec<u8> {
    let mut result = Vec::new();
    let bytes_len = bit_len / 8;
    let mut i = 0u8;
    while result.len() < bytes_len {
        let mut input = Vec::new();
        input.extend_from_slice(label);
        input.push(0);
        input.extend_from_slice(data);
        input.push(i);
        let hmac = Hmac(DigestType::SHA1, key, &input);
        result.extend_from_slice(&hmac);
        i += 1;
    }
    result.truncate(bytes_len);
    result
}

// Helper: Calculate PTK (Supplicant side)
fn calc_ptk(pmk: &[u8], bssid: &[u8], sta: &[u8], anonce: &[u8], snonce: &[u8]) -> Vec<u8> {
    let label = b"Pairwise key expansion";
    let mut data = Vec::new();
    let (min_addr, max_addr) = if bssid < sta { (bssid, sta) } else { (sta, bssid) };
    let (min_nonce, max_nonce) = if anonce < snonce { (anonce, snonce) } else { (snonce, anonce) };
    data.extend_from_slice(min_addr);
    data.extend_from_slice(max_addr);
    data.extend_from_slice(min_nonce);
    data.extend_from_slice(max_nonce);
    prf(pmk, label, &data, 384)
}

// ============================================================================
// Feature: AP Connectivity
// ============================================================================

// Scenario: Station Association
// Given a registered AP "AssocAP"
// When a Station sends an Association Request
// Then the Station receives an Association Response
#[tokio::test]
async fn test_station_association_flow() {
    info!("Scenario: Station Association");
    let mut world = ApWorld::new().await;
    world.given_a_registered_ap("AssocAP").await;

    let station_mac = "02:00:00:00:00:99";

    // When
    world.when_station_sends_assoc_req(station_mac).await;

    // Then
    world.then_station_receives_assoc_resp(station_mac).await;
    world.then_station_port_state_is(station_mac, world.ap_bssid, false);
}

const FC_DATA_FROM_DS: u16 = 0x0208;
const OLD_BSSID: MacAddr = MacAddr::new([2, 0, 0, 0, 0, 0x22]);
const ENTERPRISE_BSSID: MacAddr = MacAddr::new([2, 0, 0, 0, 0, 0x33]);
const TEST_STA_93: &str = "02:00:00:00:00:93";
const TEST_STA_94: &str = "02:00:00:00:00:94";
const TEST_STA_95: &str = "02:00:00:00:00:95";
const TEST_STA_96: &str = "02:00:00:00:00:96";
const TEST_STA_99: &str = "02:00:00:00:00:99";
const KEY_INFO_M2: u16 = 0x010A;
const KEY_INFO_M4: u16 = 0x030A;
const EAPOL_KEY_BODY_LEN: u16 = 95;
const PBKDF2_ITERATIONS: u32 = 4096;
const PMK_LEN: usize = 32;
const KCK_LEN: usize = 16;
const TEST_SNONCE: [u8; 32] = [0x55; 32];
const WRONG_PMK_BYTE: u8 = 0xEE;

fn parse_mac(mac: &str) -> MacAddr {
    mac.try_into().expect("valid MAC")
}

fn sta_ptk(pmk: &[u8], bssid: MacAddr, sta: MacAddr, anonce: &[u8; 32]) -> Vec<u8> {
    calc_ptk(pmk, &bssid.bytes, &sta.bytes, anonce, &TEST_SNONCE)
}

fn decode_eapol_key(raw_msg: &[u8]) -> EapolKeyFrame {
    let payload = Ieee80211::decode(raw_msg).expect("802.11 EAPOL").get_payload();
    let (_, eapol) = LlcSnapHeader::read_from_prefix(&payload).expect("LLC/SNAP");
    let (hdr, body) = EapolHeader::read_from_prefix(eapol).expect("EAPOL header");
    assert_eq!(hdr.packet_type, EAPOL_TYPE_KEY);
    EapolKeyFrame::read_from_prefix(body).expect("EAPOL-Key body").0
}

impl ApWorld {
    fn send_to_ap(&self, frame: Vec<u8>) {
        self.tx_to_ap.as_ref().expect("AP").send(bytes::Bytes::from(frame)).expect("send");
    }

    fn build_eapol_key_frame(&self, sta: MacAddr, info: u16, rc: [u8; 8], ptk: &[u8]) -> Vec<u8> {
        let eapol = EapolHeader::new(EAPOL_VERSION, EAPOL_TYPE_KEY, EAPOL_KEY_BODY_LEN);
        let key_nonce = if info == KEY_INFO_M2 { TEST_SNONCE } else { [0; 32] };
        let mut kf = EapolKeyFrame {
            descriptor_type: EAPOL_KEY_DESC_TYPE_RSN,
            key_info: info.to_be_bytes(),
            replay_counter: rc,
            key_nonce,
            ..Default::default()
        };
        let mut unsigned = eapol.as_bytes().to_vec();
        unsigned.extend_from_slice(kf.as_bytes());
        kf.mic = calc_mic(&ptk[..KCK_LEN], &unsigned).try_into().expect("16-byte MIC");
        self.build_eapol_frame_with_bssid(
            self.ap_bssid,
            sta,
            FC_DATA_TO_DS,
            EAPOL_VERSION,
            EAPOL_TYPE_KEY,
            kf.as_bytes(),
        )
    }

    async fn recv_eapol_key_for(&mut self, sta: MacAddr) -> EapolKeyFrame {
        let msg = self
            .recv_frame(|f, _| f.is_eapol().unwrap_or(false) && f.get_destination() == sta)
            .await;
        decode_eapol_key(&msg)
    }

    async fn given_default_wpa_ap(&mut self, ssid: &str, pass: &str) {
        self.create_default_ap(Self::ap_cfg(ssid, self.ap_bssid, Some(pass))).await;
    }

    fn ap_cfg(ssid: &str, bssid: MacAddr, passphrase: Option<&str>) -> ApConfig {
        ApConfig {
            ssid: ssid.into(),
            bssid,
            wpa_passphrase: passphrase.map(str::to_string),
            ..ApConfig::default()
        }
    }

    async fn given_old_wpa_ap(&mut self) -> TestApId {
        let cfg = Self::ap_cfg("OldWpaAP", OLD_BSSID, Some("OldPassword"));
        self.given_a_registered_ap_with_config(cfg).await;
        self.primary_ap_id()
    }

    async fn given_extra_open_ap(&mut self, ssid: &str, bssid: MacAddr) -> TestApId {
        self.given_extra_ap(Self::ap_cfg(ssid, bssid, None)).await
    }

    async fn given_extra_enterprise_ap(&mut self, ssid: &str, bssid: MacAddr) -> TestApId {
        let mut cfg = Self::ap_cfg(ssid, bssid, None);
        cfg.enterprise_enabled = true;
        self.given_extra_ap(cfg).await
    }

    async fn given_station_associated_to_bssid(&mut self, sta: &str, bssid: MacAddr) {
        self.when_station_sends_assoc_req_to_bssid(sta, bssid).await;
        self.then_station_receives_assoc_resp(sta).await;
    }

    async fn given_authorized_wpa_station(&mut self, sta: &str, ssid: &str, pass: &str) {
        self.given_station_is_associated(sta).await;
        self.when_station_completes_wpa_handshake(sta, ssid, pass).await;
    }

    async fn given_captured_eapol_m4(&mut self, sta: &str, ssid: &str, pass: &str) -> Vec<u8> {
        let sta = parse_mac(sta);
        let m1 = self.recv_eapol_key_for(sta).await;
        let pmk = Pbkdf2HmacSha1(pass.as_bytes(), ssid.as_bytes(), PBKDF2_ITERATIONS, PMK_LEN);
        let ptk = sta_ptk(&pmk, self.ap_bssid, sta, &m1.key_nonce);
        self.send_to_ap(self.build_eapol_key_frame(sta, KEY_INFO_M2, m1.replay_counter, &ptk));
        let m3 = self.recv_eapol_key_for(sta).await;
        assert!(u16::from_be_bytes(m3.key_data_len) > 0, "M3 must contain RSN IE + GTK");
        self.build_eapol_key_frame(sta, KEY_INFO_M4, m3.replay_counter, &ptk)
    }

    async fn recv_eapol_pair(&mut self, a: MacAddr, b: MacAddr) -> (EapolKeyFrame, EapolKeyFrame) {
        let (mut fa, mut fb) = (None, None);
        while fa.is_none() || fb.is_none() {
            let msg = self.recv_frame(|f, _| f.is_eapol().unwrap_or(false)).await;
            let dst = Ieee80211::decode(&msg).expect("802.11").get_destination();
            if dst == a {
                fa = Some(decode_eapol_key(&msg));
            } else if dst == b {
                fb = Some(decode_eapol_key(&msg));
            }
        }
        (fa.expect("EAPOL A"), fb.expect("EAPOL B"))
    }

    fn given_frame_encrypted_with_current_ptk(&self, station_mac: &str) -> Ieee80211 {
        let plain = create_downlink_data_frame(parse_mac(station_mac), self.ap_bssid);
        let enc = self.shared_keys.try_encrypt(&plain).expect("encrypt with active PTK");
        let frame = Ieee80211::decode(&enc).expect("valid CCMP frame");
        assert!(self.shared_keys.try_decrypt(&frame).is_some(), "active PTK must decrypt");
        frame
    }

    async fn given_station_has_pending_wpa_handshake(&mut self, mac: &str) {
        self.given_station_is_associated(mac).await;
        let _ = self.recv_eapol_key_for(parse_mac(mac)).await;
    }

    async fn when_unassociated_station_sends_eapol_key(&mut self, mac: &str) {
        let sta = parse_mac(mac);
        self.send_to_ap(self.build_eapol_key_frame(sta, KEY_INFO_M2, [0; 8], &[0; KCK_LEN]));
        self.await_probe_barrier(sta, false).await;
    }

    async fn when_station_sends_invalid_m2(&mut self, station_mac: &str) {
        let sta = parse_mac(station_mac);
        let m1 = self.recv_eapol_key_for(sta).await;
        let ptk = sta_ptk(&[WRONG_PMK_BYTE; PMK_LEN], self.ap_bssid, sta, &m1.key_nonce);
        self.send_to_ap(self.build_eapol_key_frame(sta, KEY_INFO_M2, m1.replay_counter, &ptk));
    }

    async fn when_station_completes_wpa_handshake(
        &mut self,
        station_mac: &str,
        ssid: &str,
        pass: &str,
    ) {
        let m4 = self.given_captured_eapol_m4(station_mac, ssid, pass).await;
        self.send_to_ap(m4);
        self.await_probe_barrier(parse_mac(station_mac), false).await;
    }

    async fn when_two_stations_complete_wpa(
        &mut self,
        sta_a_str: &str,
        sta_b_str: &str,
        ssid: &str,
        pass: &str,
    ) {
        let (sta_a, sta_b) = (parse_mac(sta_a_str), parse_mac(sta_b_str));
        let pmk = Pbkdf2HmacSha1(pass.as_bytes(), ssid.as_bytes(), PBKDF2_ITERATIONS, PMK_LEN);

        let (m1_a, m1_b) = self.recv_eapol_pair(sta_a, sta_b).await;
        let ptk_a = sta_ptk(&pmk, self.ap_bssid, sta_a, &m1_a.key_nonce);
        let ptk_b = sta_ptk(&pmk, self.ap_bssid, sta_b, &m1_b.key_nonce);
        self.send_to_ap(self.build_eapol_key_frame(
            sta_a,
            KEY_INFO_M2,
            m1_a.replay_counter,
            &ptk_a,
        ));
        self.send_to_ap(self.build_eapol_key_frame(
            sta_b,
            KEY_INFO_M2,
            m1_b.replay_counter,
            &ptk_b,
        ));

        let (m3_a, m3_b) = self.recv_eapol_pair(sta_a, sta_b).await;
        self.send_to_ap(self.build_eapol_key_frame(
            sta_a,
            KEY_INFO_M4,
            m3_a.replay_counter,
            &ptk_a,
        ));
        self.send_to_ap(self.build_eapol_key_frame(
            sta_b,
            KEY_INFO_M4,
            m3_b.replay_counter,
            &ptk_b,
        ));

        self.await_probe_barrier(sta_a, false).await;
    }

    async fn when_station_deauths_immediately(&mut self, mac: &str) {
        let sta = parse_mac(mac);
        self.when_station_sends_deauth(sta, DEAUTH_REASON_STA_LEAVING).await;
        self.await_probe_barrier(sta, false).await;
    }

    async fn when_station_sends_eapol_start_to_bssid(&mut self, sta_mac: &str, bssid: MacAddr) {
        let frame = self.build_eapol_frame_with_bssid(
            bssid,
            parse_mac(sta_mac),
            FC_DATA_TO_DS,
            EAPOL_VERSION,
            EAPOL_TYPE_START,
            &[],
        );
        self.send_to_ap(frame);
    }

    async fn when_station_sends_from_ds_eapol_start(&mut self, sta_mac: &str) {
        let bssid = self.ap_bssid;
        let header = MacHeader3Addr::new(
            FrameControl::new(FC_DATA_FROM_DS),
            0,
            bssid,
            bssid,
            parse_mac(sta_mac),
            SequenceControl::new(0),
        );
        self.send_to_ap(self.build_eapol_frame_with_header(
            header,
            EAPOL_VERSION,
            EAPOL_TYPE_START,
            &[],
        ));
    }

    async fn when_station_replays_eapol_frame(&mut self, mac: &str, frame: Vec<u8>) {
        self.send_to_ap(frame);
        self.await_probe_barrier(parse_mac(mac), true).await;
    }

    fn then_station_has_ptk_session(&self, station_mac: &str, expected: bool) {
        self.then_station_has_ptk_session_on_bssid(station_mac, self.ap_bssid, expected);
    }

    fn then_station_has_ptk_session_on_bssid(&self, sta: &str, bssid: MacAddr, expected: bool) {
        let frame = create_downlink_data_frame(parse_mac(sta), bssid);
        assert_eq!(self.shared_keys.try_encrypt(&frame).is_some(), expected);
    }

    fn then_encrypted_frame_fails_decryption(&self, frame: &Ieee80211) {
        assert!(self.shared_keys.try_decrypt(frame).is_none());
    }

    fn then_roamed_station_state_is_preserved(&self, sta: &str) {
        self.then_station_state_is(sta, self.ap_bssid);
        self.then_station_has_ptk_session(sta, true);
    }

    async fn then_station_stays_blocked_without_eapol(&mut self, sta: &str) {
        self.await_probe_barrier(parse_mac(sta), false).await;
        self.then_station_port_state_is(sta, self.ap_bssid, true);
    }

    fn then_unassociated_station_is_cleared(&self, station_mac: &str) {
        let sta = parse_mac(station_mac);
        assert!(!self.shared_keys.has_station(&sta));
        self.then_station_has_ptk_session(station_mac, false);
    }

    fn then_default_ap_requires_controlled_port(&self) {
        assert!(self.shared_keys.requires_controlled_port(&self.ap_bssid));
    }
}

// Scenario: WPA Handshake Failure (Wrong Password)
#[tokio::test]
async fn test_wpa_handshake_failure_wrong_password() {
    // Given
    let mut world = ApWorld::new().await;
    world.given_a_registered_ap_with_wpa("WpaAP", "CorrectPassword").await;
    let sta = "02:00:00:00:00:99";
    world.given_station_is_associated(sta).await;
    let buffered = world.when_downlink_frame_is_gated(sta);
    world.then_downlink_action_is_buffered(&buffered);
    // When
    world.when_station_sends_invalid_m2(sta).await;
    // Then
    world.then_no_eap_or_auth_frame_is_received().await;
    world.then_station_port_state_is(sta, world.ap_bssid, true);
    world.then_station_buffered_frame_count_is(sta, 1);
}

// Scenario: WPA Handshake Success
#[tokio::test]
async fn test_wpa_handshake_success() {
    // Given
    let mut world = ApWorld::new().await;
    world.given_a_registered_ap_with_wpa("WpaAP", "CorrectPassword").await;
    let sta = "02:00:00:00:00:99";
    world.given_station_is_associated(sta).await;
    // When
    world.when_station_completes_wpa_handshake(sta, "WpaAP", "CorrectPassword").await;
    // Then
    world.then_station_has_ptk_session(sta, true);
}

// Scenario: Unassociated station without an active WPA session sending EAPOL-Key
// is dropped without invoking a peer station's WPA session, emitting frames,
// or installing a PTK session.
#[tokio::test]
async fn test_unassociated_station_eapol_key_is_dropped() {
    // Given a WPA2 AP with a peer station holding an active WPA session and an unassociated station
    let mut world = ApWorld::with_error_stats().await;
    world.given_a_registered_ap_with_wpa("WpaAP", "CorrectPassword").await;
    world.given_station_has_pending_wpa_handshake("22:33:44:55:66:77").await;
    let sta = "02:00:00:00:11:12";
    // When the unassociated station sends an EAPOL-Key M2 frame with an all-zero KCK
    world.when_unassociated_station_sends_eapol_key(sta).await;
    // Then no WPA MIC error is recorded on the peer session, no EAP/Auth frames are emitted,
    // and no PTK session is installed
    world.then_error_stats_are(&[]).await;
    world.then_no_eap_or_auth_frame_is_received().await;
    world.then_station_has_ptk_session(sta, false);
}

// Scenario: Re-associating an authorized WPA2 station without Auth seq 1
// immediately clears its stale PTK session in SharedKeyStore.
#[tokio::test]
async fn test_reassociation_without_auth_clears_stale_ptk_session() {
    // Given an authorized WPA2 station and a frame encrypted under its PTK
    let mut world = ApWorld::new().await;
    world.given_a_registered_ap_with_wpa("WpaAP", "CorrectPassword").await;
    let sta = "02:00:00:00:11:14";
    world.given_station_is_associated(sta).await;
    world.when_station_completes_wpa_handshake(sta, "WpaAP", "CorrectPassword").await;
    let stale_frame = world.given_frame_encrypted_with_current_ptk(sta);
    // When the station re-associates via AssocReq without sending Auth seq 1
    world.when_station_sends_assoc_req(sta).await;
    world.then_station_receives_assoc_resp(sta).await;
    // Then SharedKeyStore rejects decryption under the stale PTK immediately
    world.then_encrypted_frame_fails_decryption(&stale_frame);
}

// Scenario: Two stations associating concurrently to the same WPA2 AP each keep
// independent WPA authenticator sessions and both install PTK sessions cleanly.
#[tokio::test]
async fn test_concurrent_two_station_wpa_handshakes_both_authorize() {
    // Given a WPA2 AP and two stations associated prior to either handshake
    let mut world = ApWorld::new().await;
    world.given_a_registered_ap_with_wpa("WpaAP", "CorrectPassword").await;
    let (sta_a, sta_b) = ("02:00:00:00:00:91", "02:00:00:00:00:92");
    world.given_station_is_associated(sta_a).await;
    world.given_station_is_associated(sta_b).await;

    // When both stations complete their interleaved 4-way handshakes
    world.when_two_stations_complete_wpa(sta_a, sta_b, "WpaAP", "CorrectPassword").await;

    // Then both stations have installed PTK sessions
    world.then_station_has_ptk_session(sta_a, true);
    world.then_station_has_ptk_session(sta_b, true);
}

// Scenario: Re-associating or deauthenticating inside EAPOL_M1_DELAY cancels
// the superseded delayed M1 frame.
#[tokio::test]
async fn test_reassoc_and_deauth_purge_delayed_m1() {
    // Given a WPA2 AP and a station that just associated (queuing M1 #1)
    let mut world = ApWorld::new().await;
    world.given_a_registered_ap_with_wpa("WpaAP", "CorrectPassword").await;
    let sta = "02:00:00:00:11:15";
    world.given_station_is_associated(sta).await;

    // When the station immediately re-associates inside the 150ms M1 delay window
    world.given_station_is_associated(sta).await;

    // Then only the second association's M1 arrives and completes the handshake
    world.when_station_completes_wpa_handshake(sta, "WpaAP", "CorrectPassword").await;
    world.then_station_has_ptk_session(sta, true);

    // When another station associates and immediately deauthenticates inside 150ms
    let sta2 = "02:00:00:00:11:16";
    world.given_station_is_associated(sta2).await;
    world.when_station_deauths_immediately(sta2).await;

    // Then no delayed M1 frame is emitted past the 150ms delay window
    world.then_no_eap_or_auth_frame_is_received().await;
}

// Scenario: Deauthenticating an authorized WPA2 station clears its station
// state and PTK session.
#[tokio::test]
async fn test_deauthentication_clears_station_state_and_session() {
    // Given an authorized WPA2 station
    let mut world = ApWorld::new().await;
    world.given_a_registered_ap_with_wpa("WpaAP", "CorrectPassword").await;
    world.given_authorized_wpa_station(TEST_STA_99, "WpaAP", "CorrectPassword").await;

    // When the station deauthenticates from the AP
    world.when_station_deauthenticates(TEST_STA_99).await;

    // Then the station is unassociated and has no PTK session
    world.then_unassociated_station_is_cleared(TEST_STA_99);
}

// Scenario: A late deauth frame sent to a prior AP does not clear the roamed
// station's authorized state on the current WPA2 AP.
#[tokio::test]
async fn test_deauth_to_prior_ap_does_not_clear_roamed_station_state() {
    // Given a station that associated to an old AP and then roamed and authorized on a new WPA2 AP
    let mut world = ApWorld::new().await;
    world.given_extra_open_ap("OldAP", OLD_BSSID).await;
    world.given_a_registered_ap_with_wpa("WpaAP", "CorrectPassword").await;
    world.given_station_associated_to_bssid(TEST_STA_94, OLD_BSSID).await;
    world.given_authorized_wpa_station(TEST_STA_94, "WpaAP", "CorrectPassword").await;

    // When the station sends a deauth frame to the prior AP's BSSID
    world.when_station_deauthenticates_from_bssid(TEST_STA_94, OLD_BSSID).await;

    // Then the station remains associated with an active PTK session on the new WPA2 AP
    world.then_station_state_is(TEST_STA_94, world.ap_bssid);
    world.then_station_has_ptk_session(TEST_STA_94, true);
}

// Scenario: Disconnecting and force-disconnecting roamed stations from a prior
// AP that still holds stale associations preserves their new-AP state and PTK.
#[tokio::test]
async fn test_prior_ap_disconnect_preserves_roamed_wpa2_station_state() {
    // Given two stations authorized on an old WPA2 AP that then associate and
    // authorize on a new WPA2 AP without Auth seq 1
    let mut world = ApWorld::new().await;
    let old_id = world.given_old_wpa_ap().await;
    world.given_authorized_wpa_station(TEST_STA_95, "OldWpaAP", "OldPassword").await;
    world.given_authorized_wpa_station(TEST_STA_96, "OldWpaAP", "OldPassword").await;
    world.given_a_registered_ap_with_wpa("WpaAP", "CorrectPassword").await;
    world.given_authorized_wpa_station(TEST_STA_95, "WpaAP", "CorrectPassword").await;
    world.given_authorized_wpa_station(TEST_STA_96, "WpaAP", "CorrectPassword").await;
    world.then_ap_associations_are(old_id, &[TEST_STA_95, TEST_STA_96]).await;

    // When the prior AP disconnects or force-disconnects the stations
    world.when_disconnect_is_requested_on(old_id, TEST_STA_96).await;
    world.when_force_disconnect_is_requested_on(old_id, TEST_STA_95).await;

    // Then the prior AP clears its stale associations while new-AP state persists
    world.then_ap_associations_are(old_id, &[]).await;
    world.then_roamed_station_state_is_preserved(TEST_STA_95);
    world.then_roamed_station_state_is_preserved(TEST_STA_96);
}

// Scenario: Disconnecting and force-disconnecting stations on their current
// WPA2 AP clears each station's association state and PTK session.
#[tokio::test]
async fn test_current_ap_disconnect_and_force_disconnect_clear_station_state() {
    // Given two authorized WPA2 stations on the current AP
    let mut world = ApWorld::new().await;
    world.given_a_registered_ap_with_wpa("WpaAP", "CorrectPassword").await;
    world.given_authorized_wpa_station(TEST_STA_95, "WpaAP", "CorrectPassword").await;
    world.given_authorized_wpa_station(TEST_STA_96, "WpaAP", "CorrectPassword").await;

    // When one station is force-disconnected and the other is disconnected on the current AP
    world.when_force_disconnect_is_requested_for(TEST_STA_95).await;
    world.when_disconnect_is_requested_for(TEST_STA_96).await;

    // Then both stations are unassociated and have no PTK session
    world.then_unassociated_station_is_cleared(TEST_STA_95);
    world.then_unassociated_station_is_cleared(TEST_STA_96);
}

// Scenario: Roaming Auth seq 1 to a new AP clears the prior AP's association
// and PTK session before the station associates and authorizes on the new AP.
#[tokio::test]
async fn test_roam_auth_seq1_clears_prior_ap_and_session() {
    // Given two WPA2 APs and a station authenticated, associated, and authorized on AP 1
    let mut world = ApWorld::new().await;
    let old_id = world.given_old_wpa_ap().await;
    world.given_authorized_wpa_station(TEST_STA_95, "OldWpaAP", "OldPassword").await;
    world.given_a_registered_ap_with_wpa("WpaAP", "CorrectPassword").await;
    let (new_id, new_bssid) = (world.primary_ap_id(), world.ap_bssid);

    // When the station sends Auth seq 1 to AP 2
    world.when_station_sends_open_auth_to_bssid(TEST_STA_95, new_bssid).await;

    // Then AP 1's association, BSSID mapping, and PTK session are cleared
    world.then_ap_associations_are(old_id, &[]).await;
    world.then_unassociated_station_is_cleared(TEST_STA_95);

    // When the station associates and completes WPA2 authorization on AP 2
    world.given_authorized_wpa_station(TEST_STA_95, "WpaAP", "CorrectPassword").await;

    // Then AP 2 holds active state and PTK session while AP 1 stays empty
    world.then_ap_associations_are(old_id, &[]).await;
    world.then_ap_associations_are(new_id, &[TEST_STA_95]).await;
    world.then_station_state_is(TEST_STA_95, new_bssid);
    world.then_station_has_ptk_session_on_bssid(TEST_STA_95, OLD_BSSID, false);
    world.then_station_has_ptk_session(TEST_STA_95, true);
}

// Scenario: Unassociated EAPOL-Start on another Enterprise AP is ignored and keeps port blocked.
#[tokio::test]
async fn test_unassociated_enterprise_eapol_start_keeps_other_ap_blocked() {
    // Given a blocked WPA2 station with M1 drained and an Enterprise AP it is not associated to
    let mut world = ApWorld::new().await;
    world.given_a_registered_ap_with_wpa("WpaAP", "CorrectPassword").await;
    world.given_station_has_pending_wpa_handshake(TEST_STA_94).await;
    world.given_extra_enterprise_ap("EntAP", ENTERPRISE_BSSID).await;
    // When the unassociated station sends EAPOL-Start to the Enterprise AP's BSSID
    world.when_station_sends_eapol_start_to_bssid(TEST_STA_94, ENTERPRISE_BSSID).await;
    // Then no EAP frame is emitted and the station remains blocked on its associated WPA2 AP
    world.then_station_stays_blocked_without_eapol(TEST_STA_94).await;
}

// Scenario: Broadcast-addr1 EAPOL-Start is ignored and keeps Enterprise port blocked.
#[tokio::test]
async fn test_broadcast_eapol_start_keeps_enterprise_port_blocked() {
    // Given a blocked station associated to an Enterprise AP
    let mut world = ApWorld::new().await;
    world.given_an_enterprise_ap("EntAP").await;
    world.given_station_is_associated(TEST_STA_93).await;
    // When the station sends a broadcast-addr1 EAPOL-Start frame
    world.when_station_sends_eapol_start_to_bssid(TEST_STA_93, [0xff; 6].into()).await;
    // Then no EAP frame is emitted and the controlled port stays blocked
    world.then_station_stays_blocked_without_eapol(TEST_STA_93).await;
}

// Scenario: FromDS=1 EAPOL-Start is ignored and keeps Enterprise port blocked.
#[tokio::test]
async fn test_from_ds_eapol_start_keeps_enterprise_port_blocked() {
    // Given a blocked station associated to an Enterprise AP
    let mut world = ApWorld::new().await;
    world.given_an_enterprise_ap("EntAP").await;
    world.given_station_is_associated(TEST_STA_93).await;
    // When the station sends an EAPOL-Start frame with FromDS=1
    world.when_station_sends_from_ds_eapol_start(TEST_STA_93).await;
    // Then no EAP frame is emitted and the controlled port stays blocked
    world.then_station_stays_blocked_without_eapol(TEST_STA_93).await;
}

// Scenario: Replaying an old AP M4 after roaming does not authorize the new AP.
#[tokio::test]
async fn test_stale_old_ap_m4_after_roam_does_not_authorize_new_ap() {
    // Given a station capturing M4 on OldWpaAP before associating to a new WPA2 AP
    let mut world = ApWorld::new().await;
    world.given_old_wpa_ap().await;
    world.given_station_is_associated(TEST_STA_95).await;
    let old_m4 = world.given_captured_eapol_m4(TEST_STA_95, "OldWpaAP", "OldPassword").await;
    world.given_a_registered_ap_with_wpa("WpaAP", "CorrectPassword").await;
    world.given_station_is_associated(TEST_STA_95).await;
    // When the station replays the stale M4 frame (Addr1 = OLD_BSSID)
    world.when_station_replays_eapol_frame(TEST_STA_95, old_m4).await;
    // Then the new AP keeps the controlled port blocked without installing a PTK session
    world.then_station_port_state_is(TEST_STA_95, world.ap_bssid, true);
    world.then_station_has_ptk_session(TEST_STA_95, false);
}

// Scenario: Creating a WPA2 AP registers its controlled-port flag in SharedKeyStore.
#[tokio::test]
async fn test_wpa_ap_creation_registers_controlled_port_flag() {
    // Given an initialized AP world
    let mut world = ApWorld::new().await;
    // When the default WPA2 AP is created
    world.given_default_wpa_ap("WpaAP", "CorrectPassword").await;
    // Then the default AP requires a controlled port
    world.then_default_ap_requires_controlled_port();
}

// Scenario: Resetting the AP service preserves the default WPA2 AP controlled-port flag.
#[tokio::test]
async fn test_reset_preserves_default_ap_controlled_port_flag() {
    // Given the default WPA2 AP
    let mut world = ApWorld::new().await;
    world.given_default_wpa_ap("WpaAP", "CorrectPassword").await;
    // When the AP service is reset
    world.when_ap_service_is_reset().await;
    // Then the default AP still requires a controlled port
    world.then_default_ap_requires_controlled_port();
}

// Scenario: Resetting the AP service keeps re-associated stations blocked on the default WPA2 AP.
#[tokio::test]
async fn test_reset_blocks_reassociated_station_on_default_wpa_ap() {
    // Given the default WPA2 AP has been reset
    let mut world = ApWorld::new().await;
    world.given_default_wpa_ap("WpaAP", "CorrectPassword").await;
    world.when_ap_service_is_reset().await;
    // When a station authenticates and associates to the default WPA2 AP
    world.when_station_sends_open_auth(parse_mac(TEST_STA_99)).await;
    world.given_station_is_associated(TEST_STA_99).await;
    // Then the station starts with a blocked controlled port
    world.then_station_port_state_is(TEST_STA_99, world.ap_bssid, true);
}
