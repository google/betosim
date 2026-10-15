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

use serde_json::Value;
use std::time::{SystemTime, UNIX_EPOCH};

pub fn build_packet_json(layers: Value) -> Value {
    let now = SystemTime::now();
    let epoch_secs = now.duration_since(UNIX_EPOCH).unwrap().as_secs_f64();
    let n = 0;
    let frame = serde_json::json!({
        "frame.time_epoch": format!("{:.9}", epoch_secs),
        "frame.len": n,
        "frame.cap_len": n,
        "frame.protocols": "eth"
    });
    let mut source = serde_json::json!({ "layers": layers });
    let layers_mut = source["layers"].as_object_mut().unwrap();
    layers_mut.insert("frame".to_string(), frame);
    serde_json::json!([{
        "_index": "packets-2025-06-14",
        "_type": "doc",
        "_score": null,
        "_source": source
    }])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_packet_json() {
        let layers = serde_json::json!({
            "eth": {
                "eth.dst": "ff:ff:ff:ff:ff:ff",
                "eth.src": "00:00:00:00:00:00"
            }
        });
        let result = build_packet_json(layers);
        let source = &result[0]["_source"];
        assert!(source["layers"]["frame"]["frame.time_epoch"].is_string());
        assert_eq!(source["layers"]["eth"]["eth.dst"], "ff:ff:ff:ff:ff:ff");
    }
}
