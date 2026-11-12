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

use std::{sync::Arc, time::Duration};

use actor_framework::ResourceActor;
use ap_actor::{ApActor, ApClient, ApConfig, SharedKeyStore};
use common::util::netsim_logger::{ErrorStatsLayer, ErrorStatsTable};
use netsim_model::WifiMode;
use netsim_packets::{
    AssociationRequestFixedFields, BeaconFixedFields, BeaconFrameHeader, EAPOL_TYPE_KEY,
    EAPOL_VERSION, FrameControl, Ieee80211, MacAddr, MacHeader3Addr, SequenceControl,
    management_subtype,
};
use rand::prelude::*;
use tokio::sync::mpsc;
use tracing::info;
use tracing_subscriber::layer::SubscriberExt;
use zerocopy::{IntoBytes, Ref, U16};

const MGMT_AUTH_FC: u16 = 0x00B0;
const EAPOL_M1_DELAY: Duration = Duration::from_millis(150);
const DELAYED_FRAME_READY_WAIT: Duration = EAPOL_M1_DELAY.saturating_add(Duration::from_millis(10));

pub fn generate_random_mac() -> [u8; 6] {
    let mut rng = rand::rng();
    [0x02, 0x00, 0x00, 0x00, rng.random(), rng.random()]
}

pub struct ApWorld {
    pub client: ApClient,
    pub tx_to_ap: Option<mpsc::UnboundedSender<bytes::Bytes>>,
    pub rx_from_ap: Option<mpsc::UnboundedReceiver<bytes::Bytes>>,
    pub ap_id: Option<u32>,
    pub ap_bssid: MacAddr,
    pub actor_handle: Option<tokio::task::JoinHandle<()>>,
    pub next_ap_id: u32,
    pub shared_keys: Arc<SharedKeyStore>,
    pub error_stats: Arc<ErrorStatsTable>,
}

impl ApWorld {
    pub async fn new() -> Self {
        Self::build(false)
    }

    pub async fn with_error_stats() -> Self {
        Self::build(true)
    }

    fn build(record_error_stats: bool) -> Self {
        netsim_testing::setup(None);
        let shared_keys = Arc::new(SharedKeyStore::new());
        let error_stats = Arc::new(ErrorStatsTable::new());
        let ap_actor_impl = ApActor::new(Arc::clone(&shared_keys));
        let (runner, client_base) = ResourceActor::new(32);
        let client = ApClient::new(client_base);

        let handle = if record_error_stats {
            let subscriber = tracing_subscriber::registry()
                .with(ErrorStatsLayer::with_table(Arc::clone(&error_stats)));
            tokio::spawn(tracing::instrument::WithSubscriber::with_subscriber(
                runner.run(ap_actor_impl),
                subscriber,
            ))
        } else {
            tokio::spawn(runner.run(ap_actor_impl))
        };

        Self {
            client,
            tx_to_ap: None,
            rx_from_ap: None,
            ap_id: None,
            ap_bssid: MacAddr::new([0x02, 0x00, 0x00, 0x00, 0x00, 0x01]),
            actor_handle: Some(handle),
            next_ap_id: 1001,
            shared_keys,
            error_stats,
        }
    }

    pub fn base_config(ssid: &str, bssid: &str) -> ApConfig {
        ApConfig {
            ssid: ssid.to_string(),
            bssid: bssid.parse().expect("Invalid BSSID"),
            channel: 36,
            hw_mode: WifiMode::Ax,
            wpa_passphrase: None,
            beacon_interval: 100,
            country_code: None,
            dtim_period: 2,
            hidden_ssid: false,
            sae: false,
            wmm_enabled: true,
            enterprise_enabled: false,
            mac_acl_mode: 0,
            mac_acl_list: vec![],
            ftm_responder_enabled: true,
            position: netsim_model::Position::default(),
        }
    }

