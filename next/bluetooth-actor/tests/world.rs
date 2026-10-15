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

use std::collections::HashMap;

use actor_framework::ResourceClient;
use bluetooth_actor::{BluetoothActor, BluetoothClient, BluetoothError};
use common::util::scanner_util::parse_hci_scan_report;
use device_actor::client::DeviceClient;
use netsim_model::{
    AdvertiseSettings, AdvertiseTxPower, BeaconParams, BleBeacon, BluetoothMode, Chip, ChipCreate,
    ChipId, ChipUpdate, ChipVariant, DeviceId, DeviceParams, Interval, PacketSink, PacketStream,
    ScannerParams, TxPower,
};
use netsim_proto::{hci_packet::hcipacket::PacketType, protobuf::Enum};
use tokio::{sync::mpsc, task::JoinHandle};
use tracing::{info, warn};
use zerocopy::{Immutable, IntoBytes, KnownLayout};

/// The BDD World for Bluetooth Actor tests.
pub struct World {
    /// The Bluetooth Client under test.
    pub client: BluetoothClient,
    /// The mocked or real DeviceClient for interactions.
    pub _device_client: DeviceClient,
    /// The background task running the actor (dropped on World drop).
    pub _actor_task: JoinHandle<()>,
    /// Counter for generating unique ChipIds in tests.
    pub chip_id_counter: u32,
    /// The ID of the simulated device.
    pub device_id: DeviceId,
    /// Map of chip names to their IDs.
    pub chips: HashMap<String, ChipId>,
    /// Map of chip names to their stream senders (to inject packets).
    pub streams: HashMap<String, mpsc::Sender<bytes::Bytes>>,
    /// Map of chip names to their sink receivers (to capture packets).
    pub sinks: HashMap<String, mpsc::Receiver<Vec<u8>>>,
}

impl World {
    /// Creates a new World instance.
    pub fn new() -> Self {
        netsim_testing::setup(None);
        let (device_tx, mut _device_rx) = mpsc::channel(10);
        // Use real DeviceClient for integration testing.
        let resource_client = DeviceClient::new(Box::new(ResourceClient::new(device_tx)));
        let (actor, client) = bluetooth_actor::new();
        let resource_client_clone = resource_client.clone();
        let _client_clone = client.clone();
        // Dropping _device_rx causes the channel to close, which prevents blocking
        // (sends fail immediately). BluetoothActor handles these failures
        // gracefully (ignoring them), so explicit draining is not needed.

        let actor_task = tokio::spawn(async move {
            actor.run(BluetoothActor::new(resource_client_clone, false)).await;
        });

        World {
            client,
            _device_client: resource_client,
            _actor_task: actor_task,
            chip_id_counter: 0,
            device_id: DeviceId(1),
            chips: HashMap::new(),
            streams: HashMap::new(),
            sinks: HashMap::new(),
        }
    }

    /// Helper to get a fresh Chip ID.
    pub fn next_chip_id(&mut self) -> ChipId {
        self.chip_id_counter += 1;
        ChipId(self.chip_id_counter)
    }

    /// Internal helper to create and register a chip
    async fn create_chip(
        &mut self,
        name: &str,
        mode: BluetoothMode,
        packet_stream: Option<PacketStream>,
        packet_sink: Option<PacketSink>,
        device_id: DeviceId,
    ) -> ChipId {
        let id = self.next_chip_id();
        // Use two octets for the chip ID to support up to 65535 chips.
        let address = format!("60:70:80:90:{:02X}:{:02X}", (id.0 >> 8) & 0xFF, id.0 & 0xFF);

        let mut chip = Chip::new_test_ble(name);
        chip.id = id.0;
        chip.device_id = device_id;
        chip.product_name = name.to_string();
        chip.variant = Some(ChipVariant::Bluetooth(Box::new(netsim_model::Bluetooth {
            low_energy: Default::default(),
            classic: Default::default(),
            address,
            bt_properties: Default::default(),
            mode,
            ..Default::default()
        })));

        let params = ChipCreate { packet_stream, packet_sink, chip };

        if let Err(e) = self.client.0.create_with_id(id, params).await {
            panic!("Failed to create chip {}: {:?}", name, e);
        }

        self.chips.insert(name.to_string(), id);
        id
    }

