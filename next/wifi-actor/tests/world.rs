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

use std::sync::Arc;

use actor_framework::ResourceActor;
use ap_actor::{ApActor, ApClient};
use bytes::Bytes;
use device_actor::DeviceActor;
use device_api::{DeviceAction, DeviceId};
use netsim_model::{ChipClient, ChipCreate, ChipId, Position};
use netsim_packets::{
    EthernetFrame, FrameDirection, FrameType, Ieee80211, Ieee80211ToAp, MacAddr, MacAddress,
    ether_type,
};
use slirp_actor::SlirpActor;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tracing::{error, info, warn};
use wifi_actor::WifiActor;
use zerocopy::IntoBytes;

use crate::hwsim_helper::wrap_ethernet_in_hwsim;

pub struct ChipChannels {
    pub id: u32,
    pub mac: [u8; 6],
    pub stream_rx: mpsc::Receiver<Bytes>, // Output from Actor (Source)
    pub sink_tx: mpsc::Sender<Bytes>,     // Input to Actor (Sink)
}

pub struct World {
    pub wifi_client: wifi_actor::WifiClient,
    pub ap_client: ApClient,
    pub chips: Vec<ChipChannels>,
    // Injector for AP packets
    pub ap_injector: mpsc::UnboundedSender<Bytes>,
    pub device_action_rx: mpsc::UnboundedReceiver<DeviceAction>,
    pub baseline_stats: Option<netsim_proto::stats::WifiStats>,
    pub mock_clock: std::sync::Arc<wifi_actor::MockClock>,
    pub shared_keys: std::sync::Arc<ap_actor::SharedKeyStore>,
}

impl World {
    pub async fn new() -> Self {
        Self::new_internal(None).await
    }

    pub async fn new_with_gateway(gateway: Box<dyn wifi_actor::GatewayTrait>) -> Self {
        Self::new_internal(Some(gateway)).await
    }

    async fn new_internal(gateway: Option<Box<dyn wifi_actor::GatewayTrait>>) -> Self {
        let _ = tracing_subscriber::fmt().with_test_writer().try_init();
        // Setup dependencies
        let slirp_actor_impl = SlirpActor::new(Default::default(), None, None).await;
        let (slirp_runner, slirp_client) = slirp_actor::new();
        tokio::spawn(slirp_runner.run(slirp_actor_impl));

        let shared_keys = Arc::new(ap_actor::SharedKeyStore::new());
        let ap_actor_impl = ApActor::new(shared_keys.clone());

        let (ap_runner, ap_client_base) = ResourceActor::new(32);
        tokio::spawn(ap_runner.run(ap_actor_impl));

        // Test DeviceClient
        let (device_tx, device_rx) = mpsc::unbounded_channel();
        // Create a Mock Device Client to verify that WifiActor interacts with
        // DeviceActor correctly.
        let mut mock_device = actor_framework::MockActorClient::<DeviceActor>::new();

        // Forward all DeviceActions to the channel for verification.
        // We need to define a closure that sets up the mock behavior, because clone_box
        // needs to return a NEW mock with the SAME behavior.
        let device_tx_clone = device_tx.clone();
        let setup_mock = move |mock: &mut actor_framework::MockActorClient<DeviceActor>| {
            let tx = device_tx_clone.clone();
            mock.expect_perform_action().returning(move |_id, action| {
                let _ = tx.send(action);
                Ok(device_api::DeviceActionResult::Success)
            });
        };

        // Apply setup to the initial mock
        setup_mock(&mut mock_device);

        // Handle clone_box: Return a new mock with the same setup
        let tx_for_clone = device_tx.clone();
        mock_device.expect_clone_box().returning(move || {
            let mut new_mock = actor_framework::MockActorClient::<DeviceActor>::new();
            let tx = tx_for_clone.clone();
            new_mock.expect_perform_action().returning(move |_id, action| {
                let _ = tx.send(action);
                Ok(device_api::DeviceActionResult::Success)
            });
            Box::new(new_mock)
        });

        // Initialize the DeviceClient with the mock.
        let device_client = device_actor::DeviceClient::new(Box::new(mock_device));

        // Create ApClient with interceptor to capture the downlink sink
        let (capture_tx, mut capture_rx) = mpsc::unbounded_channel();
        let capture_tx_arc = Arc::new(capture_tx);

        let ap_client_interceptor = {
            let capture_tx = capture_tx_arc.clone();
            move |sink: &mpsc::UnboundedSender<Bytes>| {
                let _ = capture_tx.send(sink.clone());
            }
        };

        let spying_ap_client =
            ApClient::new_with_interceptor(ap_client_base, ap_client_interceptor);
        let spying_ap_client_arc = Arc::new(spying_ap_client.clone());

        let mock_clock = std::sync::Arc::new(wifi_actor::MockClock::new());

        let wifi_actor_impl = if let Some(gw) = gateway {
            WifiActor::new_with_gateway(
                Some(spying_ap_client_arc.clone()),
                gw,
                device_client,
                shared_keys.clone(),
                mock_clock.clone(),
                false,
            )
        } else {
            WifiActor::new(
                Some(spying_ap_client_arc.clone()),
                Some(slirp_client),
                device_client,
                None,
                shared_keys.clone(),
                mock_clock.clone(),
                false,
            )
        };

        let (wifi_runner, wifi_client) = wifi_actor::new();
        tokio::spawn(wifi_runner.run(wifi_actor_impl));

        // Wait for registration to capture the injector
        let ap_injector = match tokio::time::timeout(
            std::time::Duration::from_secs(1),
            capture_rx.recv(),
        )
        .await
        {
            Ok(Some(sink)) => sink,
            _ => {
                panic!("Failed to capture AP injection sink via MockApClient::register");
            }
        };

        Self {
            wifi_client,
            ap_client: spying_ap_client,
            chips: Vec::new(),
            ap_injector,
            device_action_rx: device_rx,
            baseline_stats: None,
            mock_clock,
            shared_keys,
        }
    }

