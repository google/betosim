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

//! Provides JSON serialization and deserialization for LLC and SNAP headers.
//!
//! This module defines `serde`-compatible structures that mirror the `zerocopy`
//! LLC and SNAP structures from the `llc` module. It includes functions
//! for converting between these types and for serializing to/from JSON strings.

use std::{fmt, num::ParseIntError};

use serde::{Deserialize, Serialize};

use crate::llc::{LlcHeader, LlcSnapHeader, SnapHeader};

/// A custom error type for JSON operations and conversions related to LLC/SNAP.
#[derive(Debug)]
pub enum JsonError {
    SerdeJson(serde_json::Error),
    /// Indicates an error during conversion from a JSON representation to a
    /// zerocopy type.
    HexParse(String),
    /// Required field missing for conversion (e.g. OUI for SNAP).
    Conversion(String),
}

impl fmt::Display for JsonError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            JsonError::SerdeJson(e) => {
                write!(f, "JSON serialization/deserialization error: {}", e)
            }
            JsonError::HexParse(s) => write!(f, "Hex parsing error: {}", s),
            JsonError::Conversion(s) => write!(f, "Conversion error: {}", s),
        }
    }
}

impl std::error::Error for JsonError {}

impl From<serde_json::Error> for JsonError {
    fn from(err: serde_json::Error) -> Self {
        JsonError::SerdeJson(err)
    }
}

impl From<ParseIntError> for JsonError {
    fn from(err: ParseIntError) -> Self {
        JsonError::HexParse(err.to_string())
    }
}

/// Represents the fields within the "llc" JSON object, compatible with
/// `tshark`.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct JsonLlcFields {
    #[serde(rename = "llc.dsap")]
    pub dsap: u8,
    #[serde(rename = "llc.dsap.str", skip_serializing_if = "Option::is_none")]
    pub dsap_str: Option<String>,
    #[serde(rename = "llc.ssap")]
    pub ssap: u8,
    #[serde(rename = "llc.ssap.str", skip_serializing_if = "Option::is_none")]
    pub ssap_str: Option<String>,
    #[serde(rename = "llc.control")]
    pub control: u8,
    #[serde(rename = "llc.control.str", skip_serializing_if = "Option::is_none")]
    pub control_str: Option<String>,

    // SNAP fields, if present, integrated into the LLC layer as per tshark style
    #[serde(rename = "llc.oui", skip_serializing_if = "Option::is_none")]
    pub oui: Option<String>, // OUI as hex string, e.g., "0x00000c"
    #[serde(rename = "llc.type", skip_serializing_if = "Option::is_none")]
    pub snap_pid: Option<u16>, // Protocol ID from SNAP, tshark uses "llc.type"
    #[serde(rename = "llc.type.str", skip_serializing_if = "Option::is_none")]
    pub snap_pid_str: Option<String>,
    // e.g., "IPv4" or "CDP"
}

/// Top-level structure for serialization, creating the "llc" key.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct JsonLlc {
    pub llc: JsonLlcFields,
}

impl From<&LlcSnapHeader> for JsonLlc {
    fn from(header: &LlcSnapHeader) -> Self {
        let llc_fields = JsonLlcFields {
            dsap: header.llc.dsap,
            dsap_str: None,
            ssap: header.llc.ssap,
            ssap_str: None,
            control: header.llc.control,
            control_str: None,
            oui: Some(if header.snap.oui == [0, 0, 0] {
                "0".to_string()
            } else {
                let [o0, o1, o2] = header.snap.oui;
                format!("0x{o0:02x}{o1:02x}{o2:02x}")
            }),
            snap_pid: Some(header.snap.protocol_id.get()),
            snap_pid_str: None,
        };
        JsonLlc { llc: llc_fields }
    }
}

impl From<&LlcHeader> for JsonLlc {
    fn from(header: &LlcHeader) -> Self {
        let llc_fields = JsonLlcFields {
            dsap: header.dsap,
            dsap_str: None, // llc_util::sap_to_string(header.dsap) is not in tshark output
            ssap: header.ssap,
            ssap_str: None,
            control: header.control,
            control_str: None,
            oui: None,
            snap_pid: None,
            snap_pid_str: None,
        };
        JsonLlc { llc: llc_fields }
    }
}

impl TryFrom<&JsonLlc> for LlcSnapHeader {
    type Error = JsonError;
    fn try_from(json_llc_layer: &JsonLlc) -> Result<Self, Self::Error> {
        let fields = &json_llc_layer.llc;
        let llc_header = LlcHeader::new(fields.dsap, fields.ssap, fields.control);

        let oui_str = fields
            .oui
            .as_deref()
            .ok_or_else(|| JsonError::Conversion("Missing llc.oui for SNAP".to_string()))?;
        let snap_pid_val = fields
            .snap_pid
            .ok_or_else(|| JsonError::Conversion("Missing llc.type for SNAP".to_string()))?;

        let Some(oui_hex_part) = oui_str.strip_prefix("0x") else {
            return Err(JsonError::HexParse(format!(
                "Invalid OUI format: {oui_str}, expected 0xXXXXXX"
            )));
        };

        if oui_hex_part.len() != 6 || oui_hex_part.starts_with('+') {
            return Err(JsonError::HexParse(format!(
                "Invalid OUI format: {oui_str}, expected 0xXXXXXX"
            )));
        }

        let [_, o0, o1, o2] = u32::from_str_radix(oui_hex_part, 16)?.to_be_bytes();
        let snap_header = SnapHeader::new([o0, o1, o2], snap_pid_val);

        Ok(LlcSnapHeader { llc: llc_header, snap: snap_header })
    }
}

