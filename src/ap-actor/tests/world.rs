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

use std::time::Duration;

use actor_framework::ResourceActor;
use ap_actor::{ApActor, ApClient, ApConfig, SharedKeyStore};
use netsim_model::WifiMode;
use netsim_packets::{
    AssociationRequestFixedFields, BeaconFixedFields, BeaconFrameHeader, FrameControl, Ieee80211,
    MacAddr, MacHeader3Addr, SequenceControl, management_subtype,
};
use rand::prelude::*;
use tokio::sync::mpsc;
use tracing::info;
use zerocopy::{IntoBytes, Ref, U16};

pub fn generate_random_mac() -> [u8; 6] {
    let mut rng = rand::rng();
    [0x02, 0x00, 0x00, 0x00, rng.random(), rng.random()]
}

pub struct ApWorld {
    pub client: ApClient,
    pub tx_to_ap: Option<mpsc::UnboundedSender<bytes::Bytes>>,
    pub rx_from_ap: Option<mpsc::UnboundedReceiver<bytes::Bytes>>,
    pub ap_id: Option<u32>,
    pub actor_handle: Option<tokio::task::JoinHandle<()>>,
    pub next_ap_id: u32,
    pub shared_keys: std::sync::Arc<SharedKeyStore>,
}

impl ApWorld {
    pub async fn new() -> Self {
        netsim_testing::setup(None);
        let shared_keys = std::sync::Arc::new(SharedKeyStore::new());
        let ap_actor_impl = ApActor::new(shared_keys.clone());
        let (runner, client_base) = ResourceActor::new(32);
        let client = ApClient::new(client_base);

        let handle = tokio::spawn(runner.run(ap_actor_impl));

        Self {
            client,
            tx_to_ap: None,
            rx_from_ap: None,
            ap_id: None,
            actor_handle: Some(handle),
            next_ap_id: 1001,
            shared_keys,
        }
    }

    pub async fn given_a_registered_ap_with_config(&mut self, config: ApConfig) {
        info!("Given a registered AP '{}'", config.ssid);
        let id = self.next_ap_id;
        self.next_ap_id += 1;
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
            ssid: ssid.to_string(),
            bssid: MacAddr::new([0x02, 0x00, 0x00, 0x00, 0x00, 0x01]),
            channel: 6,
            hw_mode: WifiMode::G,

            wpa_passphrase,
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
        };
        self.given_a_registered_ap_with_config(config).await;
    }

    pub async fn given_a_wifi6_ap(&mut self, ssid: &str) {
        info!("Given a WiFi 6 AP '{}'", ssid);
        let config = ApConfig {
            ssid: ssid.to_string(),
            bssid: MacAddr::new(generate_random_mac()),
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
        };
        self.given_a_registered_ap_with_config(config).await;
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
        // Construct Assoc Req
        let tx = self.tx_to_ap.as_mut().expect("AP not registered");

        // Minimal Assoc Req Frame
        let mut frame = Vec::new();
        let bssid = MacAddr::new([0x02, 0x00, 0x00, 0x00, 0x00, 0x01]);

        // Header
        let header = MacHeader3Addr::new(
            FrameControl::new(0x0000), // Mgmt, Assoc Req
            0,
            bssid, // DA
            src_mac,
            bssid, // BSSID
            SequenceControl::new(0),
        );
        frame.extend_from_slice(header.as_bytes());

        // Fixed Fields
        let fixed = AssociationRequestFixedFields {
            capabilities: U16::new(0x0100), // Example caps
            listen_interval: U16::new(0x000A),
        };
        frame.extend_from_slice(fixed.as_bytes());

        // SSID IE
        // Tag 0, Len 0 (Wildcard or specific? specific usually)
        // Let's assume we associate to the AP's SSID.
        // For now just empty SSID or whatever.

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

            if let Ok(frame) = Ieee80211::decode(&msg) {
                // Let the filter decide whether to accept the frame (including Beacons)
                if filter(&frame, &msg) {
                    return msg.to_vec();
                }
            }
        }
        panic!("Timed out waiting for frame");
    }
}
