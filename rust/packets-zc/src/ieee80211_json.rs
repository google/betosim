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

//! Provides JSON serialization and deserialization for IEEE 802.11 MAC headers.
//!
//! This module defines `serde`-compatible structures that mirror the `zerocopy`
//! IEEE 802.11 structures from the `ieee80211` module. It includes functions
//! for converting between these types and for serializing to/from JSON strings.

use crate::ieee80211::{FrameControl, MacHeader3Addr, SequenceControl};
use crate::ieee80211_util; // For address interpretation
use serde::{Deserialize, Serialize};
use std::fmt;
use zerocopy::U16;
use zerocopy::byteorder::LittleEndian;

/// A custom error type for JSON operations and conversions related to IEEE 802.11.
#[derive(Debug)]
pub enum JsonError {
    SerdeJsonError(serde_json::Error),
    /// Indicates an error during conversion from a JSON representation to a zerocopy type.
    HexParseError(String),
    ConversionError(String),
}

impl fmt::Display for JsonError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            JsonError::SerdeJsonError(e) => {
                write!(f, "JSON serialization/deserialization error: {}", e)
            }
            JsonError::HexParseError(s) => write!(f, "Hex parsing error: {}", s),
            JsonError::ConversionError(s) => write!(f, "Conversion error: {}", s),
        }
    }
}

impl std::error::Error for JsonError {}

impl From<crate::ethernet_json::JsonError> for JsonError {
    fn from(err: crate::ethernet_json::JsonError) -> Self {
        JsonError::ConversionError(err.to_string())
    }
}

impl From<serde_json::Error> for JsonError {
    fn from(err: serde_json::Error) -> Self {
        JsonError::SerdeJsonError(err)
    }
}

/// A `serde`-compatible, `tshark`-like representation of an IEEE 802.11 Frame Control field.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct JsonFrameControl {
    #[serde(rename = "wlan.fc.type")]
    pub r#type: u8,
    #[serde(rename = "wlan.fc.subtype")]
    pub subtype: u8,
    #[serde(rename = "wlan.fc.tods")]
    pub to_ds: bool,
    #[serde(rename = "wlan.fc.fromds")]
    pub from_ds: bool,
    #[serde(rename = "wlan.fc.morefrag")]
    pub more_fragments: bool,
    #[serde(rename = "wlan.fc.retry")]
    pub retry: bool,
    #[serde(rename = "wlan.fc.pwrmgt")]
    pub power_management: bool,
    #[serde(rename = "wlan.fc.moredata")]
    pub more_data: bool,
    #[serde(rename = "wlan.fc.protected")]
    pub protected_frame: bool,
    #[serde(rename = "wlan.fc.order")]
    pub order: bool,
    #[serde(rename = "wlan.fc.field_hex")] // tshark often uses wlan.fc
    pub field_hex: String,
}

impl From<FrameControl> for JsonFrameControl {
    fn from(fc: FrameControl) -> Self {
        JsonFrameControl {
            r#type: fc.frame_type(),
            subtype: fc.frame_subtype(),
            to_ds: fc.to_ds(),
            from_ds: fc.from_ds(),
            more_fragments: fc.more_fragments(),
            retry: fc.retry(),
            power_management: fc.power_management(),
            more_data: fc.more_data(),
            protected_frame: fc.protected_frame(),
            order: fc.order(),
            field_hex: format!("{:04x}", fc.get()),
        }
    }
}

impl TryFrom<JsonFrameControl> for FrameControl {
    type Error = JsonError;

    fn try_from(json_fc: JsonFrameControl) -> Result<Self, Self::Error> {
        let fc_val = u16::from_str_radix(&json_fc.field_hex, 16)
            .map_err(|_| JsonError::HexParseError(json_fc.field_hex))?;
        Ok(FrameControl::new(fc_val))
    }
}

/// A `serde`-compatible, `tshark`-like representation of an IEEE 802.11 Sequence Control field.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct JsonSequenceControl {
    #[serde(rename = "wlan.seq")]
    pub sequence_number: u16,
    #[serde(rename = "wlan.frag")]
    pub fragment_number: u8,
    #[serde(rename = "wlan.sc.field_hex")] // tshark might use wlan.sc
    pub field_hex: String,
}

impl From<SequenceControl> for JsonSequenceControl {
    fn from(sc: SequenceControl) -> Self {
        JsonSequenceControl {
            sequence_number: sc.sequence_number(),
            fragment_number: sc.fragment_number(),
            field_hex: format!("{:04x}", sc.get()),
        }
    }
}

impl TryFrom<JsonSequenceControl> for SequenceControl {
    type Error = JsonError;

    fn try_from(json_sc: JsonSequenceControl) -> Result<Self, Self::Error> {
        let sc_val = u16::from_str_radix(&json_sc.field_hex, 16)
            .map_err(|_| JsonError::HexParseError(json_sc.field_hex))?;
        Ok(SequenceControl::new(sc_val))
    }
}

/// Inner fields for `JsonMacHeader3Addr`, mimicking `tshark`'s `wlan` object.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct JsonMacHeader3AddrFields {
    #[serde(flatten)]
    pub frame_control: JsonFrameControl,
    #[serde(rename = "wlan.duration")]
    pub duration_id: u16,
    #[serde(rename = "wlan.ra")]
    pub ra: String,
    #[serde(rename = "wlan.ta")]
    pub ta: String,
    #[serde(rename = "wlan.da")]
    pub da: String,
    #[serde(rename = "wlan.sa")]
    pub sa: String,
    #[serde(rename = "wlan.bssid", skip_serializing_if = "Option::is_none")]
    pub bssid: Option<String>,
    // Raw addresses for easier reconstruction and direct tshark comparison
    #[serde(rename = "wlan.addr1")]
    pub addr1: String,
    #[serde(rename = "wlan.addr2")]
    pub addr2: String,
    #[serde(rename = "wlan.addr3")]
    pub addr3: String,
    #[serde(flatten)]
    pub sequence_control: JsonSequenceControl,
}

