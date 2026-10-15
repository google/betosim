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

#[cfg(test)]
mod tests {
    use crate::utils::test_utils::validate_pcap_json;

    #[test]
    fn test_netlink_pcap_json() {
        // tshark golden file doesn't have netlink fields parsed, so we only check frame
        // metadata
        let fields = &["frame.len", "frame.cap_len"];
        validate_pcap_json(
            include_bytes!("test_data/netlink.pcap"),
            include_str!("test_data/netlink.json"),
            fields,
        );
    }
}
