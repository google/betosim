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
#[serde(default)]
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
#[serde(default)]
pub struct Orientation {
    pub yaw: f32,
    pub pitch: f32,
    pub roll: f32,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
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
    #[serde(default)]
    pub struct DeviceUpdate {
        #[serde(default)]
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
    #[serde(default)]
    pub struct DeviceCreate {
        pub device_config: DeviceConfig,
        pub chip: DeviceChipCreate,
    }

    #[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
    #[serde(default)]
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DeviceConfig {
    pub name: String,
    pub visible: bool,
    pub pose: Pose,
    pub builtin: bool,
    pub device_info: Option<DeviceInfo>,
}

impl Default for DeviceConfig {
    fn default() -> Self {
        Self {
            name: String::new(),
            visible: true,
            pose: Pose::default(),
            builtin: false,
            device_info: None,
        }
    }
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
    use super::{
        api::{ChipCreateVariant, DeviceCreate, DeviceUpdate},
        *,
    };

    #[test]
    fn test_position_distance() {
        let p1 = Position { x: 0.0, y: 0.0, z: 0.0 };
        let p2 = Position { x: 0.0, y: 0.0, z: 0.03 };
        let p3 = Position { x: 0.05, y: 0.0, z: 0.0 };
        assert!((p1.distance(&p2) - 0.03).abs() < 1e-6);
        assert!((p1.distance(&p3) - 0.05).abs() < 1e-6);
    }

