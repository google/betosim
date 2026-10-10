// Copyright 2023 The Android Open Source Project
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

use crate::devices::chip::ChipIdentifier;
use crate::ffi::ffi_bluetooth;
use crate::wireless::{WirelessChip, WirelessChipImpl};

use bytes::Bytes;
use cxx::{CxxString, CxxVector, let_cxx_string};
use log::{error, info};
use netsim_proto::config::Bluetooth as BluetoothConfig;
use netsim_proto::configuration::Controller as RootcanalController;
use netsim_proto::model::Chip as ProtoChip;
use netsim_proto::model::chip::Bluetooth as ProtoBluetooth;
use netsim_proto::model::chip::Radio as ProtoRadio;
use netsim_proto::stats::invalid_packet::Reason as InvalidPacketReason;
use netsim_proto::stats::{InvalidPacket, NetsimRadioStats as ProtoRadioStats, netsim_radio_stats};
use protobuf::{Enum, Message, MessageField};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

static WIRELESS_BT_MUTEX: Mutex<()> = Mutex::new(());

pub type RootcanalIdentifier = u32;

// BLUETOOTH_INVALID_PACKETS is a singleton that contains a hash map from
// RootcanalIdentifier to Vec<InvalidPacket>
// This singleton is only used when Rootcanal reports invalid
// packets by rootcanal_id and we add those to Bluetooth struct.
static BLUETOOTH_INVALID_PACKETS: OnceLock<
    Arc<Mutex<BTreeMap<RootcanalIdentifier, Vec<InvalidPacket>>>>,
> = OnceLock::new();

fn get_bluetooth_invalid_packets() -> Arc<Mutex<BTreeMap<RootcanalIdentifier, Vec<InvalidPacket>>>>
{
    BLUETOOTH_INVALID_PACKETS.get_or_init(|| Arc::new(Mutex::new(BTreeMap::new()))).clone()
}

/// Parameters for creating Bluetooth chips
/// allow(dead_code) due to not being used in unit tests
#[allow(dead_code)]
pub struct CreateParams {
    pub address: String,
    pub bt_properties: Option<MessageField<RootcanalController>>,
}

/// Bluetooth struct will keep track of rootcanal_id
pub struct Bluetooth {
    rootcanal_id: RootcanalIdentifier,
    low_energy_enabled: AtomicBool,
    classic_enabled: AtomicBool,
}

fn patch_state(
    enabled: &AtomicBool,
    state: Option<bool>,
    id: RootcanalIdentifier,
    is_low_energy: bool,
) {
    if let Some(state) = state {
        let _guard = WIRELESS_BT_MUTEX.lock().expect("Failed to lock WIRELESS_BT_MUTEX");
        let last_state: bool = enabled.swap(state, Ordering::SeqCst);
        match (last_state, state) {
            (false, true) => ffi_bluetooth::add_device_to_phy(id, is_low_energy),
            (true, false) => ffi_bluetooth::remove_device_from_phy(id, is_low_energy),
            _ => {}
        }
    }
}

impl Drop for Bluetooth {
    fn drop(&mut self) {
        // Lock to protect id_to_chip_info_ table in C++
        let _guard = WIRELESS_BT_MUTEX.lock().expect("Failed to acquire lock on WIRELESS_BT_MUTEX");
        ffi_bluetooth::bluetooth_remove(self.rootcanal_id);
        get_bluetooth_invalid_packets().lock().expect("invalid packets").remove(&self.rootcanal_id);
    }
}

impl WirelessChip for Bluetooth {
    fn handle_request(&self, packet: &Bytes) {
        // Lock to protect device_to_transport_ table in C++
        let _guard = WIRELESS_BT_MUTEX.lock().expect("Failed to acquire lock on WIRELESS_BT_MUTEX");
        ffi_bluetooth::handle_bt_request(self.rootcanal_id, packet[0], &packet[1..].to_vec())
    }

    fn reset(&self) {
        let _guard = WIRELESS_BT_MUTEX.lock().expect("Failed to acquire lock on WIRELESS_BT_MUTEX");
        ffi_bluetooth::bluetooth_reset(self.rootcanal_id);
        self.low_energy_enabled.store(true, Ordering::SeqCst);
        self.classic_enabled.store(true, Ordering::SeqCst);
    }

    fn get(&self) -> ProtoChip {
        let bluetooth_bytes = ffi_bluetooth::bluetooth_get_cxx(self.rootcanal_id);
        let bt_proto = ProtoBluetooth::parse_from_bytes(&bluetooth_bytes).unwrap();
        let mut chip_proto = ProtoChip::new();
        chip_proto.set_bt(ProtoBluetooth {
            low_energy: Some(ProtoRadio {
                state: self.low_energy_enabled.load(Ordering::SeqCst).into(),
                tx_count: bt_proto.low_energy.tx_count,
                rx_count: bt_proto.low_energy.rx_count,
                ..Default::default()
            })
            .into(),
            classic: Some(ProtoRadio {
                state: self.classic_enabled.load(Ordering::SeqCst).into(),
                tx_count: bt_proto.classic.tx_count,
                rx_count: bt_proto.classic.rx_count,
                ..Default::default()
            })
            .into(),
            address: bt_proto.address,
            bt_properties: bt_proto.bt_properties,
            ..Default::default()
        });
        chip_proto
    }

