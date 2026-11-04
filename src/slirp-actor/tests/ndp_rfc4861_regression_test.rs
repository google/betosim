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

//! Independent RFC 4861 IPv6/NDP Wire-Layout Regression Tests.

use std::{net::Ipv6Addr, time::Duration};

use netsim_packets::{EthernetFrame, MacAddr, icmpv6_checksum};
use slirp_actor::SlirpBackend;
use zerocopy::FromBytes;

use super::test_driver::SlirpTestDriver;

const GUEST_MAC: [u8; 6] = [0x02, 0x15, 0xb2, 0x00, 0x00, 0x00];
const GUEST_LINK_LOCAL: Ipv6Addr = Ipv6Addr::new(0xfe80, 0, 0, 0, 0x0215, 0xb2ff, 0xfe00, 0);

const GATEWAY_IPV6: Ipv6Addr = Ipv6Addr::new(0xfec0, 0, 0, 0, 0, 0, 0, 2);
const ALL_ROUTERS_MAC: [u8; 6] = [0x33, 0x33, 0x00, 0x00, 0x00, 0x02];
const ALL_ROUTERS_IPV6: Ipv6Addr = Ipv6Addr::new(0xff02, 0, 0, 0, 0, 0, 0, 2);
const SOLICITED_NODE_GATEWAY_MAC: [u8; 6] = [0x33, 0x33, 0xff, 0x00, 0x00, 0x02];
const SOLICITED_NODE_GATEWAY_IPV6: Ipv6Addr = Ipv6Addr::new(0xff02, 0, 0, 0, 0, 1, 0xff00, 2);

fn build_ipv6_icmpv6_frame(
    src_mac: &[u8; 6],
    dst_mac: &[u8; 6],
    src_ip: &[u8; 16],
    dst_ip: &[u8; 16],
    icmpv6_type: u8,
    icmpv6_code: u8,
    body_after_type_code_csum: &[u8],
) -> Vec<u8> {
    let icmpv6_len = 4 + body_after_type_code_csum.len();
    let mut eth_data = vec![0u8; 14 + 40 + icmpv6_len];
    let (eth_header_slice, eth_payload_slice) = eth_data.split_at_mut(14);
    let eth_frame =
        EthernetFrame::mut_from_bytes(eth_header_slice).expect("Failed to create EthernetFrame");
    eth_frame.dst_addr = MacAddr { bytes: *dst_mac };
    eth_frame.src_addr = MacAddr { bytes: *src_mac };
    eth_frame.ethertype = 0x86DD.into();

    eth_payload_slice[0] = 0x60;
    eth_payload_slice[4..6].copy_from_slice(&(icmpv6_len as u16).to_be_bytes());
    eth_payload_slice[6] = 58; // Next Header: ICMPv6
    eth_payload_slice[7] = 255; // Hop Limit

    eth_payload_slice[8..24].copy_from_slice(src_ip);
    eth_payload_slice[24..40].copy_from_slice(dst_ip);

    eth_payload_slice[40] = icmpv6_type;
    eth_payload_slice[41] = icmpv6_code;
    eth_payload_slice[44..].copy_from_slice(body_after_type_code_csum);

    let src = Ipv6Addr::from(*src_ip);
    let dst = Ipv6Addr::from(*dst_ip);
    let checksum = icmpv6_checksum(&eth_payload_slice[40..], src, dst);
    eth_payload_slice[42..44].copy_from_slice(&checksum.to_be_bytes());

    eth_data
}

async fn recv_icmpv6_type(
    driver: &mut SlirpTestDriver,
    expected_type: u8,
    backend: SlirpBackend,
) -> Vec<u8> {
    tokio::time::timeout(Duration::from_millis(500), async {
        loop {
            if let Some(frame) = driver.recv_packet().await {
                if frame.len() >= 58
                    && frame[12..14] == [0x86, 0xdd] // IPv6
                    && frame[20] == 58 // Next Header: ICMPv6
                    && frame[54] == expected_type
                {
                    return frame.to_vec();
                }
            } else {
                panic!(
                    "[backend={:?}] Driver closed before receiving ICMPv6 type {}",
                    backend, expected_type
                );
            }
        }
    })
    .await
    .unwrap_or_else(|_| {
        panic!("[backend={:?}] Timed out waiting for ICMPv6 type {}", backend, expected_type)
    })
}

