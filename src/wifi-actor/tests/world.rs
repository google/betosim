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

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use actor_framework::ResourceActor;
use ap_actor::{ApActor, ApClient};
use bytes::Bytes;
use common::util::netsim_logger::{ErrorStatsLayer, ErrorStatsTable};
use device_actor::DeviceActor;
use device_api::{DeviceAction, DeviceId};
use netsim_model::{ChipClient, ChipCreate, ChipId, ChipUpdate, PoseUpdate, Position};
use netsim_packets::{
    ActionHeader, BeaconFixedFields, DHCP_SERVER_PORT, DataSubType, EthernetFrame,
    FineTimingMeasurement, FrameControl, FrameDirection, FrameType, HwsimCmd, HwsimFrame, HwsimMsg,
    IP_P_UDP, IeIterator, Ieee80211, Ieee80211ToAp, Ipv4Header, MacAddr, MacAddress,
    MacHeader3Addr, NlMsgHdr, PacketBuilder, SequenceControl, UdpHeader, category, ether_type,
    management_subtype, public_action, tags, write_ie,
};
use slirp_actor::SlirpActor;
use tokio::sync::mpsc;
use tokio_stream::wrappers::{ReceiverStream, UnboundedReceiverStream};
use tracing::{error, info, warn};
use tracing_subscriber::{Layer, layer::SubscriberExt};
use wifi_actor::WifiActor;
use zerocopy::{FromBytes, IntoBytes};

use crate::hwsim_helper::wrap_ethernet_in_hwsim;

pub const AP_BSSID: [u8; 6] = [0x02, 0x00, 0x00, 0x00, 0x00, 0x00];

struct LogCaptureLayer {
    logs: Arc<Mutex<Vec<(tracing::Level, String)>>>,
}

impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for LogCaptureLayer {
    fn on_event(
        &self,
        event: &tracing::Event<'_>,
        _ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        let metadata = event.metadata();
        let level = *metadata.level();
        let is_error = level == tracing::Level::ERROR;
        let is_lifecycle_info =
            level == tracing::Level::INFO && metadata.target() == "wifi_actor::lifecycle";
        if is_error || is_lifecycle_info {
            struct Visitor(String);
            impl tracing::field::Visit for Visitor {
                fn record_debug(
                    &mut self,
                    field: &tracing::field::Field,
                    value: &dyn std::fmt::Debug,
                ) {
                    if !self.0.is_empty() {
                        self.0.push(' ');
                    }
                    use std::fmt::Write;
                    let _ = write!(self.0, "{}: {:?}", field.name(), value);
                }
            }
            let mut visitor = Visitor(String::new());
            event.record(&mut visitor);
            self.logs.lock().unwrap().push((level, visitor.0));
        }
    }
}

pub struct ChipChannels {
    pub id: u32,
    pub mac: [u8; 6],
    pub stream_rx: mpsc::Receiver<Bytes>, // Output from Actor (Source)
    pub sink_tx: mpsc::Sender<Bytes>,     // Input to Actor (Sink)
}

pub async fn recv_next_tx_info_ack(chip: &mut ChipChannels) -> bool {
    const HWSIM_TX_STAT_ACK: u32 = 1 << 2;
    let timeout = tokio::time::sleep(std::time::Duration::from_secs(3));
    tokio::pin!(timeout);

    loop {
        tokio::select! {
            Some(bytes) = chip.stream_rx.recv() => {
                if let Ok(hwsim_msg) = netsim_packets::HwsimMsg::decode_full(&bytes)
                    && hwsim_msg.hwsim_hdr.hwsim_cmd == netsim_packets::HwsimCmd::TxInfoFrame
                    && let Ok(attrs) = netsim_packets::HwsimAttrSet::parse(&hwsim_msg.attributes)
                    && let Some(flags) = attrs.flags
                {
                    return (flags & HWSIM_TX_STAT_ACK) != 0;
                }
            }
            _ = &mut timeout => {
                panic!("Timeout waiting for TxInfoFrame on chip {}", chip.id);
            }
        }
    }
}

pub struct World {
    pub wifi_client: wifi_actor::WifiClient,
    pub ap_client: ApClient,
    pub chips: Vec<ChipChannels>,
    raw_chips: HashMap<u32, ChipChannels>,
    mock_gateway: Option<MockGateway>,
    // Injector for AP packets
    pub ap_injector: mpsc::UnboundedSender<Bytes>,
    pub device_action_rx: mpsc::UnboundedReceiver<DeviceAction>,
    pub baseline_stats: Option<netsim_proto::stats::WifiStats>,
    pub mock_clock: std::sync::Arc<wifi_actor::MockClock>,
    pub shared_keys: std::sync::Arc<ap_actor::SharedKeyStore>,
    pub error_stats: std::sync::Arc<ErrorStatsTable>,
    lifecycle_and_error_logs: Arc<Mutex<Vec<(tracing::Level, String)>>>,
    #[cfg(target_os = "linux")]
    pub gateway_downlink_tx: Option<mpsc::UnboundedSender<Bytes>>,
}

const WIFI_CHANNEL_1_FREQ_MHZ: u32 = 2412;
const FTM_TRIGGER_START: u8 = 1;
const FC_TYPE_SHIFT: u16 = 2;
const FC_SUBTYPE_SHIFT: u16 = 4;
const RTM_NEWLINK: u16 = 16;

impl World {
    pub async fn new() -> Self {
        Self::new_internal(None).await
    }

    pub async fn new_with_gateway(gateway: Box<dyn wifi_actor::GatewayTrait>) -> Self {
        Self::new_internal(Some(gateway)).await
    }

    pub async fn new_with_mock_gateway() -> Self {
        let gw = MockGateway::new();
        let mut world = Self::new_internal(Some(Box::new(gw.clone()))).await;
        world.mock_gateway = Some(gw);
        world
    }

    pub async fn new_with_mock_gateway_error_stats() -> Self {
        let gw = MockGateway::new();
        let mut world = Self::new_internal_with_error_stats(Some(Box::new(gw.clone())), true).await;
        world.mock_gateway = Some(gw);
        world
    }

    #[cfg(target_os = "linux")]
    pub async fn new_with_tap_gateway() -> Self {
        let (harness, downlink_tx) = TapDownlinkHarness::new();
        let mut world = Self::new_with_gateway(Box::new(harness)).await;
        world.gateway_downlink_tx = Some(downlink_tx);
        world
    }

    pub async fn given_mock_gateway_world() -> (Self, MockGateway, u32) {
        let mock_gateway = MockGateway::new();
        let mut world = Self::new_with_gateway(Box::new(mock_gateway.clone())).await;
        world.given_an_ap().await;
        let chip_id = world.given_a_chip(1).await;
        mock_gateway.clear();
        (world, mock_gateway, chip_id)
    }

    async fn new_internal(gateway: Option<Box<dyn wifi_actor::GatewayTrait>>) -> Self {
        Self::new_internal_with_error_stats(gateway, false).await
    }

    pub async fn with_error_stats() -> Self {
        Self::new_internal_with_error_stats(None, true).await
    }

    #[cfg(target_os = "linux")]
    pub async fn with_tap_gateway_error_stats() -> Self {
        let (harness, downlink_tx) = TapDownlinkHarness::new();
        let mut world = Self::new_internal_with_error_stats(Some(Box::new(harness)), true).await;
        world.gateway_downlink_tx = Some(downlink_tx);
        world
    }

