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

//! Provides JSON serialization and deserialization for `nl80211` Netlink
//! attributes.
//!
//! This module is designed to facilitate the debugging and logging of `nl80211`
//! messages exchanged between a user space daemon and the `mac80211_hwsim`
//! kernel module. By converting the binary Netlink attribute format to a
//! human-readable JSON format (and back), it allows for easier inspection of
//! the commands and data being sent to the simulated WiFi device.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::netlink::{nl80211_attr::NlAttrHdr, nl80211_util};

/// A custom error type for JSON operations related to mac80211_hwsim Netlink
/// attributes.
#[derive(Debug)]
pub enum JsonError {
    SerdeJson(serde_json::Error),
    HexParse(hex::FromHexError),
    Conversion(String),
}

impl fmt::Display for JsonError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            JsonError::SerdeJson(e) => {
                write!(f, "JSON serialization/deserialization error: {}", e)
            }
            JsonError::HexParse(e) => write!(f, "Hex parsing error: {}", e),
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

impl From<hex::FromHexError> for JsonError {
    fn from(err: hex::FromHexError) -> Self {
        JsonError::HexParse(err)
    }
}

/// A `serde`-compatible representation of an `NlAttrHdr`.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct JsonNlAttrHdr {
    #[serde(rename = "nla.len")]
    pub nla_len: u16,
    #[serde(rename = "nla.type")]
    pub nla_type_raw: u16, // Raw type including flags
    #[serde(rename = "nla.type_id")]
    pub nla_type_id: u16, // Type ID without flags
    #[serde(rename = "nla.type_name")]
    pub nla_type_name: String, // Human-readable type name
    #[serde(rename = "nla.is_nested")]
    pub nla_is_nested: bool,
}

impl From<&NlAttrHdr> for JsonNlAttrHdr {
    fn from(hdr: &NlAttrHdr) -> Self {
        let type_raw = hdr.attr_type();
        let type_id = nl80211_util::get_attr_id_from_type(type_raw);
        JsonNlAttrHdr {
            nla_len: hdr.length(),
            nla_type_raw: type_raw,
            nla_type_id: type_id,
            nla_type_name: nl80211_util::attr_id_to_string(type_id),
            nla_is_nested: nl80211_util::is_attr_nested(type_raw),
        }
    }
}

impl TryFrom<&JsonNlAttrHdr> for NlAttrHdr {
    type Error = JsonError;
    fn try_from(json_hdr: &JsonNlAttrHdr) -> Result<Self, Self::Error> {
        // We primarily use nla_len and nla_type_raw for reconstruction.
        // The other fields are for informational purposes in JSON.
        Ok(NlAttrHdr::new(json_hdr.nla_len, json_hdr.nla_type_raw))
    }
}

#[cfg(test)]
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct JsonNlAttributeFields {
    #[serde(flatten)]
    pub header: JsonNlAttrHdr,
    #[serde(rename = "nla.payload_hex")]
    pub payload_hex: String,
}

#[cfg(test)]
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct JsonNlAttribute {
    #[serde(rename = "nl_attr")]
    pub fields: JsonNlAttributeFields,
}

#[cfg(test)]
impl JsonNlAttribute {
    /// Creates a `JsonNlAttribute` from an `NlAttrHdr` and its payload bytes.
    pub fn from_parts(hdr: &NlAttrHdr, payload: &[u8]) -> Self {
        JsonNlAttribute {
            fields: JsonNlAttributeFields {
                header: JsonNlAttrHdr::from(hdr), // Use direct function call from hex crate
                payload_hex: hex::encode_upper(payload),
            },
        }
    }

    /// Tries to convert this `JsonNlAttribute` back to `NlAttrHdr` and payload
    /// `Vec<u8>`.
    pub fn try_into_parts(&self) -> Result<(NlAttrHdr, Vec<u8>), JsonError> {
        let hdr = NlAttrHdr::try_from(&self.fields.header)?;
        let payload = hex::decode(&self.fields.payload_hex)?; // Use direct function call
        Ok((hdr, payload))
    }
}

#[cfg(test)]
pub fn to_json_string(hdr: &NlAttrHdr, payload: &[u8]) -> Result<String, JsonError> {
    let json_attr = JsonNlAttribute::from_parts(hdr, payload);
    serde_json::to_string_pretty(&json_attr).map_err(JsonError::from)
}

#[cfg(test)]
pub fn from_json_string(json_str: &str) -> Result<(NlAttrHdr, Vec<u8>), JsonError> {
    let json_attr: JsonNlAttribute = serde_json::from_str(json_str)?;
    json_attr.try_into_parts()
}