fn assert_na_wire_layout(
    frame: &[u8],
    expected_target_ip: &[u8; 16],
    expected_dst_mac: &[u8; 6],
    backend: SlirpBackend,
) -> [u8; 6] {
    let payload_len = u16::from_be_bytes([frame[18], frame[19]]) as usize;
    assert_eq!(frame.len(), 54 + payload_len, "[backend={:?}] Frame length mismatch", backend);
    let icmpv6 = &frame[54..54 + payload_len];

    assert_eq!(icmpv6.len(), 32, "[backend={:?}] NA payload length", backend);
    assert_eq!(icmpv6[0], 136, "[backend={:?}] ICMPv6 NA Type", backend);
    assert_eq!(icmpv6[1], 0, "[backend={:?}] ICMPv6 NA Code", backend);
    assert_ne!(&icmpv6[2..4], &[0, 0], "[backend={:?}] ICMPv6 Checksum must be non-zero", backend);

    let src_ip = *<&[u8; 16]>::try_from(&frame[22..38]).expect("Failed to parse src_ip");
    let dst_ip = *<&[u8; 16]>::try_from(&frame[38..54]).expect("Failed to parse dst_ip");
    assert_eq!(&src_ip, expected_target_ip, "[backend={:?}] NA IPv6 Source Address", backend);
    assert_eq!(
        &dst_ip,
        &GUEST_LINK_LOCAL.octets(),
        "[backend={:?}] NA IPv6 Destination Address",
        backend
    );
    let checksum = icmpv6_checksum(icmpv6, Ipv6Addr::from(src_ip), Ipv6Addr::from(dst_ip));
    assert_eq!(checksum, 0, "[backend={:?}] ICMPv6 Checksum validation", backend);

    assert_eq!(frame[21], 255, "[backend={:?}] IPv6 Hop Limit", backend);

    if matches!(backend, SlirpBackend::Native) {
        assert_eq!(icmpv6[4], 0xe0, "[backend={:?}] NA Flags (Native)", backend);
    } else {
        assert_eq!(icmpv6[4] & 0xc0, 0xc0, "[backend={:?}] NA Router and Solicited flags", backend);
    }

    assert_eq!(&icmpv6[5..8], &[0, 0, 0], "[backend={:?}] NA Reserved fields", backend);
    assert_eq!(&icmpv6[8..24], expected_target_ip, "[backend={:?}] NA Target IP", backend);

    assert_eq!(icmpv6[24], 2, "[backend={:?}] NA TLLA Option Type", backend);
    assert_eq!(icmpv6[25], 1, "[backend={:?}] NA TLLA Option Length", backend);

    let gateway_mac = *<&[u8; 6]>::try_from(&frame[6..12]).expect("Failed to parse gateway_mac");
    assert_eq!(&icmpv6[26..32], &gateway_mac, "[backend={:?}] NA TLLA Address", backend);

    assert_eq!(&frame[0..6], expected_dst_mac, "[backend={:?}] Ethernet Dst MAC", backend);

    gateway_mac
}

