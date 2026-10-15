// Copyright 2024 The Android Open Source Project
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

use bytes::Bytes;
use etherparse::EtherType;
use etherparse::LinkHeader::Ethernet2;
use etherparse::{NetHeaders, PacketBuilder, PacketHeaders, PayloadSlice, TransportHeader};
use libslirp_rs::libslirp::LibSlirp;
use libslirp_rs::libslirp_config::SlirpConfig;
use std::io;
use std::net::{SocketAddr, UdpSocket};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

const PAYLOAD: &[u8; 23] = b"Hello, UDP echo server!";
const PAYLOAD_PONG: &[u8; 23] = b"Hello, UDP echo client!";

/// Test UDP packets sent through libslirp
#[cfg(not(windows))] // TODO: remove once test is working on windows.
#[test]
fn udp_echo() {
    let config = SlirpConfig { ..Default::default() };

    let before_fd_count = count_open_fds().unwrap();

    let (tx, rx) = mpsc::channel::<Bytes>();
    let slirp = LibSlirp::new(config, tx, None, None);

    // Start up an IPV4 UDP echo server
    let server_addr = one_shot_udp_echo_server().unwrap();

    println!("server addr {:?}", server_addr);
    let source_ip = std::net::Ipv4Addr::UNSPECIFIED;
    // Destination address
    let host_ip = std::net::Ipv4Addr::LOCALHOST;

    // Source and destination ports
    let source_port: u16 = 20000;
    let destination_port = server_addr.port();

    // Build the UDP packet
    // with arbitrary source and destination mac addrs
    // We use guest source 0.0.0.0 to avoid ARP packets
    let builder = PacketBuilder::ethernet2([1, 2, 3, 4, 5, 6], [7, 8, 9, 10, 11, 12])
        .ipv4(source_ip.octets(), host_ip.octets(), 20)
        .udp(source_port, destination_port);

    // Get some memory to store the result
    let mut result = Vec::<u8>::with_capacity(builder.size(PAYLOAD.len()));

    // Serialize header and payload
    builder.write(&mut result, PAYLOAD).unwrap();

    let headers = PacketHeaders::from_ethernet_slice(&result).unwrap();
    if let Some(Ethernet2(ether_header)) = headers.link {
        assert_eq!(ether_header.ether_type, EtherType::IPV4);
    } else {
        panic!("expected ethernet2 header");
    }

    assert!(headers.net.is_some());
    assert!(headers.transport.is_some());

    // Send to oneshot_udp_echo_server (via libslirp)
    slirp.input(Bytes::from(result));

    // Read from oneshot_udp_echo server (via libslirp)
    // No ARP packets will be seen

    // Try to receive a packet before end_time
    match rx.recv_timeout(Duration::from_secs(2)) {
        Ok(packet) => {
            let headers = PacketHeaders::from_ethernet_slice(&packet).unwrap();

            if let Some(Ethernet2(ref ether_header)) = headers.link {
                assert_eq!(ether_header.ether_type, EtherType::IPV4);
            } else {
                panic!("expected ethernet2 header");
            }

            if let Some(NetHeaders::Ipv4(ipv4_header, _)) = headers.net {
                assert_eq!(ipv4_header.source, std::net::Ipv4Addr::LOCALHOST.octets());
                assert_eq!(ipv4_header.destination, std::net::Ipv4Addr::UNSPECIFIED.octets());
            } else {
                panic!("expected IpV4 header, got {:?}", headers.net);
            }

            if let Some(TransportHeader::Udp(udp_header)) = headers.transport {
                assert_eq!(udp_header.source_port, destination_port);
                assert_eq!(udp_header.destination_port, source_port);
            } else {
                panic!("expected Udp header");
            }

            if let PayloadSlice::Udp(payload) = headers.payload {
                assert_eq!(payload, PAYLOAD_PONG);
            } else {
                panic!("expected Udp payload");
            }
        }
        Err(mpsc::RecvTimeoutError::Timeout) => {
            panic!("Timeout waiting for udp packet");
        }
        Err(e) => {
            panic!("Failed to receive data in main thread: {}", e);
        }
    }

    // validate data packet

    slirp.shutdown();
    assert_eq!(
        rx.recv_timeout(Duration::from_millis(5)),
        Err(mpsc::RecvTimeoutError::Disconnected)
    );

    let after_fd_count = count_open_fds().unwrap();
    assert_eq!(before_fd_count, after_fd_count);
}

fn one_shot_udp_echo_server() -> std::io::Result<SocketAddr> {
    let socket = UdpSocket::bind("127.0.0.1:0")?;
    let addr = socket.local_addr()?;
    thread::spawn(move || {
        let mut buf = [0u8; 1024];
        let (len, addr) = socket.recv_from(&mut buf).unwrap();
        let data = &buf[..len];
        if data != PAYLOAD {
            panic!("mismatch payload");
        }
        println!("sending to addr {addr:?}");
        socket.send_to(PAYLOAD_PONG, addr).unwrap();
    });
    Ok(addr)
}

#[cfg(target_os = "linux")]
fn count_open_fds() -> io::Result<usize> {
    let entries = std::fs::read_dir("/proc/self/fd")?;
    Ok(entries.count())
}

#[cfg(not(target_os = "linux"))]
fn count_open_fds() -> io::Result<usize> {
    Ok(0)
}