    fn patch(&self, chip: &ProtoChip) {
        if !chip.has_bt() {
            return;
        }
        let id = self.rootcanal_id;
        patch_state(&self.low_energy_enabled, chip.bt().low_energy.state, id, true);
        patch_state(&self.classic_enabled, chip.bt().classic.state, id, false);
    }

    fn get_stats(&self, duration_secs: u64) -> Vec<ProtoRadioStats> {
        // Construct NetsimRadioStats for BLE and Classic.
        let mut ble_stats_proto = ProtoRadioStats::new();
        ble_stats_proto.set_duration_secs(duration_secs);
        if let Some(v) = get_bluetooth_invalid_packets()
            .lock()
            .expect("Failed to acquire lock on BLUETOOTH_INVALID_PACKETS")
            .get(&self.rootcanal_id)
        {
            for invalid_packet in v {
                ble_stats_proto.invalid_packets.push(invalid_packet.clone());
            }
        }
        let mut classic_stats_proto = ble_stats_proto.clone();

        // Obtain the Chip Information with get()
        let chip_proto = self.get();
        if chip_proto.has_bt() {
            // Setting values for BLE Radio Stats
            ble_stats_proto.set_kind(netsim_radio_stats::Kind::BLUETOOTH_LOW_ENERGY);
            ble_stats_proto.set_tx_count(chip_proto.bt().low_energy.tx_count);
            ble_stats_proto.set_rx_count(chip_proto.bt().low_energy.rx_count);
            // Setting values for Classic Radio Stats
            classic_stats_proto.set_kind(netsim_radio_stats::Kind::BLUETOOTH_CLASSIC);
            classic_stats_proto.set_tx_count(chip_proto.bt().classic.tx_count);
            classic_stats_proto.set_rx_count(chip_proto.bt().classic.rx_count);
        }
        vec![ble_stats_proto, classic_stats_proto]
    }
}

/// Create a new Emulated Bluetooth Chip
/// allow(dead_code) due to not being used in unit tests
#[allow(dead_code)]
pub fn add_chip(create_params: &CreateParams, chip_id: ChipIdentifier) -> WirelessChipImpl {
    // Lock to protect id_to_chip_info_ table in C++
    let _guard = WIRELESS_BT_MUTEX.lock().expect("Failed to acquire lock on WIRELESS_BT_MUTEX");
    let_cxx_string!(cxx_address = create_params.address.clone());
    let proto_bytes = match &create_params.bt_properties {
        Some(properties) => properties.write_to_bytes().unwrap(),
        None => Vec::new(),
    };
    let rootcanal_id = ffi_bluetooth::bluetooth_add(chip_id.0, &cxx_address, &proto_bytes);
    info!("Bluetooth WirelessChip created with rootcanal_id: {rootcanal_id} chip_id: {chip_id}");
    let wireless_chip = Bluetooth {
        rootcanal_id,
        low_energy_enabled: AtomicBool::new(true),
        classic_enabled: AtomicBool::new(true),
    };
    get_bluetooth_invalid_packets()
        .lock()
        .expect("invalid packets")
        .insert(rootcanal_id, Vec::new());
    Box::new(wireless_chip)
}

/// Starts the Bluetooth service.
pub fn bluetooth_start(config: &MessageField<BluetoothConfig>, instance_num: u16) {
    let proto_bytes = config.as_ref().unwrap_or_default().write_to_bytes().unwrap();
    ffi_bluetooth::bluetooth_start(&proto_bytes, instance_num);
}

/// Stops the Bluetooth service.
pub fn bluetooth_stop() {
    ffi_bluetooth::bluetooth_stop();
}

/// Report Invalid Packet
pub fn report_invalid_packet(
    rootcanal_id: RootcanalIdentifier,
    reason: InvalidPacketReason,
    description: String,
    packet: Vec<u8>,
) {
    // TODO(b/330726276): spawn task on tokio once context is provided from rust_main
    let _ = std::thread::Builder::new().name("report_invalid_packet".to_string()).spawn(move || {
        match get_bluetooth_invalid_packets().lock().unwrap().get_mut(&rootcanal_id) {
            Some(v) => {
                // Remove the earliest reported packet if length greater than 5
                if v.len() >= 5 {
                    v.remove(0);
                }
                // append error packet
                let mut invalid_packet = InvalidPacket::new();
                invalid_packet.set_reason(reason);
                invalid_packet.set_description(description.clone());
                invalid_packet.set_packet(packet.clone());
                v.push(invalid_packet);
                // Log the report
                info!("Invalid Packet for rootcanal_id: {rootcanal_id}, reason: {reason:?}, description: {description:?}, packet: {packet:?}");
            }
            None => error!("Bluetooth WirelessChip not created for rootcanal_id: {rootcanal_id}"),
        }
    });
}

/// (Called by C++) Report Invalid Packet
pub fn report_invalid_packet_cxx(
    rootcanal_id: RootcanalIdentifier,
    reason: i32,
    description: &CxxString,
    packet: &CxxVector<u8>,
) {
    report_invalid_packet(
        rootcanal_id,
        InvalidPacketReason::from_i32(reason).unwrap_or(InvalidPacketReason::UNKNOWN),
        description.to_string(),
        packet.as_slice().to_vec(),
    );
}
