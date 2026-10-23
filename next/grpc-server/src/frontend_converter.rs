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

use device_api::{
    ChipCreateVariant, Device as ApiDevice, DeviceChipCreate, Orientation as ApiOrientation,
    PoseUpdate, Position as ApiPosition,
};
use link_api::Link as ApiLink;
use netsim_model::{
    AdvertiseData, AdvertiseMode, AdvertiseSettings, AdvertiseTxPower, BleBeacon, BluetoothCreate,
    BluetoothMode, BluetoothUpdate, BtControllerProperties, ChipId, ChipKind as ApiChipKind,
    ChipUpdate, ChipVariantUpdate, Interval, RadioUpdate, TxPower, UwbCreate,
};
use netsim_proto::{
    common::ChipKind as ProtoChipKind,
    model::{
        Chip as ProtoChip, ChipCreate, Device as ProtoDevice, Link as ProtoLink,
        Orientation as ProtoOrientation, PhyKind as ProtoPhyKind, Position as ProtoPosition,
    },
    protobuf::{EnumOrUnknown, MessageField},
};

pub(crate) fn to_proto_position(p: ApiPosition) -> ProtoPosition {
    let mut pos = ProtoPosition::new();
    pos.x = p.x;
    pos.y = p.y;
    pos.z = p.z;
    pos
}

pub(crate) fn to_proto_orientation(o: ApiOrientation) -> ProtoOrientation {
    let mut orient = ProtoOrientation::new();
    orient.yaw = o.yaw;
    orient.pitch = o.pitch;
    orient.roll = o.roll;
    orient
}

pub(crate) fn to_proto_chip_kind(k: ApiChipKind) -> ProtoChipKind {
    match k {
        ApiChipKind::BLUETOOTH => ProtoChipKind::BLUETOOTH,
        ApiChipKind::WIFI => ProtoChipKind::WIFI,
        ApiChipKind::UWB => ProtoChipKind::UWB,
        // Map unknown/new types to UNSPECIFIED for now
        ApiChipKind::NFC => ProtoChipKind::NFC,
        ApiChipKind::CELLULAR => ProtoChipKind::CELLULAR,
        ApiChipKind::CELLULAR_DATA => ProtoChipKind::CELLULAR_DATA,
        ApiChipKind::ETHERNET => ProtoChipKind::ETHERNET,
        ApiChipKind::UNSPECIFIED => ProtoChipKind::UNSPECIFIED,
    }
}

pub(crate) fn to_proto_chip(c: netsim_model::Chip) -> ProtoChip {
    let mut chip = ProtoChip::new();
    chip.id = c.id;
    chip.kind = EnumOrUnknown::new(to_proto_chip_kind(c.kind));
    chip.name = c.name;
    chip.manufacturer = c.manufacturer;
    chip.product_name = c.product_name;
    // Note: netsim_model Chip has position, but netsim_proto Chip has offset
    // (Position)
    chip.offset = MessageField::some(to_proto_position(c.pose.position));

    if let Some(variant) = c.variant {
        match variant {
            netsim_model::ChipVariant::Bluetooth(bt_model) => {
                let mut bt = netsim_proto::model::chip::Bluetooth::new();
                bt.low_energy = MessageField::some(to_proto_radio(&bt_model.low_energy));
                bt.classic = MessageField::some(to_proto_radio(&bt_model.classic));
                bt.bt_properties =
                    MessageField::from_option(bt_model.bt_properties.config.to_proto());
                bt.address = bt_model.address;
                if let BluetoothMode::Beacon(params) = bt_model.mode {
                    chip.chip = Some(netsim_proto::model::chip::Chip::BleBeacon(
                        to_proto_ble_beacon(bt, params.ble_beacon),
                    ));
                } else {
                    chip.chip = Some(netsim_proto::model::chip::Chip::Bt(bt));
                }
            }
            netsim_model::ChipVariant::Wifi(wifi) => {
                chip.chip =
                    Some(netsim_proto::model::chip::Chip::Wifi(to_proto_radio(&wifi.radio)));
            }
            netsim_model::ChipVariant::Uwb(uwb) => {
                chip.chip = Some(netsim_proto::model::chip::Chip::Uwb(to_proto_radio(&uwb.radio)));
            }
            netsim_model::ChipVariant::Cell(cell) => {
                chip.chip =
                    Some(netsim_proto::model::chip::Chip::Cellular(to_proto_radio(&cell.radio)));
            }
            netsim_model::ChipVariant::CellularData(cell_data) => {
                chip.chip = Some(netsim_proto::model::chip::Chip::CellularData(to_proto_radio(
                    &cell_data.radio,
                )));
            }
            netsim_model::ChipVariant::Ethernet(eth) => {
                chip.chip =
                    Some(netsim_proto::model::chip::Chip::Ethernet(to_proto_radio(&eth.radio)));
            }
            netsim_model::ChipVariant::Nfc(nfc) => {
                chip.chip = Some(netsim_proto::model::chip::Chip::Nfc(to_proto_radio(&nfc.radio)));
            }
        }
    }

    chip
}