    pub async fn given_an_ap(&mut self) -> u32 {
        self.given_an_ap_with_passphrase(None).await
    }

    pub async fn given_a_secure_ap(&mut self) -> u32 {
        self.given_an_ap_with_passphrase(Some("secretpassword".to_string())).await
    }

    async fn given_an_ap_with_passphrase(&mut self, passphrase: Option<String>) -> u32 {
        let ap_config = ap_actor::ApConfig {
            ssid: "TestAP".to_string(),
            bssid: MacAddr::from([0x02, 0x00, 0x00, 0x00, 0x00, 0x00]),
            channel: 6,
            hw_mode: netsim_model::WifiMode::G,
            wpa_passphrase: passphrase,
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
            position: Position::default(),
        };
        let id = 1001; // WifiActor test AP ID
        self.ap_client.create_ap(Some(id), ap_config).await.expect("Failed to create AP");
        // Give it a moment to initialize
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        id
    }

    pub async fn given_a_chip(&mut self, id_val: u32) -> u32 {
        let (stream_tx, stream_rx) = mpsc::channel(128);
        let (sink_tx, sink_rx) = mpsc::channel(128);

        let packet_stream = Box::new(ReceiverStream::new(sink_rx));
        let packet_sink =
            Box::pin(futures::sink::unfold(stream_tx, |tx, item: Bytes| async move {
                let _ = tx.send(item).await;
                Ok::<_, std::io::Error>(tx)
            }));

        let chip = netsim_model::Chip {
            name: "wifi".to_string(),
            manufacturer: "google".to_string(),
            product_name: "test".to_string(),
            kind: netsim_model::ChipKind::WIFI,
            device_id: DeviceId(1),
            variant: Some(netsim_model::ChipVariant::Wifi(netsim_model::Wifi {
                radio: Default::default(),
            })),
            ..Default::default()
        };

        let params =
            ChipCreate { packet_stream: Some(packet_stream), packet_sink: Some(packet_sink), chip };

        self.wifi_client.create(ChipId(id_val), params).await.expect("Failed to create chip");
        let created_id = id_val;

        let src_mac = [0x00, 0x00, 0x00, 0x00, 0x00, created_id as u8];
        let chip = ChipChannels { id: created_id, mac: src_mac, stream_rx, sink_tx };

        // Register chip by sending dummy packet

        let dst_mac = [0x00, 0x00, 0x00, 0x00, 0x00, 0x00]; // Dummy dst
        let ap_mac = [0x02, 0x00, 0x00, 0x00, 0x00, 0x00];
        let eth = Self::create_ethernet_frame(&src_mac, &dst_mac, &[0x00; 64]);
        let msg = wrap_ethernet_in_hwsim(&eth, &ap_mac, &dst_mac, &src_mac, 2412).unwrap();
        chip.sink_tx.send(Bytes::from(msg)).await.expect("Failed to register chip");
        // Wait for registration
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;

        self.chips.push(chip);
        created_id
    }