    #[test]
    fn test_device_create_partial_json_deserialization() {
        let empty: DeviceCreate = serde_json::from_str("{}").unwrap();
        assert!(empty.device_config.visible);
        assert_eq!(empty.device_config.name, "");
        assert!(!empty.device_config.builtin);
        assert!(matches!(empty.chip.chip, ChipCreateVariant::Beacon(_)));

        let partial: DeviceCreate =
            serde_json::from_str(r#"{"device_config":{"name":"my-beacon"}}"#).unwrap();
        assert!(partial.device_config.visible);
        assert_eq!(partial.device_config.name, "my-beacon");

        let hidden: DeviceCreate =
            serde_json::from_str(r#"{"device_config":{"visible":false}}"#).unwrap();
        assert!(!hidden.device_config.visible);
    }

    #[test]
    fn test_device_update_partial_json_deserialization() {
        let update: DeviceUpdate = serde_json::from_str(concat!(
            r#"{"chips":[{"name":"c1","variant":{"Bluetooth":"#,
            r#"{"ble_beacon":{"settings":{"scannable":false}}}}}]}"#,
        ))
        .unwrap();
        assert_eq!(update.id, 0);
        assert!(update.name.is_none());
        let chips = update.chips.as_ref().unwrap();
        assert_eq!(chips.len(), 1);
        assert_eq!(chips[0].name.as_deref(), Some("c1"));
        assert!(matches!(&chips[0].variant, Some(crate::chip::ChipVariantUpdate::Bluetooth(_))));
        if let Some(crate::chip::ChipVariantUpdate::Bluetooth(bt)) = &chips[0].variant {
            let scannable =
                bt.ble_beacon.as_ref().and_then(|b| b.settings.as_ref()).and_then(|s| s.scannable);
            assert_eq!(scannable, Some(false));
        }
    }

    #[test]
    fn test_ble_beacon_partial_json_and_create_defaults() {
        use crate::bluetooth::beacon;

        let mut beacon: beacon::BleBeacon = serde_json::from_str(
            r#"{"settings":{"scannable":true},"adv_data":{"include_device_name":true}}"#,
        )
        .unwrap();
        assert_eq!(beacon.address, "");
        assert!(beacon.settings.as_ref().unwrap().scannable);
        assert!(beacon.settings.as_ref().unwrap().interval.is_none());
        assert!(beacon.settings.as_ref().unwrap().tx_power.is_none());
        assert!(beacon.adv_data.as_ref().unwrap().include_device_name);

        beacon.apply_create_defaults();
        let settings = beacon.settings.as_ref().unwrap();
        assert_eq!(
            settings.interval,
            Some(beacon::Interval::AdvertiseMode(beacon::AdvertiseMode::LowPower))
        );
        assert_eq!(
            settings.tx_power,
            Some(beacon::TxPower::TxPowerLevel(beacon::AdvertiseTxPower::Low))
        );

        let mut default_beacon = beacon::BleBeacon::default();
        default_beacon.apply_create_defaults();
        let default_settings = default_beacon.settings.as_ref().unwrap();
        assert_eq!(
            default_settings.interval,
            Some(beacon::Interval::AdvertiseMode(beacon::AdvertiseMode::LowPower))
        );
        assert_eq!(
            default_settings.tx_power,
            Some(beacon::TxPower::TxPowerLevel(beacon::AdvertiseTxPower::Low))
        );
    }

    #[test]
    fn test_chip_variant_update_beacon_boolean_flags_preserve_and_toggle() {
        use crate::{
            bluetooth::{BeaconParams, Bluetooth, BluetoothMode, BluetoothUpdate, beacon},
            chip::{ChipVariant, ChipVariantUpdate},
        };

        let mut variant = ChipVariant::Bluetooth(Box::new(Bluetooth {
            address: "02:11:22:33:44:55".to_string(),
            mode: BluetoothMode::Beacon(Box::new(BeaconParams {
                ble_beacon: beacon::BleBeacon {
                    address: "02:11:22:33:44:55".to_string(),
                    settings: Some(beacon::AdvertiseSettings {
                        scannable: true,
                        timeout: 5000,
                        ..Default::default()
                    }),
                    adv_data: Some(beacon::AdvertiseData {
                        include_device_name: true,
                        include_tx_power_level: true,
                        manufacturer_data: vec![0x01],
                        services: vec![],
                    }),
                    scan_response: Some(beacon::AdvertiseData {
                        include_device_name: true,
                        include_tx_power_level: true,
                        manufacturer_data: vec![0x02],
                        services: vec![],
                    }),
                },
            })),
            ..Default::default()
        }));

        // 1. Partial sibling JSON patches omitting boolean fields preserve
        //    existing `true` flags.
        let partial_sibling_patch: ChipVariantUpdate = serde_json::from_value(serde_json::json!({
            "Bluetooth": {
                "ble_beacon": {
                    "settings": { "timeout": 500 },
                    "adv_data": { "include_device_name": true, "manufacturer_data": [0x99] },
                    "scan_response": {}
                }
            }
        }))
        .expect("deserialize partial sibling patch");
        partial_sibling_patch.apply(&mut variant);
        if let ChipVariant::Bluetooth(bt) = &variant
            && let BluetoothMode::Beacon(params) = &bt.mode
        {
            let settings = params.ble_beacon.settings.as_ref().unwrap();
            assert_eq!(settings.timeout, 500);
            assert!(settings.scannable, "omitted scannable must stay true");
            let adv = params.ble_beacon.adv_data.as_ref().unwrap();
            assert!(adv.include_device_name);
            assert!(
                adv.include_tx_power_level,
                "omitted adv_data.include_tx_power_level must stay true"
            );
            let scan = params.ble_beacon.scan_response.as_ref().unwrap();
            assert!(scan.include_device_name, "omitted scan_response booleans must stay true");
            assert!(scan.include_tx_power_level);
        } else {
            panic!("expected Bluetooth Beacon variant");
        }

        // 2. Single-field `false` JSON patches toggle only the specified
        //    boolean `true -> false`.
        let single_field_false_patch: ChipVariantUpdate =
            serde_json::from_value(serde_json::json!({
                "Bluetooth": {
                    "ble_beacon": {
                        "settings": { "scannable": false },
                        "adv_data": { "include_device_name": false },
                        "scan_response": { "include_tx_power_level": false }
                    }
                }
            }))
            .expect("deserialize single-field false patch");
        single_field_false_patch.apply(&mut variant);
        if let ChipVariant::Bluetooth(bt) = &variant
            && let BluetoothMode::Beacon(params) = &bt.mode
        {
            let settings = params.ble_beacon.settings.as_ref().unwrap();
            assert!(!settings.scannable, "single-field scannable: false must toggle to false");
            assert_eq!(settings.timeout, 500, "timeout must remain 500");
            let adv = params.ble_beacon.adv_data.as_ref().unwrap();
            assert!(
                !adv.include_device_name,
                "single-field include_device_name: false must toggle"
            );
            assert!(adv.include_tx_power_level, "sibling include_tx_power_level must remain true");
            let scan = params.ble_beacon.scan_response.as_ref().unwrap();
            assert!(scan.include_device_name, "sibling scan include_device_name must remain true");
            assert!(!scan.include_tx_power_level);
        } else {
            panic!("expected Bluetooth Beacon variant");
        }

        // 3. CLI-style `BleBeaconUpdate` with `None` boolean fields omits unset
        //    keys in JSON and preserves `scannable: true` after round-tripping.
        let reenable_scannable: ChipVariantUpdate = serde_json::from_value(serde_json::json!({
            "Bluetooth": { "ble_beacon": { "settings": { "scannable": true } } }
        }))
        .unwrap();
        reenable_scannable.apply(&mut variant);

        let cli_beacon = beacon::BleBeaconUpdate {
            settings: Some(beacon::AdvertiseSettingsUpdate {
                tx_power: Some(beacon::TxPower::TxPowerLevel(beacon::AdvertiseTxPower::High)),
                scannable: None,
                ..Default::default()
            }),
            adv_data: Some(beacon::AdvertiseDataUpdate::default()),
            scan_response: Some(beacon::AdvertiseDataUpdate::default()),
            ..Default::default()
        };
        let cli_patch = ChipVariantUpdate::Bluetooth(BluetoothUpdate {
            ble_beacon: Some(cli_beacon),
            ..Default::default()
        });
        let wire_json = serde_json::to_string(&cli_patch).expect("serialize CLI patch");
        assert!(
            !wire_json.contains("\"scannable\""),
            "CLI patch with omitted --scannable must omit scannable in JSON: {wire_json}"
        );
        assert!(
            !wire_json.contains("\"address\""),
            "CLI patch with empty address must omit address in JSON: {wire_json}"
        );
        let decoded_cli_patch: ChipVariantUpdate =
            serde_json::from_str(&wire_json).expect("deserialize CLI patch");
        decoded_cli_patch.apply(&mut variant);
        if let ChipVariant::Bluetooth(bt) = &variant
            && let BluetoothMode::Beacon(params) = &bt.mode
        {
            let settings = params.ble_beacon.settings.as_ref().unwrap();
            assert!(settings.scannable, "CLI partial patch must preserve scannable == true");
            assert_eq!(
                settings.tx_power,
                Some(beacon::TxPower::TxPowerLevel(beacon::AdvertiseTxPower::High))
            );
        } else {
            panic!("expected Bluetooth Beacon variant");
        }

        // 4. gRPC-style `BleBeaconUpdate` (`then_some(true)` mapping `false -> None`)
        //    preserves `scannable: true` when `timeout` is updated.
        let grpc_patch = ChipVariantUpdate::Bluetooth(BluetoothUpdate {
            ble_beacon: Some(beacon::BleBeaconUpdate {
                settings: Some(beacon::AdvertiseSettingsUpdate {
                    scannable: false.then_some(true),
                    timeout: Some(2000),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        });
        grpc_patch.apply(&mut variant);
        if let ChipVariant::Bluetooth(bt) = &variant
            && let BluetoothMode::Beacon(params) = &bt.mode
        {
            let settings = params.ble_beacon.settings.as_ref().unwrap();
            assert_eq!(settings.timeout, 2000);
            assert!(
                settings.scannable,
                "gRPC patch with false -> None must preserve scannable == true"
            );
        } else {
            panic!("expected Bluetooth Beacon variant");
        }

        // 5. Full `BleBeacon` serialized into JSON (as REST integration tests do)
        //    deserializes into `BleBeaconUpdate` and applies all fields, including
        //    `scannable: false`, `timeout: 0`, and empty vectors.
        let full_beacon = beacon::BleBeacon {
            address: "02:aa:bb:cc:dd:ee".to_string(),
            settings: Some(beacon::AdvertiseSettings {
                interval: Some(beacon::Interval::AdvertiseMode(beacon::AdvertiseMode::Balanced)),
                tx_power: Some(beacon::TxPower::TxPowerLevel(beacon::AdvertiseTxPower::Medium)),
                scannable: false,
                timeout: 0,
            }),
            adv_data: Some(beacon::AdvertiseData {
                include_device_name: false,
                include_tx_power_level: true,
                manufacturer_data: vec![],
                services: vec![],
            }),
            scan_response: Some(beacon::AdvertiseData::default()),
        };
        let full_patch_json = serde_json::json!({
            "Bluetooth": {
                "ble_beacon": full_beacon
            }
        });
        let decoded_full_patch: ChipVariantUpdate =
            serde_json::from_value(full_patch_json).expect("deserialize full BleBeacon as update");
        decoded_full_patch.apply(&mut variant);
        if let ChipVariant::Bluetooth(bt) = &variant
            && let BluetoothMode::Beacon(params) = &bt.mode
        {
            assert_eq!(bt.address, "02:aa:bb:cc:dd:ee");
            assert_eq!(params.ble_beacon.address, "02:aa:bb:cc:dd:ee");
            let settings = params.ble_beacon.settings.as_ref().unwrap();
            assert!(!settings.scannable);
            assert_eq!(settings.timeout, 0);
            let adv = params.ble_beacon.adv_data.as_ref().unwrap();
            assert!(!adv.include_device_name);
            assert!(adv.include_tx_power_level);
            assert!(adv.manufacturer_data.is_empty());
            let scan = params.ble_beacon.scan_response.as_ref().unwrap();
            assert!(!scan.include_device_name);
            assert!(!scan.include_tx_power_level);
            assert!(scan.manufacturer_data.is_empty());
        } else {
            panic!("expected Bluetooth Beacon variant");
        }
    }

    #[test]
    fn test_ble_beacon_update_timeout_only_leaves_none_fields_none() {
        use crate::bluetooth::beacon;

        let mut beacon = beacon::BleBeacon {
            address: "02:11:22:33:44:55".to_string(),
            settings: Some(beacon::AdvertiseSettings { timeout: 1000, ..Default::default() }),
            adv_data: None,
            scan_response: None,
        };

        let patch_none_sub_structs = beacon::BleBeaconUpdate {
            settings: Some(beacon::AdvertiseSettingsUpdate {
                timeout: Some(2500),
                ..Default::default()
            }),
            adv_data: None,
            scan_response: None,
            ..Default::default()
        };
        patch_none_sub_structs.apply(&mut beacon);
        assert_eq!(beacon.settings.as_ref().map(|s| s.timeout), Some(2500));
        assert!(beacon.adv_data.is_none());
        assert!(beacon.scan_response.is_none());

        let patch_empty_sub_structs = beacon::BleBeaconUpdate {
            settings: Some(beacon::AdvertiseSettingsUpdate {
                timeout: Some(3000),
                ..Default::default()
            }),
            adv_data: Some(beacon::AdvertiseDataUpdate::default()),
            scan_response: Some(beacon::AdvertiseDataUpdate::default()),
            ..Default::default()
        };
        patch_empty_sub_structs.apply(&mut beacon);
        assert_eq!(beacon.settings.as_ref().map(|s| s.timeout), Some(3000));
        assert!(beacon.adv_data.is_none());
        assert!(beacon.scan_response.is_none());
    }
}
