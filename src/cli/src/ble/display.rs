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

use std::{borrow::Cow, collections::BTreeMap, io::Write};

use common::util::time_display::TimeDisplay;
use netsim_packets::{
    GapDataType, HciEvent, LeAdvertisingEventType, LeMetaEvent, OwnAddressType, parse_hci_event,
};
use serde::Serialize;

pub(super) const H4_EVENT_PACKET_TYPE: u8 = 0x04;
const SCAN_RSP_KEY_OFFSET: u16 = 0x0100;

pub(super) struct BleEvent {
    timestamp_str: String,
    addr_display: String,
    event_type_str: String,
    rssi: i8,
    data_len: usize,
    raw_hex: String,
    ad_payloads: BTreeMap<u16, AdPayload>,
}

#[derive(Clone)]
struct AdPayload {
    ad_type: u8,
    type_str: String,
    decoded_val: String,
}

pub(super) fn parse_scan_packet(
    packet: &[u8],
    duration: std::time::Duration,
    verbose: bool,
) -> Vec<BleEvent> {
    let mut events = Vec::new();
    let timestamp = TimeDisplay::from_duration(duration).utc_display_hms();

    let hex_packet = if verbose { format_hex_data(packet) } else { String::new() };

    let payload = match packet {
        [H4_EVENT_PACKET_TYPE, rest @ ..] => rest,
        other => other,
    };

    let reports = match parse_hci_event(payload) {
        Some(HciEvent::LeMetaEvent(LeMetaEvent::LeAdvertisingReport(reports))) => reports,
        _ => return events,
    };

    for report in reports {
        let addr_display = format!(
            "{} ({})",
            report.address,
            if report.address_type == OwnAddressType::PUBLIC_DEVICE_ADDRESS {
                "Public"
            } else {
                "Random"
            }
        );

        let event_type_str = report.event_type.to_string();
        let is_scan_rsp = report.event_type == LeAdvertisingEventType::SCAN_RSP;

        let ad_payloads = if !report.data.is_empty() {
            parse_ad_structures(report.data, is_scan_rsp)
        } else {
            BTreeMap::new()
        };

        events.push(BleEvent {
            timestamp_str: timestamp.clone(),
            addr_display,
            event_type_str,
            rssi: report.rssi,
            data_len: report.data_length as usize,
            raw_hex: hex_packet.clone(),
            ad_payloads,
        });
    }

    events
}