fn to_proto_radio(r: &netsim_model::Radio) -> netsim_proto::model::chip::Radio {
    let mut radio = netsim_proto::model::chip::Radio::new();
    radio.state = Some(r.state.unwrap_or(true));
    radio.range = r.range;
    radio.tx_count = std::cmp::min(r.tx_count, i32::MAX as u64) as i32;
    radio.rx_count = std::cmp::min(r.rx_count, i32::MAX as u64) as i32;
    radio
}

fn to_proto_ble_beacon(
    bt: netsim_proto::model::chip::Bluetooth,
    b: BleBeacon,
) -> netsim_proto::model::chip::BleBeacon {
    let mut proto = netsim_proto::model::chip::BleBeacon::new();
    proto.bt = MessageField::some(bt);
    proto.address = b.address;
    proto.settings =
        MessageField::some(to_proto_advertise_settings(b.settings.unwrap_or_default()));
    proto.adv_data = MessageField::some(to_proto_advertise_data(b.adv_data.unwrap_or_default()));
    proto.scan_response = MessageField::from_option(b.scan_response.map(to_proto_advertise_data));
    proto
}

fn to_proto_advertise_settings(
    s: AdvertiseSettings,
) -> netsim_proto::model::chip::ble_beacon::AdvertiseSettings {
    use netsim_proto::model::chip::ble_beacon::advertise_settings::{
        AdvertiseMode as ProtoMode, AdvertiseTxPower as ProtoLevel, Interval as ProtoInterval,
        Tx_power as ProtoTxPower,
    };
    let mut proto = netsim_proto::model::chip::ble_beacon::AdvertiseSettings::new();
    proto.interval = s.interval.map(|i| match i {
        Interval::AdvertiseMode(AdvertiseMode::LowPower) => {
            ProtoInterval::AdvertiseMode(EnumOrUnknown::new(ProtoMode::LOW_POWER))
        }
        Interval::AdvertiseMode(AdvertiseMode::Balanced) => {
            ProtoInterval::AdvertiseMode(EnumOrUnknown::new(ProtoMode::BALANCED))
        }
        Interval::AdvertiseMode(AdvertiseMode::LowLatency) => {
            ProtoInterval::AdvertiseMode(EnumOrUnknown::new(ProtoMode::LOW_LATENCY))
        }
        Interval::Milliseconds(ms) => ProtoInterval::Milliseconds(ms),
    });
    proto.tx_power = s.tx_power.map(|p| match p {
        TxPower::TxPowerLevel(AdvertiseTxPower::UltraLow) => {
            ProtoTxPower::TxPowerLevel(EnumOrUnknown::new(ProtoLevel::ULTRA_LOW))
        }
        TxPower::TxPowerLevel(AdvertiseTxPower::Low) => {
            ProtoTxPower::TxPowerLevel(EnumOrUnknown::new(ProtoLevel::LOW))
        }
        TxPower::TxPowerLevel(AdvertiseTxPower::Medium) => {
            ProtoTxPower::TxPowerLevel(EnumOrUnknown::new(ProtoLevel::MEDIUM))
        }
        TxPower::TxPowerLevel(AdvertiseTxPower::High) => {
            ProtoTxPower::TxPowerLevel(EnumOrUnknown::new(ProtoLevel::HIGH))
        }
        TxPower::Dbm(dbm) => ProtoTxPower::Dbm(dbm),
    });
    proto.scannable = s.scannable;
    proto.timeout = s.timeout;
    proto
}

