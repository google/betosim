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

use std::collections::BTreeMap;

use common::util::time_display::TimeDisplay;
use netsim_packets::{
    GapDataType, HciEvent, LeAdvertisingEventType, LeMetaEvent, OwnAddressType, parse_hci_event,
};
use netsim_proto::ble_service::{ScanResponse, SniffResponse};
use serde::Serialize;

const H4_EVENT_PACKET_TYPE: u8 = 0x04;
const SCAN_RSP_KEY_OFFSET: u16 = 0x0100;

pub(crate) struct BleEvent {
    pub timestamp_str: String,
    pub addr_display: String,
    pub event_type_str: String,
    pub rssi: i8,
    pub data_len: usize,
    pub raw_hex: String,
    pub ad_payloads: BTreeMap<u16, AdPayload>,
}

#[derive(Clone)]
pub(crate) struct AdPayload {
    pub ad_type: u8,
    pub type_str: String,
    pub decoded_val: String,
}

pub(crate) fn parse_scan_response(res: &ScanResponse, verbose: bool) -> Vec<BleEvent> {
    let mut events = Vec::new();
    let now =
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    let timestamp = TimeDisplay::from_duration(now).utc_display_hms();

    let packet = &res.packet;
    let hex_packet = if verbose {
        packet.iter().map(|b| format!("{:02x}", b)).collect::<Vec<_>>().join(" ")
    } else {
        String::new()
    };

    let payload = match packet.as_slice() {
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
                let mut uuids = Vec::new();
                for &[b0, b1] in ad_data.as_chunks::<2>().0 {
                    uuids.push(format!("0x{:02x}{:02x}", b1, b0));
                }
                uuids.join(", ")
            }
            GapDataType::INCOMPLETE_32BIT_UUIDS | GapDataType::COMPLETE_32BIT_UUIDS
                if ad_data.len().is_multiple_of(4) =>
            {
                let mut uuids = Vec::new();
                for &[b0, b1, b2, b3] in ad_data.as_chunks::<4>().0 {
                    uuids.push(format!("0x{:02x}{:02x}{:02x}{:02x}", b3, b2, b1, b0));
                }
                uuids.join(", ")
            }
            GapDataType::INCOMPLETE_128BIT_UUIDS | GapDataType::COMPLETE_128BIT_UUIDS
                if ad_data.len().is_multiple_of(16) =>
            {
                let mut uuids = Vec::new();
                for chunk in ad_data.as_chunks::<16>().0 {
                    uuids.push(format_128bit_uuid(chunk));
                }
                uuids.join(", ")
            }
            GapDataType::SERVICE_DATA_16BIT => {
                if let [b0, b1, rest_data @ ..] = ad_data {
                    let uuid = format!("0x{:02x}{:02x}", b1, b0);
                    format_uuid_service_data(uuid, rest_data)
                } else {
                    format_hex_data(ad_data)
                }
            }
            GapDataType::SERVICE_DATA_32BIT => {
                if let [b0, b1, b2, b3, rest_data @ ..] = ad_data {
                    let uuid = format!("0x{:02x}{:02x}{:02x}{:02x}", b3, b2, b1, b0);
                    format_uuid_service_data(uuid, rest_data)
                } else {
                    format_hex_data(ad_data)
                }
            }
            GapDataType::SERVICE_DATA_128BIT => {
                if let Some((uuid_chunk, rest_data)) = ad_data.split_first_chunk::<16>() {
                    format_uuid_service_data(format_128bit_uuid(uuid_chunk), rest_data)
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

fn format_128bit_uuid(uuid: &[u8; 16]) -> String {
    let &[b0, b1, b2, b3, b4, b5, b6, b7, b8, b9, b10, b11, b12, b13, b14, b15] = uuid;
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        b15, b14, b13, b12, b11, b10, b9, b8, b7, b6, b5, b4, b3, b2, b1, b0
    )
}

fn format_uuid_service_data(uuid: String, data: &[u8]) -> String {
    let data_str = if data.is_empty() { String::from("None") } else { format_hex_data(data) };
    format!("UUID: {}, Data: {}", uuid, data_str)
}

fn format_hex_data(data: &[u8]) -> String {
    data.iter().map(|b| format!("{:02x}", b)).collect::<Vec<_>>().join(" ")
}

pub(crate) fn print_json_event(event: &BleEvent, verbose: bool) {
    use serde::Serialize;

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
        Ok(json_str) => println!("{}", json_str),
        Err(e) => eprintln!("Failed to serialize BLE event to JSON: {}", e),
    }
}

pub(crate) fn print_sniff_response(res: &SniffResponse, _verbose: bool) {
    let duration = std::time::Duration::from_micros(res.timestamp as u64);
    let timestamp = TimeDisplay::from_duration(duration).utc_display_hms();
    let hex_packet = res.packet.iter().map(|b| format!("{:02x}", b)).collect::<Vec<_>>().join("");

    let packet_type = match res.packet.first() {
        None => "EMPTY".to_string(),
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
            name.map(str::to_string).unwrap_or_else(|| format!("0x{byte:02X}"))
        }
    };

    #[derive(Serialize)]
    struct JsonSniffResponse {
        timestamp: String,
        packet_type: String,
        baseband_packet: String,
        length: usize,
    }

    let out = JsonSniffResponse {
        timestamp: timestamp.to_string(),
        packet_type,
        baseband_packet: hex_packet,
        length: res.packet.len(),
    };

    if let Ok(json_str) = serde_json::to_string(&out) {
        println!("{}", json_str);
    } else {
        println!("[{}] Baseband Packet: {}", timestamp, out.baseband_packet);
    }
}
