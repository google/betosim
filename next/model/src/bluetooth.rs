// Copyright 2026 The Android Open Source Project
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

// Rust definitions for Bluetooth related structures
use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::chip::{Radio, RadioUpdate};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Bluetooth {
    pub low_energy: Radio,
    pub classic: Radio,
    pub address: String,
    pub bt_properties: Controller,
    pub mode: BluetoothMode,
    pub preset: Option<String>,
}

impl Default for Bluetooth {
    fn default() -> Self {
        Bluetooth {
            low_energy: Radio::default(),
            classic: Radio::default(),
            address: "00:00:00:00:00:00".to_string(),
            bt_properties: Controller::default(),
            mode: BluetoothMode::Device(DeviceParams::default()),
            preset: None,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct BluetoothUpdate {
    pub classic: RadioUpdate,
    pub low_energy: RadioUpdate,
    pub preset: Option<String>,
}

impl BluetoothUpdate {
    pub fn apply(&self, bluetooth: &mut Bluetooth) {
        self.classic.apply(&mut bluetooth.classic);
        self.low_energy.apply(&mut bluetooth.low_energy);
        if let Some(preset) = &self.preset {
            bluetooth.preset = Some(preset.clone());
        }
    }
}

/// Parameters for creating a Bluetooth chip.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BluetoothCreate {
    /// The Bluetooth address of the device.
    pub address: String,
    /// Rootcanal controller properties.
    pub bt_properties: Controller,
    /// The operational mode of the Bluetooth chip.
    pub mode: BluetoothMode,
}

impl From<BluetoothCreate> for Bluetooth {
    fn from(create: BluetoothCreate) -> Self {
        Bluetooth {
            low_energy: Radio::default(),
            classic: Radio::default(),
            address: create.address,
            bt_properties: create.bt_properties,
            mode: create.mode,
            preset: None,
        }
    }
}

/// An enum to differentiate between the kinds of Bluetooth chips.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum BluetoothMode {
    /// A full, virtual Bluetooth controller that can be paired with.
    Device(DeviceParams),
    /// A simple, non-interactive BLE beacon that broadcasts advertisements.
    Beacon(Box<BeaconParams>),
    /// A passive Bluetooth scanner to capture nearby traffic.
    Scanner(ScannerParams),
    /// A raw link-layer sniffer for baseband capture.
    Sniffer(SnifferParams),
}

/// Parameters for creating a virtual Bluetooth device.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DeviceParams {}

/// Parameters for creating a BLE beacon.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct BeaconParams {
    /// The BLE beacon's configuration.
    pub ble_beacon: beacon::BleBeacon,
}

/// Parameters for a Bluetooth scanner.
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScannerParams {
    pub active: bool,
}

/// Parameters for a Bluetooth sniffer.
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
pub struct SnifferParams {
    // Future sniffer-specific properties can be added here.
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Controller {
    pub vendor: String,
    pub product: String,
    pub version: String,
    pub address: String,
    pub properties: HashMap<String, String>,
}

impl Default for Controller {
    fn default() -> Self {
        Controller {
            vendor: "Netsim".to_string(),
            product: "Netsim".to_string(),
            version: "1.0".to_string(),
            address: "00:00:00:00:00:00".to_string(),
            properties: HashMap::new(),
        }
    }
}

pub(crate) mod beacon {
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
    pub struct BleBeacon {
        pub address: String,
        // Settings on how beacon functions
        pub settings: Option<AdvertiseSettings>,
        // Advertising Data
        pub adv_data: Option<AdvertiseData>,
        // Scan Response Data
        pub scan_response: Option<AdvertiseData>,
    }

    #[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
    pub struct AdvertiseSettings {
        pub interval: Option<Interval>,
        pub tx_power: Option<TxPower>,
        pub scannable: bool,
        pub timeout: u64,
    }

    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub enum Interval {
        AdvertiseMode(AdvertiseMode),
        Milliseconds(u64),
    }

    impl Default for Interval {
        fn default() -> Self {
            Interval::AdvertiseMode(AdvertiseMode::LowPower)
        }
    }

    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub enum TxPower {
        TxPowerLevel(AdvertiseTxPower),
        Dbm(i32),
    }