fn assert_ra_wire_layout(frame: &[u8], backend: SlirpBackend) {
    let payload_len = u16::from_be_bytes([frame[18], frame[19]]) as usize;
    assert_eq!(frame.len(), 54 + payload_len, "[backend={:?}] Frame length mismatch", backend);
    let icmpv6 = &frame[54..54 + payload_len];

    assert!(icmpv6.len() >= 16, "[backend={:?}] RA payload length", backend);
    assert_eq!(icmpv6[0], 134, "[backend={:?}] ICMPv6 RA Type", backend);
    assert_eq!(icmpv6[1], 0, "[backend={:?}] ICMPv6 RA Code", backend);
    assert_ne!(&icmpv6[2..4], &[0, 0], "[backend={:?}] ICMPv6 Checksum must be non-zero", backend);

    let src_ip = *<&[u8; 16]>::try_from(&frame[22..38]).expect("Failed to parse src_ip");
    let dst_ip = *<&[u8; 16]>::try_from(&frame[38..54]).expect("Failed to parse dst_ip");
    let checksum = icmpv6_checksum(icmpv6, Ipv6Addr::from(src_ip), Ipv6Addr::from(dst_ip));
    assert_eq!(checksum, 0, "[backend={:?}] ICMPv6 Checksum validation", backend);

    assert_eq!(frame[21], 255, "[backend={:?}] IPv6 Hop Limit", backend);

    assert!(icmpv6[4] > 0, "[backend={:?}] RA Cur Hop Limit must be > 0", backend);
    if matches!(backend, SlirpBackend::Native) {
        assert_eq!(icmpv6[4], 64, "[backend={:?}] RA Cur Hop Limit", backend);
        assert_eq!(
            u16::from_be_bytes([icmpv6[6], icmpv6[7]]),
            1800,
            "[backend={:?}] RA Router Lifetime",
            backend
        );
    } else {
        assert!(
            u16::from_be_bytes([icmpv6[6], icmpv6[7]]) > 0,
            "[backend={:?}] RA Router Lifetime must be > 0",
            backend
        );
    }

    let mut offset = 16;
    let gateway_mac = &frame[6..12];
    let mut slla_found = false;
    let mut pi_found = false;
    let mut rdnss_found = false;

    while offset < icmpv6.len() {
        assert!(
            offset + 2 <= icmpv6.len(),
            "[backend={:?}] Truncated NDP option header at offset {}",
            backend,
            offset
        );

        let opt_type = icmpv6[offset];
        let opt_len = icmpv6[offset + 1];
        assert_ne!(opt_type, 0, "[backend={:?}] Option type must not be 0", backend);
        assert!(opt_len > 0, "[backend={:?}] Option length must be > 0", backend);

        let byte_len = (opt_len as usize) * 8;
        assert!(
            offset + byte_len <= icmpv6.len(),
            "[backend={:?}] Option length exceeds payload",
            backend
        );

        match opt_type {
            1 => {
                // SLLA
                assert_eq!(opt_len, 1, "[backend={:?}] SLLA Option length", backend);
                assert_eq!(
                    &icmpv6[offset + 2..offset + 8],
                    gateway_mac,
                    "[backend={:?}] SLLA Address",
                    backend
                );
                slla_found = true;
            }
            3 => {
                // PI
                assert_eq!(opt_len, 4, "[backend={:?}] PI Option length", backend);
                assert_eq!(icmpv6[offset + 2], 64, "[backend={:?}] PI Prefix length", backend);
                assert_eq!(
                    icmpv6[offset + 3] & 0xc0,
                    0xc0,
                    "[backend={:?}] PI Flags (On-link and Autonomous)",
                    backend
                );
                let prefix = &icmpv6[offset + 16..offset + 32];
                let expected_prefix = Ipv6Addr::new(0xfec0, 0, 0, 0, 0, 0, 0, 0).octets();
                assert_eq!(prefix, &expected_prefix, "[backend={:?}] PI Prefix", backend);
                pi_found = true;
            }
            25 => {
                // RDNSS
                rdnss_found = true;
                assert_eq!(opt_len, 3, "[backend={:?}] RDNSS Option length", backend);
                let address = &icmpv6[offset + 8..offset + 24];
                if matches!(backend, SlirpBackend::Native) {
                    let expected_address = GATEWAY_IPV6.octets();
                    assert_eq!(address, &expected_address, "[backend={:?}] RDNSS Address", backend);
                } else {
                    let expected_address1 = Ipv6Addr::new(0xfec0, 0, 0, 0, 0, 0, 0, 3).octets();
                    let expected_address2 = GATEWAY_IPV6.octets();
                    assert!(
                        address == expected_address1 || address == expected_address2,
                        "[backend={:?}] RDNSS Address",
                        backend
                    );
                }
            }
            _ => {}
        }

        offset += byte_len;
    }

    assert_eq!(
        offset,
        icmpv6.len(),
        "[backend={:?}] Options must exactly match payload length",
        backend
    );
    assert!(slla_found, "[backend={:?}] RA must contain SLLA option", backend);
    assert!(pi_found, "[backend={:?}] RA must contain Prefix Information option", backend);
    if matches!(backend, SlirpBackend::Native) {
        assert!(
            rdnss_found,
            "[backend={:?}] Native RA must contain RDNSS option (Type 25)",
            backend
        );
    }
}