    pub async fn when_chip_transmits_unicast(
        &mut self,
        sender_idx: usize,
        receiver_idx: usize,
        payload: &str,
    ) {
        let src_mac = self.chips[sender_idx].mac;
        let dst_mac = self.chips[receiver_idx].mac;
        let ap_mac = [0x02, 0x00, 0x00, 0x00, 0x00, 0x00];

        // Helper to construct and transmit a unicast Ethernet frame via Hwsim.
        let eth = Self::create_ethernet_frame(&src_mac, &dst_mac, payload.as_bytes());
        let msg = wrap_ethernet_in_hwsim(&eth, &ap_mac, &dst_mac, &src_mac, 2412)
            .expect("Sender failed to wrap");

        self.chips[sender_idx].sink_tx.send(Bytes::from(msg)).await.expect("Sender failed to send");
    }

    pub async fn when_chip_transmits_broadcast(&mut self, sender_idx: usize, payload: &str) {
        let src_mac = self.chips[sender_idx].mac;
        let dst_mac = [0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF]; // Broadcast
        let ap_mac = [0x02, 0x00, 0x00, 0x00, 0x00, 0x00];

        let eth = Self::create_ethernet_frame(&src_mac, &dst_mac, payload.as_bytes());
        let msg = wrap_ethernet_in_hwsim(&eth, &ap_mac, &dst_mac, &src_mac, 2412).unwrap();

        self.chips[sender_idx].sink_tx.send(Bytes::from(msg)).await.unwrap();
    }

    pub async fn when_infra_transmits_unicast(&mut self, receiver_idx: usize, payload: &str) {
        let receiver_mac = self.chips[receiver_idx].mac;
        let dst_mac = receiver_mac;

        // Note: Receiver should already be registered by given_a_chip.
        // No need to send dummy frame here.

        let src_mac = [0x02, 0x00, 0x00, 0x00, 0x00, 0x00]; // AP

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let eth = Self::create_ethernet_frame(&src_mac, &dst_mac, payload.as_bytes());
        let bssid = MacAddress::new(src_mac);
        let ieee80211 =
            Ieee80211::from_ieee8023(&Bytes::from(eth), bssid, FrameDirection::FromAp, 100)
                .unwrap();
        let bytes = ieee80211.encode_to_vec().unwrap();

        self.ap_injector.send(Bytes::from(bytes)).expect("Failed to inject AP packet");
    }

    pub async fn when_infra_transmits_unicast_to_mac(&mut self, dst_mac: [u8; 6], payload: &str) {
        let src_mac = [0x02, 0x00, 0x00, 0x00, 0x00, 0x00]; // AP

        let eth = Self::create_ethernet_frame(&src_mac, &dst_mac, payload.as_bytes());
        let bssid = MacAddress::new(src_mac);
        let ieee80211 =
            Ieee80211::from_ieee8023(&Bytes::from(eth), bssid, FrameDirection::FromAp, 100)
                .unwrap();
        let bytes = ieee80211.encode_to_vec().unwrap();

        self.ap_injector.send(Bytes::from(bytes)).expect("Failed to inject AP packet");
    }

    pub async fn when_infra_transmits_multicast(&mut self, payload: &str) {
        let src_mac = [0x02, 0x00, 0x00, 0x00, 0x00, 0x00]; // AP
        let dst_mac = [0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF]; // Broadcast/Multicast

        let eth = Self::create_ethernet_frame(&src_mac, &dst_mac, payload.as_bytes());
        let bssid = MacAddress::new(src_mac);
        let ieee80211 =
            Ieee80211::from_ieee8023(&Bytes::from(eth), bssid, FrameDirection::FromAp, 100)
                .unwrap();
        let bytes = ieee80211.encode_to_vec().unwrap();

        self.ap_injector.send(Bytes::from(bytes)).expect("Failed to inject AP multicast packet");
    }