fn to_proto_advertise_data(
    d: AdvertiseData,
) -> netsim_proto::model::chip::ble_beacon::AdvertiseData {
    let mut proto = netsim_proto::model::chip::ble_beacon::AdvertiseData::new();
    proto.include_device_name = d.include_device_name;
    proto.include_tx_power_level = d.include_tx_power_level;
    proto.manufacturer_data = d.manufacturer_data;
    proto.services = d
        .services
        .into_iter()
        .map(|s| {
            let mut srv = netsim_proto::model::chip::ble_beacon::advertise_data::Service::new();
            srv.uuid = s.uuid;
            srv.data = s.data;
            srv
        })
        .collect();
    proto
}

pub(crate) fn to_proto_device(d: ApiDevice) -> ProtoDevice {
    let mut device = ProtoDevice::new();
    device.id = d.id;
    device.name = d.name;
    device.visible = Some(d.visible);
    device.position = MessageField::some(to_proto_position(d.pose.position));
    device.orientation = MessageField::some(to_proto_orientation(d.pose.orientation));
    for chip in d.chips {
        device.chips.push(to_proto_chip(chip));
    }
    device
}

pub(crate) fn from_proto_chip_create(c: ChipCreate) -> Option<DeviceChipCreate> {
    // Supports BLE beacons, Bluetooth devices, and UWB anchors.
    if c.has_ble_beacon() {
        let beacon_create = c.ble_beacon();
        let settings =
            beacon_create.settings.as_ref().map(|s| from_proto_advertise_settings(s, true));
        let adv_data = beacon_create.adv_data.as_ref().map(from_proto_advertise_data);
        let scan_response = beacon_create.scan_response.as_ref().map(from_proto_advertise_data);

        let beacon =
            BleBeacon { address: beacon_create.address.clone(), settings, adv_data, scan_response };

        Some(DeviceChipCreate {
            name: c.name,
            manufacturer: c.manufacturer,
            product_name: c.product_name,
            chip: ChipCreateVariant::Beacon(beacon),
        })
    } else if c.kind.enum_value_or_default() == ProtoChipKind::BLUETOOTH {
        let bt_create = BluetoothCreate {
            address: c.address,
            bt_properties: c
                .bt_properties
                .as_ref()
                .and_then(BtControllerProperties::from_proto)
                .map(netsim_model::Controller::from)
                .unwrap_or_default(),
            mode: BluetoothMode::Device(Default::default()),
        };
        Some(DeviceChipCreate {
            name: c.name,
            manufacturer: c.manufacturer,
            product_name: c.product_name,
            chip: ChipCreateVariant::Bluetooth(bt_create),
        })
    } else if c.kind.enum_value_or_default() == ProtoChipKind::UWB {
        // Proto ChipCreate reuses the top-level address field for UWB anchor short MAC.
        let uwb_create =
            UwbCreate::Anchor { address: (!c.address.is_empty()).then_some(c.address) };
        Some(DeviceChipCreate {
            name: c.name,
            manufacturer: c.manufacturer,
            product_name: c.product_name,
            chip: ChipCreateVariant::Uwb(uwb_create),
        })
    } else {
        None
    }
}