    // --- Given Steps ---

    /// Creates a Bluetooth chip in Device mode with a given name.
    /// The stream and sink are automatically created and managed by the World.
    pub async fn given_device(&mut self, name: &str) {
        if self.chips.contains_key(name) {
            panic!("Chip with name '{}' already exists", name);
        }

        let (stream, stream_tx) = crate::test_utils::mock_stream();
        let (sink, sink_rx) = crate::test_utils::mock_sink();

        self.create_chip(
            name,
            BluetoothMode::Device(DeviceParams {}),
            Some(stream),
            Some(sink),
            self.device_id,
        )
        .await;

        self.streams.insert(name.to_string(), stream_tx);
        self.sinks.insert(name.to_string(), sink_rx);
    }

    pub async fn given_slow_device(&mut self, name: &str, delay: tokio::time::Duration) {
        if self.chips.contains_key(name) {
            panic!("Chip with name '{}' already exists", name);
        }

        let (stream, stream_tx) = crate::test_utils::mock_stream();
        let (sink, mut sink_rx) = crate::test_utils::mock_sink();

        // Spawn a background task to slow down the reading from the packet sink.
        let (slow_tx, slow_rx) = tokio::sync::mpsc::channel(100);
        tokio::spawn(async move {
            while let Some(packet) = sink_rx.recv().await {
                tokio::time::sleep(delay).await; // 👈 Continuous slow consumer!
                let _ = slow_tx.send(packet).await;
            }
        });

        self.create_chip(
            name,
            BluetoothMode::Device(DeviceParams {}),
            Some(stream),
            Some(sink),
            self.device_id,
        )
        .await;

        self.streams.insert(name.to_string(), stream_tx);
        self.sinks.insert(name.to_string(), slow_rx);
    }

    /// Internal helper to create a beacon chip
    async fn create_beacon_chip(
        &mut self,
        name: &str,
        address: Option<String>,
        tx_power: Option<AdvertiseTxPower>,
    ) {
        let id_val = self.chip_id_counter + 1;
        // Use two octets for the ID_VAL to support more than 255 beacons.
        let address = address.unwrap_or_else(|| {
            format!("00:00:00:00:{:02x}:{:02x}", (id_val >> 8) & 0xFF, id_val & 0xFF)
        });

        let settings = Some(AdvertiseSettings {
            tx_power: tx_power.map(TxPower::TxPowerLevel),
            scannable: true,
            ..Default::default()
        });

        let mode = BluetoothMode::Beacon(Box::new(BeaconParams {
            ble_beacon: BleBeacon { address, settings, ..Default::default() },
        }));

        // Use self.device_id for consistency (unless specific overridden device is
        // needed) If separation is needed, tests should explicitly set up
        // different World or device. For now, defaulting to self.device_id (1)
        // as used in other beacons.
        self.create_chip(name, mode, None, None, self.device_id).await;
    }

    /// Creates a Bluetooth chip in Beacon mode.
    pub async fn given_beacon(&mut self, name: &str) {
        self.create_beacon_chip(name, None, None).await;
    }

    /// Creates a Bluetooth chip in Beacon mode with default name and address.
    pub async fn when_create_beacon_with_defaults(&mut self) -> ChipId {
        let id = self.next_chip_id();
        let mode = BluetoothMode::Beacon(Box::new(BeaconParams {
            ble_beacon: BleBeacon {
                address: "".to_string(),
                settings: Some(AdvertiseSettings { scannable: true, ..Default::default() }),
                ..Default::default()
            },
        }));

        let mut chip = Chip::new_test_ble("");
        chip.id = id.0;
        chip.device_id = self.device_id;
        chip.variant = Some(ChipVariant::Bluetooth(Box::new(netsim_model::Bluetooth {
            low_energy: Default::default(),
            classic: Default::default(),
            address: "".to_string(),
            bt_properties: Default::default(),
            mode,
            ..Default::default()
        })));

        let params = ChipCreate { packet_stream: None, packet_sink: None, chip };

        if let Err(e) = self.client.0.create_with_id(id, params).await {
            panic!("Failed to create default beacon: {:?}", e);
        }
        id
    }

