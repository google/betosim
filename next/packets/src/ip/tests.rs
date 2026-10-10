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

#[cfg(test)]
mod tests {
    use std::net::{Ipv4Addr, Ipv6Addr};

    use crate::{
        ip::{Ipv4Builder, Ipv6Builder},
        utils::test_utils::validate_pcap_json,
    };

    #[test]
    fn test_ip_builders_fail_on_small_buffer() {
        let mut small_buffer = [0u8; 10];

        assert!(
            Ipv4Builder::new(
                &mut small_buffer,
                6,
                Ipv4Addr::new(127, 0, 0, 1),
                Ipv4Addr::new(127, 0, 0, 2)
            )
            .is_none()
        );

        assert!(
            Ipv6Builder::new(
                &mut small_buffer,
                6,
                Ipv6Addr::new(0, 0, 0, 0, 0, 0, 0, 1),
                Ipv6Addr::new(0, 0, 0, 0, 0, 0, 0, 2)
            )
            .is_none()
        );
    }

    #[test]
    fn test_ipv4_builder_success_and_overflow() {
        let mut buffer = [0u8; 40]; // 20 bytes header, 20 bytes payload
        let mut builder = Ipv4Builder::new(
            &mut buffer,
            17, // UDP
            Ipv4Addr::new(127, 0, 0, 1),
            Ipv4Addr::new(127, 0, 0, 2),
        )
        .unwrap();

        // payload_mut should work
        let payload = builder.payload_mut();
        assert_eq!(payload.len(), 20);
        payload[0] = 9;

        // Set payload length to 10 (fits)
        builder.payload_len(10);
        let len = builder.build().unwrap();
        assert_eq!(len, 30); // 20 + 10

        // Re-create with smaller buffer to test overflow
        let mut small_buffer = [0u8; 25]; // 20 bytes header, only 5 bytes payload
        let mut builder = Ipv4Builder::new(
            &mut small_buffer,
            17,
            Ipv4Addr::new(127, 0, 0, 1),
            Ipv4Addr::new(127, 0, 0, 2),
        )
        .unwrap();

        // payload length 5 fits
        builder.payload_len(5);
        assert!(builder.build().is_some());

        // Re-create again to test build-time overflow
        let mut small_buffer2 = [0u8; 25];
        let mut builder = Ipv4Builder::new(
            &mut small_buffer2,
            17,
            Ipv4Addr::new(127, 0, 0, 1),
            Ipv4Addr::new(127, 0, 0, 2),
        )
        .unwrap();
        // payload length 6 overflows (only 5 bytes available)
        builder.payload_len(6);
        assert!(builder.build().is_none());
    }

    #[test]
    fn test_ipv6_builder_success_and_overflow() {
        let mut buffer = [0u8; 60]; // 40 bytes header, 20 bytes payload
        let mut builder = Ipv6Builder::new(
            &mut buffer,
            17, // UDP
            Ipv6Addr::new(0, 0, 0, 0, 0, 0, 0, 1),
            Ipv6Addr::new(0, 0, 0, 0, 0, 0, 0, 2),
        )
        .unwrap();

        // payload_mut should work
        let payload = builder.payload_mut();
        assert_eq!(payload.len(), 20);

        // Set payload length to 10 (fits)
        builder.payload_len(10);
        let len = builder.build().unwrap();
        assert_eq!(len, 50); // 40 + 10

        // Re-create with smaller buffer to test overflow
        let mut small_buffer = [0u8; 45]; // 40 bytes header, only 5 bytes payload
        let mut builder = Ipv6Builder::new(
            &mut small_buffer,
            17,
            Ipv6Addr::new(0, 0, 0, 0, 0, 0, 0, 1),
            Ipv6Addr::new(0, 0, 0, 0, 0, 0, 0, 2),
        )
        .unwrap();

        // payload length 6 overflows
        builder.payload_len(6);
        assert!(builder.build().is_none());
    }

    #[test]
    fn test_ipv4_pcap_json() {
        let fields = &["ip.src", "ip.dst", "ip.proto", "ip.ttl"];
        validate_pcap_json(
            include_bytes!("../icmp/test_data/icmp.pcap"),
            include_str!("../icmp/test_data/icmp.json"),
            fields,
        );
    }

    #[test]
    fn test_ipv6_pcap_json() {
        let fields = &["ipv6.src", "ipv6.dst", "ipv6.nxt"];
        validate_pcap_json(
            include_bytes!("../icmp/test_data/icmpv6.pcap"),
            include_str!("../icmp/test_data/icmpv6.json"),
            fields,
        );
    }
}
