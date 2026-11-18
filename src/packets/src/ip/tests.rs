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
        ip::{IP_P_UDP, Ipv4Builder, Ipv4Header, Ipv6Builder, Ipv6Header},
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
    fn test_ip_builders_header_only_slice_with_payload_len() {
        let mut v4_packet = [0u8; 32];
        let (v4_header_slice, v4_payload_slice) = v4_packet.split_at_mut(20);
        v4_payload_slice.copy_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12]);

        let mut v4_builder = Ipv4Builder::new(
            v4_header_slice,
            17, // UDP
            Ipv4Addr::new(192, 168, 0, 1),
            Ipv4Addr::new(192, 168, 0, 2),
        )
        .unwrap();
        assert_eq!(v4_builder.payload_mut().len(), 0);
        v4_builder.payload_len(12);
        let v4_built_len =
            v4_builder.build().expect("build should succeed on IPv4 header-only slice");
        assert_eq!(v4_built_len, 32);

        let (parsed_v4_header, parsed_v4_payload) =
            Ipv4Header::parse(&v4_packet).expect("Failed to parse round-trip IPv4 packet");
        assert_eq!(parsed_v4_header.total_length.get(), 32);
        assert_ne!(parsed_v4_header.header_checksum.get(), 0);
        assert_eq!(parsed_v4_payload, &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12]);

        let mut v6_packet = [0u8; 52];
        let (v6_header_slice, v6_payload_slice) = v6_packet.split_at_mut(40);
        v6_payload_slice.copy_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12]);

        let mut v6_builder = Ipv6Builder::new(
            v6_header_slice,
            17, // UDP
            Ipv6Addr::new(0, 0, 0, 0, 0, 0, 0, 1),
            Ipv6Addr::new(0, 0, 0, 0, 0, 0, 0, 2),
        )
        .unwrap();
        assert_eq!(v6_builder.payload_mut().len(), 0);
        v6_builder.payload_len(12);
        let v6_built_len =
            v6_builder.build().expect("build should succeed on IPv6 header-only slice");
        assert_eq!(v6_built_len, 52);

        let (parsed_v6_header, parsed_v6_payload) =
            Ipv6Header::parse(&v6_packet).expect("Failed to parse round-trip IPv6 packet");
        assert_eq!(parsed_v6_header.payload_length.get(), 12);
        assert_eq!(parsed_v6_payload, &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12]);
    }

    #[test]
    fn test_ipv4_builder_payload_and_oversized_preserves_len() {
        const PAYLOAD_BYTES: &[u8] = &[0x11, 0x22, 0x33, 0x44];
        const OVERSIZED_FILL: u8 = 0xff;
        let hdr_len = std::mem::size_of::<Ipv4Header>();
        let mut buffer = vec![0u8; hdr_len + PAYLOAD_BYTES.len()];
        let mut builder =
            Ipv4Builder::new(&mut buffer, IP_P_UDP, Ipv4Addr::LOCALHOST, Ipv4Addr::BROADCAST)
                .unwrap();

        assert!(builder.payload(PAYLOAD_BYTES).is_some());
        let oversized_for_buf = vec![OVERSIZED_FILL; PAYLOAD_BYTES.len() + 1];
        assert!(builder.payload(&oversized_for_buf).is_none());

        let built_len = builder.build().unwrap();
        assert_eq!(built_len, hdr_len + PAYLOAD_BYTES.len());

        let (header, parsed_payload) = Ipv4Header::parse(&buffer).unwrap();
        assert_eq!(usize::from(header.total_length.get()), built_len);
        assert_eq!(parsed_payload, PAYLOAD_BYTES);

        let max_v4_total = usize::from(u16::MAX);
        let max_v4_payload = max_v4_total - hdr_len;
        let mut max_buf = vec![0u8; max_v4_total + 1];
        let mut max_builder =
            Ipv4Builder::new(&mut max_buf, IP_P_UDP, Ipv4Addr::LOCALHOST, Ipv4Addr::BROADCAST)
                .unwrap();
        let over_u16_payload = vec![OVERSIZED_FILL; max_v4_payload + 1];
        assert!(max_builder.payload(&over_u16_payload).is_none());

        for (len, expected) in
            [(max_v4_payload, Some(max_v4_total)), (max_v4_payload + 1, None), (usize::MAX, None)]
        {
            let mut hdr_only = vec![0u8; hdr_len];
            let mut hdr_builder =
                Ipv4Builder::new(&mut hdr_only, IP_P_UDP, Ipv4Addr::LOCALHOST, Ipv4Addr::BROADCAST)
                    .unwrap();
            hdr_builder.payload_len(len);
            assert_eq!(hdr_builder.build(), expected);
        }
    }

    #[test]
    fn test_ipv4_builder_setters() {
        const TEST_TTL: u8 = 42;
        const TEST_DSCP_ECN: u8 = 0x2e;
        const TEST_IDENTIFICATION: u16 = 0x1234;
        const TEST_FLAGS_FRAGMENT_OFFSET: u16 = 0x4000;
        let mut buffer = vec![0u8; std::mem::size_of::<Ipv4Header>()];
        let mut builder =
            Ipv4Builder::new(&mut buffer, IP_P_UDP, Ipv4Addr::LOCALHOST, Ipv4Addr::BROADCAST)
                .unwrap();
        builder
            .ttl(TEST_TTL)
            .dscp_ecn(TEST_DSCP_ECN)
            .identification(TEST_IDENTIFICATION)
            .flags_fragment_offset(TEST_FLAGS_FRAGMENT_OFFSET);
        assert!(builder.build().is_some());

        let (header, _) = Ipv4Header::parse(&buffer).unwrap();
        assert_eq!(header.ttl, TEST_TTL);
        assert_eq!(header.dscp_ecn, TEST_DSCP_ECN);
        assert_eq!(header.identification.get(), TEST_IDENTIFICATION);
        assert_eq!(header.flags_fragment_offset.get(), TEST_FLAGS_FRAGMENT_OFFSET);
    }

    #[test]
    fn test_ipv6_builder_payload_hop_limit_and_limits() {
        const V6_PAYLOAD_BYTES: &[u8] = &[0xaa, 0xbb, 0xcc];
        const TEST_HOP_LIMIT: u8 = 128;
        const OVERSIZED_FILL: u8 = 0xee;
        const DST_V6: Ipv6Addr = Ipv6Addr::new(0x2001, 0xdb8, 0, 0, 0, 0, 0, 2);
        let hdr_len = std::mem::size_of::<Ipv6Header>();
        let mut buffer = vec![0u8; hdr_len + V6_PAYLOAD_BYTES.len()];

        let mut builder =
            Ipv6Builder::new(&mut buffer, IP_P_UDP, Ipv6Addr::LOCALHOST, DST_V6).unwrap();
        builder.hop_limit(TEST_HOP_LIMIT);
        assert!(builder.payload(V6_PAYLOAD_BYTES).is_some());
        let too_large_for_buf = vec![OVERSIZED_FILL; V6_PAYLOAD_BYTES.len() + 1];
        assert!(builder.payload(&too_large_for_buf).is_none());
        let built_len = builder.build().unwrap();
        assert_eq!(built_len, hdr_len + V6_PAYLOAD_BYTES.len());

        let (header, parsed_payload) = Ipv6Header::parse(&buffer).unwrap();
        assert_eq!(header.hop_limit, TEST_HOP_LIMIT);
        assert_eq!(usize::from(header.payload_length.get()), V6_PAYLOAD_BYTES.len());
        assert_eq!(parsed_payload, V6_PAYLOAD_BYTES);

        let max_u16_len = usize::from(u16::MAX);
        let over_u16_len = max_u16_len + 1;
        let mut huge_buf = vec![0u8; hdr_len + over_u16_len];
        let mut huge_builder =
            Ipv6Builder::new(&mut huge_buf, IP_P_UDP, Ipv6Addr::LOCALHOST, DST_V6).unwrap();
        let over_u16_payload = vec![OVERSIZED_FILL; over_u16_len];
        assert!(huge_builder.payload(&over_u16_payload).is_none());

        let mut hdr_only_max_buf = vec![0u8; hdr_len];
        let mut hdr_only_max_builder =
            Ipv6Builder::new(&mut hdr_only_max_buf, IP_P_UDP, Ipv6Addr::LOCALHOST, DST_V6).unwrap();
        hdr_only_max_builder.payload_len(max_u16_len);
        assert_eq!(hdr_only_max_builder.build(), Some(hdr_len + max_u16_len));

        let mut hdr_only_buf = vec![0u8; hdr_len];
        let mut hdr_only_builder =
            Ipv6Builder::new(&mut hdr_only_buf, IP_P_UDP, Ipv6Addr::LOCALHOST, DST_V6).unwrap();
        hdr_only_builder.payload_len(over_u16_len);
        assert!(hdr_only_builder.build().is_none());
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