    /// Simulates a Chip transmitting a Data Frame (ToDS=1) to another Chip via
    /// the AP. This requires `simulate_ap_reflection: true` in Medium
    /// (default).
    pub async fn when_chip_transmits_to_ds_unicast(
        &mut self,
        sender_idx: usize,
        receiver_idx: usize,
        payload: &str,
    ) {
        let sender_mac = self.chips[sender_idx].mac;
        let receiver_mac = self.chips[receiver_idx].mac;
        let bssid = [0x02, 0x00, 0x00, 0x00, 0x00, 0x00]; // AP BSSID

        // Construct ToDS Frame
        let to_ds_frame = Ieee80211ToAp {
            version: 0,
            ftype: FrameType::Data, // Data
            stype: 0,               // Data
            destination: MacAddress::new(receiver_mac),
            source: MacAddress::new(sender_mac),
            bssid: MacAddress::new(bssid),
            duration_id: 0,
            seq_ctrl: 0,
            qos_ctrl: None,
            protected: 0,
            order: 0,
            more_frags: 0,
            retry: 0,
            pm: 0,
            more_data: 0,
            payload: {
                let mut p = vec![
                    0xAA, 0xAA, 0x03, 0x00, 0x00, 0x00, // LLC SNAP
                    0x08, 0x00, // IPv4 EtherType (dummy)
                ];
                p.extend_from_slice(payload.as_bytes());
                p
            },
        };

        let ieee80211: Ieee80211 = to_ds_frame.try_into().unwrap();

        // Wrap the 802.11 frame in a Hwsim message.
        // For ToDS frames, the Hwsim Destination is the AP (BSSID).
        let msg = wifi_actor::create_hwsim_msg_from_frame(
            &ieee80211,
            &MacAddress::new(bssid), // Dest Hwsim (AP)
            2412,
            Some(&MacAddress::new(sender_mac)), // Src Hwsim
        )
        .expect("Failed to create HwsimMsg");

        let bytes = msg.encode_to_vec().unwrap();
        self.chips[sender_idx].sink_tx.send(Bytes::from(bytes)).await.unwrap();
    }

    pub async fn when_chip_transmits_mgmt_to_ap(&mut self, sender_idx: usize) {
        let sender_mac = self.chips[sender_idx].mac;
        let bssid = [0x02, 0x00, 0x00, 0x00, 0x00, 0x00]; // AP BSSID

        // Construct Mgmt Frame (e.g., Auth or Assoc Request)
        let mgmt_frame = Ieee80211ToAp {
            version: 0,
            ftype: FrameType::Management, // Management
            stype: 0,                     // Assoc Req? (Doesn't matter for routing)
            destination: MacAddress::new(bssid),
            source: MacAddress::new(sender_mac),
            bssid: MacAddress::new(bssid),
            duration_id: 0,
            seq_ctrl: 0,
            qos_ctrl: None,
            protected: 0,
            order: 0,
            more_frags: 0,
            retry: 0,
            pm: 0,
            more_data: 0,
            payload: vec![0x11; 32], // Mgmt payload
        };

        let ieee80211: Ieee80211 = mgmt_frame.try_into().unwrap();

        let msg = wifi_actor::create_hwsim_msg_from_frame(
            &ieee80211,
            &MacAddress::new(bssid), // Dest Hwsim (AP)
            2412,
            Some(&MacAddress::new(sender_mac)), // Src Hwsim
        )
        .expect("Failed to create HwsimMsg");

        let bytes = msg.encode_to_vec().unwrap();
        self.chips[sender_idx].sink_tx.send(Bytes::from(bytes)).await.unwrap();
    }