    async fn new_internal_with_error_stats(
        gateway: Option<Box<dyn wifi_actor::GatewayTrait>>,
        record_error_stats: bool,
    ) -> Self {
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

        let error_stats = Arc::new(ErrorStatsTable::new());
        let (wifi_runner, wifi_client) = wifi_actor::new();
        let lifecycle_and_error_logs = Arc::new(Mutex::new(Vec::new()));
        let log_layer = LogCaptureLayer { logs: lifecycle_and_error_logs.clone() }
            .with_filter(tracing_subscriber::filter::LevelFilter::INFO);
        let test_layer = tracing_subscriber::fmt::layer()
            .with_test_writer()
            .with_filter(tracing_subscriber::filter::LevelFilter::INFO);

        if record_error_stats {
            let error_stats_layer = ErrorStatsLayer::with_table(Arc::clone(&error_stats));
            let subscriber = tracing_subscriber::registry()
                .with(test_layer)
                .with(log_layer)
                .with(error_stats_layer);
            tokio::spawn(tracing::instrument::WithSubscriber::with_subscriber(
                wifi_runner.run(wifi_actor_impl),
                subscriber,
            ));
        } else {
            let subscriber = tracing_subscriber::registry().with(test_layer).with(log_layer);
            tokio::spawn(tracing::instrument::WithSubscriber::with_subscriber(
                wifi_runner.run(wifi_actor_impl),
                subscriber,
            ));
        }

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
            raw_chips: HashMap::new(),
            mock_gateway: None,
            ap_injector,
            device_action_rx: device_rx,
            baseline_stats: None,
            mock_clock,
            shared_keys,
            error_stats,
            lifecycle_and_error_logs,
            #[cfg(target_os = "linux")]
            gateway_downlink_tx: None,
        }
    }

    async fn wait_for_lifecycle_info_and_assert_no_delete_error(
        &self,
        expected_info: &str,
        context: &str,
    ) {
        let start = std::time::Instant::now();
        let timeout = std::time::Duration::from_secs(1);
        loop {
            let found = {
                let logs = self.lifecycle_and_error_logs.lock().unwrap();
                logs.iter().any(|(level, msg)| {
                    *level == tracing::Level::INFO && msg.contains(expected_info)
                })
            };
            if found {
                break;
            }
            if start.elapsed() > timeout {
                panic!("Timed out waiting for info log: '{}'", expected_info);
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }

        self.wifi_client.list().await.expect("list() should succeed after lifecycle event");

        let logs = self.lifecycle_and_error_logs.lock().unwrap();
        let error_logs: Vec<_> = logs
            .iter()
            .filter(|(level, msg)| {
                *level == tracing::Level::ERROR && msg.contains("Failed to delete chip")
            })
            .collect();
        assert!(
            error_logs.is_empty(),
            "Found unexpected delete error log after {}: {:?}",
            context,
            error_logs
        );
    }

    pub async fn then_sink_task_is_aborted_without_close_log(&self, chip_id: u32) {
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        self.wifi_client.list().await.expect("Failed to list chips");

        let sink_closed_msg = format!("Sink task closed for chip {chip_id}");
        let stream_closed_msg = format!("Stream closed for chip {chip_id}");
        let logs = self.lifecycle_and_error_logs.lock().unwrap();
        let has_sink_close = logs
            .iter()
            .any(|(level, msg)| *level == tracing::Level::INFO && msg.contains(&sink_closed_msg));
        assert!(
            !has_sink_close,
            "Found unexpected sink task close log for chip {chip_id}: {logs:?}"
        );
        let has_stream_close = logs
            .iter()
            .any(|(level, msg)| *level == tracing::Level::INFO && msg.contains(&stream_closed_msg));
        assert!(
            !has_stream_close,
            "Found unexpected stream close log for chip {chip_id}: {logs:?}"
        );
        let has_delete_error = logs.iter().any(|(level, msg)| {
            *level == tracing::Level::ERROR && msg.contains("Failed to delete chip")
        });
        assert!(
            !has_delete_error,
            "Found unexpected delete error log for chip {chip_id}: {logs:?}"
        );
    }

    pub async fn when_mock_gateway_stream_closes(&self) {
        self.mock_gateway.as_ref().expect("MockGateway not configured").close_incoming();
    }

    pub async fn then_mock_gateway_stream_closes_without_delete_error(&self) {
        self.wait_for_lifecycle_info_and_assert_no_delete_error(
            &format!("Stream closed for chip {}", MOCK_GATEWAY_STREAM_ID),
            "mock gateway stream closes",
        )
        .await;
    }

    #[cfg(target_os = "linux")]
    pub async fn then_tap_gateway_stream_closes_without_delete_error(&self, chip_id: u32) {
        self.wait_for_lifecycle_info_and_assert_no_delete_error(
            &format!("Stream closed for chip {}", chip_id | TAP_FLAG),
            "tap gateway downlink stream closed",
        )
        .await;
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
            bssid: MacAddr::from(AP_BSSID),
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

    async fn create_chip_channels(
        &mut self,
        chip_id: u32,
        device_id: u32,
        mac: [u8; 6],
    ) -> ChipChannels {
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
            device_id: DeviceId(device_id),
            variant: Some(netsim_model::ChipVariant::Wifi(netsim_model::Wifi {
                radio: Default::default(),
            })),
            ..Default::default()
        };

        let params =
            ChipCreate { packet_stream: Some(packet_stream), packet_sink: Some(packet_sink), chip };

        self.wifi_client.create(ChipId(chip_id), params).await.expect("Failed to create chip");

        ChipChannels { id: chip_id, mac, stream_rx, sink_tx }
    }

    pub async fn given_a_chip(&mut self, id_val: u32) -> u32 {
        let src_mac = [0x00, 0x00, 0x00, 0x00, 0x00, id_val as u8];
        let chip = self.create_chip_channels(id_val, 1, src_mac).await;

        // Register chip by sending dummy packet

        let dst_mac = [0x00, 0x00, 0x00, 0x00, 0x00, 0x00]; // Dummy dst
        let eth = Self::create_ethernet_frame(&src_mac, &dst_mac, &[0x00; 64]);
        let msg = wrap_ethernet_in_hwsim(&eth, &AP_BSSID, &dst_mac, &src_mac, 2412).unwrap();
        chip.sink_tx.send(Bytes::from(msg)).await.expect("Failed to register chip");
        // Wait for registration
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;

        self.chips.push(chip);
        id_val
    }

    #[cfg(target_os = "linux")]
    pub async fn given_an_associated_chip(&mut self, id_val: u32) -> u32 {
        let chip_id = self.given_a_chip(id_val).await;
        self.when_chip_transmits_data_to_slirp(self.chips.len() - 1).await;
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        chip_id
    }

    pub async fn when_chip_transmits_unicast(
        &mut self,
        sender_idx: usize,
        receiver_idx: usize,
        payload: &str,
    ) {
        let src_mac = self.chips[sender_idx].mac;
        let dst_mac = self.chips[receiver_idx].mac;

        // Helper to construct and transmit a unicast Ethernet frame via Hwsim.
        let eth = Self::create_ethernet_frame(&src_mac, &dst_mac, payload.as_bytes());
        let msg = wrap_ethernet_in_hwsim(&eth, &AP_BSSID, &dst_mac, &src_mac, 2412)
            .expect("Sender failed to wrap");

        self.chips[sender_idx].sink_tx.send(Bytes::from(msg)).await.expect("Sender failed to send");
    }

    pub async fn when_chip_transmits_broadcast(&mut self, sender_idx: usize, payload: &str) {
        let src_mac = self.chips[sender_idx].mac;
        let dst_mac = [0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF]; // Broadcast

        let eth = Self::create_ethernet_frame(&src_mac, &dst_mac, payload.as_bytes());
        let msg = wrap_ethernet_in_hwsim(&eth, &AP_BSSID, &dst_mac, &src_mac, 2412).unwrap();

        self.chips[sender_idx].sink_tx.send(Bytes::from(msg)).await.unwrap();
    }

    pub async fn when_infra_transmits_unicast(&mut self, receiver_idx: usize, payload: &str) {
        let dst_mac = self.chips[receiver_idx].mac;

        // Note: Receiver should already be registered by given_a_chip.
        // No need to send dummy frame here.

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let eth = Self::create_ethernet_frame(&AP_BSSID, &dst_mac, payload.as_bytes());
        let bssid = MacAddress::new(AP_BSSID);
        let ieee80211 =
            Ieee80211::from_ieee8023(&Bytes::from(eth), bssid, FrameDirection::FromAp, 100)
                .unwrap();
        let bytes = ieee80211.encode_to_vec().unwrap();

        self.ap_injector.send(Bytes::from(bytes)).expect("Failed to inject AP packet");
    }

    pub async fn when_infra_transmits_unicast_to_mac(&mut self, dst_mac: [u8; 6], payload: &str) {
        let eth = Self::create_ethernet_frame(&AP_BSSID, &dst_mac, payload.as_bytes());
        let bssid = MacAddress::new(AP_BSSID);
        let ieee80211 =
            Ieee80211::from_ieee8023(&Bytes::from(eth), bssid, FrameDirection::FromAp, 100)
                .unwrap();
        let bytes = ieee80211.encode_to_vec().unwrap();

        self.ap_injector.send(Bytes::from(bytes)).expect("Failed to inject AP packet");
    }

    pub async fn when_infra_transmits_multicast(&mut self, payload: &str) {
        let dst_mac = [0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF]; // Broadcast/Multicast

        let eth = Self::create_ethernet_frame(&AP_BSSID, &dst_mac, payload.as_bytes());
        let bssid = MacAddress::new(AP_BSSID);
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

        // Construct ToDS Frame
        let to_ds_frame = Ieee80211ToAp {
            version: 0,
            ftype: FrameType::Data, // Data
            stype: 0,               // Data
            destination: MacAddress::new(receiver_mac),
            source: MacAddress::new(sender_mac),
            bssid: MacAddress::new(AP_BSSID),
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
            &MacAddress::new(AP_BSSID), // Dest Hwsim (AP)
            2412,
            Some(&MacAddress::new(sender_mac)), // Src Hwsim
        )
        .expect("Failed to create HwsimMsg");

        let bytes = msg.encode_to_vec().unwrap();
        self.chips[sender_idx].sink_tx.send(Bytes::from(bytes)).await.unwrap();
    }

    async fn transmit_mgmt_to_ap(&mut self, sender_idx: usize, stype: u8, payload: &[u8]) {
        let sender_mac = self.chips[sender_idx].mac;

        let mgmt_frame = Ieee80211ToAp {
            version: 0,
            ftype: FrameType::Management,
            stype,
            destination: MacAddress::new(AP_BSSID),
            source: MacAddress::new(sender_mac),
            bssid: MacAddress::new(AP_BSSID),
            duration_id: 0,
            seq_ctrl: 0,
            qos_ctrl: None,
            protected: 0,
            order: 0,
            more_frags: 0,
            retry: 0,
            pm: 0,
            more_data: 0,
            payload: payload.to_vec(),
        };

        let ieee80211: Ieee80211 = mgmt_frame.try_into().unwrap();

        let msg = wifi_actor::create_hwsim_msg_from_frame(
            &ieee80211,
            &MacAddress::new(AP_BSSID),
            2412,
            Some(&MacAddress::new(sender_mac)),
        )
        .expect("Failed to create HwsimMsg");

        let bytes = msg.encode_to_vec().unwrap();
        self.chips[sender_idx].sink_tx.send(Bytes::from(bytes)).await.unwrap();
    }

    pub async fn when_chip_transmits_mgmt_to_ap(&mut self, sender_idx: usize) {
        self.transmit_mgmt_to_ap(sender_idx, management_subtype::ASSOCIATION_REQUEST, &[0x11; 32])
            .await;
    }

    pub async fn when_chip_transmits_probe_request(&mut self, sender_idx: usize) {
        let mut payload = Vec::new();
        write_ie(&mut payload, tags::SSID, b"TestAP");
        self.transmit_mgmt_to_ap(sender_idx, management_subtype::PROBE_REQUEST, &payload).await;
    }

    pub fn when_chip_installs_ptk_session(&mut self, chip_idx: usize) {
        let mac = MacAddress::new(self.chips[chip_idx].mac);
        self.shared_keys.add_session(mac, vec![0x42; 16]);
    }

    pub async fn then_chip_receives_assoc_response(&mut self, receiver_idx: usize) {
        let chip = &mut self.chips[receiver_idx];
        let timeout = tokio::time::sleep(std::time::Duration::from_secs(3));
        tokio::pin!(timeout);

        loop {
            tokio::select! {
                Some(bytes) = chip.stream_rx.recv() => {
                    if let Some(ieee) = crate::hwsim_helper::unwrap_hwsim_to_ieee80211(&bytes)
                        && ieee.is_mgmt()
                        && ieee.stype() == management_subtype::ASSOCIATION_RESPONSE
                    {
                        info!("Chip {} received Association Response", chip.id);
                        return;
                    }
                }
                _ = &mut timeout => {
                    panic!("Timeout waiting for Association Response on chip {}", chip.id);
                }
            }
        }
    }

    pub async fn then_chip_receives_plaintext_mgmt_with_ssid(
        &mut self,
        receiver_idx: usize,
        expected_stype: u8,
        expected_ssid: &[u8],
    ) {
        let chip = &mut self.chips[receiver_idx];
        let timeout = tokio::time::sleep(std::time::Duration::from_secs(3));
        tokio::pin!(timeout);

        loop {
            tokio::select! {
                Some(bytes) = chip.stream_rx.recv() => {
                    if let Some(ieee) = crate::hwsim_helper::unwrap_hwsim_to_ieee80211(&bytes)
                        && ieee.is_mgmt()
                        && ieee.stype() == expected_stype
                    {
                        assert!(
                            !ieee.needs_decryption(),
                            "Management frame stype {} should not be encrypted",
                            expected_stype
                        );
                        let payload = ieee.get_payload();
                        let (_, ies) = BeaconFixedFields::ref_from_prefix(&payload)
                            .expect("valid beacon fixed fields");
                        let found_ssid = IeIterator::new(ies)
                            .find(|ie| ie.id == tags::SSID)
                            .map(|ie| ie.body);
                        assert_eq!(
                            found_ssid,
                            Some(expected_ssid),
                            "Management frame SSID mismatch"
                        );
                        return;
                    }
                }
                _ = &mut timeout => {
                    panic!(
                        "Timeout waiting for plaintext mgmt frame stype {} on chip {}",
                        expected_stype, chip.id
                    );
                }
            }
        }
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

        // Construct ToDS Data Frame
        let to_ds_frame = Ieee80211ToAp {
            version: 0,
            ftype: FrameType::Data, // Data
            stype: 0,               // Data
            destination: MacAddress::new(internet_gateway),
            source: MacAddress::new(sender_mac),
            bssid: MacAddress::new(AP_BSSID),
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
            &MacAddress::new(AP_BSSID), // Dest Hwsim (AP)
            2412,
            Some(&MacAddress::new(sender_mac)), // Src Hwsim
        )
        .expect("Failed to create HwsimMsg");

        let bytes = msg.encode_to_vec().unwrap();
        self.chips[sender_idx].sink_tx.send(Bytes::from(bytes)).await.unwrap();
    }

    pub async fn when_chip_transmits_mdns(&mut self, sender_idx: usize, payload: &str) {
        let sender_mac = self.chips[sender_idx].mac;
        let eth = Self::create_ethernet_frame(&sender_mac, &MDNS_MULTICAST_MAC, payload.as_bytes());

        // P2P/Ad-hoc Context: Use wildcard BSSID for simple multicast/broadcast.
        let bssid_p2p = [0x00, 0x00, 0x00, 0x00, 0x00, 0x00];

        let msg = wrap_ethernet_in_hwsim(&eth, &bssid_p2p, &MDNS_MULTICAST_MAC, &sender_mac, 2412)
            .unwrap();
        self.chips[sender_idx].sink_tx.send(Bytes::from(msg)).await.unwrap();
    }

    pub async fn when_chip_transmits_infra_mdns(&mut self, sender_idx: usize, payload: &str) {
        let sender_mac = self.chips[sender_idx].mac;
        let eth = Self::create_ethernet_frame(&sender_mac, &MDNS_MULTICAST_MAC, payload.as_bytes());

        // Infra Context: The packet goes to the AP (dest_hwsim_addr = AP_BSSID).
        let msg = wrap_ethernet_in_hwsim(&eth, &AP_BSSID, &AP_BSSID, &sender_mac, 2412).unwrap();
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
        self.then_chip_receives_nothing_within(chip_idx, 1000).await;
    }

    pub async fn then_chip_receives_nothing_within(&mut self, chip_idx: usize, wait_ms: u64) {
        let chip = &mut self.chips[chip_idx];
        // Wait for the requested duration to verify no data/management frames are received.
        let timeout = tokio::time::sleep(std::time::Duration::from_millis(wait_ms));
        tokio::pin!(timeout);

        loop {
            tokio::select! {
                Some(packet) = chip.stream_rx.recv() => {
                    if let Ok(msg) = HwsimMsg::decode_full(&packet)
                        && usize::try_from(msg.nl_hdr.nlmsg_len) == Ok(packet.len())
                    {
                        let hwsim_cmd = msg.hwsim_hdr.hwsim_cmd;
                        if hwsim_cmd == HwsimCmd::TxInfoFrame {
                            info!("Ignored TxInfoFrame (len={})", packet.len());
                            continue;
                        }
                        panic!(
                            "Chip {} received an unexpected HwsimMsg ({:?}, len={}) but expected nothing",
                            chip.id, hwsim_cmd, packet.len()
                        );
                    }
                    if let Ok((nl_hdr, _)) = NlMsgHdr::ref_from_prefix(&packet) {
                        let nlmsg_len = nl_hdr.nlmsg_len;
                        let nlmsg_type = nl_hdr.nlmsg_type;
                        if usize::try_from(nlmsg_len) == Ok(packet.len())
                            && nlmsg_type == RTM_NEWLINK
                        {
                            info!("Ignored Netlink Control Packet (RTM_NEWLINK, len={})", packet.len());
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

    pub async fn given_chip_position(&mut self, chip_idx: usize, position: Position) {
        let chip_id = self.chips[chip_idx].id;
        let patch = ChipUpdate {
            pose: PoseUpdate { position: Some(position), ..Default::default() },
            ..Default::default()
        };
        self.wifi_client
            .update(ChipId(chip_id), patch)
            .await
            .expect("Failed to update chip position");
    }

    async fn transmit_3addr_station_frame(
        &mut self,
        sender_idx: usize,
        receiver_idx: usize,
        frame_type: FrameType,
        subtype: u8,
        body: &[u8],
    ) {
        let sa = MacAddress::new(self.chips[sender_idx].mac);
        let da = MacAddress::new(self.chips[receiver_idx].mac);
        let bssid = da;

        let fc_val =
            ((frame_type as u16) << FC_TYPE_SHIFT) | ((subtype as u16) << FC_SUBTYPE_SHIFT);
        let fc = FrameControl::new(fc_val);
        let header = MacHeader3Addr::new(fc, 0, da, sa, bssid, SequenceControl::new(0));

        let mut frame_bytes =
            Vec::with_capacity(std::mem::size_of::<MacHeader3Addr>() + body.len());
        frame_bytes.extend_from_slice(header.as_bytes());
        frame_bytes.extend_from_slice(body);
        let ieee80211 = Ieee80211::decode(&frame_bytes).expect("Failed to decode Ieee80211");

        let msg = wifi_actor::create_hwsim_msg_from_frame(
            &ieee80211,
            &da,
            WIFI_CHANNEL_1_FREQ_MHZ,
            Some(&sa),
        )
        .expect("Failed to create HwsimMsg");

        let bytes = msg.encode_to_vec().expect("Failed to encode HwsimMsg");
        self.chips[sender_idx]
            .sink_tx
            .send(Bytes::from(bytes))
            .await
            .expect("Failed to send 3-address station frame");
    }

    pub async fn when_chip_transmits_ftm_request(
        &mut self,
        sender_idx: usize,
        receiver_idx: usize,
    ) {
        let body = [category::PUBLIC, public_action::FTM_REQUEST, FTM_TRIGGER_START];
        self.transmit_3addr_station_frame(
            sender_idx,
            receiver_idx,
            FrameType::Management,
            management_subtype::ACTION,
            &body,
        )
        .await;
    }

    pub async fn then_chip_receives_ftm_measurement_rtt_ps(
        &mut self,
        chip_idx: usize,
        expected_rtt_ps: u64,
        tolerance_ps: u64,
    ) {
        let chip = &mut self.chips[chip_idx];
        let timeout = tokio::time::sleep(std::time::Duration::from_millis(3000));
        tokio::pin!(timeout);

        let mut received_frames: Vec<Ieee80211> = Vec::new();

        loop {
            tokio::select! {
                Some(bytes) = chip.stream_rx.recv() => {
                    if let Ok(msg) = HwsimMsg::decode_full(&bytes)
                        && usize::try_from(msg.nl_hdr.nlmsg_len) == Ok(bytes.len())
                    {
                        if msg.hwsim_hdr.hwsim_cmd != HwsimCmd::Frame {
                            continue;
                        }
                        if let Ok(frame) = HwsimFrame::parse(&msg) {
                            received_frames.push(frame.ieee80211);
                        }
                    } else if let Ok(frame) = Ieee80211::decode(&bytes) {
                        received_frames.push(frame);
                    }

                    if received_frames.len() == 2 {
                        break;
                    }
                }
                _ = &mut timeout => {
                    panic!(
                        "Timed out waiting for FTM measurement responses (received {} frames)",
                        received_frames.len()
                    );
                }
            }
        }

        let initial_payload = received_frames[0].get_payload();
        let (initial_action_hdr, initial_rest) =
            ActionHeader::ref_from_prefix(&initial_payload).expect("Initial ActionHeader");
        assert_eq!(initial_action_hdr.category, category::PUBLIC);
        assert_eq!(initial_action_hdr.action, public_action::FINE_TIMING_MEASUREMENT);

        let (initial_ftm, _) = FineTimingMeasurement::ref_from_prefix(initial_rest)
            .expect("Initial FineTimingMeasurement");
        let initial_tod = initial_ftm.tod;
        let initial_toa = initial_ftm.toa;
        assert_eq!(initial_tod, [0u8; 6], "Initial FTM frame should have zero TOD");
        assert_eq!(initial_toa, [0u8; 6], "Initial FTM frame should have zero TOA");

        let meas_payload = received_frames[1].get_payload();
        let (meas_action_hdr, meas_rest) =
            ActionHeader::ref_from_prefix(&meas_payload).expect("Measurement ActionHeader");
        assert_eq!(meas_action_hdr.category, category::PUBLIC);
        assert_eq!(meas_action_hdr.action, public_action::FINE_TIMING_MEASUREMENT);

        let (meas_ftm, _) = FineTimingMeasurement::ref_from_prefix(meas_rest)
            .expect("Measurement FineTimingMeasurement");
        let meas_tod = meas_ftm.tod;
        let meas_toa = meas_ftm.toa;

        let mut tod_buf = [0u8; 8];
        tod_buf[..6].copy_from_slice(&meas_tod);
        let t1 = u64::from_le_bytes(tod_buf);

        let mut toa_buf = [0u8; 8];
        toa_buf[..6].copy_from_slice(&meas_toa);
        let t4 = u64::from_le_bytes(toa_buf);

        let rtt = t4.saturating_sub(t1);
        assert!(
            rtt.abs_diff(expected_rtt_ps) <= tolerance_ps,
            "RTT {} ps not within tolerance {} ps of expected {} ps",
            rtt,
            tolerance_ps,
            expected_rtt_ps
        );
    }

    pub async fn when_chip_transmits_qos_cf_ack_with_ftm_bytes(
        &mut self,
        sender_idx: usize,
        receiver_idx: usize,
    ) {
        let body = [category::PUBLIC, public_action::FTM_REQUEST, FTM_TRIGGER_START, 0];
        self.transmit_3addr_station_frame(
            sender_idx,
            receiver_idx,
            FrameType::Data,
            u8::from(DataSubType::QosCfAck),
            &body,
        )
        .await;
    }

    pub async fn then_chip_receives_data_subtype(
        &mut self,
        receiver_idx: usize,
        expected_subtype: DataSubType,
    ) {
        let chip = &mut self.chips[receiver_idx];
        let timeout = tokio::time::sleep(std::time::Duration::from_secs(3));
        tokio::pin!(timeout);

        loop {
            tokio::select! {
                Some(bytes) = chip.stream_rx.recv() => {
                    if let Ok(msg) = HwsimMsg::decode_full(&bytes)
                        && usize::try_from(msg.nl_hdr.nlmsg_len) == Ok(bytes.len())
                    {
                        if msg.hwsim_hdr.hwsim_cmd != HwsimCmd::Frame {
                            continue;
                        }
                        if let Ok(frame) = HwsimFrame::parse(&msg) {
                            assert!(
                                frame.ieee80211.is_data(),
                                "Expected data frame, got fc={:#06x}",
                                frame.ieee80211.get_fc()
                            );
                            assert_eq!(
                                frame.ieee80211.stype(),
                                u8::from(expected_subtype),
                                "Expected data subtype {:?}, got {}",
                                expected_subtype,
                                frame.ieee80211.stype()
                            );
                            return;
                        }
                    }
                }
                _ = &mut timeout => {
                    panic!("Timeout waiting for data frame with subtype {:?} on chip {}", expected_subtype, chip.id);
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

    pub async fn when_ap_is_deleted(&mut self, ap_id: u32) {
        self.ap_client.destroy_ap(ap_id).await.unwrap();
    }

    pub async fn when_chip_is_deleted_closing_test_channels(&mut self, chip_id: u32) {
        self.wifi_client.delete(ChipId(chip_id)).await.expect("Failed to delete chip");
        self.chips.retain(|c| c.id != chip_id);
    }

    #[cfg(target_os = "linux")]
    pub fn when_gateway_downlink_stream_closed(&mut self) {
        self.gateway_downlink_tx = None;
    }

    pub async fn then_chip_is_not_found(&self, chip_id: u32) {
        let chip = self.wifi_client.read(ChipId(chip_id)).await;
        assert!(chip.is_err());
    }

    pub fn then_error_stats_are_empty(&self) {
        assert_eq!(self.error_stats.snapshot(), &[]);
    }

    pub async fn when_chip_rx_is_drained(&mut self, chip_idx: usize) {
        while self.chips[chip_idx].stream_rx.try_recv().is_ok() {}
    }

    pub async fn then_chip_receives_tx_nack(&mut self, chip_idx: usize) {
        let acked = recv_next_tx_info_ack(&mut self.chips[chip_idx]).await;
        assert!(
            !acked,
            "Expected TxInfoFrame NACK (HWSIM_TX_STAT_ACK cleared), but frame was ACKed"
        );
    }

    pub async fn then_other_errors_increased_by(&mut self, expected_increase: i32) {
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        let final_stats = self.wifi_client.get_global_stats_proto().await.unwrap();
        assert_eq!(
            final_stats.other_errors(),
            self.baseline_stats.as_ref().unwrap().other_errors() + expected_increase
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

    pub async fn given_default_open_ap(&mut self) {
        let ap_config = ap_actor::ApConfig {
            ssid: "AndroidWifi".to_string(),
            bssid: ap_actor::expected_default_ap_bssid(),
            channel: 1,
            beacon_interval: 50,
            wpa_passphrase: None,
            ..Default::default()
        };
        self.ap_client
            .create_ap(Some(0), ap_config)
            .await
            .expect("Failed to create default open AP (ID 0)");
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }

    pub async fn given_raw_chip(&mut self, id_val: u32, sta_mac: [u8; 6]) {
        let chip = self.create_chip_channels(id_val, id_val, sta_mac).await;
        self.raw_chips.insert(id_val, chip);
    }

    pub async fn given_raw_chip_registered_with_nullfunc(
        &mut self,
        id_val: u32,
        sta_mac: [u8; 6],
        hwsim_addr: [u8; 6],
        bssid: [u8; 6],
        freq: u32,
    ) {
        self.given_raw_chip(id_val, sta_mac).await;
        self.when_raw_chip_sends_nullfunc(id_val, hwsim_addr, bssid, freq).await;
        self.when_raw_chip_rx_drained(id_val);
    }

    pub async fn given_raw_chip_registered_with_data(
        &mut self,
        id_val: u32,
        sta_mac: [u8; 6],
        hwsim_addr: [u8; 6],
        dst_mac: [u8; 6],
        bssid: [u8; 6],
        freq: u32,
    ) {
        self.given_raw_chip(id_val, sta_mac).await;
        self.when_raw_chip_sends_to_ds_data(
            id_val,
            hwsim_addr,
            dst_mac,
            bssid,
            freq,
            b"RegisterChip",
        )
        .await;
        self.when_raw_chip_rx_drained(id_val);
    }

    pub fn when_raw_chip_rx_drained(&mut self, id_val: u32) {
        if let Some(chip) = self.raw_chips.get_mut(&id_val) {
            while chip.stream_rx.try_recv().is_ok() {}
        }
    }

    pub async fn when_raw_chip_sends_nullfunc(
        &mut self,
        id_val: u32,
        hwsim_addr: [u8; 6],
        bssid: [u8; 6],
        freq: u32,
    ) {
        let chip = self.raw_chips.get_mut(&id_val).expect("Chip not found");
        let sta_mac = chip.mac;
        let nullfunc =
            crate::hwsim_helper::build_to_ds_nullfunc_hwsim(sta_mac, hwsim_addr, bssid, freq);
        chip.sink_tx.send(nullfunc).await.expect("Failed to send NullFunc");
        assert!(
            recv_next_tx_info_ack(chip).await,
            "Expected TxInfoFrame ACK on NullFunc from chip {id_val}"
        );
    }

    pub async fn when_raw_chip_sends_to_ds_ethernet(
        &mut self,
        id_val: u32,
        hwsim_addr: [u8; 6],
        bssid: [u8; 6],
        freq: u32,
        eth: &[u8],
    ) {
        let chip = self.raw_chips.get_mut(&id_val).expect("Chip not found");
        let uplink = crate::hwsim_helper::build_to_ds_data_hwsim(eth, bssid, hwsim_addr, freq);
        chip.sink_tx.send(uplink).await.expect("Failed to send ToDS data");
        assert!(
            recv_next_tx_info_ack(chip).await,
            "Expected TxInfoFrame ACK on ToDS data from chip {id_val}"
        );
    }

    pub async fn when_raw_chip_sends_to_ds_data(
        &mut self,
        id_val: u32,
        hwsim_addr: [u8; 6],
        dst_mac: [u8; 6],
        bssid: [u8; 6],
        freq: u32,
        payload: &[u8],
    ) {
        let chip_mac = self.raw_chips.get(&id_val).expect("Chip not found").mac;
        let eth = Self::create_ethernet_frame(&chip_mac, &dst_mac, payload);
        self.when_raw_chip_sends_to_ds_ethernet(id_val, hwsim_addr, bssid, freq, &eth).await;
    }

    pub async fn when_raw_chip_sends_ibss_data(
        &mut self,
        id_val: u32,
        hwsim_addr: [u8; 6],
        dst_mac: [u8; 6],
        bssid: [u8; 6],
        freq: u32,
        payload: &[u8],
    ) {
        let chip = self.raw_chips.get_mut(&id_val).expect("Chip not found");
        let ibss = crate::hwsim_helper::build_ibss_data_hwsim(
            chip.mac, dst_mac, bssid, hwsim_addr, freq, payload,
        );
        chip.sink_tx.send(ibss).await.expect("Failed to send IBSS data");
        assert!(
            !recv_next_tx_info_ack(chip).await,
            "Expected unacked TxInfoFrame on multi-peer unknown-unicast flood from chip {id_val}"
        );
    }

    pub fn when_gateway_injects_downlink(
        &self,
        src_mac: &[u8; 6],
        dst_mac: &[u8; 6],
        payload: &[u8],
    ) {
        let gw = self.mock_gateway.as_ref().expect("MockGateway not configured");
        let eth = Self::create_ethernet_frame(src_mac, dst_mac, payload);
        gw.inject_downlink_ethernet(eth);
    }

    pub async fn when_witness_chip_observes_beacons(
        &mut self,
        id_val: u32,
        expected_bssid: [u8; 6],
        expected_rx_hwsim: [u8; 6],
        count: usize,
    ) {
        for _ in 0..count {
            self.then_raw_chip_receives_beacon(id_val, expected_bssid, expected_rx_hwsim).await;
        }
    }

    pub async fn then_raw_chip_receives_beacon(
        &mut self,
        id_val: u32,
        expected_bssid: [u8; 6],
        expected_rx_hwsim: [u8; 6],
    ) {
        let chip = self.raw_chips.get_mut(&id_val).expect("Chip not found");
        let timeout = tokio::time::sleep(std::time::Duration::from_millis(800));
        tokio::pin!(timeout);

        loop {
            tokio::select! {
                Some(bytes) = chip.stream_rx.recv() => {
                    if let Ok(hwsim_msg) = netsim_packets::HwsimMsg::decode_full(&bytes)
                        && hwsim_msg.hwsim_hdr.hwsim_cmd == netsim_packets::HwsimCmd::Frame
                        && let Ok(frame) = netsim_packets::HwsimFrame::parse(&hwsim_msg)
                        && frame.ieee80211.is_mgmt()
                        && frame.ieee80211.stype() == netsim_packets::management_subtype::BEACON
                    {
                        assert_eq!(
                            frame.ieee80211.get_bssid(),
                            Some(MacAddress::new(expected_bssid)),
                            "Beacon BSSID mismatch on chip {id_val}"
                        );
                        assert_eq!(
                            frame.attrs.receiver,
                            Some(MacAddress::new(expected_rx_hwsim)),
                            "Beacon receiver hwsim mismatch on chip {id_val}"
                        );
                        return;
                    }
                }
                _ = &mut timeout => {
                    panic!("Timeout waiting for AP Beacon on chip {id_val}");
                }
            }
        }
    }

    pub async fn then_raw_chip_receives_downlink(
        &mut self,
        id_val: u32,
        expected_payload: &[u8],
        expected_dst_mac: [u8; 6],
        expected_rx_hwsim: [u8; 6],
        expected_freq: Option<u32>,
    ) {
        let chip = self.raw_chips.get_mut(&id_val).expect("Chip not found");
        let timeout = tokio::time::sleep(std::time::Duration::from_millis(800));
        tokio::pin!(timeout);

        loop {
            tokio::select! {
                Some(bytes) = chip.stream_rx.recv() => {
                    if let Ok(hwsim_msg) = netsim_packets::HwsimMsg::decode_full(&bytes)
                        && hwsim_msg.hwsim_hdr.hwsim_cmd == netsim_packets::HwsimCmd::Frame
                        && let Ok(frame) = netsim_packets::HwsimFrame::parse(&hwsim_msg)
                        && let Ok(eth) = crate::hwsim_helper::unwrap_hwsim_to_ethernet(&bytes)
                        && eth.len() >= expected_payload.len()
                        && eth.windows(expected_payload.len()).any(|w| w == expected_payload)
                    {
                        let mut dst_mac = [0u8; 6];
                        dst_mac.copy_from_slice(&eth[0..6]);
                        assert_eq!(
                            dst_mac, expected_dst_mac,
                            "Downlink destination MAC mismatch on chip {id_val}"
                        );
                        assert_eq!(
                            frame.attrs.receiver,
                            Some(MacAddress::new(expected_rx_hwsim)),
                            "Downlink receiver hwsim mismatch on chip {id_val}"
                        );
                        if let Some(freq) = expected_freq {
                            assert_eq!(
                                frame.attrs.freq,
                                Some(freq),
                                "Downlink frequency mismatch on chip {id_val}"
                            );
                        }
                        return;
                    }
                }
                _ = &mut timeout => {
                    panic!(
                        "Timeout waiting for downlink payload {:?} on chip {id_val}",
                        std::str::from_utf8(expected_payload).unwrap_or("<binary>")
                    );
                }
            }
        }
    }

    pub async fn then_raw_chip_receives_ibss_data(
        &mut self,
        id_val: u32,
        expected_payload: &[u8],
        expected_dst_mac: [u8; 6],
        expected_rx_hwsim: [u8; 6],
        expected_freq: u32,
    ) {
        let chip = self.raw_chips.get_mut(&id_val).expect("Chip not found");
        let timeout = tokio::time::sleep(std::time::Duration::from_millis(800));
        tokio::pin!(timeout);

        loop {
            tokio::select! {
                Some(bytes) = chip.stream_rx.recv() => {
                    if let Ok(hwsim_msg) = netsim_packets::HwsimMsg::decode_full(&bytes)
                        && hwsim_msg.hwsim_hdr.hwsim_cmd == netsim_packets::HwsimCmd::Frame
                        && let Ok(frame) = netsim_packets::HwsimFrame::parse(&hwsim_msg)
                        && frame.ieee80211.is_data()
                        && !frame.ieee80211.is_to_ap()
                        && !frame.ieee80211.is_from_ds()
                        && frame.ieee80211.get_payload() == expected_payload
                    {
                        assert_eq!(
                            frame.ieee80211.get_destination(),
                            MacAddress::new(expected_dst_mac),
                            "IBSS destination MAC mismatch on chip {id_val}"
                        );
                        assert_eq!(
                            frame.attrs.receiver,
                            Some(MacAddress::new(expected_rx_hwsim)),
                            "IBSS receiver hwsim mismatch on chip {id_val}"
                        );
                        assert_eq!(
                            frame.attrs.freq,
                            Some(expected_freq),
                            "IBSS frequency mismatch on chip {id_val}"
                        );
                        return;
                    }
                }
                _ = &mut timeout => {
                    panic!(
                        "Timeout waiting for IBSS payload {:?} on chip {id_val}",
                        std::str::from_utf8(expected_payload).unwrap_or("<binary>")
                    );
                }
            }
        }
    }

    pub async fn then_raw_chip_receives_no_frames(&mut self, id_val: u32) {
        let _ = self.wifi_client.list().await.expect("Failed to list chips");
        for _ in 0..10 {
            tokio::task::yield_now().await;
        }
        let chip = self.raw_chips.get_mut(&id_val).expect("Chip not found");
        assert!(
            chip.stream_rx.try_recv().is_err(),
            "Expected no frames on chip {id_val}, but found a frame"
        );
    }

    #[cfg(target_os = "linux")]
    pub fn given_mock_clock_millis(&self, millis: u64) {
        self.mock_clock.store(millis);
    }

    #[cfg(target_os = "linux")]
    pub fn when_gateway_sends_downlink_frame(&self, frame: &[u8]) {
        let tx = self
            .gateway_downlink_tx
            .as_ref()
            .expect("gateway_downlink_tx must be configured; call new_with_tap_gateway()");
        tx.send(Bytes::copy_from_slice(frame))
            .expect("sending downlink frame into gateway should succeed");
    }

    #[cfg(target_os = "linux")]
    pub async fn then_chip_receives_ethernet_frame(
        &mut self,
        receiver_idx: usize,
        expected_frame: &[u8],
    ) {
        let chip = &mut self.chips[receiver_idx];
        let timeout = tokio::time::sleep(std::time::Duration::from_secs(3));
        tokio::pin!(timeout);

        loop {
            tokio::select! {
                maybe_pkt = chip.stream_rx.recv() => {
                    let bytes = maybe_pkt.expect("chip stream_rx closed unexpectedly");
                    if let Ok(eth) = crate::hwsim_helper::unwrap_hwsim_to_ethernet(&bytes) {
                        assert_eq!(eth, expected_frame);
                        return;
                    }
                }
                _ = &mut timeout => {
                    panic!(
                        "Timed out waiting for downlink Ethernet frame on chip {}",
                        chip.id
                    );
                }
            }
        }
    }

    #[cfg(target_os = "linux")]
    pub async fn then_max_download_throughput_is(&self, expected_mbps: f32) {
        let stats = self.wifi_client.get_global_stats_proto().await.unwrap();
        assert!(
            (stats.max_download_throughput() - expected_mbps).abs() < 1e-6,
            "expected max_download_throughput {} Mbps, got {}",
            expected_mbps,
            stats.max_download_throughput()
        );
    }

    pub async fn given_a_chip_with_randomized_mac(
        &mut self,
        id_val: u32,
        randomized_mac: [u8; 6],
        hw_mac: [u8; 6],
    ) -> u32 {
        let created_id = self.given_a_chip(id_val).await;
        let idx = self.chips.len() - 1;
        self.chips[idx].mac = randomized_mac;

        let dst_mac = [0x00, 0x00, 0x00, 0x00, 0x00, 0xFE];
        let outgoing_eth = Self::create_ethernet_frame(&randomized_mac, &dst_mac, b"DualMapInit");
        let hwsim_packet =
            wrap_ethernet_in_hwsim(&outgoing_eth, &AP_BSSID, &AP_BSSID, &hw_mac, 2412)
                .expect("Failed to wrap dual-mapping init packet");

        self.chips[idx]
            .sink_tx
            .send(Bytes::from(hwsim_packet))
            .await
            .expect("Failed to send dual-mapping init packet");

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(1);
        loop {
            if self.shared_keys.get_station_bssid(&MacAddress::new(randomized_mac)).is_some() {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "Timed out waiting for randomized MAC mapping in SharedKeyStore"
            );
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        created_id
    }

    pub fn then_shared_keys_maps_mac_to_bssid(&self, mac: [u8; 6], expected_bssid: [u8; 6]) {
        let mapped = self.shared_keys.get_station_bssid(&MacAddress::new(mac));
        assert_eq!(
            mapped,
            Some(MacAddress::new(expected_bssid)),
            "Expected MAC {:02X?} to map to BSSID {:02X?}",
            mac,
            expected_bssid
        );
    }

    pub fn then_slirp_gateway_delivers_downlink_to_mac(
        &self,
        chip_idx: usize,
        hw_mac: [u8; 6],
        payload: &str,
    ) {
        let chip_id = self.chips[chip_idx].id;
        let randomized_mac = self.chips[chip_idx].mac;
        let mut medium = wifi_actor::Medium::new(
            self.shared_keys.clone(),
            wifi_actor::WifiStats::new(self.mock_clock.clone()),
            Arc::new(wifi_actor::DebugArgs::default()),
        );
        medium.add(chip_id);
        let reg_eth = Self::create_ethernet_frame(&randomized_mac, &AP_BSSID, b"Reg");
        let reg_hwsim = wrap_ethernet_in_hwsim(&reg_eth, &AP_BSSID, &AP_BSSID, &hw_mac, 2412)
            .expect("Failed to wrap station registration packet");
        let _ = medium
            .resolve_tx_packet(chip_id, &Bytes::from(reg_hwsim))
            .expect("Failed to resolve station registration packet");

        let gateway = wifi_actor::SlirpGateway::new(None);
        let incoming_eth = Self::create_ethernet_frame(&AP_BSSID, &hw_mac, payload.as_bytes());
        let mut out_queue = Vec::new();
        wifi_actor::GatewayTrait::handle_incoming(
            &gateway,
            ChipId(0),
            Bytes::from(incoming_eth),
            &mut medium,
            &self.shared_keys,
            &mut out_queue,
        );
        assert!(
            !out_queue.is_empty(),
            "SlirpGateway::handle_incoming dropped downlink packet addressed to hw_mac {:02X?}",
            hw_mac
        );
        let (target_chip_id, delivered_hwsim) = &out_queue[0];
        assert_eq!(*target_chip_id, chip_id);
        let unwrapped = crate::hwsim_helper::unwrap_hwsim_to_ethernet(delivered_hwsim)
            .expect("Failed to unwrap SlirpGateway downlink HWSIM packet");
        assert!(
            unwrapped.windows(payload.len()).any(|w| w == payload.as_bytes()),
            "Delivered SlirpGateway downlink frame did not contain expected payload"
        );
    }

    pub async fn when_wifi_client_lists_chips(&self) -> Vec<ChipId> {
        self.wifi_client
            .list()
            .await
            .expect("Failed to list chips via WifiClient")
            .into_iter()
            .map(|c| ChipId(c.id))
            .collect()
    }

    pub async fn when_rf_state_is_set(&self, chip_id: u32, enabled: bool) {
        self.wifi_client
            .set_rf_state(ChipId(chip_id), enabled)
            .await
            .expect("Failed to set RF state via WifiClient");
    }

    pub async fn when_chip_is_reset(&self, chip_id: u32) {
        self.wifi_client.reset(ChipId(chip_id)).await.expect("Failed to reset chip");
    }

    pub async fn then_chip_rf_state_is(&self, chip_id: u32, expected: bool) {
        let chip = self.wifi_client.read(ChipId(chip_id)).await.expect("Failed to read chip");
        assert_eq!(chip.enabled, expected, "Unexpected chip.enabled state");
        if let Some(netsim_model::ChipVariant::Wifi(wifi)) = chip.variant {
            assert_eq!(wifi.radio.state, Some(expected), "Unexpected Wifi radio.state");
        } else {
            panic!("Expected Wifi chip variant");
        }
    }

    pub async fn then_chip_rf_state_is_default(&self, chip_id: u32) {
        let chip = self.wifi_client.read(ChipId(chip_id)).await.expect("Failed to read chip");
        assert!(chip.enabled, "Expected chip.enabled == true after reset");
        if let Some(netsim_model::ChipVariant::Wifi(wifi)) = chip.variant {
            assert_eq!(wifi.radio.state, None, "Expected Wifi radio.state == None after reset");
        } else {
            panic!("Expected Wifi chip variant");
        }
    }

    pub async fn then_chip_counters_are_non_zero(&self, chip_id: u32) {
        let stats = self.wifi_client.read_statistics().await.expect("Failed to read statistics");
        let chip_stats = stats.iter().find(|s| s.id == chip_id).expect("Chip stats not found");
        assert!(chip_stats.tx_count > 0, "Expected tx_count > 0 before reset");
        assert!(chip_stats.rx_count > 0, "Expected rx_count > 0 before reset");
        assert!(chip_stats.p2p_tx_count > 0, "Expected p2p_tx_count > 0 before reset");
        assert!(chip_stats.p2p_rx_count > 0, "Expected p2p_rx_count > 0 before reset");
    }

    pub async fn then_chip_counters_are_zero(&self, chip_id: u32) {
        let stats = self.wifi_client.read_statistics().await.expect("Failed to read statistics");
        let chip_stats = stats.iter().find(|s| s.id == chip_id).expect("Chip stats not found");
        assert_eq!(chip_stats.rx_count, 0, "Expected rx_count == 0 after reset");
        assert_eq!(chip_stats.tx_count, 0, "Expected tx_count == 0 after reset");
        assert_eq!(chip_stats.p2p_rx_count, 0, "Expected p2p_rx_count == 0 after reset");
        assert_eq!(chip_stats.p2p_tx_count, 0, "Expected p2p_tx_count == 0 after reset");
    }

    pub async fn given_chip_registers_virtual_macs(
        &mut self,
        chip_idx: usize,
        peer_idx: usize,
        macs_and_freqs: &[([u8; 6], u32)],
    ) {
        let hwsim_mac = self.chips[chip_idx].mac;
        let peer_mac = self.chips[peer_idx].mac;
        for (i, (sa_mac, freq)) in macs_and_freqs.iter().enumerate() {
            let tag = format!("RegVirtualMac{i}");
            let eth = Self::create_ethernet_frame(sa_mac, &peer_mac, tag.as_bytes());
            let msg = wrap_ethernet_in_hwsim(&eth, &AP_BSSID, &peer_mac, &hwsim_mac, *freq)
                .expect("Failed to wrap frame with custom SA on channel");
            self.chips[chip_idx]
                .sink_tx
                .send(Bytes::from(msg))
                .await
                .expect("Failed to transmit frame with custom SA on channel");
            self.then_chip_receives_payload(peer_idx, &tag).await;
        }
    }

    pub async fn when_chip_sends_udp_to_host(
        &mut self,
        chip_idx: usize,
        host_port: u16,
        payload: &[u8],
    ) {
        use netsim_packets::test_utils::PacketBuilder;

        let client_mac = self.chips[chip_idx].mac;
        let client_ip = [10, 0, 2, 15];
        let gateway_ip = [10, 0, 2, 2];
        let gateway_mac = [0xde, 0xad, 0xbe, 0xef, 0x12, 0x35];

        // Announce Guest MAC -> IP mapping to Slirp's ARP table via an ARP Request.
        let arp_req = PacketBuilder::new([0xff; 6], client_mac, 0x0806)
            .arp(1, client_mac, client_ip, [0; 6], gateway_ip)
            .build();
        let arp_hwsim = wrap_ethernet_in_hwsim(&arp_req, &AP_BSSID, &AP_BSSID, &client_mac, 2412)
            .expect("Failed to wrap ARP request in HWSIM");
        self.chips[chip_idx]
            .sink_tx
            .send(Bytes::from(arp_hwsim))
            .await
            .expect("Failed to send ARP request to sink_tx");
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;

        let eth_frame = PacketBuilder::new(gateway_mac, client_mac, 0x0800)
            .ipv4(client_ip, gateway_ip, 17)
            .udp(12345, host_port)
            .payload(payload);

        let hwsim_msg = wrap_ethernet_in_hwsim(&eth_frame, &AP_BSSID, &AP_BSSID, &client_mac, 2412)
            .expect("Failed to wrap UDP frame in HWSIM");

        self.chips[chip_idx]
            .sink_tx
            .send(Bytes::from(hwsim_msg))
            .await
            .expect("Failed to send UDP frame to sink_tx");
    }

    pub async fn then_chip_receives_udp_reply_handling_arp(
        &mut self,
        chip_idx: usize,
        expected_reply: &[u8],
    ) {
        use netsim_packets::test_utils::PacketBuilder;

        let client_mac = self.chips[chip_idx].mac;
        let chip = &mut self.chips[chip_idx];
        let timeout = tokio::time::sleep(std::time::Duration::from_secs(3));
        tokio::pin!(timeout);

        loop {
            tokio::select! {
                Some(msg_bytes) = chip.stream_rx.recv() => {
                    if let Ok(eth_frame) = crate::hwsim_helper::unwrap_hwsim_to_ethernet(&msg_bytes)
                        && eth_frame.len() >= 14
                    {
                        let eth_type = u16::from_be_bytes([eth_frame[12], eth_frame[13]]);
                        if eth_type == 0x0806 && eth_frame.len() >= 32 {
                            let mut slirp_mac = [0u8; 6];
                            slirp_mac.copy_from_slice(&eth_frame[22..28]);
                            let mut slirp_ip = [0u8; 4];
                            slirp_ip.copy_from_slice(&eth_frame[28..32]);
                            let client_ip = [10, 0, 2, 15];

                            let arp_reply = PacketBuilder::new(slirp_mac, client_mac, 0x0806)
                                .arp(2, client_mac, client_ip, slirp_mac, slirp_ip)
                                .payload(&[]);

                            let hwsim_msg = wrap_ethernet_in_hwsim(
                                &arp_reply,
                                &AP_BSSID,
                                &AP_BSSID,
                                &client_mac,
                                2412,
                            )
                            .expect("Failed to wrap ARP reply");

                            chip.sink_tx
                                .send(Bytes::from(hwsim_msg))
                                .await
                                .expect("Failed to send ARP reply");
                        } else if eth_frame
                            .windows(expected_reply.len())
                            .any(|w| w == expected_reply)
                        {
                            return;
                        }
                    }
                }
                _ = &mut timeout => {
                    panic!("Guest chip {} did not receive UDP reply from Host via Slirp", chip.id);
                }
            }
        }
    }

    pub fn given_station_ptk(&self, sta_mac: [u8; 6], tk: &[u8; 16]) {
        self.shared_keys.add_session(MacAddress::new(sta_mac), tk.to_vec());
    }

    pub async fn when_infra_transmits_mdns(&mut self, payload: &str) {
        self.when_infra_transmits_unicast_to_mac(MDNS_MULTICAST_MAC, payload).await;
    }

    async fn recv_raw_chip_dhcp_reply(
        &mut self,
        id_val: u32,
        expected_xid: u32,
        expected_msg_type: u8,
    ) -> [u8; 4] {
        let chip = self.raw_chips.get_mut(&id_val).expect("Chip not found");
        let timeout_fut = tokio::time::timeout(std::time::Duration::from_secs(3), async {
            loop {
                let bytes = chip.stream_rx.recv().await.expect("stream_rx closed");
                if let Ok(eth) = crate::hwsim_helper::unwrap_hwsim_to_ethernet(&bytes)
                    && let Some(dhcp) = extract_dhcp_from_ethernet(&eth)
                    && get_dhcp_msg_type(dhcp) == Some(expected_msg_type)
                {
                    let xid_bytes: [u8; 4] = dhcp[DHCP_XID_OFFSET..DHCP_XID_OFFSET + 4]
                        .try_into()
                        .expect("slice length 4");
                    let xid = u32::from_be_bytes(xid_bytes);
                    if xid == expected_xid
                        && let Ok(yiaddr) =
                            dhcp[DHCP_YIADDR_OFFSET..DHCP_YIADDR_OFFSET + 4].try_into()
                    {
                        return yiaddr;
                    }
                }
            }
        });
        timeout_fut.await.unwrap_or_else(|_| {
            panic!("Timeout waiting for DHCP message type {expected_msg_type} on chip {id_val}");
        })
    }

    pub async fn when_raw_chip_acquires_dhcp_lease(
        &mut self,
        id_val: u32,
        hwsim_addr: [u8; 6],
        ap_bssid: [u8; 6],
        freq: u32,
    ) -> std::net::Ipv4Addr {
        let xid = DHCP_XID_PREFIX | (id_val & 0xFFFF);
        let sta_mac = self.raw_chips.get(&id_val).expect("Chip not found").mac;

        let disc_eth = build_dhcp_packet(&sta_mac, xid, DHCPDISCOVER, None);
        self.when_raw_chip_sends_to_ds_ethernet(id_val, hwsim_addr, ap_bssid, freq, &disc_eth)
            .await;
        let offered_ip = self.recv_raw_chip_dhcp_reply(id_val, xid, DHCPOFFER).await;

        let req_eth = build_dhcp_packet(&sta_mac, xid, DHCPREQUEST, Some(offered_ip));
        self.when_raw_chip_sends_to_ds_ethernet(id_val, hwsim_addr, ap_bssid, freq, &req_eth).await;
        let ack_ip = self.recv_raw_chip_dhcp_reply(id_val, xid, DHCPACK).await;

        std::net::Ipv4Addr::from(ack_ip)
    }

    pub fn then_leases_are_valid_and_distinct(&self, leases: &[std::net::Ipv4Addr]) {
        assert!(!leases.is_empty(), "Lease list must not be empty");
        let mut unique_leases = std::collections::HashSet::new();
        for lease in leases {
            let octets = lease.octets();
            assert_eq!(
                octets[0..3],
                SLIRP_SUBNET_PREFIX,
                "Lease {lease} does not match expected subnet 10.0.2.0/24"
            );
            assert!(
                octets[3] != 0 && octets[3] != 255,
                "Host octet {} is network or broadcast address",
                octets[3]
            );
            assert!(unique_leases.insert(*lease), "Duplicate DHCP lease detected: {lease}");
        }
    }

    pub async fn then_raw_chip_receives_encrypted_m2u_frame(
        &mut self,
        id_val: u32,
        tk: &[u8; 16],
        expected_payload: &str,
    ) {
        let chip = self.raw_chips.get_mut(&id_val).expect("Chip not found");
        let chip_mac = chip.mac;
        let expected_bytes = expected_payload.as_bytes();
        let timeout_fut = tokio::time::timeout(std::time::Duration::from_secs(3), async {
            loop {
                let bytes = chip.stream_rx.recv().await.expect("stream_rx closed");
                if let Ok(hwsim_msg) = netsim_packets::HwsimMsg::decode_full(&bytes)
                    && hwsim_msg.hwsim_hdr.hwsim_cmd == netsim_packets::HwsimCmd::Frame
                    && let Ok(frame) = netsim_packets::HwsimFrame::parse(&hwsim_msg)
                {
                    let ieee = frame.ieee80211;
                    if ieee.is_data()
                        && ieee.needs_decryption()
                        && ieee.get_destination() == MacAddress::new(chip_mac)
                    {
                        let store = ap_actor::SharedKeyStore::new();
                        store.add_session(ieee.get_source(), tk.to_vec());
                        if let Some(decrypted) = store.try_decrypt(&ieee)
                            && decrypted.windows(expected_bytes.len()).any(|w| w == expected_bytes)
                        {
                            return;
                        }
                    }
                }
            }
        });
        timeout_fut.await.unwrap_or_else(|_| {
            panic!("Timeout waiting for encrypted M2U frame on chip {id_val}");
        });
    }
}

pub(crate) const MDNS_MULTICAST_MAC: [u8; 6] = [0x01, 0x00, 0x5E, 0x00, 0x00, 0xFB];
const DHCP_CLIENT_PORT: u16 = 68;
const DHCP_OP_BOOTREQUEST: u8 = 1;
const DHCP_HTYPE_ETHERNET: u8 = 1;
const DHCP_HLEN_ETHERNET: u8 = 6;
const DHCP_XID_OFFSET: usize = 4;
const DHCP_FLAGS_OFFSET: usize = 10;
const DHCP_BROADCAST_FLAG: u16 = 0x8000;
const DHCP_YIADDR_OFFSET: usize = 16;
const DHCP_CHADDR_OFFSET: usize = 28;
const DHCP_FIXED_HEADER_LEN: usize = 236;
const DHCP_MAGIC_COOKIE: [u8; 4] = [99, 130, 83, 99];
const DHCP_MIN_HEADER_LEN: usize = DHCP_FIXED_HEADER_LEN + DHCP_MAGIC_COOKIE.len();
const DHCP_MIN_BOOTP_PACKET_LEN: usize = 300;
const DHCP_OPT_PAD: u8 = 0;
const DHCP_OPT_REQUESTED_IP: u8 = 50;
const DHCP_OPT_MSG_TYPE: u8 = 53;
const DHCP_OPT_END: u8 = 255;
const DHCPDISCOVER: u8 = 1;
const DHCPOFFER: u8 = 2;
const DHCPREQUEST: u8 = 3;
const DHCPACK: u8 = 5;
const DHCP_XID_PREFIX: u32 = 0x5814_0000;
const SLIRP_SUBNET_PREFIX: [u8; 3] = [10, 0, 2];

fn build_dhcp_packet(
    src_mac: &[u8; 6],
    xid: u32,
    msg_type: u8,
    requested_ip: Option<[u8; 4]>,
) -> Vec<u8> {
    let mut dhcp = vec![0u8; DHCP_FIXED_HEADER_LEN];
    dhcp[0] = DHCP_OP_BOOTREQUEST;
    dhcp[1] = DHCP_HTYPE_ETHERNET;
    dhcp[2] = DHCP_HLEN_ETHERNET;
    dhcp[DHCP_XID_OFFSET..DHCP_XID_OFFSET + 4].copy_from_slice(&xid.to_be_bytes());
    dhcp[DHCP_FLAGS_OFFSET..DHCP_FLAGS_OFFSET + 2]
        .copy_from_slice(&DHCP_BROADCAST_FLAG.to_be_bytes());
    dhcp[DHCP_CHADDR_OFFSET..DHCP_CHADDR_OFFSET + 6].copy_from_slice(src_mac);

    dhcp.extend_from_slice(&DHCP_MAGIC_COOKIE);
    dhcp.extend_from_slice(&[DHCP_OPT_MSG_TYPE, 1, msg_type]);

    if let Some(ip) = requested_ip {
        dhcp.extend_from_slice(&[DHCP_OPT_REQUESTED_IP, 4, ip[0], ip[1], ip[2], ip[3]]);
    }

    dhcp.push(DHCP_OPT_END);

    if dhcp.len() < DHCP_MIN_BOOTP_PACKET_LEN {
        dhcp.resize(DHCP_MIN_BOOTP_PACKET_LEN, 0);
    }

    PacketBuilder::new([0xFF; 6], *src_mac, ether_type::IPV4)
        .ipv4([0; 4], [255; 4], IP_P_UDP)
        .udp(DHCP_CLIENT_PORT, DHCP_SERVER_PORT)
        .payload(&dhcp)
}

fn extract_dhcp_from_ethernet(eth_bytes: &[u8]) -> Option<&[u8]> {
    let (eth, eth_payload) = EthernetFrame::parse(eth_bytes)?;
    if eth.ethertype.get() != ether_type::IPV4 {
        return None;
    }
    let (ip, ip_payload) = Ipv4Header::parse(eth_payload)?;
    if ip.protocol != IP_P_UDP {
        return None;
    }
    let (udp, udp_payload) = UdpHeader::parse(ip_payload)?;
    if udp.dest_port.get() == DHCP_CLIENT_PORT || udp.source_port.get() == DHCP_CLIENT_PORT {
        return Some(udp_payload);
    }
    None
}

fn get_dhcp_msg_type(dhcp: &[u8]) -> Option<u8> {
    if dhcp.len() < DHCP_MIN_HEADER_LEN {
        return None;
    }
    if dhcp[DHCP_FIXED_HEADER_LEN..DHCP_MIN_HEADER_LEN] != DHCP_MAGIC_COOKIE {
        return None;
    }
    let mut idx = DHCP_MIN_HEADER_LEN;
    while idx < dhcp.len() {
        let opt = dhcp[idx];
        if opt == DHCP_OPT_END {
            break;
        }
        if opt == DHCP_OPT_PAD {
            idx += 1;
            continue;
        }
        if idx + 1 >= dhcp.len() {
            break;
        }
        let len = dhcp[idx + 1] as usize;
        if idx + 2 + len > dhcp.len() {
            break;
        }
        if opt == DHCP_OPT_MSG_TYPE && len >= 1 {
            return Some(dhcp[idx + 2]);
        }
        idx += 2 + len;
    }
    None
}

const MOCK_GATEWAY_STREAM_ID: ChipId = ChipId(u32::MAX - 1);

#[derive(Clone, Debug)]
pub struct MockGateway {
    pub outgoing_packets: Arc<std::sync::Mutex<Vec<(ChipId, Bytes)>>>,
    incoming_tx: Arc<std::sync::Mutex<Option<mpsc::UnboundedSender<Bytes>>>>,
    incoming_rx: Arc<std::sync::Mutex<Option<mpsc::UnboundedReceiver<Bytes>>>>,
}

impl MockGateway {
    pub fn new() -> Self {
        let (incoming_tx, incoming_rx) = mpsc::unbounded_channel();
        Self {
            outgoing_packets: Arc::new(std::sync::Mutex::new(Vec::new())),
            incoming_tx: Arc::new(std::sync::Mutex::new(Some(incoming_tx))),
            incoming_rx: Arc::new(std::sync::Mutex::new(Some(incoming_rx))),
        }
    }

    fn inject_downlink_ethernet(&self, eth_frame: Vec<u8>) {
        self.incoming_tx
            .lock()
            .unwrap()
            .as_ref()
            .expect("MockGateway incoming channel closed")
            .send(Bytes::from(eth_frame))
            .expect("Failed to inject downlink Ethernet frame into MockGateway");
    }

    pub fn clear(&self) {
        self.outgoing_packets.lock().unwrap().clear();
    }

    pub async fn wait_for_packet_count(&self, expected: usize) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(1);
        loop {
            if self.outgoing_packets.lock().unwrap().len() == expected {
                return;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "Timed out waiting for MockGateway packet count == {expected}"
            );
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    }

    pub async fn assert_no_packets_within(&self, wait_ms: u64) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(wait_ms);
        while std::time::Instant::now() < deadline {
            assert!(
                self.outgoing_packets.lock().unwrap().is_empty(),
                "MockGateway unexpectedly received a packet"
            );
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    }

    pub fn close_incoming(&self) {
        let _ = self.incoming_tx.lock().unwrap().take();
    }
}

#[async_trait::async_trait]
impl wifi_actor::GatewayTrait for MockGateway {
    async fn send_80211(
        &self,
        chip_id: ChipId,
        ieee80211: &Ieee80211,
    ) -> Result<usize, wifi_actor::WifiError> {
        let bytes = ieee80211
            .encode_to_vec()
            .map_err(|e| wifi_actor::WifiError::Frame(Box::from(e.to_string())))?;
        let len = bytes.len();
        self.outgoing_packets.lock().unwrap().push((chip_id, Bytes::from(bytes)));
        Ok(len)
    }

    fn should_handle(&self, chip_id: ChipId) -> bool {
        chip_id == MOCK_GATEWAY_STREAM_ID
    }

    fn handle_incoming(
        &self,
        _chip_id: ChipId,
        packet: Bytes,
        medium: &mut wifi_actor::Medium,
        _shared_keys: &ap_actor::SharedKeyStore,
        out_queue: &mut Vec<(u32, Bytes)>,
    ) {
        medium
            .process_ieee8023_response(&packet, out_queue)
            .expect("MockGateway failed to process IEEE 802.3 downlink response");
    }

    async fn on_start(&mut self, ctx: &mut actor_framework::DynContext<WifiActor>) {
        let rx = self.incoming_rx.lock().unwrap().take();
        if let Some(rx) = rx {
            ctx.add_stream(MOCK_GATEWAY_STREAM_ID, Box::pin(UnboundedReceiverStream::new(rx)));
        }
    }

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

#[cfg(target_os = "linux")]
#[derive(Debug)]
struct TapDownlinkHarness {
    inner: wifi_actor::TapGateway,
    downlink_rx: Option<mpsc::UnboundedReceiver<Bytes>>,
}

#[cfg(target_os = "linux")]
impl TapDownlinkHarness {
    fn new() -> (Self, mpsc::UnboundedSender<Bytes>) {
        let (downlink_tx, downlink_rx) = mpsc::unbounded_channel();
        (
            Self {
                inner: wifi_actor::TapGateway::new("test-tap0".to_string()),
                downlink_rx: Some(downlink_rx),
            },
            downlink_tx,
        )
    }
}

#[cfg(target_os = "linux")]
const TAP_FLAG: u32 = 0x8000_0000;
#[cfg(target_os = "linux")]
const ETHERNET_HEADER_LEN: usize = 14;

#[cfg(target_os = "linux")]
#[async_trait::async_trait]
impl wifi_actor::GatewayTrait for TapDownlinkHarness {
    async fn send_80211(
        &self,
        _chip_id: netsim_model::ChipId,
        ieee80211: &netsim_packets::Ieee80211,
    ) -> Result<usize, wifi_actor::WifiError> {
        if ieee80211.is_qos_nodata() {
            return Ok(0);
        }
        let payload_len = ieee80211
            .to_ieee8023()
            .map(|eth| eth.len().saturating_sub(ETHERNET_HEADER_LEN))
            .unwrap_or_else(|_| ieee80211.get_payload().len());
        Ok(payload_len)
    }

    fn should_handle(&self, chip_id: netsim_model::ChipId) -> bool {
        self.inner.should_handle(chip_id)
    }

    fn handle_incoming(
        &self,
        chip_id: netsim_model::ChipId,
        packet: Bytes,
        medium: &mut wifi_actor::Medium,
        shared_keys: &ap_actor::SharedKeyStore,
        out_queue: &mut Vec<(u32, Bytes)>,
    ) {
        self.inner.handle_incoming(chip_id, packet, medium, shared_keys, out_queue);
    }

    async fn on_start(&mut self, _ctx: &mut actor_framework::DynContext<WifiActor>) {}

    async fn on_chip_create(
        &mut self,
        chip_id: netsim_model::ChipId,
        ctx: &mut actor_framework::DynContext<WifiActor>,
    ) {
        if let Some(rx) = self.downlink_rx.take() {
            ctx.add_stream(
                netsim_model::ChipId(chip_id.0 | TAP_FLAG),
                Box::pin(tokio_stream::wrappers::UnboundedReceiverStream::new(rx)),
            );
        }
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