/// A `serde`-compatible, `tshark`-like representation of an IEEE 802.11 MAC header with 3 addresses.
/// This structure creates the top-level "wlan" key.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct JsonMacHeader3Addr {
    pub wlan: JsonMacHeader3AddrFields,
}

impl From<&MacHeader3Addr> for JsonMacHeader3Addr {
    fn from(header: &MacHeader3Addr) -> Self {
        let fields = JsonMacHeader3AddrFields {
            frame_control: header.frame_control.into(),
            duration_id: header.duration_id.get(),
            ra: ieee80211_util::get_receiver_address(header).to_string(),
            ta: ieee80211_util::get_transmitter_address(header).to_string(),
            da: ieee80211_util::get_destination_address(header).to_string(),
            sa: ieee80211_util::get_source_address(header).to_string(),
            bssid: ieee80211_util::get_bssid(header).map(|a| a.to_string()),
            addr1: header.addr1.to_string(),
            addr2: header.addr2.to_string(),
            addr3: header.addr3.to_string(),
            sequence_control: header.sequence_control.into(),
        };
        JsonMacHeader3Addr { wlan: fields }
    }
}

impl TryFrom<&JsonMacHeader3Addr> for MacHeader3Addr {
    type Error = JsonError;

    fn try_from(json_header: &JsonMacHeader3Addr) -> Result<Self, Self::Error> {
        let wlan_fields = &json_header.wlan;
        Ok(MacHeader3Addr {
            frame_control: wlan_fields.frame_control.clone().try_into()?,
            duration_id: U16::<LittleEndian>::new(wlan_fields.duration_id),
            addr1: wlan_fields.addr1.parse().map_err(JsonError::ConversionError)?,
            addr2: wlan_fields.addr2.parse().map_err(JsonError::ConversionError)?,
            addr3: wlan_fields.addr3.parse().map_err(JsonError::ConversionError)?,
            sequence_control: wlan_fields.sequence_control.clone().try_into()?,
        })
    }
}

/// Serializes a `MacHeader3Addr` to a JSON string.
pub fn to_json_string(header: &MacHeader3Addr) -> Result<String, serde_json::Error> {
    let json_header = JsonMacHeader3Addr::from(header);
    serde_json::to_string_pretty(&json_header)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ethernet::MacAddr;

    #[test]
    fn test_frame_control_conversion() {
        let fc = FrameControl::new(0x0108); // Type: Data, Subtype: Data, ToDS=1
        let json_fc: JsonFrameControl = fc.into();
        assert_eq!(json_fc.r#type, 2);
        assert_eq!(json_fc.subtype, 0);
        assert!(json_fc.to_ds);
        assert!(!json_fc.from_ds);
        assert!(!json_fc.protected_frame);
        assert_eq!(json_fc.field_hex, "0108");

        let recon_fc: FrameControl = json_fc.try_into().unwrap();
        assert_eq!(recon_fc.get(), 0x0108);
    }

    #[test]
    fn test_sequence_control_conversion() {
        let sc = SequenceControl::new(0x1234); // Frag: 4, Seq: 291
        let json_sc: JsonSequenceControl = sc.into();
        assert_eq!(json_sc.fragment_number, 4);
        assert_eq!(json_sc.sequence_number, 291);
        assert_eq!(json_sc.field_hex, "1234");

        let recon_sc: SequenceControl = json_sc.try_into().unwrap();
        assert_eq!(recon_sc.get(), 0x1234);
    }

    #[test]
    fn test_mac_header_3_addr_conversion() {
        let header = MacHeader3Addr {
            frame_control: FrameControl::new(0x0008), // Type: Mgmt, Subtype: Beacon
            duration_id: U16::new(0),
            addr1: "ff:ff:ff:ff:ff:ff".parse().unwrap(), // DA
            addr2: "00:11:22:33:44:55".parse().unwrap(), // SA
            addr3: "00:11:22:33:44:55".parse().unwrap(), // BSSID
            sequence_control: SequenceControl::new(0),
        };

        let json_header = JsonMacHeader3Addr::from(&header);
        assert_eq!(json_header.wlan.frame_control.r#type, 2);
        assert_eq!(json_header.wlan.frame_control.subtype, 0);
        assert_eq!(json_header.wlan.addr1, "FF:FF:FF:FF:FF:FF");
        assert_eq!(json_header.wlan.addr2, "00:11:22:33:44:55");
        assert_eq!(json_header.wlan.bssid, Some("00:11:22:33:44:55".to_string()));

        let recon_header: MacHeader3Addr = (&json_header).try_into().unwrap();
        assert_eq!(recon_header.frame_control.get(), 0x0008);
        assert_eq!(recon_header.addr1, "ff:ff:ff:ff:ff:ff".parse::<MacAddr>().unwrap());
    }

    #[test]
    fn test_to_json_string() {
        let header = MacHeader3Addr {
            frame_control: FrameControl::new(0x0008),
            duration_id: U16::new(0),
            addr1: "ff:ff:ff:ff:ff:ff".parse().unwrap(),
            addr2: "00:11:22:33:44:55".parse().unwrap(),
            addr3: "00:11:22:33:44:55".parse().unwrap(),
            sequence_control: SequenceControl::new(0),
        };
        let json_string = to_json_string(&header).unwrap();
        assert!(json_string.contains("\"wlan.addr1\": \"FF:FF:FF:FF:FF:FF\""));
    }
}
