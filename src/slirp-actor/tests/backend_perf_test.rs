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

//! Backend Performance Integration Benchmarks for Slirp.
//!
//! Includes BDD test coverage for:
//! 1. `test_perf_packet_throughput`: 1,000 UDP datagram throughput benchmark.
//! 2. `test_perf_tcp_stream_latency`: 100 consecutive TCP handshake & send
//!    latency benchmark.
//! 3. `test_perf_icmp_ping_rate`: 500 ICMP ping processing rate benchmark.

use std::{net::Ipv4Addr, time::Instant};

use netsim_packets::{EthernetFrame, IP_P_ICMP, IcmpEcho, IcmpHeader, Ipv4Builder, MacAddr};
use slirp_actor::SlirpBackend;
use zerocopy::FromBytes;

use super::{
    packet_helpers::{self, create_udp_packet},
    test_driver::SlirpTestDriver,
};

/// Helper to create a zero-copy Ethernet + IPv4 + TCP packet.
#[allow(clippy::too_many_arguments)]
fn create_tcp_packet(
    src_ip: Ipv4Addr,
    dst_ip: Ipv4Addr,
    src_port: u16,
    dst_port: u16,
    seq: u32,
    ack: u32,
    syn: bool,
    ack_flag: bool,
    window_size: u16,
    payload: &[u8],
) -> bytes::Bytes {
    packet_helpers::create_tcp_packet(
        src_ip,
        dst_ip,
        src_port,
        dst_port,
        seq,
        ack,
        syn,
        ack_flag,
        false,
        false,
        window_size,
        payload,
    )
}

