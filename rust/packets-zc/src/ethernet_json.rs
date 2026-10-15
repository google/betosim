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

//! Provides JSON serialization and deserialization for Ethernet frames.
//!
//! This module defines `serde`-compatible structures that mirror the `zerocopy`
//! Ethernet structures from the `ethernet` module. It includes functions
//! for converting between these types and for serializing to/from JSON strings.
use crate::ethernet::{EthernetFrame, EthernetPacket, MacAddr};
use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;
use std::fmt;

pub fn to_json(ethernet_packet: &EthernetPacket, n: usize) -> Value {
    let (frame, maybe_vlan) = match ethernet_packet {
        EthernetPacket::Untagged { frame, .. } => (frame, None),
        EthernetPacket::Vlan { frame, vlan_header, .. } => (frame, Some(vlan_header)),
    };
    let mut eth = serde_json::to_value(parse_ethernet_header(frame, n)).unwrap();
    if let Some(vlan_header) = maybe_vlan {
        eth["vlan"] = serde_json::to_value(**vlan_header).unwrap();
    }
    eth
}

#[derive(Debug)]
pub enum JsonError {
    SerdeJsonError(serde_json::Error),
    MacAddrParseError(String),
}

impl fmt::Display for JsonError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            JsonError::SerdeJsonError(e) => write!(f, "JSON error: {}", e),
            JsonError::MacAddrParseError(s) => write!(f, "MAC address parse error: {}", s),
        }
    }
}

impl From<serde_json::Error> for JsonError {
    fn from(err: serde_json::Error) -> Self {
        JsonError::SerdeJsonError(err)
    }
}

/// A `serde`-compatible representation of a MAC address.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct JsonMacAddr(pub String);

impl From<MacAddr> for JsonMacAddr {
    fn from(mac: MacAddr) -> Self {
        JsonMacAddr(mac.to_string())
    }
}

impl TryFrom<JsonMacAddr> for MacAddr {
    type Error = JsonError;

    fn try_from(json_mac: JsonMacAddr) -> Result<Self, Self::Error> {
        json_mac.0.parse().map_err(JsonError::MacAddrParseError)
    }
}

/// Inner fields for `JsonEthernetFrame`, mimicking `tshark`'s `eth` object.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct JsonEthernetFrameFields {}

#[derive(Serialize)]
pub struct Ethernet {
    #[serde(rename = "eth.dst")]
    dst: String,
    #[serde(rename = "eth.dst_tree")]
    dst_tree: EthTree,
    #[serde(rename = "eth.src")]
    src: String,
    #[serde(rename = "eth.src_tree")]
    src_tree: EthTree,
    #[serde(rename = "eth.type")]
    ethertype: String,
    #[serde(rename = "eth.stream")]
    stream: usize,
}

#[derive(Serialize)]
pub struct EthTree {
    #[serde(rename = "eth.addr")]
    addr: String,
}

pub fn parse_ethernet_header(ethernet_frame: &EthernetFrame, n: usize) -> Ethernet {
    let dst_addr = ethernet_frame.dst_addr.to_string();
    let src_addr = ethernet_frame.src_addr.to_string();
    Ethernet {
        dst: dst_addr.clone(),
        dst_tree: EthTree { addr: dst_addr },
        src: src_addr.clone(),
        src_tree: EthTree { addr: src_addr },
        ethertype: format!("{:#06x}", ethernet_frame.ethertype.get()),
        stream: n,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ethernet::{MacAddr, ether_type};
    use zerocopy::IntoBytes;

    #[test]
    fn test_json_mac_addr_conversion() {
        let mac_str = "01:02:03:04:05:06";
        let mac_addr = mac_str.parse::<MacAddr>().unwrap();

        // Test From<MacAddr> for JsonMacAddr
        let json_mac: JsonMacAddr = mac_addr.into();
        assert_eq!(json_mac.0, mac_str);

        // Test TryFrom<JsonMacAddr> for MacAddr
        let converted_mac: MacAddr = json_mac.try_into().unwrap();
        assert_eq!(converted_mac, mac_addr);
    }

    #[test]
    fn test_invalid_json_mac_addr() {
        let json_mac = JsonMacAddr("invalid-mac-address".to_string());
        let result: Result<MacAddr, _> = json_mac.try_into();
        assert!(result.is_err());
    }

    #[test]
    fn test_to_json_untagged_packet() {
        let dst_addr: MacAddr = "11:22:33:44:55:66".parse().unwrap();
        let src_addr: MacAddr = "aa:bb:cc:dd:ee:ff".parse().unwrap();
        let ethertype = ether_type::IPV4;
        let payload_data = [1, 2, 3, 4];

        let mut buffer = Vec::new();
        buffer.extend_from_slice(dst_addr.as_bytes());
        buffer.extend_from_slice(src_addr.as_bytes());
        buffer.extend_from_slice(&ethertype.to_be_bytes());
        buffer.extend_from_slice(&payload_data);

        let packet = EthernetPacket::parse(&buffer).unwrap();
        let json = to_json(&packet, 10);

        assert_eq!(json["eth.dst"], "11:22:33:44:55:66");
        assert_eq!(json["eth.src"], "AA:BB:CC:DD:EE:FF");
        assert_eq!(json["eth.type"], "0x0800");
        assert_eq!(json["eth.stream"], 10);
        assert!(json["vlan"].is_null());
    }

    #[test]
    fn test_to_json_vlan_packet() {
        let dst_addr: MacAddr = "11:22:33:44:55:66".parse().unwrap();
        let src_addr: MacAddr = "aa:bb:cc:dd:ee:ff".parse().unwrap();
        let vlan_ethertype = ether_type::VLAN;
        let inner_ethertype = ether_type::IPV4;
        let tci = 0x1234u16;
        let payload_data = [5, 6, 7, 8];

        let mut buffer = Vec::new();
        buffer.extend_from_slice(dst_addr.as_bytes());
        buffer.extend_from_slice(src_addr.as_bytes());
        buffer.extend_from_slice(&vlan_ethertype.to_be_bytes());
        buffer.extend_from_slice(&tci.to_be_bytes());
        buffer.extend_from_slice(&inner_ethertype.to_be_bytes());
        buffer.extend_from_slice(&payload_data);

        let packet = EthernetPacket::parse(&buffer).unwrap();
        let json = to_json(&packet, 20);

        assert_eq!(json["eth.dst"], "11:22:33:44:55:66");
        assert_eq!(json["eth.src"], "AA:BB:CC:DD:EE:FF");
        assert_eq!(json["eth.type"], "0x8100");
        assert_eq!(json["eth.stream"], 20);
        assert!(!json["vlan"].is_null());
    }
}