    /// Creates a Bluetooth chip in Beacon mode with a specific address.
    pub async fn given_beacon_with_address(&mut self, name: &str, address: &str) {
        self.create_beacon_chip(name, Some(address.to_string()), None).await;
    }

    /// Creates a Bluetooth chip in Beacon mode with a specific interval.
    pub async fn given_beacon_with_interval(&mut self, name: &str, interval: Interval) {
        let id_val = self.chip_id_counter + 1;
        let address = format!("00:00:00:00:{:02x}:{:02x}", (id_val >> 8) & 0xFF, id_val & 0xFF);
        let settings = Some(AdvertiseSettings {
            interval: Some(interval),
            scannable: true,
            ..Default::default()
        });
        let mode = BluetoothMode::Beacon(Box::new(BeaconParams {
            ble_beacon: BleBeacon { address, settings, ..Default::default() },
        }));
        self.create_chip(name, mode, None, None, self.device_id).await;
    }

    /// Creates a Bluetooth chip in Beacon mode with specified Tx Power.
    pub async fn given_beacon_with_tx_power(&mut self, name: &str, tx_power: &str) {
        let power_level = match tx_power {
            "UltraLow" => AdvertiseTxPower::UltraLow,
            "Low" => AdvertiseTxPower::Low,
            "Medium" => AdvertiseTxPower::Medium,
            "High" => AdvertiseTxPower::High,
            _ => panic!("Unknown Tx Power level: {}", tx_power),
        };

        self.create_beacon_chip(name, None, Some(power_level)).await;
    }

    pub async fn given_scanner(&mut self, name: &str) {
        if self.chips.contains_key(name) {
            panic!("Chip with name '{}' already exists", name);
        }

        let (sink, sink_rx) = crate::test_utils::mock_sink();

        self.create_chip(
            name,
            BluetoothMode::Scanner(ScannerParams::default()),
            None,
            Some(sink),
            self.device_id,
        )
        .await;

        self.sinks.insert(name.to_string(), sink_rx);
    }

    pub async fn given_active_scanner(&mut self, name: &str) {
        if self.chips.contains_key(name) {
            panic!("Chip with name '{}' already exists", name);
        }

        let (sink, sink_rx) = crate::test_utils::mock_sink();

        self.create_chip(
            name,
            BluetoothMode::Scanner(ScannerParams { active: true }),
            None,
            Some(sink),
            self.device_id,
        )
        .await;

        self.sinks.insert(name.to_string(), sink_rx);
    }

    // --- When Steps ---

    pub async fn when_delete_chip(
        &self,
        name: &str,
    ) -> Result<(), actor_framework::FrameworkError<BluetoothError>> {
        let id = *self.chips.get(name).expect("Chip not found");
        self.client.0.delete(id).await
    }

    /// Drops the stream sender for the given chip, simulating a stream closure.
    pub fn when_stream_dropped(&mut self, name: &str) {
        self.streams.remove(name).expect("Stream not found for chip");
    }

    /// Drops the sink receiver for the given chip, simulating a sink closure.
    pub fn when_sink_dropped(&mut self, name: &str) {
        self.sinks.remove(name).expect("Sink not found for chip");
    }

    pub async fn when_update_chip_position(&self, name: &str, position: netsim_model::Position) {
        let id = *self.chips.get(name).expect("Chip not found");
        let update = ChipUpdate {
            pose: netsim_model::PoseUpdate { position: Some(position), orientation: None },
            ..Default::default()
        };
        self.client.0.update(id, update).await.expect("Failed to update chip");
    }

    // --- Then Steps ---

    pub async fn then_chip_position_is(&self, name: &str, expected: netsim_model::Position) {
        let id = *self.chips.get(name).expect("Chip not found");
        let chip =
            self.client.0.get(id).await.expect("Failed to get chip").expect("Chip should exist");
        assert_eq!(chip.pose.position, expected, "Chip position matches");
    }

    pub async fn then_chip_count_is(&self, expected: usize) {
        let count = self.client.0.list().await.expect("Failed to list chips").len();
        assert_eq!(count, expected, "Chip count should be {}", expected);
    }