    pub async fn given_a_registered_ap_with_config(&mut self, config: ApConfig) {
        info!("Given a registered AP '{}'", config.ssid);
        let id = self.next_ap_id;
        self.next_ap_id += 1;
        self.ap_bssid = config.bssid;
        self.client.create_ap(Some(id), config).await.expect("Failed to create AP");
        self.ap_id = Some(id);

        if self.tx_to_ap.is_none() {
            let (tx_to_ap, rx_for_ap) = mpsc::unbounded_channel::<bytes::Bytes>();
            let (tx_from_ap, rx_from_ap) = mpsc::unbounded_channel();

            let stream = Box::pin(tokio_stream::wrappers::UnboundedReceiverStream::new(rx_for_ap));
            self.client
                .register(stream, tx_from_ap, self.shared_keys.clone(), Duration::from_millis(100))
                .await
                .expect("Failed to register");
            self.tx_to_ap = Some(tx_to_ap);
            self.rx_from_ap = Some(rx_from_ap);
        }
    }

    pub async fn given_a_registered_ap(&mut self, ssid: &str) {
        self.given_a_registered_ap_with_wpa(ssid, "").await;
    }

    pub async fn given_a_registered_ap_with_wpa(&mut self, ssid: &str, passphrase: &str) {
        info!("Given a registered AP '{}' with WPA", ssid);
        let wpa_passphrase =
            if passphrase.is_empty() { None } else { Some(passphrase.to_string()) };
        let config = ApConfig {
            channel: 6,
            hw_mode: WifiMode::G,
            wpa_passphrase,
            ..Self::base_config(ssid, "02:00:00:00:00:01")
        };
        self.given_a_registered_ap_with_config(config).await;
    }

    pub async fn given_a_wifi6_ap(&mut self, ssid: &str) {
        info!("Given a WiFi 6 AP '{}'", ssid);
        let config = ApConfig {
            bssid: MacAddr::new(generate_random_mac()),
            ..Self::base_config(ssid, "02:00:00:00:00:01")
        };
        self.given_a_registered_ap_with_config(config).await;
    }

    pub async fn given_a_sae_ap(&mut self, ssid: &str, passphrase: &str) -> MacAddr {
        let config = ApConfig {
            wpa_passphrase: Some(passphrase.to_string()),
            sae: true,
            ..Self::base_config(ssid, "02:00:00:00:00:01")
        };
        let bssid = config.bssid;
        self.given_a_registered_ap_with_config(config).await;
        bssid
    }

    pub async fn given_an_enterprise_ap(&mut self, ssid: &str) {
        let config =
            ApConfig { enterprise_enabled: true, ..Self::base_config(ssid, "02:00:00:00:00:10") };
        self.given_a_registered_ap_with_config(config).await;
    }

    pub async fn given_an_ap_with_acl(&mut self, ssid: &str, mode: u8, list: Vec<MacAddr>) {
        let config = ApConfig {
            mac_acl_mode: mode,
            mac_acl_list: list,
            ..Self::base_config(ssid, "02:00:00:00:00:10")
        };
        self.given_a_registered_ap_with_config(config).await;
    }

    fn header_to_ap(&self, fc: u16, src_mac: MacAddr) -> MacHeader3Addr {
        MacHeader3Addr::new(
            FrameControl::new(fc),
            0,
            self.ap_bssid,
            src_mac,
            self.ap_bssid,
            SequenceControl::new(0),
        )
    }
    pub async fn then_beacon_is_received(&mut self, ssid: &str) {
        info!("Then the World receives a Beacon for '{}'", ssid);
        let rx = self.rx_from_ap.as_mut().expect("AP not registered");
        // Drain until we find a beacon or timeout
        let start = std::time::Instant::now();
        while start.elapsed() < Duration::from_secs(2) {
            let msg = match tokio::time::timeout(Duration::from_millis(200), rx.recv()).await {
                Ok(Some(msg)) => msg,
                _ => continue,
            };

            // Parse Beacon Header
            let (header, rest) = match Ref::<&[u8], BeaconFrameHeader>::from_prefix(&msg) {
                Ok(res) => res,
                _ => continue,
            };

            // Check Frame Control (Mgmt=0, Beacon=8)
            if header.frame_control.frame_type() != 0 || header.frame_control.frame_subtype() != 8 {
                continue;
            }

            // Parse Fixed Fields (Timestamp, Interval, Caps)
            let (_fixed, rest) = match Ref::<&[u8], BeaconFixedFields>::from_prefix(rest) {
                Ok(res) => res,
                _ => continue,
            };

            // Parse SSID IE (Tag 0)
            if rest.len() < 2 || rest[0] != 0 {
                continue;
            }
            let len = rest[1] as usize;
            if rest.len() < 2 + len {
                continue;
            }

            let received_ssid = String::from_utf8_lossy(&rest[2..2 + len]);
            if received_ssid == ssid {
                return; // Found it
            }
        }
        panic!("Did not receive beacon for SSID: {}", ssid);
    }