    pub async fn then_chip_receives_payload(
        &mut self,
        receiver_idx: usize,
        expected_payload: &str,
    ) {
        let chip = &mut self.chips[receiver_idx];
        let timeout = tokio::time::sleep(std::time::Duration::from_secs(3));
        tokio::pin!(timeout);
        let expected_bytes = expected_payload.as_bytes();

        loop {
            tokio::select! {
                Some(bytes) = chip.stream_rx.recv() => {
                    info!("Chip {} received {} bytes", chip.id, bytes.len());
                    if let Ok(eth) = crate::hwsim_helper::unwrap_hwsim_to_ethernet(&bytes) {
                        // Check if the received payload matches the expected payload.
                         if eth.len() >= expected_bytes.len() && eth.windows(expected_bytes.len()).any(|w| w == expected_bytes) {
                            info!("Chip {} received expected payload!", chip.id);
                            return;
                        } else {
                            warn!("Chip {} received payload mismatch", chip.id);
                        }
                    } else {
                         error!("Chip {} received non-ethernet or invalid packet", chip.id);
                    }
                }
                _ = &mut timeout => {
                    panic!("Timeout waiting for payload on chip {}", chip.id);
                }
            }
        }
    }

    pub async fn then_chip_receives_payload_and_dst(
        &mut self,
        receiver_idx: usize,
        expected_payload: &str,
        expected_dst: [u8; 6],
    ) {
        let chip = &mut self.chips[receiver_idx];
        let timeout = tokio::time::sleep(std::time::Duration::from_secs(3));
        tokio::pin!(timeout);
        let expected_bytes = expected_payload.as_bytes();

        loop {
            tokio::select! {
                Some(bytes) = chip.stream_rx.recv() => {
                    info!("Chip {} received {} bytes", chip.id, bytes.len());
                    if let Ok(eth) = crate::hwsim_helper::unwrap_hwsim_to_ethernet(&bytes) {
                         if eth.len() >= expected_bytes.len() && eth.windows(expected_bytes.len()).any(|w| w == expected_bytes) {
                            // Check Destination MAC
                            if eth.len() >= 6 && eth[0..6] == expected_dst {
                                info!("Chip {} received expected payload AND mac matches!", chip.id);
                                return;
                            } else {
                                warn!("Chip {} received payload match but MAC mismatch. Got {:?}, expected {:?}", chip.id, &eth[0..6], expected_dst);
                            }
                        } else {
                            warn!("Chip {} received payload mismatch", chip.id);
                        }
                    } else {
                         error!("Chip {} received non-ethernet or invalid packet", chip.id);
                    }
                }
                _ = &mut timeout => {
                    panic!("Timeout waiting for payload on chip {}", chip.id);
                }
            }
        }
    }

    /// Simulates a Chip transmitting a Data Frame (ToDS=1) destined for the
    /// Internet Gateway (Slirp).
    pub async fn when_chip_transmits_data_to_slirp(&mut self, sender_idx: usize) {
        let sender_mac = self.chips[sender_idx].mac;
        let internet_gateway = [0x00, 0x00, 0x00, 0x00, 0x00, 0xFE]; // Dummy Gateway
        let bssid = [0x02, 0x00, 0x00, 0x00, 0x00, 0x00]; // AP BSSID

        // Construct ToDS Data Frame
        let to_ds_frame = Ieee80211ToAp {
            version: 0,
            ftype: FrameType::Data, // Data
            stype: 0,               // Data
            destination: MacAddress::new(internet_gateway),
            source: MacAddress::new(sender_mac),
            bssid: MacAddress::new(bssid),
            duration_id: 0,
            seq_ctrl: 0,
            qos_ctrl: None,
            protected: 0,
            order: 0,
            more_frags: 0,
            retry: 0,
            pm: 0,
            more_data: 0,
            payload: vec![0xAB; 64],
        };

        let ieee80211: Ieee80211 = to_ds_frame.try_into().unwrap();

        let msg = wifi_actor::create_hwsim_msg_from_frame(
            &ieee80211,
            &MacAddress::new(bssid), // Dest Hwsim (AP)
            2412,
            Some(&MacAddress::new(sender_mac)), // Src Hwsim
        )
        .expect("Failed to create HwsimMsg");

        let bytes = msg.encode_to_vec().unwrap();
        self.chips[sender_idx].sink_tx.send(Bytes::from(bytes)).await.unwrap();
    }

    pub async fn when_chip_transmits_mdns(&mut self, sender_idx: usize, payload: &str) {
        let sender_mac = self.chips[sender_idx].mac;
        let mdns_multicast = [0x01, 0x00, 0x5E, 0x00, 0x00, 0xFB]; // IPv4 mDNS

        let eth = Self::create_ethernet_frame(&sender_mac, &mdns_multicast, payload.as_bytes());

        // P2P/Ad-hoc Context: Use wildcard BSSID for simple multicast/broadcast.
        let bssid_p2p = [0x00, 0x00, 0x00, 0x00, 0x00, 0x00];

        let msg =
            wrap_ethernet_in_hwsim(&eth, &bssid_p2p, &mdns_multicast, &sender_mac, 2412).unwrap();
        self.chips[sender_idx].sink_tx.send(Bytes::from(msg)).await.unwrap();
    }