    pub async fn then_chip_name_is(&self, id: ChipId, expected: &str) {
        let chip = self.client.0.get(id).await.expect("Failed to get chip").expect("Chip missing");
        assert_eq!(chip.name, expected, "Chip name match");
    }

    pub async fn when_packet_sent(&mut self, name: &str, packet: bytes::Bytes) {
        let tx = self.streams.get_mut(name).expect("Stream not found for chip");
        tx.send(packet).await.expect("Failed to send packet");
    }

    /// Encodes and sends an HCI command with the packet type prefix.
    pub async fn when_command_sent<
        T: netsim_packets::HciCommand + IntoBytes + Immutable + KnownLayout,
    >(
        &mut self,
        name: &str,
        payload: T,
    ) {
        let header = netsim_packets::HciCommandHeader {
            op_code: T::OP_CODE,
            parameter_total_length: payload.as_bytes().len() as u8,
        };
        let h4_packet = std::iter::once(PacketType::COMMAND.value() as u8)
            .chain(header.as_bytes().into_iter().copied())
            .chain(payload.as_bytes().into_iter().copied())
            .collect();
        self.when_packet_sent(name, h4_packet).await;
    }

    pub async fn then_packet_received(&mut self, name: &str, expected: &[u8]) {
        let actual = self.receive_packet(name).await;
        assert_eq!(actual, expected, "Packet received by {} does not match expected", name);
    }

    pub async fn receive_packet(&mut self, name: &str) -> Vec<u8> {
        let rx = self.sinks.get_mut(name).expect("Sink not found for chip");
        tokio::time::timeout(std::time::Duration::from_secs(2), rx.recv())
            .await
            .expect("Timed out waiting for packet")
            .expect("Packet stream closed unexpectedly")
    }

    pub async fn receive_scan_report(
        &mut self,
        name: &str,
    ) -> Vec<common::util::scanner_util::ScanResult> {
        let start = std::time::Instant::now();
        let timeout = std::time::Duration::from_secs(5);
        loop {
            if start.elapsed() > timeout {
                panic!("Timed out waiting for scan report from {}", name);
            }
            let packet = self.receive_packet(name).await;
            match parse_hci_scan_report(&packet) {
                Ok(reports) => return reports,
                Err(e) => {
                    warn!("Ignored packet from {}: {:?} (Error: {})", name, packet, e);
                }
            }
        }
    }

    pub async fn then_scanner_sees_any_adv(&mut self, name: &str) {
        let reports = self.receive_scan_report(name).await;
        for report in reports {
            info!("Received Scan Report: {:?}", report.mac);
        }
    }

    pub async fn then_scanner_sees_adv_type(
        &mut self,
        scanner_name: &str,
        expected_event_type: u8,
    ) {
        let start = std::time::Instant::now();
        let timeout = std::time::Duration::from_secs(5);
        // Outer loop skips over unrelated ambient traffic in multi-device tests.
        while start.elapsed() < timeout {
            let reports = self.receive_scan_report(scanner_name).await;
            for report in reports {
                if report.event_type == expected_event_type {
                    return;
                }
            }
        }
        panic!(
            "Scanner '{}' did not see adv with event type {} within timeout",
            scanner_name, expected_event_type
        );
    }

    /// Verifies that the scanner receives an advertisement from the specified
    /// beacon using BDD style.
    pub async fn then_scanner_sees_adv_from(&mut self, scanner_name: &str, beacon_name: &str) {
        let beacon_id =
            *self.chips.get(beacon_name).expect(&format!("Beacon '{}' not found", beacon_name));
        // Beacons created by World have address ...:ID.
        // We know from previous analysis that raw packet data is Big Endian [0, 0, 0,
        // 0, 0, ID]. So we just check the last byte.
        let expected_byte_5 = (beacon_id.0 & 0xFF) as u8;
        let expected_byte_4 = ((beacon_id.0 >> 8) & 0xFF) as u8;

        let start = std::time::Instant::now();
        let timeout = std::time::Duration::from_secs(5);

        while start.elapsed() < timeout {
            let reports = self.receive_scan_report(scanner_name).await;
            for report in reports {
                if report.mac[5] == expected_byte_5 && report.mac[4] == expected_byte_4 {
                    return;
                }
            }
        }
        panic!(
            "Scanner '{}' did not see beacon '{}' (ID {}) within timeout",
            scanner_name, beacon_name, beacon_id.0
        );
    }