    pub async fn when_station_sends_assoc_req<M>(&mut self, src_mac: M)
    where
        M: TryInto<MacAddr>,
        M::Error: std::fmt::Debug,
    {
        let src_mac = src_mac.try_into().expect("Invalid MAC address");
        info!("When a Station sends an Association Request from {}", src_mac);
        let header = self.header_to_ap(0x0000, src_mac);
        let fixed = AssociationRequestFixedFields {
            capabilities: U16::new(0x0100),
            listen_interval: U16::new(0x000A),
        };
        let mut frame = Vec::new();
        frame.extend_from_slice(header.as_bytes());
        frame.extend_from_slice(fixed.as_bytes());

        let tx = self.tx_to_ap.as_mut().expect("AP not registered");
        tx.send(bytes::Bytes::from(frame)).expect("Failed to send Assoc Req");
    }

    pub async fn then_station_receives_assoc_resp<M>(&mut self, dst_mac: M)
    where
        M: TryInto<MacAddr>,
        M::Error: std::fmt::Debug,
    {
        let dst_mac = dst_mac.try_into().expect("Invalid MAC address");
        info!("Then the Station receives an Association Response at {}", dst_mac);
        let rx = self.rx_from_ap.as_mut().expect("AP not registered");
        let start = std::time::Instant::now();
        while start.elapsed() < Duration::from_secs(2) {
            match tokio::time::timeout(Duration::from_millis(200), rx.recv()).await {
                Ok(Some(msg)) => {
                    if let Ok(frame) = Ieee80211::decode(&msg)
                        && frame.stype() == management_subtype::ASSOCIATION_RESPONSE
                        && frame.get_addr1() == dst_mac
                    {
                        // DA == Station
                        return; // Success
                    }
                }
                _ => continue,
            }
        }
        panic!("Did not receive Assoc Resp for {}", dst_mac);
    }

    pub async fn when_station_sends_open_auth(&mut self, src_mac: MacAddr) {
        use netsim_packets::AuthenticationFixedFields;

        info!("When a Station sends an Authentication Request from {}", src_mac);
        let header = self.header_to_ap(0x00B0, src_mac);
        let fixed = AuthenticationFixedFields {
            algorithm: U16::new(0), // Open System
            sequence: U16::new(1),
            status: U16::new(0),
        };

        let mut frame = Vec::new();
        frame.extend_from_slice(header.as_bytes());
        frame.extend_from_slice(fixed.as_bytes());
        let tx = self.tx_to_ap.as_mut().expect("AP not registered");
        tx.send(bytes::Bytes::from(frame)).expect("Failed to send Auth Req");
    }

    pub async fn when_station_sends_sae_auth(
        &mut self,
        client_mac: MacAddr,
        sequence: u16,
        status: u16,
        body: &[u8],
    ) {
        use netsim_packets::AuthenticationFixedFields;

        let header = self.header_to_ap(0x00B0, client_mac);
        let fixed = AuthenticationFixedFields {
            algorithm: U16::new(3), // SAE
            sequence: U16::new(sequence),
            status: U16::new(status),
        };

        let mut auth_frame = Vec::new();
        auth_frame.extend_from_slice(header.as_bytes());
        auth_frame.extend_from_slice(fixed.as_bytes());
        auth_frame.extend_from_slice(body);

        let tx = self.tx_to_ap.as_mut().expect("AP not registered");
        tx.send(bytes::Bytes::from(auth_frame)).expect("Failed to send SAE Auth frame");
    }

    pub async fn when_station_sends_deauth(&mut self, station_mac: MacAddr, reason_code: u16) {
        let header = self.header_to_ap(0x00C0, station_mac);

        let mut frame = Vec::new();
        frame.extend_from_slice(header.as_bytes());
        frame.extend_from_slice(&reason_code.to_le_bytes());

        let tx = self.tx_to_ap.as_mut().expect("AP not registered");
        tx.send(bytes::Bytes::from(frame)).expect("Failed to send Deauth frame");
    }