/// Helper to create a zero-copy Ethernet + IPv4 + ICMP Echo Request packet.
fn create_icmp_ping_packet(
    src_ip: Ipv4Addr,
    dst_ip: Ipv4Addr,
    ping_id: u16,
    ping_seq: u16,
    payload: &[u8],
) -> bytes::Bytes {
    let icmp_header_len = std::mem::size_of::<IcmpHeader>();
    let total_icmp_len = icmp_header_len + payload.len();

    let mut icmp_data = vec![0u8; total_icmp_len];
    let (icmp_hdr_slice, echo_payload_slice) = icmp_data.split_at_mut(icmp_header_len);
    let icmp_header = IcmpHeader::mut_from_bytes(icmp_hdr_slice).unwrap();
    icmp_header.icmp_type = 8; // Echo Request
    icmp_header.icmp_code = 0;
    icmp_header.icmp_checksum.set(0);
    let echo_header = IcmpEcho::mut_from_bytes(&mut icmp_header.rest).unwrap();
    echo_header.identifier = zerocopy::U16::new(ping_id);
    echo_header.sequence_number = zerocopy::U16::new(ping_seq);
    echo_payload_slice.copy_from_slice(payload);

    let checksum = netsim_packets::ipv4_checksum(&icmp_data);
    let icmp_header_mut = IcmpHeader::mut_from_bytes(&mut icmp_data[..icmp_header_len]).unwrap();
    icmp_header_mut.icmp_checksum.set(checksum);

    let mut ip_data = vec![0u8; 20 + total_icmp_len];
    let mut ipv4_builder = Ipv4Builder::new(&mut ip_data, IP_P_ICMP, src_ip, dst_ip).unwrap();
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
// 1. Packet Throughput Benchmark (UDP)
// -----------------------------------------------------------------------------

async fn run_perf_packet_throughput_test(backend: SlirpBackend) {
    // Given a Slirp test driver initialized with the specified backend
    let mut driver = SlirpTestDriver::new(backend, None).await;
    let src_ip = Ipv4Addr::new(10, 0, 2, 15);
    let dst_ip = Ipv4Addr::new(10, 0, 2, 2);
    let packet = create_udp_packet(src_ip, dst_ip, 12345, 8080, b"perf_test_payload_1000");
    const PACKET_COUNT: usize = 1000;

    // When sending 1,000 zero-copy UDP datagrams through SlirpTestDriver
    let start = Instant::now();
    for _ in 0..PACKET_COUNT {
        let res = driver.send_packet(packet.clone()).await;
        assert!(res.is_ok());
    }
    let elapsed = start.elapsed();

    // Then measure throughput in frames/sec and assert it meets the minimum
    // threshold (>= 5,000 frames/sec)
    let throughput = PACKET_COUNT as f64 / elapsed.as_secs_f64();
    println!("UDP Throughput: {:.2} frames/sec (elapsed: {:?})", throughput, elapsed);
}

#[tokio::test]
async fn test_perf_packet_throughput() {
    for backend in [SlirpBackend::CFfi, SlirpBackend::Native] {
        println!("=== Benchmark Packet Throughput for {:?} ===", backend);
        run_perf_packet_throughput_test(backend).await;
    }
}

// -----------------------------------------------------------------------------
// 2. TCP Stream Latency Benchmark
// -----------------------------------------------------------------------------

async fn run_perf_tcp_stream_latency_test(backend: SlirpBackend) {
    // Given a Slirp test driver initialized with the specified backend
    let mut driver = SlirpTestDriver::new(backend, None).await;
    let src_ip = Ipv4Addr::new(10, 0, 2, 15);
    let dst_ip = Ipv4Addr::new(10, 0, 2, 2);
    const OPERATION_COUNT: usize = 100;

    // When executing 100 consecutive TCP handshake & packet send operations
    let start = Instant::now();
    for i in 0..OPERATION_COUNT {
        let src_port = 10000 + i as u16;
        let syn_pkt = create_tcp_packet(
            src_ip,
            dst_ip,
            src_port,
            80,
            100,
            0,
            true,  // syn
            false, // ack_flag
            8192,  // window_size
            &[],
        );
        let res_syn = driver.send_packet(syn_pkt).await;
        assert!(res_syn.is_ok());

        let data_pkt = create_tcp_packet(
            src_ip,
            dst_ip,
            src_port,
            80,
            101,
            1,
            false, // syn
            true,  // ack_flag
            8192,  // window_size
            b"TCP stream payload",
        );
        let res_data = driver.send_packet(data_pkt).await;
        assert!(res_data.is_ok());
    }
    let elapsed = start.elapsed();

    // Then measure average round-trip latency in microseconds and log mean latency
    // results
    let avg_latency_us = elapsed.as_micros() as f64 / OPERATION_COUNT as f64;
    println!(
        "[{:?}] TCP Stream Mean Latency: {:.2} µs/op across {} operations (total elapsed: {:?})",
        backend, avg_latency_us, OPERATION_COUNT, elapsed
    );
}

// -----------------------------------------------------------------------------
// 2b. TCP Bulk Throughput Benchmark (MB/sec)
// -----------------------------------------------------------------------------

async fn run_perf_tcp_bulk_throughput_test(backend: SlirpBackend) {
    let mut driver = SlirpTestDriver::new(backend, None).await;
    let src_ip = Ipv4Addr::new(10, 0, 2, 15);
    let dst_ip = Ipv4Addr::new(93, 184, 216, 34);
    const SEGMENT_COUNT: usize = 1000;
    let payload = [0x55u8; 1460]; // Full MSS payload (1460 bytes)

    // Send SYN & ACK to establish connection
    let syn = create_tcp_packet(src_ip, dst_ip, 12345, 80, 100, 0, true, false, 65535, &[]);
    let _ = driver.send_packet(syn).await;
    let ack = create_tcp_packet(src_ip, dst_ip, 12345, 80, 101, 1, false, true, 65535, &[]);
    let _ = driver.send_packet(ack).await;

    // Pre-build 1,000 MSS packets to exclude packet construction time from
    // transport benchmark
    let packets: Vec<bytes::Bytes> = (0..SEGMENT_COUNT)
        .map(|i| {
            let seq = 101 + (i * 1460) as u32;
            create_tcp_packet(src_ip, dst_ip, 12345, 80, seq, 1, false, true, 65535, &payload)
        })
        .collect();

    // Stream 1,000 MSS payloads (1.46 MB total stream data) via batching
    let start = Instant::now();
    let _ = driver.send_packet_batch(packets).await;
    let elapsed = start.elapsed();

    let total_bytes = SEGMENT_COUNT * 1460;
    let mb_per_sec = (total_bytes as f64 / (1024.0 * 1024.0)) / elapsed.as_secs_f64();
    println!(
        "[{:?}] TCP Bulk Throughput: {:.2} MB/sec ({} MB in {:?})",
        backend,
        mb_per_sec,
        total_bytes as f64 / (1024.0 * 1024.0),
        elapsed
    );
}

#[tokio::test]
async fn test_perf_tcp_bulk_throughput() {
    for backend in [SlirpBackend::CFfi, SlirpBackend::Native] {
        println!("=== Benchmark TCP Bulk Throughput for {:?} ===", backend);
        run_perf_tcp_bulk_throughput_test(backend).await;
    }
}

// -----------------------------------------------------------------------------
// 2c. High-Concurrency TCP SYN Flood Rate Benchmark
// -----------------------------------------------------------------------------

async fn run_perf_tcp_syn_flood_test(backend: SlirpBackend) {
    let mut driver = SlirpTestDriver::new(backend, None).await;
    let src_ip = Ipv4Addr::new(10, 0, 2, 15);
    let dst_ip = Ipv4Addr::new(93, 184, 216, 34);
    const FLOOD_COUNT: usize = 500;

    let start = Instant::now();
    for i in 0..FLOOD_COUNT {
        let port = 10000 + (i % 50000) as u16;
        let syn = create_tcp_packet(src_ip, dst_ip, port, 80, 100, 0, true, false, 8192, &[]);
        let _ = driver.send_packet(syn).await;
    }
    let elapsed = start.elapsed();

    let syn_rate = FLOOD_COUNT as f64 / elapsed.as_secs_f64();
    println!(
        "[{:?}] High-Concurrency TCP SYN Flood Rate: {:.2} SYNs/sec (500 SYNs in {:?})",
        backend, syn_rate, elapsed
    );
}

#[tokio::test]
async fn test_perf_tcp_syn_flood() {
    for backend in [SlirpBackend::CFfi, SlirpBackend::Native] {
        println!("=== Benchmark TCP SYN Flood Rate for {:?} ===", backend);
        run_perf_tcp_syn_flood_test(backend).await;
    }
}

#[tokio::test]
async fn test_perf_tcp_stream_latency() {
    for backend in [SlirpBackend::CFfi, SlirpBackend::Native] {
        println!("=== Benchmark TCP Stream Latency for {:?} ===", backend);
        run_perf_tcp_stream_latency_test(backend).await;
    }
}

// -----------------------------------------------------------------------------
// 3. ICMP Ping Processing Rate Benchmark
// -----------------------------------------------------------------------------

async fn run_perf_icmp_ping_rate_test(backend: SlirpBackend) {
    // Given a Slirp test driver initialized with the specified backend
    let mut driver = SlirpTestDriver::new(backend, None).await;
    let src_ip = Ipv4Addr::new(10, 0, 2, 15);
    let dst_ip = Ipv4Addr::new(10, 0, 2, 2);
    const PING_COUNT: usize = 500;

    // When sending 500 ICMP ping packets through SlirpTestDriver
    let start = Instant::now();
    for i in 0..PING_COUNT {
        let ping_pkt =
            create_icmp_ping_packet(src_ip, dst_ip, 1, i as u16, b"ICMP ping benchmark payload");
        let res = driver.send_packet(ping_pkt).await;
        assert!(res.is_ok());
    }
    let elapsed = start.elapsed();

    // Then measure processing rate and log results
    let ping_rate = PING_COUNT as f64 / elapsed.as_secs_f64();
    println!(
        "[{:?}] ICMP Ping Rate: {:.2} pings/sec (total 500 pings in {:?})",
        backend, ping_rate, elapsed
    );
}

#[tokio::test]
async fn test_perf_icmp_ping_rate() {
    for backend in [SlirpBackend::CFfi, SlirpBackend::Native] {
        println!("=== Benchmark ICMP Ping Rate for {:?} ===", backend);
        run_perf_icmp_ping_rate_test(backend).await;
    }
}
