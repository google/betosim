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

//! Compact DHCPv4 (< 548 Bytes) Regression Tests (b/565477868).

use std::{net::Ipv4Addr, time::Duration};

use netsim_packets::{
    EthernetFrame, IP_P_UDP, Ipv4Builder, Ipv4Header, MacAddr, UdpHeader, UdpPacketBuilder,
    ether_type,
};
use slirp_actor::SlirpBackend;
use tokio::time::timeout_at;
use zerocopy::FromBytes;

use super::test_driver::SlirpTestDriver;

const DEFAULT_SRC_MAC: [u8; 6] = [0x02, 0x15, 0xb2, 0x00, 0x00, 0x00];

fn build_compact_bootp_dhcp_payload(xid: u32, chaddr: &[u8; 6], options: &[u8]) -> Vec<u8> {
    let mut payload = vec![0u8; 236];
    payload[0] = 1; // op = 1 (BOOTREQUEST)
    payload[1] = 1; // htype = 1 (Ethernet)
    payload[2] = 6; // hlen = 6
    payload[3] = 0; // hops = 0
    payload[4..8].copy_from_slice(&xid.to_be_bytes());
    payload[10..12].copy_from_slice(&0x8000u16.to_be_bytes()); // flags = broadcast (0x8000)
    payload[28..34].copy_from_slice(chaddr);

    // 4-byte DHCP magic cookie
    payload.extend_from_slice(&[99, 130, 83, 99]);

    // Options
    payload.extend_from_slice(options);

    assert!(payload.len() < 548, "Payload should be strictly < 548 bytes for this regression test");
    payload
}

fn build_dhcp_eth_frame(dhcp_payload: &[u8]) -> Vec<u8> {
    let mut udp_data = vec![0u8; 8 + dhcp_payload.len()];
    let mut udp_builder =
        UdpPacketBuilder::new(&mut udp_data, 68, 67).expect("Failed to build UDP packet");
    udp_builder.payload_mut()[..dhcp_payload.len()].copy_from_slice(dhcp_payload);
    udp_builder.payload_len(dhcp_payload.len());
    udp_builder.build();

    let mut ip_data = vec![0u8; 20 + udp_data.len()];
    let mut ipv4_builder =
        Ipv4Builder::new(&mut ip_data, IP_P_UDP, Ipv4Addr::UNSPECIFIED, Ipv4Addr::BROADCAST)
            .expect("Failed to construct IPv4 builder");
    ipv4_builder.payload(&udp_data).expect("Failed to set IPv4 payload");
    ipv4_builder.build().expect("Failed to build IPv4 packet");

    let mut eth_data = vec![0u8; 14 + ip_data.len()];
    let (eth_header_slice, eth_payload_slice) = eth_data.split_at_mut(14);

    let eth_frame = EthernetFrame::mut_from_bytes(eth_header_slice)
        .expect("Failed to construct Ethernet frame");
    eth_frame.dst_addr = MacAddr { bytes: [0xff; 6] };
    eth_frame.src_addr = MacAddr { bytes: DEFAULT_SRC_MAC };
    eth_frame.ethertype = ether_type::IPV4.into();
    eth_payload_slice.copy_from_slice(&ip_data);

    eth_data
}

async fn recv_dhcp_reply(driver: &mut SlirpTestDriver, expected_xid: u32) -> (u8, [u8; 4]) {
    let deadline = tokio::time::Instant::now() + Duration::from_millis(500);
    loop {
        let frame = timeout_at(deadline, driver.recv_packet())
            .await
            .expect("Timed out waiting for DHCP reply (netsim-slirp silently dropped < 548 byte DHCPv4 payload?)")
            .expect("Driver channel closed");

        let Some((eth, eth_payload)) = EthernetFrame::parse(&frame) else {
            continue;
        };
        if eth.ethertype.get() != 0x0800 {
            continue;
        }

        let Some((ip, ip_payload)) = Ipv4Header::parse(eth_payload) else {
            continue;
        };
        if ip.protocol != IP_P_UDP {
            continue;
        }

        let Some((udp, udp_payload)) = UdpHeader::parse(ip_payload) else {
            continue;
        };
        if udp.source_port.get() != 67 || udp.dest_port.get() != 68 {
            continue;
        }

        let payload = udp_payload;
        if payload.len() < 240 {
            continue;
        }
        if payload[0] != 2 {
            continue; // BOOTREPLY
        }

        let xid = u32::from_be_bytes(payload[4..8].try_into().expect("Failed to parse XID"));
        if xid != expected_xid {
            continue;
        }

        let yiaddr: [u8; 4] = payload[16..20].try_into().expect("Failed to parse YIADDR");
        let cookie = &payload[236..240];
        if cookie != [99, 130, 83, 99] {
            continue;
        }

        // Find Option 53
        let mut msg_type = 0;
        let mut i = 240;
        while i < payload.len() {
            let opt = payload[i];
            if opt == 255 {
                break;
            }
            if opt == 0 {
                i += 1;
                continue;
            }
            if i + 1 >= payload.len() {
                break;
            }
            let len = payload[i + 1] as usize;
            if i + 2 + len > payload.len() {
                break;
            }

            if opt == 53 && len == 1 {
                msg_type = payload[i + 2];
            }
            i += 2 + len;
        }

        assert_eq!(&payload[28..34], &DEFAULT_SRC_MAC, "DHCP reply chaddr must match client MAC");

        return (msg_type, yiaddr);
    }
}