    fn build_eapol_frame(
        &self,
        client_mac: MacAddr,
        frame_control: u16,
        version: u8,
        packet_type: u8,
        body: &[u8],
    ) -> Vec<u8> {
        use netsim_packets::{EapolHeader, LlcSnapHeader, control_field, sap};

        let mut frame = Vec::new();
        let header = self.header_to_ap(frame_control, client_mac);
        frame.extend_from_slice(header.as_bytes());

        let llc = LlcSnapHeader::new(
            sap::SNAP,
            sap::SNAP,
            control_field::UI,
            [0x00, 0x00, 0x00],
            netsim_packets::ether_type::EAPOL,
        );
        frame.extend_from_slice(llc.as_bytes());

        let eapol = EapolHeader::new(version, packet_type, body.len() as u16);
        frame.extend_from_slice(eapol.as_bytes());
        frame.extend_from_slice(body);
        frame
    }

    pub async fn when_station_sends_eapol_start(&mut self, client_mac: MacAddr) {
        use netsim_packets::EAPOL_TYPE_START;

        let start_frame = self.build_eapol_frame(client_mac, 0x0008, 1, EAPOL_TYPE_START, &[]);
        let tx = self.tx_to_ap.as_mut().expect("AP not registered");
        tx.send(bytes::Bytes::from(start_frame)).expect("Failed to send EAPOL-Start");
    }

    pub async fn when_station_sends_eap_response_identity(
        &mut self,
        client_mac: MacAddr,
        id: u8,
        identity: &[u8],
    ) {
        use netsim_packets::{EAP_CODE_RESPONSE, EAP_TYPE_IDENTITY};

        let mut type_and_data = Vec::with_capacity(1 + identity.len());
        type_and_data.push(EAP_TYPE_IDENTITY);
        type_and_data.extend_from_slice(identity);
        self.when_station_sends_eap_packet(client_mac, EAP_CODE_RESPONSE, id, &type_and_data).await;
    }

    pub async fn when_station_sends_eap_packet(
        &mut self,
        client_mac: MacAddr,
        code: u8,
        id: u8,
        eap_type_and_data: &[u8],
    ) {
        use netsim_packets::{EAPOL_TYPE_PACKET, EapHeader};

        let total_len = (4 + eap_type_and_data.len()) as u16;
        let eap_header = EapHeader::new(code, id, total_len);
        let mut body = Vec::with_capacity(total_len as usize);
        body.extend_from_slice(eap_header.as_bytes());
        body.extend_from_slice(eap_type_and_data);

        let frame = self.build_eapol_frame(client_mac, 0x0008, 1, EAPOL_TYPE_PACKET, &body);
        let tx = self.tx_to_ap.as_mut().expect("AP not registered");
        tx.send(bytes::Bytes::from(frame)).expect("Failed to send EAP packet");
    }

    pub async fn when_station_sends_raw_eapol_packet(
        &mut self,
        client_mac: MacAddr,
        raw_body: &[u8],
    ) {
        use netsim_packets::EAPOL_TYPE_PACKET;

        let frame = self.build_eapol_frame(client_mac, 0x0008, 1, EAPOL_TYPE_PACKET, raw_body);
        let tx = self.tx_to_ap.as_mut().expect("AP not registered");
        tx.send(bytes::Bytes::from(frame)).expect("Failed to send raw EAPOL packet");
    }

    pub async fn then_station_receives_eap_request_identity(&mut self) -> u8 {
        use netsim_packets::{
            EAP_CODE_REQUEST, EAP_TYPE_IDENTITY, EAPOL_TYPE_PACKET, EapHeader, EapolHeader,
        };

        let msg = self
            .recv_frame(|frame, msg| {
                if frame.stype() == management_subtype::BEACON {
                    return false;
                }
                msg.len() > 32 && msg[30] == 0x88 && msg[31] == 0x8E
            })
            .await;

        let payload = &msg[32..];
        let eapol_header = Ref::<&[u8], EapolHeader>::from_prefix(payload).unwrap().0;
        assert_eq!(eapol_header.packet_type, EAPOL_TYPE_PACKET);

        let eap_packet = &payload[4..];
        let eap_header = Ref::<&[u8], EapHeader>::from_prefix(eap_packet).unwrap().0;
        assert_eq!(eap_header.code, EAP_CODE_REQUEST);
        assert_eq!(eap_packet[4], EAP_TYPE_IDENTITY);
        eap_header.id
    }