fn from_proto_advertise_settings(
    s: &netsim_proto::model::chip::ble_beacon::AdvertiseSettings,
    use_defaults: bool,
) -> AdvertiseSettings {
    use netsim_proto::model::chip::ble_beacon::advertise_settings::{
        Interval as ProtoInterval, Tx_power as ProtoTxPower,
    };

    let interval = match s.interval {
        Some(ProtoInterval::AdvertiseMode(mode)) => match mode.enum_value_or_default() {
            netsim_proto::model::chip::ble_beacon::advertise_settings::AdvertiseMode::LOW_POWER => {
                Some(Interval::AdvertiseMode(AdvertiseMode::LowPower))
            }
            netsim_proto::model::chip::ble_beacon::advertise_settings::AdvertiseMode::BALANCED => {
                Some(Interval::AdvertiseMode(AdvertiseMode::Balanced))
            }
            netsim_proto::model::chip::ble_beacon::advertise_settings::AdvertiseMode::LOW_LATENCY => {
                Some(Interval::AdvertiseMode(AdvertiseMode::LowLatency))
            }
        },
        Some(ProtoInterval::Milliseconds(ms)) => Some(Interval::Milliseconds(ms)),
        None => use_defaults.then_some(Interval::AdvertiseMode(AdvertiseMode::LowPower)), // Default
        Some(_) => Some(Interval::AdvertiseMode(AdvertiseMode::LowPower)), // Unknown/Future variant
    };

    let tx_power = match s.tx_power {
        Some(ProtoTxPower::TxPowerLevel(level)) => match level.enum_value_or_default() {
            netsim_proto::model::chip::ble_beacon::advertise_settings::AdvertiseTxPower::ULTRA_LOW => {
                Some(TxPower::TxPowerLevel(AdvertiseTxPower::UltraLow))
            }
            netsim_proto::model::chip::ble_beacon::advertise_settings::AdvertiseTxPower::LOW => {
                Some(TxPower::TxPowerLevel(AdvertiseTxPower::Low))
            }
            netsim_proto::model::chip::ble_beacon::advertise_settings::AdvertiseTxPower::MEDIUM => {
                Some(TxPower::TxPowerLevel(AdvertiseTxPower::Medium))
            }
            netsim_proto::model::chip::ble_beacon::advertise_settings::AdvertiseTxPower::HIGH => {
                Some(TxPower::TxPowerLevel(AdvertiseTxPower::High))
            }
        },
        Some(ProtoTxPower::Dbm(dbm)) => Some(TxPower::Dbm(dbm)),
        None => use_defaults.then_some(TxPower::TxPowerLevel(AdvertiseTxPower::Low)), // Default
        Some(_) => Some(TxPower::TxPowerLevel(AdvertiseTxPower::Low)), // Unknown/Future variant
    };

    AdvertiseSettings { interval, tx_power, scannable: s.scannable, timeout: s.timeout }
}

fn from_proto_advertise_data(
    d: &netsim_proto::model::chip::ble_beacon::AdvertiseData,
) -> AdvertiseData {
    AdvertiseData {
        include_device_name: d.include_device_name,
        include_tx_power_level: d.include_tx_power_level,
        manufacturer_data: d.manufacturer_data.clone(),
        services: d
            .services
            .iter()
            .map(|s| netsim_model::Service { uuid: s.uuid.clone(), data: s.data.clone() })
            .collect(),
    }
}

pub(crate) fn from_proto_position(p: ProtoPosition) -> ApiPosition {
    ApiPosition { x: p.x, y: p.y, z: p.z }
}

pub(crate) fn from_proto_orientation(o: ProtoOrientation) -> ApiOrientation {
    ApiOrientation { yaw: o.yaw, pitch: o.pitch, roll: o.roll }
}

pub(crate) fn from_proto_chip_kind(k: ProtoChipKind) -> Option<ApiChipKind> {
    match k {
        ProtoChipKind::BLUETOOTH => Some(ApiChipKind::BLUETOOTH),
        ProtoChipKind::WIFI => Some(ApiChipKind::WIFI),
        ProtoChipKind::UWB => Some(ApiChipKind::UWB),
        ProtoChipKind::BLUETOOTH_BEACON => Some(ApiChipKind::BLUETOOTH),
        ProtoChipKind::NFC => Some(ApiChipKind::NFC),
        ProtoChipKind::CELLULAR => Some(ApiChipKind::CELLULAR),
        ProtoChipKind::CELLULAR_DATA => Some(ApiChipKind::CELLULAR_DATA),
        ProtoChipKind::ETHERNET => Some(ApiChipKind::ETHERNET),
        _ => None,
    }
}

pub(crate) fn to_proto_link(l: ApiLink) -> ProtoLink {
    let mut link = ProtoLink::new();
    link.id = l.id.0;
    link.sender_id = l.sender.0;
    link.receiver_id = l.receiver.0;
    link.rssi = l.rssi as i32;
    link.kind = EnumOrUnknown::new(to_proto_chip_kind(l.kind));
    link.link_kind = EnumOrUnknown::new(match l.kind {
        ApiChipKind::BLUETOOTH => ProtoPhyKind::BLUETOOTH_LOW_ENERGY,
        ApiChipKind::WIFI => ProtoPhyKind::WIFI,
        ApiChipKind::UWB => ProtoPhyKind::UWB,
        _ => ProtoPhyKind::NONE,
    });
    link
}