    impl Default for TxPower {
        fn default() -> Self {
            TxPower::TxPowerLevel(AdvertiseTxPower::UltraLow)
        }
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
    pub enum AdvertiseMode {
        #[default]
        LowPower = 0,
        Balanced = 1,
        LowLatency = 2,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
    pub enum AdvertiseTxPower {
        #[default]
        UltraLow = 0,
        Low = 1,
        Medium = 2,
        High = 3,
    }

    #[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
    pub struct AdvertiseData {
        pub include_device_name: bool,
        pub include_tx_power_level: bool,
        pub manufacturer_data: Vec<u8>,
        pub services: Vec<Service>,
    }

    #[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
    pub struct Service {
        pub uuid: String,
        pub data: Vec<u8>,
    }
}

/// The type of HCI packet exchanged between the Bluetooth chip and higher
/// layers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PacketType {
    /// Unspecified packet type containing raw bytes.
    #[default]
    HciPacketUnspecified = 0,
    /// HCI Command packet.
    Command = 1,
    /// ACL (asynchronous connectionless) packet.
    Acl = 2,
    /// SCO (synchronous connection-oriented) packet.
    Sco = 3,
    /// HCI Event packet.
    Event = 4,
    /// ISO (isochronous channel) packet.
    Iso = 5,
}

impl PacketType {
    /// Backward-compatible alias for `HciPacketUnspecified`.
    pub const HCI_PACKET_UNSPECIFIED: Self = Self::HciPacketUnspecified;
    /// Backward-compatible alias for `Command`.
    pub const COMMAND: Self = Self::Command;
    /// Backward-compatible alias for `Acl`.
    pub const ACL: Self = Self::Acl;
    /// Backward-compatible alias for `Sco`.
    pub const SCO: Self = Self::Sco;
    /// Backward-compatible alias for `Event`.
    pub const EVENT: Self = Self::Event;
    /// Backward-compatible alias for `Iso`.
    pub const ISO: Self = Self::Iso;

    /// Returns the integer value of the packet type.
    pub fn value(&self) -> i32 {
        *self as i32
    }

    /// Converts an integer to `PacketType`, returning `None` if invalid.
    pub fn from_i32(v: i32) -> Option<Self> {
        match v {
            0 => Some(Self::HciPacketUnspecified),
            1 => Some(Self::Command),
            2 => Some(Self::Acl),
            3 => Some(Self::Sco),
            4 => Some(Self::Event),
            5 => Some(Self::Iso),
            _ => None,
        }
    }

    /// Converts a byte to `PacketType`, returning `None` if invalid.
    pub fn from_u8(v: u8) -> Option<Self> {
        Self::from_i32(v as i32)
    }
}

impl From<PacketType> for u8 {
    fn from(packet_type: PacketType) -> Self {
        packet_type as u8
    }
}

impl From<PacketType> for i32 {
    fn from(packet_type: PacketType) -> Self {
        packet_type as i32
    }
}

impl From<i32> for PacketType {
    fn from(v: i32) -> Self {
        Self::from_i32(v).unwrap_or(Self::HciPacketUnspecified)
    }
}

impl From<u8> for PacketType {
    fn from(v: u8) -> Self {
        Self::from_u8(v).unwrap_or(Self::HciPacketUnspecified)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_packet_type_conversions_and_values() {
        assert_eq!(PacketType::default(), PacketType::HciPacketUnspecified);
        assert_eq!(PacketType::HCI_PACKET_UNSPECIFIED.value(), 0);
        assert_eq!(PacketType::COMMAND.value(), 1);
        assert_eq!(PacketType::ACL.value(), 2);
        assert_eq!(PacketType::SCO.value(), 3);
        assert_eq!(PacketType::EVENT.value(), 4);
        assert_eq!(PacketType::ISO.value(), 5);

        assert_eq!(u8::from(PacketType::Command), 1u8);
        assert_eq!(i32::from(PacketType::Acl), 2i32);

        assert_eq!(PacketType::from(1u8), PacketType::Command);
        assert_eq!(PacketType::from(2i32), PacketType::Acl);
        assert_eq!(PacketType::from(99i32), PacketType::HciPacketUnspecified);

        assert_eq!(PacketType::from_i32(3), Some(PacketType::Sco));
        assert_eq!(PacketType::from_i32(100), None);
        assert_eq!(PacketType::from_u8(4), Some(PacketType::Event));
        assert_eq!(PacketType::from_u8(255), None);
    }
}