fn parse_ad_structures(data: &[u8], is_scan_rsp: bool) -> BTreeMap<u16, AdPayload> {
    let mut rest = data;
    let mut outputs = BTreeMap::new();
    while let Some((&len_byte, after_len)) = rest.split_first() {
        let len = len_byte as usize;
        if len == 0 {
            break;
        }
        let Some((struct_bytes, next_rest)) = after_len.split_at_checked(len) else {
            break;
        };
        let Some((&ad_type, ad_data)) = struct_bytes.split_first() else {
            break;
        };

        let type_str = GapDataType(ad_type).to_string();

        let decoded_val = match GapDataType(ad_type) {
            GapDataType::SHORTENED_LOCAL_NAME | GapDataType::COMPLETE_LOCAL_NAME => {
                String::from_utf8_lossy(ad_data).to_string()
            }
            GapDataType::TX_POWER_LEVEL => {
                if let [power] = ad_data {
                    format!("{} dBm", power.cast_signed())
                } else {
                    format_hex_data(ad_data)
                }
            }
            GapDataType::INCOMPLETE_16BIT_UUIDS | GapDataType::COMPLETE_16BIT_UUIDS
                if ad_data.len().is_multiple_of(2) =>
            {
                let uuids: Vec<String> =
                    ad_data.as_chunks::<2>().0.iter().copied().map(format_16bit_uuid).collect();
                uuids.join(", ")
            }
            GapDataType::INCOMPLETE_32BIT_UUIDS | GapDataType::COMPLETE_32BIT_UUIDS
                if ad_data.len().is_multiple_of(4) =>
            {
                let uuids: Vec<String> =
                    ad_data.as_chunks::<4>().0.iter().copied().map(format_32bit_uuid).collect();
                uuids.join(", ")
            }
            GapDataType::INCOMPLETE_128BIT_UUIDS | GapDataType::COMPLETE_128BIT_UUIDS
                if ad_data.len().is_multiple_of(16) =>
            {
                let uuids: Vec<String> =
                    ad_data.as_chunks::<16>().0.iter().map(format_128bit_uuid).collect();
                uuids.join(", ")
            }
            GapDataType::SERVICE_DATA_16BIT => {
                if let Some((&uuid_chunk, rest_data)) = ad_data.split_first_chunk::<2>() {
                    format_uuid_service_data(&format_16bit_uuid(uuid_chunk), rest_data)
                } else {
                    format_hex_data(ad_data)
                }
            }
            GapDataType::SERVICE_DATA_32BIT => {
                if let Some((&uuid_chunk, rest_data)) = ad_data.split_first_chunk::<4>() {
                    format_uuid_service_data(&format_32bit_uuid(uuid_chunk), rest_data)
                } else {
                    format_hex_data(ad_data)
                }
            }
            GapDataType::SERVICE_DATA_128BIT => {
                if let Some((uuid_chunk, rest_data)) = ad_data.split_first_chunk::<16>() {
                    format_uuid_service_data(&format_128bit_uuid(uuid_chunk), rest_data)
                } else {
                    format_hex_data(ad_data)
                }
            }
            _ => format_hex_data(ad_data),
        };

        let map_key =
            if is_scan_rsp { SCAN_RSP_KEY_OFFSET | (ad_type as u16) } else { ad_type as u16 };

        outputs.insert(map_key, AdPayload { ad_type, type_str, decoded_val });

        rest = next_rest;
    }
    outputs
}

fn format_16bit_uuid(uuid: [u8; 2]) -> String {
    format!("0x{:04x}", u16::from_le_bytes(uuid))
}

fn format_32bit_uuid(uuid: [u8; 4]) -> String {
    format!("0x{:08x}", u32::from_le_bytes(uuid))
}

fn format_128bit_uuid(uuid: &[u8; 16]) -> String {
    let &[b0, b1, b2, b3, b4, b5, b6, b7, b8, b9, b10, b11, b12, b13, b14, b15] = uuid;
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        b15, b14, b13, b12, b11, b10, b9, b8, b7, b6, b5, b4, b3, b2, b1, b0
    )
}

fn format_uuid_service_data(uuid: &str, data: &[u8]) -> String {
    let data_str = if data.is_empty() { String::from("None") } else { format_hex_data(data) };
    format!("UUID: {uuid}, Data: {data_str}")
}

fn format_hex_data(data: &[u8]) -> String {
    let bytes: Vec<String> = data.iter().map(|b| format!("{b:02x}")).collect();
    bytes.join(" ")
}

pub(super) fn write_json_event(
    writer: &mut impl Write,
    event: &BleEvent,
    verbose: bool,
) -> std::io::Result<()> {
    #[derive(Serialize)]
    struct JsonAdPayload<'a> {
        type_id: u8,
        type_name: &'a str,
        value: &'a str,
    }

    #[derive(Serialize)]
    struct JsonBleEvent<'a> {
        timestamp: &'a str,
        address: &'a str,
        event_type: &'a str,
        rssi: i8,
        data_len: usize,
        #[serde(skip_serializing_if = "Option::is_none")]
        raw_hex: Option<&'a str>,
        ad_payloads: Vec<JsonAdPayload<'a>>,
    }

    let out = JsonBleEvent {
        timestamp: &event.timestamp_str,
        address: &event.addr_display,
        event_type: &event.event_type_str,
        rssi: event.rssi,
        data_len: event.data_len,
        raw_hex: if verbose { Some(&event.raw_hex) } else { None },
        ad_payloads: event
            .ad_payloads
            .values()
            .map(|ad| JsonAdPayload {
                type_id: ad.ad_type,
                type_name: &ad.type_str,
                value: &ad.decoded_val,
            })
            .collect(),
    };

    match serde_json::to_string(&out) {
        Ok(json_str) => {
            writeln!(writer, "{json_str}")?;
            writer.flush()
        }
        Err(e) => {
            eprintln!("Failed to serialize BLE event to JSON: {e}");
            Ok(())
        }
    }
}

