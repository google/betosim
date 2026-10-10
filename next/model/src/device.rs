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

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::chip::{PacketSink, PacketStream};

// DEVICE SERVICE
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct Position {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl Position {
    /// Calculate the Euclidean distance between two positions in meters.
    pub fn distance(&self, other: &Position) -> f32 {
        let dx = self.x - other.x;
        let dy = self.y - other.y;
        let dz = self.z - other.z;
        (dx * dx + dy * dy + dz * dz).sqrt()
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct Orientation {
    pub yaw: f32,
    pub pitch: f32,
    pub roll: f32,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct Pose {
    pub position: Position,
    pub orientation: Orientation,
}

/// A unique identifier for a simulated device, represented as a u32.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub struct DeviceId(pub u32);

impl From<DeviceId> for u32 {
    fn from(id: DeviceId) -> Self {
        id.0
    }
}

impl From<u32> for DeviceId {
    fn from(id: u32) -> Self {
        DeviceId(id)
    }
}

impl fmt::Display for DeviceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

pub(crate) mod api {
    use serde::{Deserialize, Serialize};

    use crate::{
        chip::{BleBeacon, BluetoothCreate, CellCreate, Uwb, UwbCreate, UwbMode, WifiCreate},
        device::{DeviceConfig, Orientation, Position},
        nfc::{Nfc, NfcCreate},
    };

    #[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
    pub struct PoseUpdate {
        pub position: Option<Position>,
        pub orientation: Option<Orientation>,
    }

    impl PoseUpdate {
        pub fn apply(&self, pose: &mut super::Pose) {
            if let Some(pos) = &self.position {
                pose.position = *pos;
            }
            if let Some(orient) = &self.orientation {
                pose.orientation = *orient;
            }
        }
    }

    #[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
    pub struct DeviceUpdate {
        pub id: u32,
        pub name: Option<String>,
        pub visible: Option<bool>,
        //TODO: pub chip_id: Option<ChipId>,
        #[serde(default)]
        pub pose: PoseUpdate,
        //TODO: pub links: Option<Vec<Link>,
        pub chips: Option<Vec<crate::chip::ChipUpdate>>,
    }

    impl DeviceUpdate {
        pub fn apply(&self, device: &mut super::Device) {
            if let Some(name) = &self.name {
                device.name = name.clone();
            }
            if let Some(visible) = self.visible {
                device.visible = visible;
            }
            self.pose.apply(&mut device.pose);
        }
    }

    #[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
    pub struct DeviceCreate {
        pub device_config: DeviceConfig,
        pub chip: DeviceChipCreate,
    }

    #[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
    pub struct DeviceChipCreate {
        pub name: String,
        pub manufacturer: String,
        pub product_name: String,
        pub chip: ChipCreateVariant,
    }

    impl DeviceChipCreate {
        pub fn new(
            name: impl Into<String>,
            manufacturer: impl Into<String>,
            product_name: impl Into<String>,
            chip: ChipCreateVariant,
        ) -> DeviceChipCreate {
            DeviceChipCreate {
                name: name.into(),
                manufacturer: manufacturer.into(),
                product_name: product_name.into(),
                chip,
            }
        }
    }

    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub enum ChipCreateVariant {
        Beacon(BleBeacon),
        Bluetooth(BluetoothCreate),
        Wifi(WifiCreate),
        Uwb(UwbCreate),
        Cell(CellCreate),
        CellularData(crate::cellular_data::CellularDataCreate),
        Ethernet(crate::ethernet::EthernetCreate),
        Nfc(NfcCreate),
    }

    impl Default for ChipCreateVariant {
        fn default() -> Self {
            ChipCreateVariant::Beacon(BleBeacon::default())
        }
    }

    impl ChipCreateVariant {
        pub fn kind(&self) -> crate::chip::ChipKind {
            match self {
                ChipCreateVariant::Beacon(_) | ChipCreateVariant::Bluetooth(_) => {
                    crate::chip::ChipKind::BLUETOOTH
                }
                ChipCreateVariant::Wifi(_) => crate::chip::ChipKind::WIFI,
                ChipCreateVariant::Uwb(_) => crate::chip::ChipKind::UWB,
                ChipCreateVariant::Cell(_) => crate::chip::ChipKind::CELLULAR,
                ChipCreateVariant::CellularData(_) => crate::chip::ChipKind::CELLULAR_DATA,
                ChipCreateVariant::Ethernet(_) => crate::chip::ChipKind::ETHERNET,
                ChipCreateVariant::Nfc(_) => crate::chip::ChipKind::NFC,
            }
        }
    }
    impl From<ChipCreateVariant> for crate::chip::ChipVariant {
        fn from(chip: ChipCreateVariant) -> Self {
            match chip {
                ChipCreateVariant::Beacon(beacon) => {
                    crate::chip::ChipVariant::Bluetooth(Box::new(crate::bluetooth::Bluetooth {
                        address: beacon.address.clone(),
                        mode: crate::chip::BluetoothMode::Beacon(Box::new(
                            crate::chip::BeaconParams { ble_beacon: beacon },
                        )),
                        ..Default::default()
                    }))
                }
                ChipCreateVariant::Bluetooth(bt) => crate::chip::ChipVariant::Bluetooth(Box::new(
                    crate::bluetooth::Bluetooth::from(bt),
                )),
                ChipCreateVariant::Wifi(wifi) => {
                    crate::chip::ChipVariant::Wifi(crate::wifi::Wifi::from(wifi))
                }
                ChipCreateVariant::Uwb(uwb) => crate::chip::ChipVariant::Uwb(Uwb::from(uwb)),
                ChipCreateVariant::Cell(cell) => {
                    crate::chip::ChipVariant::Cell(crate::cell::Cell {
                        sim_type: cell.sim_type,
                        sim_profile: cell.sim_profile,
                        network_configs: cell.network_configs,
                        ..Default::default()
                    })
                }
                ChipCreateVariant::CellularData(cell_data) => {
                    crate::chip::ChipVariant::CellularData(
                        crate::cellular_data::CellularData::from(cell_data),
                    )
                }
                ChipCreateVariant::Ethernet(eth) => {
                    crate::chip::ChipVariant::Ethernet(crate::ethernet::Ethernet::from(eth))
                }
                ChipCreateVariant::Nfc(nfc) => crate::chip::ChipVariant::Nfc(Nfc::from(nfc)),
            }
        }
    }
    impl From<DeviceChipCreate> for crate::chip::Chip {
        fn from(create: DeviceChipCreate) -> Self {
            crate::chip::Chip {
                kind: create.chip.kind(),
                name: create.name,
                manufacturer: create.manufacturer,
                product_name: create.product_name,
                variant: Some(crate::chip::ChipVariant::from(create.chip)),
                ..Default::default()
            }
        }
    }

    impl From<crate::chip::Chip> for DeviceChipCreate {
        fn from(chip: crate::chip::Chip) -> Self {
            DeviceChipCreate {
                name: chip.name,
                manufacturer: chip.manufacturer,
                product_name: chip.product_name,
                chip: match chip.variant {
                    Some(crate::chip::ChipVariant::Bluetooth(bt)) => match bt.mode {
                        crate::chip::BluetoothMode::Beacon(beacon_params) => {
                            ChipCreateVariant::Beacon(beacon_params.ble_beacon)
                        }
                        _ => ChipCreateVariant::Bluetooth(crate::chip::BluetoothCreate {
                            address: bt.address.clone(),
                            bt_properties: bt.bt_properties.clone(),
                            mode: bt.mode.clone(),
                        }),
                    },
                    Some(crate::chip::ChipVariant::Wifi(_wifi)) => {
                        ChipCreateVariant::Wifi(crate::chip::WifiCreate::default())
                    }
                    Some(crate::chip::ChipVariant::Uwb(uwb)) => {
                        ChipCreateVariant::Uwb(match uwb.mode {
                            UwbMode::Device => UwbCreate::Device,
                            UwbMode::Anchor => UwbCreate::Anchor {
                                address: (!uwb.address.is_empty()).then_some(uwb.address),
                            },
                        })
                    }
                    Some(crate::chip::ChipVariant::Cell(cell)) => {
                        ChipCreateVariant::Cell(crate::chip::CellCreate {
                            sim_type: cell.sim_type,
                            sim_profile: cell.sim_profile,
                            network_configs: cell.network_configs,
                        })
                    }
                    Some(crate::chip::ChipVariant::CellularData(_cell_data)) => {
                        ChipCreateVariant::CellularData(
                            crate::cellular_data::CellularDataCreate::default(),
                        )
                    }
                    Some(crate::chip::ChipVariant::Ethernet(_eth)) => {
                        ChipCreateVariant::Ethernet(crate::ethernet::EthernetCreate::default())
                    }
                    Some(crate::chip::ChipVariant::Nfc(_nfc)) => {
                        ChipCreateVariant::Nfc(NfcCreate::default())
                    }
                    None => ChipCreateVariant::Beacon(crate::chip::BleBeacon::default()), /* Fallback */
                },
            }
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Device {
    pub id: u32,
    pub name: String,
    pub visible: bool,
    pub pose: Pose,
    pub builtin: bool,
    pub chips: Vec<crate::chip::Chip>,
    pub device_info: Option<DeviceInfo>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DeviceInfo {
    pub name: String,
    pub kind: String,
    pub version: String,
    pub sdk_version: String,
    pub build_id: String,
    pub variant: String,
    pub arch: String,
    pub avd_path: String,
}

impl From<netsim_types::DeviceInfo> for DeviceInfo {
    fn from(info: netsim_types::DeviceInfo) -> Self {
        Self {
            name: info.name,
            kind: info.kind,
            version: info.version,
            sdk_version: info.sdk_version,
            build_id: info.build_id,
            variant: info.variant,
            arch: info.arch,
            avd_path: info.avd_path,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DeviceConfig {
    pub name: String,
    pub visible: bool,
    pub pose: Pose,
    pub builtin: bool,
    pub device_info: Option<DeviceInfo>,
}

impl DeviceConfig {
    pub fn new(name: impl Into<String>, visible: bool, pose: Pose, builtin: bool) -> DeviceConfig {
        DeviceConfig { name: name.into(), visible, pose, builtin, device_info: None }
    }
}

pub struct DeviceAddChip {
    pub device_guid: String,
    pub packet_stream: Option<PacketStream>,
    pub packet_sink: Option<PacketSink>,
    pub device_config: DeviceConfig,
    pub chip: crate::chip::Chip,
}
impl fmt::Debug for DeviceAddChip {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DeviceAddChip")
            .field("device_guid", &self.device_guid)
            .field("device_config", &self.device_config)
            .field("chip", &self.chip)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_position_distance() {
        let p1 = Position { x: 0.0, y: 0.0, z: 0.0 };
        let p2 = Position { x: 0.0, y: 0.0, z: 0.03 };
        let p3 = Position { x: 0.05, y: 0.0, z: 0.0 };
        assert!((p1.distance(&p2) - 0.03).abs() < 1e-6);
        assert!((p1.distance(&p3) - 0.05).abs() < 1e-6);
    }
}
