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

//! Integration Tests: TFTP Read Request (RRQ) & Protocol Handling
//! (b/560172253).

use std::net::Ipv4Addr;

use netsim_packets::{EthernetFrame, IP_P_UDP, Ipv4Builder, MacAddr, UdpPacketBuilder};
use slirp_actor::SlirpBackend;
use zerocopy::FromBytes;

use super::test_driver::SlirpTestDriver;

fn create_tftp_rrq_packet(filename: &str) -> bytes::Bytes {
    let mut tftp_payload = vec![0u8, 1]; // Opcode 1 (RRQ)
    tftp_payload.extend_from_slice(filename.as_bytes());
    tftp_payload.push(0);
    tftp_payload.extend_from_slice(b"octet");
    tftp_payload.push(0);

    let mut udp_data = vec![0u8; 8 + tftp_payload.len()];
    let mut udp_builder = UdpPacketBuilder::new(&mut udp_data, 54321, 69).unwrap();
    udp_builder.payload_mut()[..tftp_payload.len()].copy_from_slice(&tftp_payload);
    udp_builder.payload_len(tftp_payload.len());
    udp_builder.build();

    let mut ip_data = vec![0u8; 20 + udp_data.len()];
    let mut ipv4_builder = Ipv4Builder::new(
        &mut ip_data,
        IP_P_UDP,
        Ipv4Addr::new(10, 0, 2, 15),
        Ipv4Addr::new(10, 0, 2, 2),
    )
    .unwrap();
    ipv4_builder.payload(&udp_data).unwrap();
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
// 1. TFTP Read Request (RRQ) Test
// -----------------------------------------------------------------------------

async fn run_tftp_rrq_test(backend: SlirpBackend) {
    // Given a Slirp driver running the specified backend
    let mut driver = SlirpTestDriver::new(backend, None).await;

    // When guest sends TFTP RRQ packet to 10.0.2.2:69 for "pxelinux.0"
    let rrq_packet = create_tftp_rrq_packet("pxelinux.0");
    let result = driver.send_packet(rrq_packet).await;

    // Then Slirp processes TFTP RRQ packet
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_tftp_rrq() {
    for backend in [SlirpBackend::CFfi, SlirpBackend::Native] {
        run_tftp_rrq_test(backend).await;
    }
}
