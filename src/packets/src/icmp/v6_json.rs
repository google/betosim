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

use std::collections::BTreeMap;

use crate::icmp::v6::Icmpv6Header;

pub fn to_json(header: &Icmpv6Header) -> BTreeMap<String, String> {
    let mut icmpv6 = BTreeMap::new();
    icmpv6.insert("icmpv6.type".to_string(), format!("{}", header.icmpv6_type));
    icmpv6.insert("icmpv6.code".to_string(), format!("{}", header.icmpv6_code));
    icmpv6.insert("icmpv6.checksum".to_string(), format!("{:#06x}", header.icmpv6_checksum.get()));
    icmpv6
}

#[cfg(test)]
mod tests {
    use zerocopy::byteorder::U16;

    use super::*;

    #[test]
    fn test_to_json() {
        let header = Icmpv6Header {
            icmpv6_type: 128,
            icmpv6_code: 0,
            icmpv6_checksum: U16::new(0x1234),
            rest: [0; 4],
        };
        let json_map = to_json(&header);
        let value = serde_json::to_value(json_map).unwrap();
        assert_eq!(value["icmpv6.type"], "128");
        assert_eq!(value["icmpv6.code"], "0");
        assert_eq!(value["icmpv6.checksum"], "0x1234");
    }
}