pub(super) fn write_sniff_packet(
    writer: &mut impl Write,
    packet: &[u8],
    duration: std::time::Duration,
) -> std::io::Result<()> {
    let timestamp = TimeDisplay::from_duration(duration).utc_display_hms();
    let hex_packet = hex::encode(packet);

    let packet_type: Cow<'static, str> = match packet.first() {
        None => Cow::Borrowed("EMPTY"),
        Some(&byte) => {
            let name: Option<&'static str> = match byte {
                0x00 => Some("UNKNOWN"),
                0x01 => Some("ACL"),
                0x02 => Some("SCO"),
                0x03 => Some("LE_CONNECTED_ISOCHRONOUS_PDU"),
                0x04 => Some("LE_BROADCAST_ISOCHRONOUS_PDU"),
                0x05 => Some("DISCONNECT"),
                0x06 => Some("INQUIRY"),
                0x07 => Some("INQUIRY_RESPONSE"),
                0x0B => Some("LE_LEGACY_ADVERTISING_PDU"),
                0x37 => Some("LE_EXTENDED_ADVERTISING_PDU"),
                0x40 => Some("LE_PERIODIC_ADVERTISING_PDU"),
                0x0C => Some("LE_CONNECT"),
                0x0D => Some("LE_CONNECT_COMPLETE"),
                0x0E => Some("LE_SCAN"),
                0x0F => Some("LE_SCAN_RESPONSE"),
                0x10 => Some("PAGE"),
                0x11 => Some("PAGE_RESPONSE"),
                0x12 => Some("PAGE_REJECT"),
                0x1D => Some("REMOTE_NAME_REQUEST"),
                0x1E => Some("REMOTE_NAME_REQUEST_RESPONSE"),
                0x34 => Some("LMP"),
                0x41 => Some("LLCP"),
                _ => None,
            };
            name.map_or_else(|| Cow::Owned(format!("0x{byte:02X}")), Cow::Borrowed)
        }
    };

    #[derive(Serialize)]
    struct JsonSniffResponse<'a> {
        timestamp: &'a str,
        packet_type: Cow<'static, str>,
        baseband_packet: String,
        length: usize,
    }

    let out = JsonSniffResponse {
        timestamp: &timestamp,
        packet_type,
        baseband_packet: hex_packet,
        length: packet.len(),
    };

    if let Ok(json_str) = serde_json::to_string(&out) {
        writeln!(writer, "{json_str}")?;
    } else {
        writeln!(writer, "[{}] Baseband Packet: {}", timestamp, out.baseband_packet)?;
    }
    writer.flush()
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    const LE_META_EVENT_CODE: u8 = 0x3E;
    const LE_ADVERTISING_REPORT_SUBEVENT: u8 = 0x02;
    const SINGLE_REPORT: u8 = 0x01;
    const ADV_IND_EVENT: u8 = 0x00;
    const PUBLIC_ADDR_TYPE: u8 = 0x00;
    const ZERO_AD_DATA_LEN: u8 = 0x00;
    const SAMPLE_RSSI: u8 = 0xCE;

    const UUID_16_PAIR_FORMATTED: &str = "0x180d, 0x180f";
    const UUID_32_FORMATTED: &str = "0x12345678";
    const UUID_128_FORMATTED: &str = "0f0e0d0c-0b0a-0908-0706-050403020100";

    const SNIFF_DURATION_SECS: u64 = 1;
    const SNIFF_EXPECTED_TIMESTAMP: &str = "00:00:01";
    const SNIFF_ACL_OPCODE: u8 = 0x01;
    const SNIFF_ACL_NAME: &str = "ACL";
    const SNIFF_ACL_HEX: &str = "0102ff";
    const SNIFF_UNKNOWN_OPCODE: u8 = 0xFF;
    const SNIFF_UNKNOWN_NAME: &str = "0xFF";
    const SNIFF_UNKNOWN_HEX: &str = "ffab";
    const SNIFF_EMPTY_NAME: &str = "EMPTY";

    const UUID_16_BYTES_PAIR: [u8; 4] = [0x0d, 0x18, 0x0f, 0x18];
    const UUID_32_BYTES: [u8; 4] = [0x78, 0x56, 0x34, 0x12];
    const UUID_128_BYTES: [u8; 16] = [
        0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e,
        0x0f,
    ];

    const SERVICE_DATA_16_BYTES: [u8; 4] = [0x0d, 0x18, 0xaa, 0xbb];
    const SERVICE_DATA_128_BYTES: [u8; 18] = [
        0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e,
        0x0f, 0xde, 0xad,
    ];
    const SERVICE_DATA_16_EXPECTED: &str = "UUID: 0x180d, Data: aa bb";
    const SERVICE_DATA_32_EXPECTED: &str = "UUID: 0x12345678, Data: None";
    const SERVICE_DATA_128_EXPECTED: &str =
        "UUID: 0f0e0d0c-0b0a-0908-0706-050403020100, Data: de ad";
    const INCOMPLETE_16_BYTES: u8 = 0xfa;
    const INCOMPLETE_16_EXPECTED: &str = "fa";

    const SHORT_SERVICE_16_BYTES: u8 = 0x0d;
    const SHORT_SERVICE_32_BYTES: [u8; 3] = [0x01, 0x02, 0x03];
    const SHORT_SERVICE_128_BYTES: [u8; 2] = [0x04, 0x05];
    const SHORT_SERVICE_16_EXPECTED: &str = "0d";
    const SHORT_SERVICE_32_EXPECTED: &str = "01 02 03";
    const SHORT_SERVICE_128_EXPECTED: &str = "04 05";

    const VERBOSE_PARAM_LEN: u8 = 12;
    const VERBOSE_BD_ADDR: [u8; 6] = [0x11, 0x22, 0x33, 0x44, 0x55, 0x66];
    const VERBOSE_EXPECTED_HEX: &str = "04 3e 0c 02 01 00 00 11 22 33 44 55 66 00 ce";

    const SNIFF_ACL_PAYLOAD: [u8; 2] = [0x02, 0xff];
    const SNIFF_UNKNOWN_PAYLOAD: u8 = 0xab;

    fn sniff_json(packet: &[u8]) -> serde_json::Value {
        let mut buf = Vec::new();
        write_sniff_packet(&mut buf, packet, Duration::from_secs(SNIFF_DURATION_SECS)).unwrap();
        serde_json::from_slice(&buf).unwrap()
    }

    #[test]
    fn test_parse_ad_structures_uuid_lists() {
        let data = [
            5,
            GapDataType::COMPLETE_16BIT_UUIDS.0,
            UUID_16_BYTES_PAIR[0],
            UUID_16_BYTES_PAIR[1],
            UUID_16_BYTES_PAIR[2],
            UUID_16_BYTES_PAIR[3],
            5,
            GapDataType::INCOMPLETE_32BIT_UUIDS.0,
            UUID_32_BYTES[0],
            UUID_32_BYTES[1],
            UUID_32_BYTES[2],
            UUID_32_BYTES[3],
            17,
            GapDataType::COMPLETE_128BIT_UUIDS.0,
            UUID_128_BYTES[0],
            UUID_128_BYTES[1],
            UUID_128_BYTES[2],
            UUID_128_BYTES[3],
            UUID_128_BYTES[4],
            UUID_128_BYTES[5],
            UUID_128_BYTES[6],
            UUID_128_BYTES[7],
            UUID_128_BYTES[8],
            UUID_128_BYTES[9],
            UUID_128_BYTES[10],
            UUID_128_BYTES[11],
            UUID_128_BYTES[12],
            UUID_128_BYTES[13],
            UUID_128_BYTES[14],
            UUID_128_BYTES[15],
        ];

        let parsed = parse_ad_structures(&data, false);
        assert_eq!(
            parsed.get(&(GapDataType::COMPLETE_16BIT_UUIDS.0 as u16)).unwrap().decoded_val,
            UUID_16_PAIR_FORMATTED
        );
        assert_eq!(
            parsed.get(&(GapDataType::INCOMPLETE_32BIT_UUIDS.0 as u16)).unwrap().decoded_val,
            UUID_32_FORMATTED
        );
        assert_eq!(
            parsed.get(&(GapDataType::COMPLETE_128BIT_UUIDS.0 as u16)).unwrap().decoded_val,
            UUID_128_FORMATTED
        );
    }

    #[test]
    fn test_parse_ad_structures_service_data_variants_and_fallbacks() {
        let data = [
            5,
            GapDataType::SERVICE_DATA_16BIT.0,
            SERVICE_DATA_16_BYTES[0],
            SERVICE_DATA_16_BYTES[1],
            SERVICE_DATA_16_BYTES[2],
            SERVICE_DATA_16_BYTES[3],
            5,
            GapDataType::SERVICE_DATA_32BIT.0,
            UUID_32_BYTES[0],
            UUID_32_BYTES[1],
            UUID_32_BYTES[2],
            UUID_32_BYTES[3],
            19,
            GapDataType::SERVICE_DATA_128BIT.0,
            SERVICE_DATA_128_BYTES[0],
            SERVICE_DATA_128_BYTES[1],
            SERVICE_DATA_128_BYTES[2],
            SERVICE_DATA_128_BYTES[3],
            SERVICE_DATA_128_BYTES[4],
            SERVICE_DATA_128_BYTES[5],
            SERVICE_DATA_128_BYTES[6],
            SERVICE_DATA_128_BYTES[7],
            SERVICE_DATA_128_BYTES[8],
            SERVICE_DATA_128_BYTES[9],
            SERVICE_DATA_128_BYTES[10],
            SERVICE_DATA_128_BYTES[11],
            SERVICE_DATA_128_BYTES[12],
            SERVICE_DATA_128_BYTES[13],
            SERVICE_DATA_128_BYTES[14],
            SERVICE_DATA_128_BYTES[15],
            SERVICE_DATA_128_BYTES[16],
            SERVICE_DATA_128_BYTES[17],
            2,
            GapDataType::INCOMPLETE_16BIT_UUIDS.0,
            INCOMPLETE_16_BYTES,
        ];

        let parsed = parse_ad_structures(&data, false);
        assert_eq!(
            parsed.get(&(GapDataType::SERVICE_DATA_16BIT.0 as u16)).unwrap().decoded_val,
            SERVICE_DATA_16_EXPECTED
        );
        assert_eq!(
            parsed.get(&(GapDataType::SERVICE_DATA_32BIT.0 as u16)).unwrap().decoded_val,
            SERVICE_DATA_32_EXPECTED
        );
        assert_eq!(
            parsed.get(&(GapDataType::SERVICE_DATA_128BIT.0 as u16)).unwrap().decoded_val,
            SERVICE_DATA_128_EXPECTED
        );
        assert_eq!(
            parsed.get(&(GapDataType::INCOMPLETE_16BIT_UUIDS.0 as u16)).unwrap().decoded_val,
            INCOMPLETE_16_EXPECTED
        );
    }

    #[test]
    fn test_parse_ad_structures_short_service_data_falls_back_to_hex() {
        let data = [
            2,
            GapDataType::SERVICE_DATA_16BIT.0,
            SHORT_SERVICE_16_BYTES,
            4,
            GapDataType::SERVICE_DATA_32BIT.0,
            SHORT_SERVICE_32_BYTES[0],
            SHORT_SERVICE_32_BYTES[1],
            SHORT_SERVICE_32_BYTES[2],
            3,
            GapDataType::SERVICE_DATA_128BIT.0,
            SHORT_SERVICE_128_BYTES[0],
            SHORT_SERVICE_128_BYTES[1],
        ];

        let parsed = parse_ad_structures(&data, false);
        assert_eq!(
            parsed.get(&(GapDataType::SERVICE_DATA_16BIT.0 as u16)).unwrap().decoded_val,
            SHORT_SERVICE_16_EXPECTED
        );
        assert_eq!(
            parsed.get(&(GapDataType::SERVICE_DATA_32BIT.0 as u16)).unwrap().decoded_val,
            SHORT_SERVICE_32_EXPECTED
        );
        assert_eq!(
            parsed.get(&(GapDataType::SERVICE_DATA_128BIT.0 as u16)).unwrap().decoded_val,
            SHORT_SERVICE_128_EXPECTED
        );
    }

    #[test]
    fn test_parse_scan_packet_verbose_formats_raw_hex() {
        let packet = [
            H4_EVENT_PACKET_TYPE,
            LE_META_EVENT_CODE,
            VERBOSE_PARAM_LEN,
            LE_ADVERTISING_REPORT_SUBEVENT,
            SINGLE_REPORT,
            ADV_IND_EVENT,
            PUBLIC_ADDR_TYPE,
            VERBOSE_BD_ADDR[0],
            VERBOSE_BD_ADDR[1],
            VERBOSE_BD_ADDR[2],
            VERBOSE_BD_ADDR[3],
            VERBOSE_BD_ADDR[4],
            VERBOSE_BD_ADDR[5],
            ZERO_AD_DATA_LEN,
            SAMPLE_RSSI,
        ];
        let duration = Duration::from_secs(SNIFF_DURATION_SECS);

        let verbose_events = parse_scan_packet(&packet, duration, true);
        assert_eq!(verbose_events.len(), 1);
        assert_eq!(verbose_events[0].raw_hex, VERBOSE_EXPECTED_HEX);

        let quiet_events = parse_scan_packet(&packet, duration, false);
        assert_eq!(quiet_events.len(), 1);
        assert!(quiet_events[0].raw_hex.is_empty());
    }

    #[test]
    fn test_write_sniff_packet_empty_json() {
        let json = sniff_json(&[]);
        assert_eq!(json["timestamp"], SNIFF_EXPECTED_TIMESTAMP);
        assert_eq!(json["packet_type"], SNIFF_EMPTY_NAME);
        assert_eq!(json["baseband_packet"], "");
        assert_eq!(json["length"], 0);
    }

    #[test]
    fn test_write_sniff_packet_acl_json() {
        let packet = [SNIFF_ACL_OPCODE, SNIFF_ACL_PAYLOAD[0], SNIFF_ACL_PAYLOAD[1]];
        let json = sniff_json(&packet);
        assert_eq!(json["timestamp"], SNIFF_EXPECTED_TIMESTAMP);
        assert_eq!(json["packet_type"], SNIFF_ACL_NAME);
        assert_eq!(json["baseband_packet"], SNIFF_ACL_HEX);
        assert_eq!(json["length"], packet.len());
    }

    #[test]
    fn test_write_sniff_packet_unknown_opcode_json() {
        let packet = [SNIFF_UNKNOWN_OPCODE, SNIFF_UNKNOWN_PAYLOAD];
        let json = sniff_json(&packet);
        assert_eq!(json["timestamp"], SNIFF_EXPECTED_TIMESTAMP);
        assert_eq!(json["packet_type"], SNIFF_UNKNOWN_NAME);
        assert_eq!(json["baseband_packet"], SNIFF_UNKNOWN_HEX);
        assert_eq!(json["length"], packet.len());
    }
}