async fn run_dhcp_minimal_244_byte_discover_returns_offer(backend: SlirpBackend) {
    let mut driver = SlirpTestDriver::new(backend, None).await;
    let xid = 0xdead_beef;

    let dhcp_payload = build_compact_bootp_dhcp_payload(xid, &DEFAULT_SRC_MAC, &[53, 1, 1, 255]);
    assert_eq!(dhcp_payload.len(), 244, "Payload length must be exactly 244 bytes");

    let eth_data = build_dhcp_eth_frame(&dhcp_payload);

    driver.send_packet(bytes::Bytes::from(eth_data)).await.expect("Failed to send packet");

    let (msg_type, yiaddr) = recv_dhcp_reply(&mut driver, xid).await;
    assert_eq!(msg_type, 2, "Expected DHCPOFFER");
    assert_ne!(yiaddr, [0, 0, 0, 0], "DHCPOFFER should include an IP address");
}

#[tokio::test]
async fn test_dhcp_minimal_244_byte_discover_returns_offer_cffi() {
    run_dhcp_minimal_244_byte_discover_returns_offer(SlirpBackend::CFfi).await;
}

#[tokio::test]
async fn test_dhcp_minimal_244_byte_discover_returns_offer_native() {
    run_dhcp_minimal_244_byte_discover_returns_offer(SlirpBackend::Native).await;
}

async fn run_dhcp_compact_discover_and_request_handshake_returns_ack(backend: SlirpBackend) {
    let mut driver = SlirpTestDriver::new(backend, None).await;
    let xid_discover = 0xdead_beef;

    let discover_payload = build_compact_bootp_dhcp_payload(
        xid_discover,
        &DEFAULT_SRC_MAC,
        &[53, 1, 1, 55, 4, 1, 3, 6, 15, 255],
    );
    assert!(discover_payload.len() < 260, "Discover payload must be compact");

    let eth_data_disc = build_dhcp_eth_frame(&discover_payload);

    driver
        .send_packet(bytes::Bytes::from(eth_data_disc))
        .await
        .expect("Failed to send Discover packet");

    let (msg_type, offered_ip) = recv_dhcp_reply(&mut driver, xid_discover).await;
    assert_eq!(msg_type, 2, "Expected DHCPOFFER");

    let xid_request = 0xcafebabe;
    let mut request_options = vec![53, 1, 3, 50, 4];
    request_options.extend_from_slice(&offered_ip);
    request_options.extend_from_slice(&[54, 4, 10, 0, 2, 2, 255]);

    let request_payload =
        build_compact_bootp_dhcp_payload(xid_request, &DEFAULT_SRC_MAC, &request_options);
    assert!(request_payload.len() < 300, "Request payload must be compact");

    let eth_data_req = build_dhcp_eth_frame(&request_payload);

    driver
        .send_packet(bytes::Bytes::from(eth_data_req))
        .await
        .expect("Failed to send Request packet");

    let (msg_type_ack, ack_yiaddr) = recv_dhcp_reply(&mut driver, xid_request).await;
    assert_eq!(msg_type_ack, 5, "Expected DHCPACK");
    assert_eq!(ack_yiaddr, offered_ip, "DHCPACK IP should match requested IP");
}

#[tokio::test]
async fn test_dhcp_compact_discover_and_request_handshake_returns_ack_cffi() {
    run_dhcp_compact_discover_and_request_handshake_returns_ack(SlirpBackend::CFfi).await;
}

#[tokio::test]
async fn test_dhcp_compact_discover_and_request_handshake_returns_ack_native() {
    run_dhcp_compact_discover_and_request_handshake_returns_ack(SlirpBackend::Native).await;
}