/// Serializes a full Netlink packet (header + payload) to a JSON Value.
/// This mimics tshark's output for a Netlink message.
pub fn packet_to_json(packet: &[u8]) -> serde_json::Value {
    use crate::netlink::frame::NlMsgHdr;

    let mut layers = serde_json::Map::new();

    // Parse Netlink Header
    if let Ok((hdr, _payload)) = zerocopy::Ref::<&[u8], NlMsgHdr>::from_prefix(packet) {
        let nlmsg_type = hdr.nlmsg_type;
        let nlmsg_len = hdr.nlmsg_len;
        let nlmsg_flags = hdr.nlmsg_flags;
        let nlmsg_seq = hdr.nlmsg_seq;
        let nlmsg_pid = hdr.nlmsg_pid;

        let mut genl_map = serde_json::Map::new();
        genl_map.insert(
            "genl.cmd".to_string(),
            serde_json::Value::String(format!("0x{:02x}", nlmsg_type)),
        ); // Placeholder
        // Tshark output for Netlink is complex and depends on protocol (Generic
        // Netlink, Route, etc.) For our test, we might just want to dump the
        // header fields and payload.

        // Let's create a "netlink" layer
        let mut nl_map = serde_json::Map::new();
        nl_map.insert("netlink.len".to_string(), serde_json::Value::Number(nlmsg_len.into()));
        nl_map.insert("netlink.type".to_string(), serde_json::Value::Number(nlmsg_type.into()));
        nl_map.insert(
            "netlink.flags".to_string(),
            serde_json::Value::String(format!("0x{:04x}", nlmsg_flags)),
        );
        nl_map.insert("netlink.seq".to_string(), serde_json::Value::Number(nlmsg_seq.into()));
        nl_map.insert("netlink.pid".to_string(), serde_json::Value::Number(nlmsg_pid.into()));

        layers.insert("netlink".to_string(), serde_json::Value::Object(nl_map));

        // If it's Generic Netlink (type >= 16 usually), we might have a Genl
        // header. But for now, let's just dump attributes if we can
        // parse them? Or just leave it as is.

        // We can try to parse attributes from payload if it looks like
        // attributes. But without knowing the family, it's hard to
        // interpret.

        // Let's rely on `build_packet_json` to wrap it.
    }

    crate::utils::json::build_packet_json(
        serde_json::Value::Object(layers),
        packet.len(),
        "netlink",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::netlink::nl80211::attr_id;

    #[test]
    fn test_nl_attr_hdr_json_conversion() {
        let original_hdr = NlAttrHdr::new(8, attr_id::IFACE_MAC); // len=8, type=IFACE_MAC (2)
        let json_hdr = JsonNlAttrHdr::from(&original_hdr);

        assert_eq!(json_hdr.nla_len, 8);
        assert_eq!(json_hdr.nla_type_raw, attr_id::IFACE_MAC);
        assert_eq!(json_hdr.nla_type_id, attr_id::IFACE_MAC);
        assert_eq!(json_hdr.nla_type_name, "IFACE_MAC");
        assert!(!json_hdr.nla_is_nested);

        let reconstructed_hdr = NlAttrHdr::try_from(&json_hdr).unwrap();
        assert_eq!(reconstructed_hdr.length(), original_hdr.length());
        assert_eq!(reconstructed_hdr.attr_type(), original_hdr.attr_type());
    }

    #[test]
    fn test_nl_attribute_json_serialization_deserialization() {
        let original_hdr = NlAttrHdr::new(10, attr_id::IFACE_NAME); // len=10 (4 hdr + 6 payload)
        let original_payload: [u8; 6] = [0x77, 0x6c, 0x61, 0x6e, 0x30, 0x00]; // "wlan0"

        let json_string = to_json_string(&original_hdr, &original_payload).unwrap();

        let (deserialized_hdr, deserialized_payload) = from_json_string(&json_string).unwrap();

        assert_eq!(deserialized_hdr.length(), original_hdr.length());
        assert_eq!(deserialized_hdr.attr_type(), original_hdr.attr_type());
        assert_eq!(deserialized_payload, original_payload.to_vec());

        // Verify some fields in the JSON string itself (optional, but good for
        // understanding)
        let parsed_value: serde_json::Value = serde_json::from_str(&json_string).unwrap();
        let nl_attr_layer = parsed_value.get("nl_attr").expect("JSON should have 'nl_attr' key");

        assert_eq!(nl_attr_layer["nla.len"].as_u64().unwrap(), 10);
        assert_eq!(nl_attr_layer["nla.type_name"].as_str().unwrap(), "IFACE_NAME");
        assert_eq!(nl_attr_layer["nla.payload_hex"].as_str().unwrap(), "776C616E3000");
    }

    #[test]
    fn test_invalid_payload_hex_deserialize() {
        let json_str = r#"{
            "nl_attr": {
                "nla.len": 4, "nla.type_raw": 1, "nla.type_id": 1, "nla.type_name": "HW_INDEX",
                "nla.is_nested": false, "nla.payload_hex": "INVALIDHEX"
            }
        }"#;
        assert!(from_json_string(json_str).is_err());
    }
}