    pub async fn when_chip_transmits_infra_mdns(&mut self, sender_idx: usize, payload: &str) {
        let sender_mac = self.chips[sender_idx].mac;
        let mdns_multicast = [0x01, 0x00, 0x5E, 0x00, 0x00, 0xFB]; // IPv4 mDNS

        let eth = Self::create_ethernet_frame(&sender_mac, &mdns_multicast, payload.as_bytes());

        // Infra Context: Use AP BSSID.
        let ap_bssid = [0x02, 0x00, 0x00, 0x00, 0x00, 0x00];

        // The packet goes to the AP (dest_hwsim_addr = ap_bssid)
        let msg = wrap_ethernet_in_hwsim(&eth, &ap_bssid, &ap_bssid, &sender_mac, 2412).unwrap();
        self.chips[sender_idx].sink_tx.send(Bytes::from(msg)).await.unwrap();
    }

    pub fn create_ethernet_frame(src: &[u8; 6], dst: &[u8; 6], payload: &[u8]) -> Vec<u8> {
        let dst_mac = MacAddr::new(*dst);
        let src_mac = MacAddr::new(*src);
        let frame = EthernetFrame::new(dst_mac, src_mac, ether_type::IPV4);

        let mut bytes = frame.as_bytes().to_vec();
        bytes.extend_from_slice(payload);
        // Pad to minimum Ethernet frame size (60 bytes without FCS)
        if bytes.len() < 60 {
            bytes.resize(60, 0);
        }
        bytes
    }

    pub async fn given_chip_is_disabled(&mut self, chip_idx: usize) {
        let chip = &self.chips[chip_idx];
        let id_val = chip.id;
        use netsim_model::{ChipId, ChipUpdate, ChipVariantUpdate};
        let patch = ChipUpdate {
            variant: Some(ChipVariantUpdate::Wifi(Default::default())),
            enabled: Some(false),
            ..Default::default()
        };
        self.wifi_client.update(ChipId(id_val), patch).await.expect("Failed to disable chip");
    }

    pub async fn then_chip_receives_nothing(&mut self, chip_idx: usize) {
        let chip = &mut self.chips[chip_idx];
        // Wait for a short duration to verify no packets are received.
        let timeout = tokio::time::sleep(std::time::Duration::from_millis(1000));
        tokio::pin!(timeout);

        loop {
            tokio::select! {
                Some(packet) = chip.stream_rx.recv() => {
                     // Filter out RTM_NEWLINK (Type 16)
                     if packet.len() >= 6 {
                         let msg_type = u16::from_le_bytes([packet[4], packet[5]]);
                         if msg_type == 16 {
                             info!("Ignored Netlink Control Packet (Type 16, len={})", packet.len());
                             continue;
                         }
                     }
                     let len = packet.len();
                     let preview = if len > 10 { &packet[0..10] } else { &packet[..] };
                     panic!("Chip {} received a packet (len={}, bytes={:?}) but expected nothing", chip.id, len, preview);
                }
                _ = &mut timeout => {
                    // Success
                    return;
                }
            }
        }
    }

    pub async fn when_global_stats_are_captured(&mut self) {
        self.baseline_stats = Some(self.wifi_client.get_global_stats_proto().await.unwrap());
    }

    pub async fn when_chip_transmits_malformed_packet(&mut self, chip_idx: usize) {
        self.chips[chip_idx].sink_tx.send(bytes::Bytes::from(vec![0x00, 0x01])).await.unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }

    pub async fn then_client_errors_increased_by(&self, expected_increase: i32) {
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        let final_stats = self.wifi_client.get_global_stats_proto().await.unwrap();
        assert_eq!(
            final_stats.client_errors(),
            self.baseline_stats.as_ref().unwrap().client_errors() + expected_increase
        );
    }