pub(crate) fn from_proto_chip_update(c: ProtoChip) -> ChipUpdate {
    let variant = if let Some(chip_oneof) = c.chip {
        match chip_oneof {
            netsim_proto::model::chip::Chip::Bt(bt) => {
                let bt_properties =
                    bt.bt_properties.as_ref().and_then(BtControllerProperties::from_proto);
                let preset = bt_properties.as_ref().and_then(|p| p.preset.clone());
                Some(ChipVariantUpdate::Bluetooth(BluetoothUpdate {
                    classic: from_proto_radio_update(bt.classic.into_option()),
                    low_energy: from_proto_radio_update(bt.low_energy.into_option()),
                    preset,
                    bt_properties,
                    ble_beacon: None,
                }))
            }
            netsim_proto::model::chip::Chip::BleBeacon(b) => {
                let bt_update = b.bt.into_option().map(|bt| {
                    (
                        from_proto_radio_update(bt.classic.into_option()),
                        from_proto_radio_update(bt.low_energy.into_option()),
                    )
                });
                let (classic, low_energy) = bt_update.unwrap_or_default();
                let ble_beacon = BleBeacon {
                    address: b.address,
                    settings: b.settings.as_ref().map(|s| from_proto_advertise_settings(s, false)),
                    adv_data: b.adv_data.as_ref().map(from_proto_advertise_data),
                    scan_response: b.scan_response.as_ref().map(from_proto_advertise_data),
                };
                Some(ChipVariantUpdate::Bluetooth(BluetoothUpdate {
                    classic,
                    low_energy,
                    preset: None,
                    bt_properties: None,
                    ble_beacon: Some(ble_beacon),
                }))
            }
            netsim_proto::model::chip::Chip::Uwb(uwb) => {
                Some(ChipVariantUpdate::Uwb(netsim_model::UwbUpdate {
                    radio: from_proto_radio_update(Some(uwb)),
                }))
            }
            netsim_proto::model::chip::Chip::Wifi(wifi) => {
                Some(ChipVariantUpdate::Wifi(netsim_model::WifiUpdate {
                    radio: from_proto_radio_update(Some(wifi)),
                }))
            }
            netsim_proto::model::chip::Chip::Cellular(cell) => {
                let state = if cell.state.unwrap_or(true) {
                    "idle".to_string()
                } else {
                    "down".to_string()
                };
                Some(ChipVariantUpdate::Cell(netsim_model::CellUpdate {
                    radio: from_proto_radio_update(Some(cell)),
                    state: Some(state),
                    ..Default::default()
                }))
            }
            netsim_proto::model::chip::Chip::CellularData(cell_data) => {
                Some(ChipVariantUpdate::CellularData(netsim_model::CellularDataUpdate {
                    radio: from_proto_radio_update(Some(cell_data)),
                }))
            }
            netsim_proto::model::chip::Chip::Ethernet(eth) => {
                Some(ChipVariantUpdate::Ethernet(netsim_model::EthernetUpdate {
                    radio: from_proto_radio_update(Some(eth)),
                }))
            }
            netsim_proto::model::chip::Chip::Nfc(nfc) => {
                Some(ChipVariantUpdate::Nfc(netsim_model::NfcUpdate {
                    radio: from_proto_radio_update(Some(nfc)),
                }))
            }
            _ => None,
        }
    } else {
        None
    };

    ChipUpdate {
        id: if c.id != 0 { Some(ChipId(c.id)) } else { None },
        name: if c.name.is_empty() { None } else { Some(c.name) },
        manufacturer: if c.manufacturer.is_empty() { None } else { Some(c.manufacturer) },
        product_name: if c.product_name.is_empty() { None } else { Some(c.product_name) },
        pose: PoseUpdate {
            position: c.offset.into_option().map(from_proto_position),
            orientation: None, // Proto Chip does not have orientation.
        },
        variant,
        links: None, // TODO
        enabled: None,
    }
}

fn from_proto_radio_update(r: Option<netsim_proto::model::chip::Radio>) -> RadioUpdate {
    match r {
        Some(r) => RadioUpdate { state: r.state },
        None => RadioUpdate { state: None },
    }
}

