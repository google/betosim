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

// BLE Constants
const MAX_AD_PAYLOAD_LEN: usize = 31;

// AD Structure Types
const AD_TYPE_FLAGS: u8 = 0x01;
const AD_TYPE_NAME_SHORTENED: u8 = 0x08;
const AD_TYPE_NAME_COMPLETE: u8 = 0x09;
const AD_TYPE_TX_POWER_LEVEL: u8 = 0x0A;

const AD_TYPE_MANUFACTURER_SPECIFIC: u8 = 0xFF;

// BLE Flags (Bluetooth Core Spec Vol 3, Part C, Section 9.1.1.2 & 9.2.2.2):
// Non-connectable BLE broadcasters (ADV_SCAN_IND / ADV_NONCONN_IND) operate in
// Non-Discoverable Mode and must not set LE Limited/General Discoverable Mode
// (0x01 / 0x02), which would also cause Android's btm_ble_is_discoverable() to
// report simulated BLE beacons during Classic BluetoothAdapter.startDiscovery()
// inquiries. Only set BR/EDR Not Supported (0x04).
const AD_FLAG_BR_EDR_NOT_SUPPORTED: u8 = 0x04;
const FLAGS_DATA: [u8; 3] = [0x02, AD_TYPE_FLAGS, AD_FLAG_BR_EDR_NOT_SUPPORTED];

/// Helper to construct advertising or scan response data payload.
///
/// Per Bluetooth Core Specification Supplement (CSS) Part A, Section 1.3.1,
/// the Flags AD structure (`0x01`) is included in advertising data
/// (`include_flags = true`) and omitted from Scan Response PDUs
/// (`include_flags = false`).
pub(crate) fn construct_data(
    adv_data: &netsim_model::AdvertiseData,
    device_name: Option<&str>,
    include_flags: bool,
) -> Vec<u8> {
    let mut data = Vec::new();
    if include_flags {
        data.extend_from_slice(&FLAGS_DATA);
    }

    // Manufacturer Data
    if !adv_data.manufacturer_data.is_empty() {
        data.push((adv_data.manufacturer_data.len() + 1) as u8);
        data.push(AD_TYPE_MANUFACTURER_SPECIFIC);
        data.extend_from_slice(&adv_data.manufacturer_data);
    }

    // Local Name
    if let Some(name) = device_name {
        let name_bytes = name.as_bytes();
        if data.len() + 2 < MAX_AD_PAYLOAD_LEN {
            let max_len = MAX_AD_PAYLOAD_LEN - data.len() - 2;
            let sub = name_bytes.get(..max_len).unwrap_or(name_bytes);
            if !sub.is_empty() {
                let name_type = if sub.len() == name_bytes.len() {
                    AD_TYPE_NAME_COMPLETE
                } else {
                    AD_TYPE_NAME_SHORTENED
                };
                data.push((sub.len() + 1) as u8);
                data.push(name_type);
                data.extend_from_slice(sub);
            }
        }
    }
    // Tx Power Level
    if adv_data.include_tx_power_level {
        let max_tx_power = 20; // 20 dBm (Standard default mock power)
        if data.len() + 3 <= MAX_AD_PAYLOAD_LEN {
            data.push(2);
            data.push(AD_TYPE_TX_POWER_LEVEL);
            data.push(max_tx_power as u8);
        }
    }

    // Service UUIDs (Grouped by length into single AD structures)
    let mut uuids_16 = Vec::new();
    let mut uuids_32 = Vec::new();
    let mut uuids_128 = Vec::new();

    for service in &adv_data.services {
        let hex_str = service.uuid.replace("-", "");
        if let Ok(mut uuid_bytes) = hex::decode(&hex_str) {
            uuid_bytes.reverse(); // UUIDs are little-endian in AD
            match uuid_bytes.len() {
                2 => uuids_16.extend(uuid_bytes),
                4 => uuids_32.extend(uuid_bytes),
                16 => uuids_128.extend(uuid_bytes),
                _ => {
                    tracing::error!(
                        "Invalid UUID length {} bytes for advertising data, skipping",
                        uuid_bytes.len()
                    );
                }
            }
        }
    }

    for (complete_id, incomplete_id, uuid_size, uuids) in
        [(0x03, 0x02, 2, uuids_16), (0x05, 0x04, 4, uuids_32), (0x07, 0x06, 16, uuids_128)]
    {
        if data.len() + 2 < MAX_AD_PAYLOAD_LEN {
            let max_space = MAX_AD_PAYLOAD_LEN - data.len() - 2;
            let max_uuid_bytes = (max_space / uuid_size) * uuid_size;
            let sub = uuids.get(..max_uuid_bytes).unwrap_or(&uuids);
            if !sub.is_empty() {
                let class_id = if sub.len() == uuids.len() { complete_id } else { incomplete_id };
                data.push((sub.len() + 1) as u8);
                data.push(class_id);
                data.extend_from_slice(sub);
            }
        }
    }
    data
}