/// Serializes an `LlcSnapHeader` to a JSON Value.
pub fn to_json_snap(header: &LlcSnapHeader) -> serde_json::Value {
    serde_json::to_value(JsonLlc::from(header)).unwrap_or(serde_json::Value::Null)
}

/// Serializes an `LlcHeader` to a JSON Value.
pub fn to_json_llc(header: &LlcHeader) -> serde_json::Value {
    serde_json::to_value(JsonLlc::from(header)).unwrap_or(serde_json::Value::Null)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        ethernet::ether_type,
        llc::{control_field, sap},
    };

    #[test]
    fn test_llc_snap_header_json_serialization_deserialization() {
        let original_header = LlcSnapHeader::new(
            sap::SNAP,
            sap::SNAP,
            control_field::UI, // 0x03
            [0x00, 0x80, 0xC2],
            ether_type::IPV4, // Using an EtherType for PID example
        );

        let json_llc_layer = JsonLlc::from(&original_header);
        let json_string = serde_json::to_string_pretty(&json_llc_layer).unwrap();
        println!("JSON: {}", json_string);

        let parsed_json_llc: JsonLlc = serde_json::from_str(&json_string).unwrap();
        let deserialized_header = LlcSnapHeader::try_from(&parsed_json_llc).unwrap();

        assert_eq!(deserialized_header.llc.dsap, original_header.llc.dsap);
        assert_eq!(deserialized_header.llc.ssap, original_header.llc.ssap);
        assert_eq!(deserialized_header.llc.control, original_header.llc.control);
        assert_eq!(deserialized_header.snap.oui, original_header.snap.oui);
        assert_eq!(
            deserialized_header.snap.protocol_id.get(),
            original_header.snap.protocol_id.get()
        );

        // Check string representations in the JSON structure
        let parsed_json_value: serde_json::Value = serde_json::from_str(&json_string).unwrap();
        let llc_layer = parsed_json_value.get("llc").expect("JSON should have 'llc' key");

        assert_eq!(llc_layer["llc.dsap"].as_u64().unwrap(), u64::from(sap::SNAP));
        assert!(llc_layer.get("llc.dsap.str").is_none());
        assert_eq!(llc_layer["llc.ssap"].as_u64().unwrap(), u64::from(sap::SNAP));
        assert!(llc_layer.get("llc.ssap.str").is_none());
        assert_eq!(llc_layer["llc.control"].as_u64().unwrap(), u64::from(control_field::UI));
        assert!(llc_layer.get("llc.control.str").is_none());
        assert_eq!(llc_layer["llc.oui"].as_str().unwrap(), "0x0080c2");
        assert_eq!(llc_layer["llc.type"].as_u64().unwrap(), u64::from(ether_type::IPV4));
        assert!(llc_layer.get("llc.type.str").is_none());
    }

    #[test]
    fn test_invalid_oui_format_deserialize() {
        // Test tshark-like OUI format "0xXXXXXX" but with wrong length
        let json_str = r#"{
            "llc": {
                "llc.dsap": 170, "llc.dsap.str": "SNAP",
                "llc.ssap": 170, "llc.ssap.str": "SNAP",
                "llc.control": 3, "llc.control.str": "UI",
                "llc.oui": "0x0080C", "llc.type": 2048, "llc.type.str": "IPv4"
            }
        }"#; // Invalid OUI length
        let json_llc_layer: Result<JsonLlc, _> = serde_json::from_str(json_str);
        assert!(
            json_llc_layer.is_err() || LlcSnapHeader::try_from(&json_llc_layer.unwrap()).is_err()
        );

        // Test OUI format without "0x" prefix
        let json_str_no_prefix = r#"{
            "llc": {
                "llc.dsap": 170, "llc.dsap.str": "SNAP",
                "llc.ssap": 170, "llc.ssap.str": "SNAP",
                "llc.control": 3, "llc.control.str": "UI",
                "llc.oui": "0080C2", "llc.type": 2048, "llc.type.str": "IPv4"
            }
        }"#;
        let json_llc_layer: Result<JsonLlc, _> = serde_json::from_str(json_str_no_prefix);
        assert!(
            json_llc_layer.is_err() || LlcSnapHeader::try_from(&json_llc_layer.unwrap()).is_err()
        );
    }
}