    pub async fn then_frame_errors_increased_by(&self, expected_increase: i32) {
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        let final_stats = self.wifi_client.get_global_stats_proto().await.unwrap();
        assert_eq!(
            final_stats.frame_errors(),
            self.baseline_stats.as_ref().unwrap().frame_errors() + expected_increase
        );
    }

    pub async fn then_hostapd_frames_tx_increased_by(&mut self, expected_increase: i32) {
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        let final_stats = self.wifi_client.get_global_stats_proto().await.unwrap();
        assert_eq!(
            final_stats.hostapd_frames_tx(),
            self.baseline_stats.as_ref().unwrap().hostapd_frames_tx() + expected_increase
        );
        // refresh baseline since test_hostapd_and_network_tx_stats does sequential
        // checks
        self.baseline_stats = Some(final_stats);
    }

    pub async fn then_network_packets_tx_increased_by(&mut self, expected_increase: i32) {
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        let final_stats = self.wifi_client.get_global_stats_proto().await.unwrap();
        assert_eq!(
            final_stats.network_packets_tx(),
            self.baseline_stats.as_ref().unwrap().network_packets_tx() + expected_increase
        );
        self.baseline_stats = Some(final_stats);
    }

    pub async fn then_max_upload_throughput_is_greater_than(&mut self, expected_min: f32) {
        let final_stats = self.wifi_client.get_global_stats_proto().await.unwrap();
        assert!(
            final_stats.max_upload_throughput() > expected_min,
            "max_upload_throughput {} is not greater than {}",
            final_stats.max_upload_throughput(),
            expected_min
        );
    }

    pub async fn then_p2p_tx_count_is(&self, chip_idx: usize, expected: u64) {
        use netsim_model::ChipClient;
        let stats = self.wifi_client.read_statistics().await.unwrap();
        let chip_id = self.chips[chip_idx].id;
        let chip_stats = stats.iter().find(|s| s.id == chip_id).unwrap();
        assert_eq!(chip_stats.p2p_tx_count, expected);
    }

    pub async fn then_p2p_rx_count_is(&self, chip_idx: usize, expected: u64) {
        use netsim_model::ChipClient;
        let stats = self.wifi_client.read_statistics().await.unwrap();
        let chip_id = self.chips[chip_idx].id;
        let chip_stats = stats.iter().find(|s| s.id == chip_id).unwrap();
        assert_eq!(chip_stats.p2p_rx_count, expected);
    }
}

#[derive(Clone, Debug)]
pub struct MockGateway {
    pub outgoing_packets: Arc<std::sync::Mutex<Vec<(netsim_model::ChipId, bytes::Bytes)>>>,
}

impl MockGateway {
    pub fn new() -> Self {
        Self { outgoing_packets: Arc::new(std::sync::Mutex::new(Vec::new())) }
    }
}

#[async_trait::async_trait]
impl wifi_actor::GatewayTrait for MockGateway {
    async fn send_80211(
        &self,
        chip_id: netsim_model::ChipId,
        ieee80211: &Ieee80211,
    ) -> Result<usize, wifi_actor::WifiError> {
        let bytes = ieee80211
            .encode_to_vec()
            .map_err(|e| wifi_actor::WifiError::Frame(Box::from(e.to_string())))?;
        let len = bytes.len();
        self.outgoing_packets.lock().unwrap().push((chip_id, bytes::Bytes::from(bytes)));
        Ok(len)
    }

    fn should_handle(&self, _chip_id: netsim_model::ChipId) -> bool {
        false
    }

    fn handle_incoming(
        &self,
        _chip_id: netsim_model::ChipId,
        _packet: bytes::Bytes,
        _medium: &mut wifi_actor::Medium,
        _shared_keys: &ap_actor::SharedKeyStore,
        _out_queue: &mut Vec<(u32, bytes::Bytes)>,
    ) {
    }

    async fn on_start(&mut self, _ctx: &mut actor_framework::DynContext<WifiActor>) {}

    async fn on_chip_create(
        &mut self,
        _chip_id: netsim_model::ChipId,
        _ctx: &mut actor_framework::DynContext<WifiActor>,
    ) {
    }

    async fn on_chip_remove(
        &mut self,
        _chip_id: netsim_model::ChipId,
        _ctx: &mut actor_framework::DynContext<WifiActor>,
    ) {
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}