/// Generates a legacy Bluetooth address from a Chip ID.
///
/// This matches the legacy C++ behavior and netsim daemon behavior where
/// the address is derived from the ID.
/// Example: ID 1000 -> 00:00:00:00:03:e8
pub(crate) fn generate_legacy_address(chip_id: u32) -> String {
    format!("00:00:00:00:{:02x}:{:02x}", (chip_id >> 8) & 0xFF, chip_id & 0xFF)
}

/// Generates a default name for a beacon from a Chip ID.
pub(crate) fn generate_default_name(chip_id: u32) -> String {
    format!("Beacon-{}", chip_id)
}

// HCI Constants used for testing and packet parsing

/// HCI Packet Type: Event
const HCI_EVENT_PACKET: u8 = 0x04;
/// HCI Event Code: LE Meta Event
const LE_META_EVENT: u8 = 0x3E;
/// LE Subevent Code: Advertising Report
const LE_ADVERTISING_REPORT: u8 = 0x02;

// Offsets within LE Advertising Report Event
// Byte 0: Packet Type
// Byte 1: Event Code
// Byte 2: Parameter Total Length
// Byte 3: Subevent Code
// Byte 4: Num Reports
// Byte 5: Event Type
// Byte 6: Address Type
// Byte 7-12: Address
/// Offset of the Number of Reports field in an LE Advertising Report
pub const REPORT_NUM_REPORTS_OFFSET: usize = 4;
/// Offset of the Address field in an LE Advertising Report
pub const REPORT_ADDR_OFFSET: usize = 12;