async fn run_ndp_neighbor_solicitation_unconditional_reply_and_wire_layout(backend: SlirpBackend) {
    // Given: a <backend> SLiRP driver and an NS carrying an SLLA option
    let mut driver = SlirpTestDriver::new(backend, None).await;

    let expected_target_ip = GATEWAY_IPV6.octets();

    let mut body = vec![0, 0, 0, 0]; // Reserved
    body.extend_from_slice(&expected_target_ip);
    // SLLA option
    body.push(1); // Type 1
    body.push(1); // Length 1
    body.extend_from_slice(&GUEST_MAC);

    let packet = build_ipv6_icmpv6_frame(
        &GUEST_MAC,
        &SOLICITED_NODE_GATEWAY_MAC,
        &GUEST_LINK_LOCAL.octets(),
        &SOLICITED_NODE_GATEWAY_IPV6.octets(),
        135, // NS
        0,
        &body,
    );

    // When: the NS is sent (multicast, then unicast)
    driver.send_packet(bytes::Bytes::from(packet)).await.expect("Failed to send first NS packet");

    // Then: an NA comes back with the RFC 4861 wire layout (TLLA at the expected offsets)
    let frame = recv_icmpv6_type(&mut driver, 136, backend).await; // NA
    let gateway_mac = assert_na_wire_layout(&frame, &expected_target_ip, &GUEST_MAC, backend);

    // unicast NS
    let packet2 = build_ipv6_icmpv6_frame(
        &GUEST_MAC,
        &gateway_mac,
        &GUEST_LINK_LOCAL.octets(),
        &expected_target_ip,
        135, // NS
        0,
        &body, // same body with SLLA
    );

    driver.send_packet(bytes::Bytes::from(packet2)).await.expect("Failed to send second NS packet");

    let frame2 = recv_icmpv6_type(&mut driver, 136, backend).await; // NA
    assert_na_wire_layout(&frame2, &expected_target_ip, &GUEST_MAC, backend);
}

// Scenario: Reply to Neighbor Solicitation with RFC 4861 Target Link-Layer Address option
#[tokio::test]
async fn test_ndp_neighbor_solicitation_unconditional_reply_and_wire_layout() {
    for backend in [SlirpBackend::CFfi, SlirpBackend::Native] {
        run_ndp_neighbor_solicitation_unconditional_reply_and_wire_layout(backend).await;
    }
}

async fn run_ndp_router_advertisement_wire_layout_on_solicited_rs_with_slla(backend: SlirpBackend) {
    // Given: a <backend> SLiRP driver and an RS carrying an SLLA option
    let mut driver = SlirpTestDriver::new(backend, None).await;

    let mut body = vec![0, 0, 0, 0]; // Reserved
    body.push(1); // Type 1
    body.push(1); // Length 1
    body.extend_from_slice(&GUEST_MAC);

    let packet = build_ipv6_icmpv6_frame(
        &GUEST_MAC,
        &ALL_ROUTERS_MAC,
        &GUEST_LINK_LOCAL.octets(),
        &ALL_ROUTERS_IPV6.octets(),
        133, // RS
        0,
        &body,
    );

    // When: the RS is sent with an SLLA option
    driver.send_packet(bytes::Bytes::from(packet)).await.expect("Failed to send RS packet");

    // Then: an RA comes back with the RFC 4861 wire layout and expected TLV options
    let frame = recv_icmpv6_type(&mut driver, 134, backend).await; // RA
    assert_ra_wire_layout(&frame, backend);
}

// Scenario: Reply to Router Solicitation carrying SLLA with RFC 4861 Router Advertisement options
#[tokio::test]
async fn test_ndp_router_advertisement_wire_layout_on_solicited_rs_with_slla() {
    for backend in [SlirpBackend::CFfi, SlirpBackend::Native] {
        run_ndp_router_advertisement_wire_layout_on_solicited_rs_with_slla(backend).await;
    }
}

async fn run_ndp_router_advertisement_wire_layout_on_optionless_rs(backend: SlirpBackend) {
    // Given: a <backend> SLiRP driver and an optionless RS
    let mut driver = SlirpTestDriver::new(backend, None).await;

    let body = vec![0, 0, 0, 0]; // Reserved, 8-byte optionless RS (4 bytes after type, code, csum)

    let packet = build_ipv6_icmpv6_frame(
        &GUEST_MAC,
        &ALL_ROUTERS_MAC,
        &GUEST_LINK_LOCAL.octets(),
        &ALL_ROUTERS_IPV6.octets(),
        133, // RS
        0,
        &body,
    );

    // When: the optionless RS is sent to the all-routers multicast address
    driver
        .send_packet(bytes::Bytes::from(packet))
        .await
        .expect("Failed to send optionless RS packet");

    // Then: an RA comes back with the RFC 4861 wire layout and expected TLV options
    let frame = recv_icmpv6_type(&mut driver, 134, backend).await; // RA
    assert_ra_wire_layout(&frame, backend);
}

// Scenario: Reply to optionless Router Solicitation with RFC 4861 Router Advertisement options
#[tokio::test]
async fn test_ndp_router_advertisement_wire_layout_on_optionless_rs() {
    for backend in [SlirpBackend::CFfi, SlirpBackend::Native] {
        run_ndp_router_advertisement_wire_layout_on_optionless_rs(backend).await;
    }
}
