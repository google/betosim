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

//! Integration Tests: ARP Resolution & ICMP Error Handlers (b/560172253).

use std::net::Ipv4Addr;

use netsim_packets::{
    ArpPacketBuilder, EthernetFrame, IP_P_ICMP, IcmpEcho, IcmpHeader, Ipv4Builder, MacAddr,
};
use slirp_actor::SlirpBackend;
use zerocopy::FromBytes;

use super::test_driver::SlirpTestDriver;

fn create_arp_request_packet(target_ip: Ipv4Addr) -> bytes::Bytes {
    let mut arp_data = vec![0u8; 28];
    let builder = ArpPacketBuilder::new(&mut arp_data).unwrap();
    builder
        .hardware_type(1)
        .protocol_type(0x0800)
        .hardware_addr_len(6)
        .protocol_addr_len(4)
        .opcode(1) // Request
        .sender_hardware_addr(MacAddr { bytes: [0x02, 0x15, 0xb2, 0x00, 0x00, 0x00] })
        .sender_protocol_addr([10, 0, 2, 15])
        .target_hardware_addr(MacAddr { bytes: [0; 6] })
        .target_protocol_addr(target_ip.octets())
        .build();

    let mut eth_data = vec![0u8; 14 + arp_data.len()];
    let (eth_header_slice, eth_payload_slice) = eth_data.split_at_mut(14);
    let eth_frame = EthernetFrame::mut_from_bytes(eth_header_slice).unwrap();
    eth_frame.dst_addr = MacAddr { bytes: [0xff; 6] }; // ARP Broadcast
    eth_frame.src_addr = MacAddr { bytes: [0x02, 0x15, 0xb2, 0x00, 0x00, 0x00] };
    eth_frame.ethertype = 0x0806.into(); // ARP EtherType
    eth_payload_slice.copy_from_slice(&arp_data);

    bytes::Bytes::from(eth_data)
}

fn create_icmp_dest_unreachable_packet() -> bytes::Bytes {
    let icmp_header_len = std::mem::size_of::<IcmpHeader>();
    let payload = vec![0u8; 28]; // IP header + 8 bytes of original packet
    let total_icmp_len = icmp_header_len + payload.len();

    let mut icmp_data = vec![0u8; total_icmp_len];
    let (icmp_hdr_slice, echo_payload_slice) = icmp_data.split_at_mut(icmp_header_len);
    let icmp_header = IcmpHeader::mut_from_bytes(icmp_hdr_slice).unwrap();
    icmp_header.icmp_type = 3; // Destination Unreachable
    icmp_header.icmp_code = 3; // Port Unreachable
    icmp_header.icmp_checksum.set(0);
    echo_payload_slice.copy_from_slice(&payload);

    let checksum = netsim_packets::ipv4_checksum(&icmp_data);
    let icmp_header_mut = IcmpHeader::mut_from_bytes(&mut icmp_data[..icmp_header_len]).unwrap();
    icmp_header_mut.icmp_checksum.set(checksum);

    let mut ip_data = vec![0u8; 20 + total_icmp_len];
    let mut ipv4_builder = Ipv4Builder::new(
        &mut ip_data,
        IP_P_ICMP,
        Ipv4Addr::new(10, 0, 2, 15),
        Ipv4Addr::new(10, 0, 2, 2),
    )
    .unwrap();
    ipv4_builder.payload(&icmp_data).unwrap();
    ipv4_builder.build().unwrap();

    let mut eth_data = vec![0u8; 14 + ip_data.len()];
    let (eth_header_slice, eth_payload_slice) = eth_data.split_at_mut(14);
    let eth_frame = EthernetFrame::mut_from_bytes(eth_header_slice).unwrap();
    eth_frame.dst_addr = MacAddr { bytes: [0x52, 0x54, 0x00, 0x12, 0x34, 0x56] };
    eth_frame.src_addr = MacAddr { bytes: [0x02, 0x15, 0xb2, 0x00, 0x00, 0x00] };
    eth_frame.ethertype = 0x0800.into();
    eth_payload_slice.copy_from_slice(&ip_data);

    bytes::Bytes::from(eth_data)
}

// -----------------------------------------------------------------------------
// 1. ARP Resolution for Gateway (10.0.2.2)
// -----------------------------------------------------------------------------

