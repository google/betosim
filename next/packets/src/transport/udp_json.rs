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

//! JSON serialization for UDP headers.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::transport::udp::UdpHeader;

/// Converts a `UdpHeader` to a `serde_json::Map` mimicking `tshark` output.
pub fn to_json(header: &UdpHeader) -> Map<String, Value> {
    let mut map = Map::new();
    map.insert("udp.srcport".to_string(), Value::String(header.source_port.get().to_string()));
    map.insert("udp.dstport".to_string(), Value::String(header.dest_port.get().to_string()));
    map.insert("udp.length".to_string(), Value::String(header.length.get().to_string()));
    map.insert(
        "udp.checksum".to_string(),
        Value::String(format!("0x{:04x}", header.checksum.get())),
    );
    map
}

/// A `serde`-compatible representation of a UDP header.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct JsonUdpHeader {
    #[serde(rename = "udp.srcport")]
    pub src_port: String,
    #[serde(rename = "udp.dstport")]
    pub dst_port: String,
    #[serde(rename = "udp.length")]
    pub length: String,
    #[serde(rename = "udp.checksum")]
    pub checksum: String,
}

impl TryFrom<JsonUdpHeader> for UdpHeader {
    type Error = std::num::ParseIntError;

    fn try_from(json_header: JsonUdpHeader) -> Result<Self, Self::Error> {
        let checksum = if let Some(stripped) = json_header.checksum.strip_prefix("0x") {
            u16::from_str_radix(stripped, 16)?
        } else {
            json_header.checksum.parse::<u16>()?
        };

        Ok(UdpHeader {
            source_port: json_header.src_port.parse::<u16>()?.into(),
            dest_port: json_header.dst_port.parse::<u16>()?.into(),
            length: json_header.length.parse::<u16>()?.into(),
            checksum: checksum.into(),
        })
    }
}