pub(crate) fn from_proto_link(proto: ProtoLink) -> Option<ApiLink> {
    let sender = netsim_model::ChipId(proto.sender_id);
    let receiver = netsim_model::ChipId(proto.receiver_id);
    let rssi = proto.rssi as i8;

    let kind = if proto.kind.enum_value_or_default() != ProtoChipKind::UNSPECIFIED {
        from_proto_chip_kind(proto.kind.enum_value_or_default())
    } else {
        match proto.link_kind.enum_value_or_default() {
            ProtoPhyKind::BLUETOOTH_CLASSIC | ProtoPhyKind::BLUETOOTH_LOW_ENERGY => {
                Some(ApiChipKind::BLUETOOTH)
            }
            ProtoPhyKind::WIFI | ProtoPhyKind::WIFI_RTT => Some(ApiChipKind::WIFI),
            ProtoPhyKind::UWB => Some(ApiChipKind::UWB),
            _ => None,
        }
    };

    if let Some(kind) = kind {
        Some(ApiLink { id: netsim_model::LinkId(proto.id), sender, receiver, kind, rssi })
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_from_proto_chip_create_uwb_default_address() {
        let mut chip = ChipCreate::new();
        chip.name = "uwb-anchor".to_string();
        chip.kind = EnumOrUnknown::new(ProtoChipKind::UWB);
        let device_chip = from_proto_chip_create(chip).unwrap();
        assert_eq!(device_chip.name, "uwb-anchor");
        assert_eq!(device_chip.chip, ChipCreateVariant::Uwb(UwbCreate::Anchor { address: None }));
    }

    #[test]
    fn test_from_proto_chip_create_uwb_custom_address() {
        let mut chip = ChipCreate::new();
        chip.name = "uwb-anchor".to_string();
        chip.address = "00:01".to_string();
        chip.kind = EnumOrUnknown::new(ProtoChipKind::UWB);
        let device_chip = from_proto_chip_create(chip).unwrap();
        assert_eq!(device_chip.name, "uwb-anchor");
        assert_eq!(
            device_chip.chip,
            ChipCreateVariant::Uwb(UwbCreate::Anchor { address: Some("00:01".to_string()) })
        );
    }

    #[test]
    fn test_from_proto_chip_update_bluetooth_properties() {
        use netsim_proto::configuration::{Controller, ControllerFeatures, ControllerPreset};

        let mut chip = ProtoChip::new();
        let mut bt = netsim_proto::model::chip::Bluetooth::new();
        let mut ctrl = Controller::new();
        ctrl.set_preset(ControllerPreset::LAIRD_BL654);
        let mut features = ControllerFeatures::new();
        features.le_extended_advertising = Some(true);
        ctrl.features = MessageField::some(features);
        bt.bt_properties = MessageField::some(ctrl);
        chip.chip = Some(netsim_proto::model::chip::Chip::Bt(bt));

        let update = from_proto_chip_update(chip);
        match update.variant {
            Some(ChipVariantUpdate::Bluetooth(bt_update)) => {
                assert_eq!(bt_update.preset, Some("laird_bl654".to_string()));
                let props = bt_update.bt_properties.expect("bt_properties should be present");
                assert_eq!(props.preset, Some("laird_bl654".to_string()));
                assert_eq!(props.features.unwrap().le_extended_advertising, Some(true));
            }
            _ => panic!("Expected Bluetooth variant update"),
        }
    }

    #[test]
    fn test_from_proto_chip_create_bluetooth_properties() {
        use netsim_proto::configuration::{Controller, ControllerFeatures, ControllerPreset};

        let mut chip = ChipCreate::new();
        chip.name = "bt-chip".to_string();
        chip.kind = EnumOrUnknown::new(ProtoChipKind::BLUETOOTH);

        let mut ctrl = Controller::new();
        ctrl.set_preset(ControllerPreset::LAIRD_BL654);
        let mut features = ControllerFeatures::new();
        features.le_extended_advertising = Some(true);
        ctrl.features = MessageField::some(features);
        chip.bt_properties = MessageField::some(ctrl);

        let device_chip = from_proto_chip_create(chip).expect("device chip created");
        assert_eq!(device_chip.name, "bt-chip");
        match device_chip.chip {
            ChipCreateVariant::Bluetooth(bt_create) => {
                assert_eq!(bt_create.bt_properties.config.preset, Some("laird_bl654".to_string()));
                assert_eq!(
                    bt_create.bt_properties.config.features.unwrap().le_extended_advertising,
                    Some(true)
                );
            }
            _ => panic!("Expected Bluetooth variant create"),
        }
    }

    #[test]
    fn test_to_proto_chip_bluetooth_properties() {
        use netsim_proto::configuration::ControllerPreset;

        let mut chip = netsim_model::Chip::new_test_ble("bt-device");
        chip.variant =
            Some(netsim_model::ChipVariant::Bluetooth(Box::new(netsim_model::Bluetooth {
                bt_properties: netsim_model::BtControllerProperties {
                    preset: Some("laird_bl654".to_string()),
                    features: Some(netsim_model::BtControllerFeatures {
                        le_extended_advertising: Some(true),
                        ..Default::default()
                    }),
                    ..Default::default()
                }
                .into(),
                ..Default::default()
            })));

        let proto_chip = to_proto_chip(chip);
        match proto_chip.chip {
            Some(netsim_proto::model::chip::Chip::Bt(bt)) => {
                let ctrl = bt.bt_properties.into_option().expect("bt_properties present");
                assert_eq!(ctrl.preset(), ControllerPreset::LAIRD_BL654);
                assert_eq!(ctrl.features.le_extended_advertising, Some(true));
            }
            _ => panic!("Expected Bluetooth variant chip"),
        }
    }

    #[test]
    fn test_from_proto_chip_update_nfc() {
        let mut chip = ProtoChip::new();
        let mut nfc = netsim_proto::model::chip::Radio::new();
        nfc.state = Some(false);
        chip.chip = Some(netsim_proto::model::chip::Chip::Nfc(nfc));
        let update = from_proto_chip_update(chip);
        assert_eq!(
            update.variant,
            Some(ChipVariantUpdate::Nfc(netsim_model::NfcUpdate {
                radio: RadioUpdate { state: Some(false) },
            }))
        );
    }

    #[test]
    fn test_to_proto_chip_and_from_proto_chip_update_ble_beacon() {
        for (interval, tx_power) in [
            (
                Interval::AdvertiseMode(AdvertiseMode::LowPower),
                TxPower::TxPowerLevel(AdvertiseTxPower::UltraLow),
            ),
            (
                Interval::AdvertiseMode(AdvertiseMode::Balanced),
                TxPower::TxPowerLevel(AdvertiseTxPower::Low),
            ),
            (
                Interval::AdvertiseMode(AdvertiseMode::LowLatency),
                TxPower::TxPowerLevel(AdvertiseTxPower::Medium),
            ),
            (Interval::Milliseconds(500), TxPower::TxPowerLevel(AdvertiseTxPower::High)),
            (Interval::Milliseconds(250), TxPower::Dbm(-5)),
        ] {
            let ble_beacon = BleBeacon {
                address: "11:22:33:44:55:66".to_string(),
                settings: Some(AdvertiseSettings {
                    interval: Some(interval.clone()),
                    tx_power: Some(tx_power.clone()),
                    scannable: true,
                    timeout: 1200,
                }),
                adv_data: Some(AdvertiseData {
                    include_device_name: true,
                    include_tx_power_level: true,
                    manufacturer_data: vec![0x01, 0x02],
                    services: vec![netsim_model::Service {
                        uuid: "180D".to_string(),
                        data: vec![0xAA],
                    }],
                }),
                scan_response: Some(AdvertiseData {
                    include_device_name: false,
                    include_tx_power_level: false,
                    manufacturer_data: vec![0x03, 0x04],
                    services: vec![],
                }),
            };

            let mut chip = netsim_model::Chip::new_test_ble("beacon-chip");
            chip.variant =
                Some(netsim_model::ChipVariant::Bluetooth(Box::new(netsim_model::Bluetooth {
                    address: "11:22:33:44:55:66".to_string(),
                    mode: BluetoothMode::Beacon(Box::new(netsim_model::BeaconParams {
                        ble_beacon: ble_beacon.clone(),
                    })),
                    ..Default::default()
                })));

            let proto_chip = to_proto_chip(chip);
            assert!(matches!(proto_chip.chip, Some(netsim_proto::model::chip::Chip::BleBeacon(_))));

            let update = from_proto_chip_update(proto_chip);
            assert_eq!(
                update.variant,
                Some(ChipVariantUpdate::Bluetooth(BluetoothUpdate {
                    classic: RadioUpdate { state: Some(true) },
                    low_energy: RadioUpdate { state: Some(true) },
                    ble_beacon: Some(ble_beacon),
                    ..Default::default()
                }))
            );
        }
    }
}
