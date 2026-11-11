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
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

    use crate::{
        IP_P_TCP,
        transport::{TcpBuilder, UdpBuilder, UdpHeader, UdpPacketBuilder},
        utils::{
            checksum::{ip_pseudo_header_checksum_parts, tcp_checksum_v6, udp_checksum_v6},
            test_utils::validate_pcap_json,
        },
    };

    #[test]
    fn test_transport_builders_fail_on_small_buffer() {
        let mut small_buffer = [0u8; 5];

        assert!(
            TcpBuilder::new(
                &mut small_buffer,
                Ipv4Addr::new(127, 0, 0, 1),
                Ipv4Addr::new(127, 0, 0, 2)
            )
            .is_none()
        );

        assert!(
            UdpBuilder::new(
                &mut small_buffer,
                Ipv4Addr::new(127, 0, 0, 1),
                Ipv4Addr::new(127, 0, 0, 2),
                1234,
                5678
            )
            .is_none()
        );

        assert!(UdpPacketBuilder::new(&mut small_buffer, 1234, 5678).is_none());
    }

    #[test]
    fn test_tcp_builder_payload_len() {
        let mut buffer = [0u8; 100];
        let mut builder =
            TcpBuilder::new(&mut buffer, Ipv4Addr::new(127, 0, 0, 1), Ipv4Addr::new(127, 0, 0, 2))
                .unwrap();

        // Use payload_mut to write some data
        let payload = builder.payload_mut().unwrap();
        payload[0] = 1;
        payload[1] = 2;

        // Manually set payload_len
        builder.payload_len(2);

        let total_len = builder.build().unwrap();
        assert_eq!(total_len, 20 + 2); // 20 bytes TCP header + 2 bytes payload
    }

    #[test]
    fn test_udp_builder_len_and_build() {
        let mut buffer = [0u8; 100];
        let mut builder = UdpBuilder::new(
            &mut buffer,
            Ipv4Addr::new(127, 0, 0, 1),
            Ipv4Addr::new(127, 0, 0, 2),
            1234,
            5678,
        )
        .unwrap();

        // Write payload
        let payload = builder.payload_mut();
        payload[0] = 1;
        payload[1] = 2;
        payload[2] = 3;

        builder.payload_len(3);

        let total_len = builder.build().unwrap();
        assert_eq!(total_len, 8 + 3); // 8 bytes UDP header + 3 bytes payload

        // Verify checksum calculation (0xE2D2 in big endian)
        assert_eq!(u16::from_be_bytes([buffer[6], buffer[7]]), 0xE2D2);
    }

    #[test]
    fn test_tcp_builder_overflow() {
        let mut buffer = [0u8; 30]; // 20 bytes header, only 10 bytes remaining
        let mut builder =
            TcpBuilder::new(&mut buffer, Ipv4Addr::new(127, 0, 0, 1), Ipv4Addr::new(127, 0, 0, 2))
                .unwrap();

        // payload_mut should work
        assert!(builder.payload_mut().is_some());

        // payload of 10 bytes should work
        assert!(builder.payload(&[0u8; 10]).is_some());

        // payload of 11 bytes should fail
        assert!(builder.payload(&[0u8; 11]).is_none());
    }

    #[test]
    fn test_udp_builder_overflow() {
        let mut buffer = [0u8; 15]; // 8 bytes header, only 7 bytes remaining
        let mut builder = UdpBuilder::new(
            &mut buffer,
            Ipv4Addr::new(127, 0, 0, 1),
            Ipv4Addr::new(127, 0, 0, 2),
            1234,
            5678,
        )
        .unwrap();

        // payload of 7 bytes should work
        assert!(builder.payload(&[0u8; 7]).is_some());

        // payload of 8 bytes should fail
        assert!(builder.payload(&[0u8; 8]).is_none());
    }

    #[test]
    fn test_tcp_pcap_json() {
        let fields = &["tcp.srcport", "tcp.dstport"];
        validate_pcap_json(
            include_bytes!("test_data/tcp.pcap"),
            include_str!("test_data/tcp.json"),
            fields,
        );
    }

    #[test]
    fn test_udp_pcap_json() {
        let fields = &["udp.srcport", "udp.dstport"];
        validate_pcap_json(
            include_bytes!("test_data/udp.pcap"),
            include_str!("test_data/udp.json"),
            fields,
        );
    }

    #[test]
    fn test_udp_packet_builder() {
        let mut buffer = [0u8; 100];
        let mut builder = UdpPacketBuilder::new(&mut buffer, 1234, 5678).unwrap();

        let payload = builder.payload_mut();
        payload[0] = 1;
        payload[1] = 2;

        builder.payload_len(2);

        let total_len = builder.build();
        assert_eq!(total_len, 8 + 2); // 8 bytes UDP header + 2 bytes payload
    }

    #[test]
    fn test_udp_packet_builder_exact_buffer_default_len() {
        let mut buffer = [0u8; 12];
        let mut builder = UdpPacketBuilder::new(&mut buffer, 1234, 5678).unwrap();
        let payload = builder.payload_mut();
        payload.copy_from_slice(&[1, 2, 3, 4]);

        let total_len = builder.build();
        assert_eq!(total_len, 12);

        let (header, parsed_payload) =
            UdpHeader::parse(&buffer).expect("Failed to parse UDP header");
        assert_eq!(header.length.get(), 12);
        assert_eq!(parsed_payload, &[1, 2, 3, 4]);
    }

    #[test]
    fn test_tcp_builder_v6_checksum_verifies() {
        let src_v6 = Ipv6Addr::LOCALHOST;
        let dst_v6 = Ipv6Addr::new(0xfe80, 0, 0, 0, 0, 0, 0, 2);
        let mut tcp_buf = [0u8; 32];
        let mut tcp = TcpBuilder::new_v6(&mut tcp_buf, src_v6, dst_v6).unwrap();
        tcp.source_port(1234).dest_port(80).payload(&[1, 2, 3, 4]).unwrap();
        let tcp_len = tcp.build().unwrap();
        assert_eq!(tcp_checksum_v6(&tcp_buf[..tcp_len], src_v6, dst_v6), 0);
    }

    #[test]
    fn test_udp_builder_v6_checksum_verifies() {
        let src_v6 = Ipv6Addr::LOCALHOST;
        let dst_v6 = Ipv6Addr::new(0xfe80, 0, 0, 0, 0, 0, 0, 2);
        let mut udp_buf = [0u8; 16];
        let mut udp = UdpBuilder::new_v6(&mut udp_buf, src_v6, dst_v6, 1234, 5678).unwrap();
        udp.payload(&[1, 2, 3]).unwrap();
        let udp_len = udp.build().unwrap();
        assert_eq!(udp_checksum_v6(&udp_buf[..udp_len], src_v6, dst_v6), u16::MAX);
    }

    #[test]
    fn test_ip_pseudo_header_checksum_rejects_mixed_families() {
        let v4 = IpAddr::V4(Ipv4Addr::LOCALHOST);
        let v6 = IpAddr::V6(Ipv6Addr::LOCALHOST);
        assert!(ip_pseudo_header_checksum_parts(&[], &[], v4, v6, IP_P_TCP).is_none());
        assert!(ip_pseudo_header_checksum_parts(&[], &[], v6, v4, IP_P_TCP).is_none());
    }
}