    pub async fn then_station_receives_eap_success(&mut self, expected_id: u8) {
        use netsim_packets::{EAP_CODE_SUCCESS, EapHeader};

        let msg = self
            .recv_frame(|frame, msg| {
                if frame.stype() == management_subtype::BEACON {
                    return false;
                }
                msg.len() > 32 && msg[30] == 0x88 && msg[31] == 0x8E
            })
            .await;

        let payload = &msg[32..];
        let eap_packet = &payload[4..];
        let eap_header = Ref::<&[u8], EapHeader>::from_prefix(eap_packet).unwrap().0;
        assert_eq!(eap_header.code, EAP_CODE_SUCCESS);
        assert_eq!(eap_header.id, expected_id);
    }

    pub async fn then_no_eap_or_auth_frame_is_received(&mut self) {
        let rx = self.rx_from_ap.as_mut().expect("AP not registered");
        let start = std::time::Instant::now();
        while start.elapsed() < Duration::from_millis(250) {
            let Ok(Some(msg)) = tokio::time::timeout(Duration::from_millis(50), rx.recv()).await
            else {
                continue;
            };
            if let Ok(frame) = Ieee80211::decode(&msg) {
                if frame.stype() == management_subtype::BEACON {
                    continue;
                }
                assert_ne!(
                    frame.stype(),
                    management_subtype::AUTHENTICATION,
                    "Unexpected Authentication response received"
                );
            }
            assert!(
                !(msg.len() > 32 && msg[30] == 0x88 && msg[31] == 0x8E),
                "Unexpected EAPOL response received"
            );
        }
    }

    pub async fn when_force_disconnect_is_requested_for(&mut self, mac: &str) {
        info!("When a force disconnect is requested for '{}'", mac);
        let id = self.ap_id.expect("AP not registered");
        self.client
            .update_ap_config(id, None, None, Some(vec![mac.to_string()]), None)
            .await
            .expect("Force disconnect failed");
    }

    pub async fn when_station_sends_auth_frame_truncated_after_addr2<M>(&mut self, src_mac: M)
    where
        M: TryInto<MacAddr>,
        M::Error: std::fmt::Debug,
    {
        let src_mac = src_mac.try_into().expect("Invalid MAC address");
        info!("When a Station sends an Auth frame truncated after Addr2 from {}", src_mac);
        let header = self.header_to_ap(MGMT_AUTH_FC, src_mac);
        // FC(2) + Duration(2) + Addr1(6) + Addr2(6), without Addr3 and
        // Sequence Control.
        let truncated = header.as_bytes().get(..16).expect("header shorter than 16 bytes");
        let tx = self.tx_to_ap.as_mut().expect("AP not registered");
        tx.send(bytes::Bytes::copy_from_slice(truncated)).expect("Failed to send Auth frame");
    }

    pub async fn given_station_is_associated(&mut self, src_mac: &str) {
        self.when_station_sends_assoc_req(src_mac).await;
        self.then_station_receives_assoc_resp(src_mac).await;
    }

    pub async fn given_ap_beacons_disabled(&mut self) {
        let id = self.ap_id.expect("AP not registered");
        self.client
            .update_ap_config(id, None, None, None, Some(false))
            .await
            .expect("Failed to disable AP beacons");
    }

    pub async fn when_ap_enabled_after_delayed_frame_ready(&mut self) {
        tokio::time::sleep(DELAYED_FRAME_READY_WAIT).await;
        let id = self.ap_id.expect("AP not registered");
        self.client
            .update_ap_config(id, None, None, None, Some(true))
            .await
            .expect("Failed to enable AP beacons");
    }

    pub async fn when_non_existent_ap_is_deleted(&mut self, id: u32) {
        self.client.destroy_ap(id).await.expect("Failed to invoke destroy_ap");
    }