    pub async fn then_scanner_sees_adv_with_rssi(&mut self, name: &str, _expected_power: &str) {
        let reports = self.receive_scan_report(name).await;
        for report in reports {
            let rssi = report.rssi;
            info!("Received RSSI: {}", rssi);
            assert!(rssi != 0, "RSSI should be non-zero");
        }
    }

    pub async fn then_chip_eventually_removed(&self, name: &str) {
        let id = *self.chips.get(name).expect("Chip name tracked in World");
        let duration = std::time::Duration::from_millis(100);
        for _ in 0..50 {
            // ~5 seconds max
            if self.client.0.get(id).await.expect("Failed to get").is_none() {
                return; // Success, chip is gone
            }
            tokio::time::sleep(duration).await;
        }
        panic!("Chip {name} was not removed after timeout");
    }

    /// Verifies that the scanner receives advertisements from the specified
    /// beacon at the expected interval by comparing relative packet counts over
    /// a fixed time window, avoiding fragile wall-clock interval
    /// assertions.
    pub async fn then_scanner_measures_interval_from(
        &mut self,
        scanner_name: &str,
        beacon_name: &str,
        expected_ms: u64,
    ) {
        let beacon_id = *self
            .chips
            .get(beacon_name)
            .unwrap_or_else(|| panic!("Beacon '{}' not found", beacon_name));
        let expected_byte_5 = (beacon_id.0 & 0xFF) as u8;
        let expected_byte_4 = ((beacon_id.0 >> 8) & 0xFF) as u8;

        let window_ms = (expected_ms * 6).clamp(3000, 6000);
        let mut timestamps: Vec<std::time::Instant> = Vec::new();
        let start = std::time::Instant::now();
        let timeout = std::time::Duration::from_millis(window_ms);

        while start.elapsed() < timeout {
            let reports = self.receive_scan_report(scanner_name).await;
            let now = std::time::Instant::now();
            for report in reports {
                if report.mac[5] == expected_byte_5 && report.mac[4] == expected_byte_4 {
                    if timestamps.is_empty()
                        || now.duration_since(*timestamps.last().unwrap())
                            > std::time::Duration::from_millis((expected_ms / 2).min(50))
                    {
                        timestamps.push(now);
                    }
                }
            }
        }

        let count = timestamps.len();
        let actual_ms = start.elapsed().as_millis() as f64;
        let expected_count = (actual_ms / (expected_ms.max(1) as f64)).round() as usize;
        let min_count = (expected_count / 2).max(2);
        let max_count = expected_count * 2;

        let avg_interval: u64 = if count >= 2 {
            (timestamps.last().unwrap().duration_since(*timestamps.first().unwrap())).as_millis()
                as u64
                / (count - 1) as u64
        } else {
            0
        };

        info!(
            "Beacon '{}': received {} advertisements over {}ms (expected ~{} at {}ms interval, avg gap: {}ms)",
            beacon_name, count, window_ms, expected_count, expected_ms, avg_interval
        );

        assert!(
            count >= min_count && count <= max_count,
            "Beacon '{}' advertisement count {} out of bounds [{}..{}] over {}ms (expected interval {}ms, measured avg interval {}ms). Timestamps: {:?}",
            beacon_name,
            count,
            min_count,
            max_count,
            window_ms,
            expected_ms,
            avg_interval,
            timestamps
        );
    }

    pub async fn then_classic_state_is(&self, id: ChipId, expected: Option<bool>) {
        use netsim_model::ChipClient;
        let chip = self.client.read(id).await.unwrap();
        let Some(netsim_model::ChipVariant::Bluetooth(bt)) = &chip.variant else {
            panic!("Expected Bluetooth chip variant");
        };
        assert_eq!(bt.classic.state, expected);
    }
}