/// Helper to check if a packet is an LE Advertising Report
pub const fn is_le_advertising_report(packet: &[u8]) -> bool {
    let [HCI_EVENT_PACKET, LE_META_EVENT, _, LE_ADVERTISING_REPORT, _, ..] = packet else {
        return false;
    };
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_construct_data_basic() {
        let adv_data = netsim_model::AdvertiseData::default();
        let data = construct_data(&adv_data, None, true);
        assert_eq!(data, FLAGS_DATA);
    }

    #[test]
    fn test_construct_data_with_name() {
        let mut adv_data = netsim_model::AdvertiseData::default();
        adv_data.include_device_name = true;
        let data = construct_data(&adv_data, Some("Test"), true);
        // Flags (3) + Name (2 + 4) = 9 bytes
        let mut expected = FLAGS_DATA.to_vec();
        expected.extend_from_slice(&[0x05, 0x09, b'T', b'e', b's', b't']);
        assert_eq!(data, expected);
    }

    #[test]
    fn test_construct_data_with_manufacturer_data() {
        let mut adv_data = netsim_model::AdvertiseData::default();
        adv_data.manufacturer_data = vec![0x01, 0x02];
        let data = construct_data(&adv_data, None, true);
        // Flags (3) + Mfg (2 + 2) = 7 bytes
        let mut expected = FLAGS_DATA.to_vec();
        expected.extend_from_slice(&[0x03, 0xFF, 0x01, 0x02]);
        assert_eq!(data, expected);
    }

    #[test]
    fn test_construct_data_truncate_name() {
        // Flags = 3 bytes
        // Remaining = 31 - 3 = 28 bytes
        // Name header = 2 bytes
        // Max name len = 26 bytes
        let adv_data = netsim_model::AdvertiseData::default();
        let long_name = "A".repeat(30);
        let data = construct_data(&adv_data, Some(&long_name), true);
        assert_eq!(data.len(), 31);
        assert_eq!(&data[..3], &FLAGS_DATA);
        assert_eq!(data[3], 0x1b); // Length = 27 (26 + 1 type)
        assert_eq!(data[4], 0x08);
        assert_eq!(&data[5..], "A".repeat(26).as_bytes());
    }

    #[test]
    fn test_construct_data_with_tx_power_level() {
        let mut adv_data = netsim_model::AdvertiseData::default();
        adv_data.include_tx_power_level = true;
        let data = construct_data(&adv_data, None, true);
        // Flags (3) + Tx Power (3) = 6 bytes
        let mut expected = FLAGS_DATA.to_vec();
        expected.extend_from_slice(&[0x02, 0x0A, 20]);
        assert_eq!(data, expected);
    }

    #[test]
    fn test_construct_data_with_service_uuids() {
        let mut adv_data = netsim_model::AdvertiseData::default();
        adv_data.services.push(netsim_model::Service {
            uuid: "180D".to_string(), // Heart Rate Service 16-bit UUID
            data: vec![0xAB, 0xCD],
        });
        adv_data.services.push(netsim_model::Service {
            uuid: "180F".to_string(), // Battery Service 16-bit UUID
            data: vec![],
        });
        let data = construct_data(&adv_data, None, true);
        // Flags (3) + Service (1 len + 1 type + 4 uuid elements) = 9 bytes
        // 180D -> 0x0D, 0x18
        // 180F -> 0x0F, 0x18
        let mut expected = FLAGS_DATA.to_vec();
        expected.extend_from_slice(&[0x05, 0x03, 0x0D, 0x18, 0x0F, 0x18]);
        assert_eq!(data, expected);
    }

    #[test]
    fn test_construct_data_with_invalid_service_uuid() {
        let mut adv_data = netsim_model::AdvertiseData::default();
        adv_data.services.push(netsim_model::Service {
            uuid: "180".to_string(), // Invalid odd-length string
            data: vec![],
        });
        adv_data.services.push(netsim_model::Service {
            uuid: "180D11".to_string(), // 3 bytes (invalid specification length)
            data: vec![],
        });
        let data = construct_data(&adv_data, None, true);
        // Invalid services should be skipped. Only Flags remain.
        assert_eq!(data, FLAGS_DATA);
    }

    #[test]
    fn test_construct_data_truncate_service_uuids() {
        let mut adv_data = netsim_model::AdvertiseData::default();
        for i in 0..16 {
            adv_data
                .services
                .push(netsim_model::Service { uuid: format!("{:04X}", 0x1800 + i), data: vec![] });
        }
        let data = construct_data(&adv_data, None, true);
        // Flags (3) + header (2) + 13 UUIDs (26) = 31 bytes
        assert_eq!(data.len(), 31);
        assert_eq!(&data[..3], &FLAGS_DATA);
        assert_eq!(data[3], 27);
        assert_eq!(data[4], 0x02);
        assert_eq!(&data[5..7], &[0x00, 0x18]);
        assert_eq!(&data[29..31], &[0x0C, 0x18]);
    }

    #[test]
    fn test_construct_data_scan_response_omits_flags() {
        let empty = netsim_model::AdvertiseData::default();
        assert_eq!(construct_data(&empty, None, false), Vec::<u8>::new());

        let mut scan_resp = netsim_model::AdvertiseData::default();
        scan_resp.manufacturer_data = vec![0x01, 0x02, 0x03, 0x04];
        assert_eq!(
            construct_data(&scan_resp, None, false),
            vec![0x05, 0xFF, 0x01, 0x02, 0x03, 0x04]
        );
    }

    #[test]
    fn test_construct_data_with_32bit_service_uuid() {
        let mut adv_data = netsim_model::AdvertiseData::default();
        adv_data.services.push(netsim_model::Service {
            uuid: "180D1122".to_string(), // 32-bit UUID (4 bytes)
            data: vec![],
        });
        let data = construct_data(&adv_data, None, true);
        // Flags (3) + 1 len + 1 type (0x05) + 4 reversed uuid bytes
        let mut expected = FLAGS_DATA.to_vec();
        expected.extend_from_slice(&[0x05, 0x05, 0x22, 0x11, 0x0D, 0x18]);
        assert_eq!(data, expected);
    }

    #[test]
    fn test_construct_data_with_128bit_service_uuid() {
        let mut adv_data = netsim_model::AdvertiseData::default();
        adv_data.services.push(netsim_model::Service {
            uuid: "0000180D-0000-1000-8000-00805F9B34FB".to_string(), // 128-bit UUID (16 bytes)
            data: vec![],
        });
        let data = construct_data(&adv_data, None, true);
        // Flags (3) + 1 len + 1 type (0x07) + 16 reversed uuid bytes
        let mut expected = FLAGS_DATA.to_vec();
        expected.extend_from_slice(&[
            17, 0x07, 0xFB, 0x34, 0x9B, 0x5F, 0x80, 0x00, 0x00, 0x80, 0x00, 0x10, 0x00, 0x00, 0x0D,
            0x18, 0x00, 0x00,
        ]);
        assert_eq!(data, expected);
    }

    #[test]
    fn test_construct_data_with_tx_power_near_limit() {
        let mut adv_data = netsim_model::AdvertiseData::default();
        // 25 bytes with header (1 len + 1 type + 23 data)
        adv_data.manufacturer_data = vec![0x42; 23];
        adv_data.include_tx_power_level = true;
        let data = construct_data(&adv_data, None, true);
        // 3 bytes flags + 25 bytes mfg data + 3 bytes tx power = 31 bytes
        assert_eq!(data.len(), 31);
        assert_eq!(&data[..3], &FLAGS_DATA);
        assert_eq!(data[3], 24);
        assert_eq!(data[4], AD_TYPE_MANUFACTURER_SPECIFIC);
        assert_eq!(&data[28..], &[2, AD_TYPE_TX_POWER_LEVEL, 20]);

        // If manufacturer data exceeds space, tx power is omitted
        let mut adv_data_overflow = netsim_model::AdvertiseData::default();
        adv_data_overflow.manufacturer_data = vec![0x42; 24]; // 26 bytes with header
        adv_data_overflow.include_tx_power_level = true;
        let data_overflow = construct_data(&adv_data_overflow, None, true);
        assert_eq!(data_overflow.len(), 29);
    }

    #[test]
    fn test_generate_legacy_address() {
        assert_eq!(generate_legacy_address(0), "00:00:00:00:00:00");
        assert_eq!(generate_legacy_address(1), "00:00:00:00:00:01");
        assert_eq!(generate_legacy_address(255), "00:00:00:00:00:ff");
        assert_eq!(generate_legacy_address(256), "00:00:00:00:01:00");
        assert_eq!(generate_legacy_address(1000), "00:00:00:00:03:e8");
        assert_eq!(generate_legacy_address(0xABCD), "00:00:00:00:ab:cd");
        // Upper 16 bits are masked out
        assert_eq!(generate_legacy_address(0x12345678), "00:00:00:00:56:78");
    }

    #[test]
    fn test_generate_default_name() {
        assert_eq!(generate_default_name(0), "Beacon-0");
        assert_eq!(generate_default_name(1000), "Beacon-1000");
        assert_eq!(generate_default_name(42), "Beacon-42");
    }

    #[test]
    fn test_is_le_advertising_report() {
        // Valid LE Advertising Report (at least 5 bytes with valid headers)
        let valid_report =
            [0x04, 0x3E, 0x0C, 0x02, 0x01, 0x00, 0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0xCE];
        assert!(is_le_advertising_report(&valid_report));

        // Minimum length matching slice pattern (5 bytes)
        let min_report = [0x04, 0x3E, 0x02, 0x02, 0x01];
        assert!(is_le_advertising_report(&min_report));

        // Incomplete / short packets
        assert!(!is_le_advertising_report(&[]));
        assert!(!is_le_advertising_report(&[0x04]));
        assert!(!is_le_advertising_report(&[0x04, 0x3E]));
        assert!(!is_le_advertising_report(&[0x04, 0x3E, 0x02]));
        assert!(!is_le_advertising_report(&[0x04, 0x3E, 0x02, 0x02]));

        // Mismatched packet type (e.g. 0x01 Command packet)
        let wrong_pkt_type = [0x01, 0x3E, 0x02, 0x02, 0x01];
        assert!(!is_le_advertising_report(&wrong_pkt_type));

        // Mismatched event code (e.g. 0x0E Command Complete)
        let wrong_event = [0x04, 0x0E, 0x02, 0x02, 0x01];
        assert!(!is_le_advertising_report(&wrong_event));

        // Mismatched subevent code (e.g. 0x01 LE Connection Complete)
        let wrong_subevent = [0x04, 0x3E, 0x02, 0x01, 0x01];
        assert!(!is_le_advertising_report(&wrong_subevent));
    }
}