    pub async fn when_disconnect_is_requested_for(&mut self, mac: &str) {
        let id = self.ap_id.expect("AP not registered");
        self.client.disconnect(id, mac.to_string()).await.expect("Failed to disconnect station");
    }

    pub async fn when_malformed_raw_bytes_sent(&mut self, raw: &[u8]) {
        let tx = self.tx_to_ap.as_mut().expect("AP not registered");
        tx.send(bytes::Bytes::copy_from_slice(raw)).expect("Failed to send malformed frame");
    }

    pub async fn when_station_sends_auth_frame_with_truncated_fixed_fields(
        &mut self,
        src_mac: &str,
    ) {
        const TRUNCATED_AUTH_FIXED_BYTES: [u8; 2] = [0x00, 0x00];
        let src_mac: MacAddr = src_mac.parse().expect("Invalid MAC address");
        let header = self.header_to_ap(MGMT_AUTH_FC, src_mac);
        let mut frame = header.as_bytes().to_vec();
        frame.extend_from_slice(&TRUNCATED_AUTH_FIXED_BYTES);
        let tx = self.tx_to_ap.as_mut().expect("AP not registered");
        tx.send(bytes::Bytes::from(frame)).expect("Failed to send Auth frame");
    }

    pub async fn when_station_sends_truncated_eapol_key(&mut self, src_mac: MacAddr) {
        let frame = self.build_eapol_frame(src_mac, 0x0108, EAPOL_VERSION, EAPOL_TYPE_KEY, &[]);
        self.tx_to_ap
            .as_mut()
            .expect("AP not registered")
            .send(bytes::Bytes::from(frame))
            .expect("Failed to send EAPOL data frame");
    }

    /// Waits up to 2s for the recorded error stats to equal `expected`.
    pub async fn then_error_stats_are(&mut self, expected: &[(u32, u32)]) {
        info!("Then the error stats are {:?}", expected);
        let start = std::time::Instant::now();
        while self.error_stats.snapshot() != expected && start.elapsed() < Duration::from_secs(2) {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert_eq!(self.error_stats.snapshot(), expected);
    }

    pub async fn when_ap_is_deleted(&mut self) {
        info!("When the AP is deleted");
        if let Some(id) = self.ap_id {
            self.client.destroy_ap(id).await.expect("Failed to destroy AP");
            self.ap_id = None;
        }
    }

    pub async fn then_no_beacons_are_received(&mut self) {
        info!("Then no more beacons are received");
        let rx = self.rx_from_ap.as_mut().expect("AP not registered");
        // Drain
        while rx.try_recv().is_ok() {}

        let result = tokio::time::timeout(Duration::from_millis(500), rx.recv()).await;
        if result.is_ok() {
            panic!("Received packet (likely beacon) after AP deletion");
        }
    }

    pub async fn when_input_stream_closed(&mut self) {
        // Drop tx_to_ap
        self.tx_to_ap = None;
    }

    pub async fn when_sink_closed(&mut self) {
        // Drop rx_from_ap
        self.rx_from_ap = None;
    }

    pub async fn then_actor_shuts_down(&mut self) {
        if let Some(handle) = self.actor_handle.take() {
            match tokio::time::timeout(Duration::from_secs(2), handle).await {
                Ok(Ok(_)) => {} // Success
                Ok(Err(e)) => panic!("Actor joined with error: {:?}", e),
                Err(e) => panic!("Actor did not shut down in time: {e}"),
            }
        }
    }
    pub async fn recv_frame<F>(&mut self, filter: F) -> Vec<u8>
    where
        F: Fn(&Ieee80211, &[u8]) -> bool,
    {
        let rx = self.rx_from_ap.as_mut().expect("AP not registered");
        let start = std::time::Instant::now();
        while start.elapsed() < Duration::from_secs(2) {
            let msg = match tokio::time::timeout(Duration::from_millis(200), rx.recv()).await {
                Ok(Some(msg)) => msg,
                _ => continue,
            };

            if let Ok(frame) = Ieee80211::decode(&msg)
                && filter(&frame, &msg)
            {
                // Let the filter decide whether to accept the frame (including Beacons)
                return msg.to_vec();
            }
        }
        panic!("Timed out waiting for frame");
    }
}