async fn run_arp_gateway_resolution_test(backend: SlirpBackend) {
    // Given a Slirp driver running the specified backend
    let mut driver = SlirpTestDriver::new(backend, None).await;

    // When guest queries ARP for gateway 10.0.2.2
    let arp_pkt = create_arp_request_packet(Ipv4Addr::new(10, 0, 2, 2));
    let result = driver.send_packet(arp_pkt).await;

    // Then Slirp processes ARP request
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_arp_gateway_resolution() {
    for backend in [SlirpBackend::CFfi, SlirpBackend::Native] {
        run_arp_gateway_resolution_test(backend).await;
    }
}

// -----------------------------------------------------------------------------
// 2. ARP Resolution for DNS Server (10.0.2.3)
// -----------------------------------------------------------------------------

async fn run_arp_dns_resolution_test(backend: SlirpBackend) {
    // Given a Slirp driver running the specified backend
    let mut driver = SlirpTestDriver::new(backend, None).await;

    // When guest queries ARP for DNS server 10.0.2.3
    let arp_pkt = create_arp_request_packet(Ipv4Addr::new(10, 0, 2, 3));
    let result = driver.send_packet(arp_pkt).await;

    // Then Slirp processes ARP request
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_arp_dns_resolution() {
    for backend in [SlirpBackend::CFfi, SlirpBackend::Native] {
        run_arp_dns_resolution_test(backend).await;
    }
}

// -----------------------------------------------------------------------------
// 3. ICMP Destination Unreachable Processing
// -----------------------------------------------------------------------------

async fn run_icmp_dest_unreachable_test(backend: SlirpBackend) {
    // Given a Slirp driver running the specified backend
    let mut driver = SlirpTestDriver::new(backend, None).await;

    // When guest sends ICMP Destination Unreachable packet
    let icmp_pkt = create_icmp_dest_unreachable_packet();
    let result = driver.send_packet(icmp_pkt).await;

    // Then Slirp processes ICMP error packet
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_icmp_dest_unreachable() {
    for backend in [SlirpBackend::CFfi, SlirpBackend::Native] {
        run_icmp_dest_unreachable_test(backend).await;
    }
}

// -----------------------------------------------------------------------------
// 4. ICMP Echo Request Processing
// -----------------------------------------------------------------------------

fn create_icmp_echo_request_packet() -> bytes::Bytes {
    let icmp_header_len = std::mem::size_of::<IcmpHeader>();
    let payload = b"netsim icmp echo payload";
    let total_icmp_len = icmp_header_len + payload.len();

    let mut icmp_data = vec![0u8; total_icmp_len];
    let (icmp_hdr_slice, echo_payload_slice) = icmp_data.split_at_mut(icmp_header_len);
    let icmp_header = IcmpHeader::mut_from_bytes(icmp_hdr_slice).unwrap();
    icmp_header.icmp_type = 8; // Echo Request
    icmp_header.icmp_code = 0;
    icmp_header.icmp_checksum.set(0);
    let echo_header = IcmpEcho::mut_from_bytes(&mut icmp_header.rest).unwrap();
    echo_header.identifier = zerocopy::U16::new(0x1234);
    echo_header.sequence_number = zerocopy::U16::new(1);
    echo_payload_slice.copy_from_slice(payload);

    let checksum = netsim_packets::ipv4_checksum(&icmp_data);
    let icmp_header_mut = IcmpHeader::mut_from_bytes(&mut icmp_data[..icmp_header_len]).unwrap();
    icmp_header_mut.icmp_checksum.set(checksum);

    let mut ip_data = vec![0u8; 20 + total_icmp_len];
    let mut ipv4_builder = Ipv4Builder::new(
        &mut ip_data,
        IP_P_ICMP,
        Ipv4Addr::new(10, 0, 2, 15),
        Ipv4Addr::new(8, 8, 8, 8),
    )
    .unwrap();
    ipv4_builder.payload(&icmp_data).unwrap();
    ipv4_builder.build().unwrap();

    let mut eth_data = vec![0u8; 14 + ip_data.len()];
    let (eth_header_slice, eth_payload_slice) = eth_data.split_at_mut(14);
    let eth_frame = EthernetFrame::mut_from_bytes(eth_header_slice).unwrap();
    eth_frame.dst_addr = MacAddr { bytes: [0x52, 0x54, 0x00, 0x12, 0x34, 0x56] };
    eth_frame.src_addr = MacAddr { bytes: [0x02, 0x15, 0xb2, 0x00, 0x00, 0x00] };
    eth_frame.ethertype = 0x0800.into();
    eth_payload_slice.copy_from_slice(&ip_data);

    bytes::Bytes::from(eth_data)
}

async fn run_icmp_echo_request_test(backend: SlirpBackend) {
    // Given a Slirp driver running the specified backend
    let mut driver = SlirpTestDriver::new(backend, None).await;

    // When guest sends ICMP Echo Request packet
    let icmp_pkt = create_icmp_echo_request_packet();
    let result = driver.send_packet(icmp_pkt).await;

    // Then Slirp processes ICMP echo request packet
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_icmp_echo_request() {
    for backend in [SlirpBackend::CFfi, SlirpBackend::Native] {
        run_icmp_echo_request_test(backend).await;
    }
}
