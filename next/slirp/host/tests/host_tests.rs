// Copyright 2023-2026 The Android Open Source Project
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

use std::{
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    time::Duration,
};

use bytes::Bytes;
use netsim_packets::{
    EthernetFrame, IP_P_TCP, IP_P_UDP, Ipv4Builder, Ipv6Builder, MacAddr, TcpBuilder,
    UdpPacketBuilder, ether_type,
};
use slirp::{Config, ConnectionArgs, Slirp, SlirpRequest, SlirpResponse};
use slirp_host::TokioHost;
use tokio::sync::mpsc;
use zerocopy::FromBytes;

fn create_udp_packet(
    src_ip: Ipv4Addr,
    dst_ip: Ipv4Addr,
    src_port: u16,
    dst_port: u16,
    payload: &[u8],
) -> Bytes {
    let mut udp_data = vec![0u8; 8 + payload.len()];
    let mut udp_builder = UdpPacketBuilder::new(&mut udp_data, src_port, dst_port).unwrap();
    udp_builder.payload_mut()[..payload.len()].copy_from_slice(payload);
    udp_builder.build();

    let mut ip_data = vec![0u8; 20 + udp_data.len()];
    let mut ipv4_builder = Ipv4Builder::new(&mut ip_data, IP_P_UDP, src_ip, dst_ip).unwrap();
    ipv4_builder.payload(&udp_data).unwrap();
    ipv4_builder.build().unwrap();

    let mut eth_data = vec![0u8; 14 + ip_data.len()];
    let (eth_header_slice, eth_payload_slice) = eth_data.split_at_mut(14);
    let eth_frame = EthernetFrame::mut_from_bytes(eth_header_slice).unwrap();
    eth_frame.dst_addr = MacAddr { bytes: [0xde, 0xad, 0xbe, 0xef, 0x12, 0x35] }; // gateway
    eth_frame.src_addr = MacAddr { bytes: [0xde, 0xad, 0xbe, 0xef, 0x12, 0x34] }; // guest
    eth_frame.ethertype = 0x0800.into();
    eth_payload_slice.copy_from_slice(&ip_data);

    Bytes::from(eth_data)
}

#[tokio::test]
async fn test_host_save_restore() {
    // Slirp rewrites the guest's DNS query to 8.8.8.8:53, so this test needs a
    // route to external DNS. A UDP connect() only resolves the route, it sends
    // nothing, so skip where there is no egress rather than time out.
    if std::net::UdpSocket::bind("0.0.0.0:0")
        .and_then(|socket| socket.connect("8.8.8.8:53"))
        .is_err()
    {
        eprintln!("skipping test_host_save_restore: no route to external DNS");
        return;
    }

    // 1. Create channels
    let (slirp_request_sender, mut slirp_request_receiver) = mpsc::channel(100);
    let (slirp_response_sender, slirp_response_receiver) = mpsc::channel(100);
    let (guest_packet_sender, _guest_packet_receiver) = mpsc::channel(100);
    let (test_response_sender, mut test_response_receiver) = mpsc::channel(100);

    // 2. Create and spawn real Slirp task
    let config = Config::default();
    let mut slirp = Slirp::new(config.clone());

    let slirp_response_sender_clone = slirp_response_sender.clone();
    tokio::spawn(async move {
        while let Some(request) = slirp_request_receiver.recv().await {
            let responses = slirp.handle_request(request);
            for response in responses {
                if slirp_response_sender_clone.send(response).await.is_err() {
                    break;
                }
            }
        }
    });

    // 3. Create TokioHost from channels, providing test_response_sender!
    let mut host = TokioHost::from_channels(
        slirp_request_sender.clone(),
        slirp_response_receiver,
        guest_packet_sender,
        Some(test_response_sender),
        config.clone(),
    );

    // Get the cloneable handle
    let handle = host.handle();

    // Spawn host.run() in a background task
    tokio::spawn(async move {
        host.run().await;
    });

    // Wait for the host loop to start
    tokio::time::sleep(Duration::from_millis(50)).await;

    // 4. Send a UDP DNS packet to Slirp to trigger flow creation
    let dns_payload = [
        0x12, 0x34, // TXID
        0x01, 0x00, // Flags (RD=1)
        0x00, 0x01, // Questions = 1
        0x00, 0x00, // Answers = 0
        0x00, 0x00, // Authority = 0
        0x00, 0x00, // Additional = 0
        6, b'g', b'o', b'o', b'g', b'l', b'e', 3, b'c', b'o', b'm', 0, // Null terminator
        0x00, 0x01, // Type A
        0x00, 0x01, // Class IN
    ];
    let dns_packet = create_udp_packet(
        config.guest_ipv4,
        config.host_ipv4, // gateway (dns proxy will intercept)
        12345,
        53, // DNS port
        &dns_payload,
    );

    slirp_request_sender.send(SlirpRequest::Packet(dns_packet)).await.unwrap();

    // 5. Verify host receives EstablishConnection for the initial flow
    let resp1 = tokio::time::timeout(Duration::from_millis(2000), test_response_receiver.recv())
        .await
        .unwrap()
        .unwrap();

    let initial_conn_id = match resp1 {
        SlirpResponse::EstablishConnection(id, ConnectionArgs::Udp(args)) => {
            assert_eq!(args.destination, "8.8.8.8:53".parse().unwrap()); // translated to external DNS
            assert_eq!(args.guest_ip, IpAddr::V4(config.guest_ipv4));
            assert_eq!(args.guest_port, 12345);
            id
        }
        _ => panic!("Expected EstablishConnection, got {resp1:?}"),
    };

    // Ignore the subsequent WriteToConnection response (forwarding the packet
    // payload)
    let resp2 = tokio::time::timeout(Duration::from_millis(2000), test_response_receiver.recv())
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(resp2, SlirpResponse::WriteToConnection(_, _)));

    // 6. Call save_state() via the handle!
    let state_bytes = handle.save_state().await.unwrap();
    assert!(!state_bytes.is_empty());

    // 7. Call restore_state() via the handle!
    handle.restore_state(&state_bytes).await.unwrap();

    // 8. Verify host receives Reset, followed by the restored EstablishConnection!

    // A. Verify Reset event is received
    let resp_reset =
        tokio::time::timeout(Duration::from_millis(2000), test_response_receiver.recv())
            .await
            .unwrap()
            .unwrap();
    assert_eq!(resp_reset, SlirpResponse::Reset);

    // B. Verify restored EstablishConnection is received
    let resp_restore =
        tokio::time::timeout(Duration::from_millis(2000), test_response_receiver.recv())
            .await
            .unwrap()
            .unwrap();

    match resp_restore {
        SlirpResponse::EstablishConnection(id, ConnectionArgs::Udp(args)) => {
            assert_eq!(id, initial_conn_id); // must match the original connection ID!
            assert_eq!(args.destination, "8.8.8.8:53".parse().unwrap());
            assert_eq!(args.guest_ip, IpAddr::V4(config.guest_ipv4));
            assert_eq!(args.guest_port, 12345);
        }
        _ => panic!("Expected restored EstablishConnection, got {resp_restore:?}"),
    }

    // Ensure no more unexpected messages are pending
    tokio::time::sleep(Duration::from_millis(50)).await;
}

#[tokio::test]
async fn test_hostfwd_tcp_integration() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener); // close it

    let host_addr: std::net::SocketAddr = format!("127.0.0.1:{}", port).parse().unwrap();
    let guest_addr: std::net::SocketAddr = "10.0.2.15:80".parse().unwrap();

    let config = Config {
        hostfwd: vec![slirp::HostFwdRule { proto: slirp::Proto::Tcp, host_addr, guest_addr }],
        ..Config::default()
    };

    // 1. Create channels for mock Slirp
    let (slirp_request_sender, mut slirp_request_receiver) = mpsc::channel(100);
    let (slirp_response_sender, slirp_response_receiver) = mpsc::channel(100);
    let (guest_packet_sender, _guest_packet_receiver) = mpsc::channel(100);

    // 2. Spawn a mock Slirp task to intercept requests
    let (mock_request_sender, mut mock_request_receiver) = mpsc::channel(100);
    let slirp_response_sender_clone = slirp_response_sender.clone();
    tokio::spawn(async move {
        while let Some(request) = slirp_request_receiver.recv().await {
            mock_request_sender.send(request).await.unwrap();
        }
    });

    // 3. Create TokioHost
    let mut host = TokioHost::from_channels(
        slirp_request_sender.clone(),
        slirp_response_receiver,
        guest_packet_sender,
        None,
        config.clone(),
    );

    // Spawn host.run() in a background task
    tokio::spawn(async move {
        host.run().await;
    });

    // Wait for the hostfwd listener to start
    tokio::time::sleep(Duration::from_millis(100)).await;

    // 4. Connect to the hostfwd listener from the host!
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpStream,
    };

    // Retrying connection to allow time for the background host.run() to bind to
    // the port.
    let mut client_stream = None;
    for _ in 0..10 {
        if let Ok(stream) = TcpStream::connect(host_addr).await {
            client_stream = Some(stream);
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let mut client_stream =
        client_stream.expect("Failed to connect to hostfwd listener after retries");

    // 5. Verify mock Slirp receives AcceptIncoming!
    let req1 = tokio::time::timeout(Duration::from_millis(2000), mock_request_receiver.recv())
        .await
        .unwrap()
        .unwrap();

    let conn_id = match req1 {
        SlirpRequest::AcceptIncoming { conn_id, host_addr: h_addr, guest_addr: g_addr } => {
            assert_eq!(h_addr, host_addr);
            assert_eq!(g_addr, guest_addr);
            conn_id
        }
        _ => panic!("Expected AcceptIncoming, got {req1:?}"),
    };

    // 6. Host client sends data
    let test_data = b"hello_from_host_client";
    client_stream.write_all(test_data).await.unwrap();

    // 7. Verify mock Slirp receives SlirpRequest::Data!
    let req2 = tokio::time::timeout(Duration::from_millis(2000), mock_request_receiver.recv())
        .await
        .unwrap()
        .unwrap();

    match req2 {
        SlirpRequest::Data(id, data) => {
            assert_eq!(id, conn_id);
            assert_ref_eq(data.as_ref(), test_data);
        }
        _ => panic!("Expected Data, got {req2:?}"),
    }

    // 8. Mock Slirp sends data back to host client
    let reply_data = b"reply_from_slirp_core";
    slirp_response_sender_clone
        .send(SlirpResponse::WriteToConnection(conn_id, Bytes::copy_from_slice(reply_data)))
        .await
        .unwrap();

    // 9. Verify host client receives the reply!
    let mut buf = vec![0u8; reply_data.len()];
    tokio::time::timeout(Duration::from_millis(2000), client_stream.read_exact(&mut buf))
        .await
        .unwrap()
        .unwrap();
    assert_ref_eq(&buf, reply_data);

    // 10. Host client closes connection
    drop(client_stream);

    // 11. Verify mock Slirp receives RemoteClosed or ConnectionClosed!
    let req3 = tokio::time::timeout(Duration::from_millis(2000), mock_request_receiver.recv())
        .await
        .unwrap()
        .unwrap();

    assert!(
        matches!(req3, SlirpRequest::RemoteClosed(id) | SlirpRequest::ConnectionClosed(id) if id == conn_id),
        "Expected RemoteClosed or ConnectionClosed for {conn_id}, got {req3:?}"
    );
}

// Helper macro/fn to assert slice equality without requiring identical type
// references
fn assert_ref_eq<T: AsRef<[u8]>, U: AsRef<[u8]>>(a: T, b: U) {
    assert_eq!(a.as_ref(), b.as_ref());
}

async fn run_mock_socks5_server(listener: tokio::net::TcpListener) {
    use std::net::SocketAddr;

    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpStream,
    };

    while let Ok((mut client_stream, _)) = listener.accept().await {
        tokio::spawn(async move {
            let mut buf = [0u8; 3];
            client_stream.read_exact(&mut buf).await.unwrap();
            assert_eq!(buf, [5, 1, 0]); // Greeting

            client_stream.write_all(&[5, 0]).await.unwrap(); // No Auth

            let mut req_header = [0u8; 4];
            client_stream.read_exact(&mut req_header).await.unwrap();
            assert_eq!(req_header[0], 5);
            assert_eq!(req_header[1], 1); // CONNECT
            assert_eq!(req_header[2], 0);

            let atyp = req_header[3];
            let dest_addr = match atyp {
                1 => {
                    let mut ip = [0u8; 4];
                    client_stream.read_exact(&mut ip).await.unwrap();
                    let mut port = [0u8; 2];
                    client_stream.read_exact(&mut port).await.unwrap();
                    SocketAddr::new(IpAddr::V4(Ipv4Addr::from(ip)), u16::from_be_bytes(port))
                }
                _ => panic!("Unsupported address type in mock SOCKS5"),
            };

            // Connect to target!
            let target_stream = TcpStream::connect(dest_addr).await.unwrap();

            // Reply success: VER=5, REP=0, RSV=0, ATYP=1, BND.ADDR=0.0.0.0, BND.PORT=0
            client_stream.write_all(&[5, 0, 0, 1, 0, 0, 0, 0, 0, 0]).await.unwrap();

            // Shuttle data!
            let (mut cr, mut cw) = client_stream.into_split();
            let (mut tr, mut tw) = target_stream.into_split();

            let t1 = tokio::spawn(async move {
                tokio::io::copy(&mut cr, &mut tw).await.ok();
            });
            let t2 = tokio::spawn(async move {
                tokio::io::copy(&mut tr, &mut cw).await.ok();
            });
            let _ = tokio::join!(t1, t2);
        });
    }
}

async fn run_mock_target_server(listener: tokio::net::TcpListener) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    while let Ok((mut stream, _)) = listener.accept().await {
        tokio::spawn(async move {
            let mut buf = [0u8; 1024];
            if let Ok(n) = stream.read(&mut buf).await
                && n > 0
            {
                // Echo back with a prefix
                let mut resp = b"mock_target: ".to_vec();
                resp.extend_from_slice(&buf[..n]);
                stream.write_all(&resp).await.ok();
            }
        });
    }
}

#[allow(clippy::too_many_arguments)]
fn create_tcp_packet(
    src_ip: Ipv4Addr,
    dst_ip: Ipv4Addr,
    src_port: u16,
    dst_port: u16,
    seq: u32,
    ack: u32,
    flags: u16,
    payload: &[u8],
) -> Bytes {
    use netsim_packets::{IP_P_TCP, TcpBuilder};

    let mut tcp_data = vec![0u8; 20 + payload.len()];
    let mut tcp_builder = TcpBuilder::new(&mut tcp_data, src_ip, dst_ip).unwrap();
    tcp_builder
        .source_port(src_port)
        .dest_port(dst_port)
        .sequence_num(seq)
        .ack_num(ack)
        .flags(flags)
        .window_size(8192);
    tcp_builder.payload_mut().unwrap()[..payload.len()].copy_from_slice(payload);
    tcp_builder.build();

    let mut ip_data = vec![0u8; 20 + tcp_data.len()];
    let mut ipv4_builder = Ipv4Builder::new(&mut ip_data, IP_P_TCP, src_ip, dst_ip).unwrap();
    ipv4_builder.payload(&tcp_data).unwrap();
    ipv4_builder.build().unwrap();

    let mut eth_data = vec![0u8; 14 + ip_data.len()];
    let (eth_header_slice, eth_payload_slice) = eth_data.split_at_mut(14);
    let eth_frame = EthernetFrame::mut_from_bytes(eth_header_slice).unwrap();
    eth_frame.dst_addr = MacAddr { bytes: [0xde, 0xad, 0xbe, 0xef, 0x12, 0x35] }; // gateway
    eth_frame.src_addr = MacAddr { bytes: [0xde, 0xad, 0xbe, 0xef, 0x12, 0x34] }; // guest
    eth_frame.ethertype = 0x0800.into();
    eth_payload_slice.copy_from_slice(&ip_data);

    Bytes::from(eth_data)
}

#[tokio::test]
async fn test_socks5_proxy_integration() {
    use std::net::SocketAddr;

    use tokio::net::TcpListener;

    // 1. Start mock SOCKS5 server
    let socks5_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let socks5_addr = socks5_listener.local_addr().unwrap();
    tokio::spawn(run_mock_socks5_server(socks5_listener));

    // 2. Start mock target TCP server
    let target_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let target_addr = target_listener.local_addr().unwrap();
    tokio::spawn(run_mock_target_server(target_listener));

    // 3. Create config with SOCKS5 proxy and a guestfwd rule to the target!
    let virtual_addr: SocketAddr = "10.0.2.100:80".parse().unwrap();
    let config = Config {
        socks5_proxy: Some(socks5_addr),
        guestfwd: vec![slirp::GuestFwdRule { virtual_addr, host_addr: target_addr }],
        ..Config::default()
    };

    // 4. Spin up TokioHost and Slirp task
    let (slirp_request_sender, mut slirp_request_receiver) = mpsc::channel(100);
    let (slirp_response_sender, slirp_response_receiver) = mpsc::channel(100);
    let (guest_packet_sender, _guest_packet_receiver) = mpsc::channel(100);
    let (test_response_sender, mut test_response_receiver) = mpsc::channel(100);

    let mut slirp = Slirp::new(config.clone());
    let slirp_response_sender_clone = slirp_response_sender.clone();
    tokio::spawn(async move {
        while let Some(request) = slirp_request_receiver.recv().await {
            let responses = slirp.handle_request(request);
            for response in responses {
                slirp_response_sender_clone.send(response).await.ok();
            }
        }
    });

    let mut host = TokioHost::from_channels(
        slirp_request_sender.clone(),
        slirp_response_receiver,
        guest_packet_sender,
        Some(test_response_sender),
        config.clone(),
    );

    tokio::spawn(async move {
        host.run().await;
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    // 5. Guest sends TCP SYN to virtual IP:port (10.0.2.100:80)
    let guest_port = 12345;
    let guest_iss = 1000;
    let syn_packet = create_tcp_packet(
        config.guest_ipv4,
        match virtual_addr.ip() {
            IpAddr::V4(ip) => ip,
            _ => panic!(),
        },
        guest_port,
        virtual_addr.port(),
        guest_iss,
        0,
        netsim_packets::TCP_FLAG_SYN,
        &[],
    );

    slirp_request_sender.send(SlirpRequest::Packet(syn_packet)).await.unwrap();

    // 6. Verify we receive EstablishConnection (handled by SOCKS5!) and a SYN-ACK
    //    packet back!
    let resp1 = tokio::time::timeout(Duration::from_millis(2000), test_response_receiver.recv())
        .await
        .unwrap()
        .unwrap();
    let _conn_id = match resp1 {
        SlirpResponse::EstablishConnection(id, ConnectionArgs::Tcp(args)) => {
            assert_eq!(args.destination, target_addr); // Target is the host_addr of the guestfwd!
            id
        }
        _ => panic!("Expected EstablishConnection, got {resp1:?}"),
    };

    // Wait for the SYN-ACK packet to arrive at the guest
    let mut syn_ack_packet = None;
    while syn_ack_packet.is_none() {
        let resp = tokio::time::timeout(Duration::from_millis(2000), test_response_receiver.recv())
            .await
            .unwrap()
            .unwrap();
        if let SlirpResponse::Packet(p) = resp {
            let Some((eth_frame, eth_payload)) = EthernetFrame::parse(&p) else {
                continue;
            };
            if eth_frame.ethertype.get() != 0x0800 {
                continue;
            }
            let Some((_, ip_payload)) = netsim_packets::Ipv4Header::parse(eth_payload) else {
                continue;
            };
            if netsim_packets::TcpHeader::parse(ip_payload).is_some() {
                syn_ack_packet = Some(p);
                break;
            }
        }
    }
    let syn_ack = syn_ack_packet.unwrap();
    let (_, eth_payload) = EthernetFrame::parse(&syn_ack).unwrap();
    let (_, ip_payload) = netsim_packets::Ipv4Header::parse(eth_payload).unwrap();
    let (tcp_header, _) = netsim_packets::TcpHeader::parse(ip_payload).unwrap();
    assert!(tcp_header.syn());
    assert!(tcp_header.ack());
    let gateway_iss = tcp_header.sequence_num.get();
    let ack_num = tcp_header.ack_num.get();
    assert_eq!(ack_num, guest_iss + 1);

    // 7. Guest sends ACK to complete the handshake
    let ack_packet = create_tcp_packet(
        config.guest_ipv4,
        match virtual_addr.ip() {
            IpAddr::V4(ip) => ip,
            _ => panic!(),
        },
        guest_port,
        virtual_addr.port(),
        guest_iss + 1,
        gateway_iss + 1,
        netsim_packets::TCP_FLAG_ACK,
        &[],
    );
    slirp_request_sender.send(SlirpRequest::Packet(ack_packet)).await.unwrap();

    // Wait for TCP state machine to process it (transition to Established)
    tokio::time::sleep(Duration::from_millis(100)).await;

    // 8. Guest sends TCP data "hello"
    let data_payload = b"hello";
    let data_packet = create_tcp_packet(
        config.guest_ipv4,
        match virtual_addr.ip() {
            IpAddr::V4(ip) => ip,
            _ => panic!(),
        },
        guest_port,
        virtual_addr.port(),
        guest_iss + 1,
        gateway_iss + 1,
        netsim_packets::TCP_FLAG_ACK | netsim_packets::TCP_FLAG_PSH,
        data_payload,
    );
    slirp_request_sender.send(SlirpRequest::Packet(data_packet)).await.unwrap();

    // 9. Verify SOCKS5 proxy forwarded it and target replied "mock_target: hello"!
    let mut data_reply_packet = None;
    for _ in 0..10 {
        if let Ok(Some(SlirpResponse::Packet(p))) =
            tokio::time::timeout(Duration::from_millis(2000), test_response_receiver.recv()).await
        {
            // Parse it to check if it contains the target's reply
            let Some((eth_frame, eth_payload)) = EthernetFrame::parse(&p) else {
                continue;
            };
            if eth_frame.ethertype.get() != 0x0800 {
                continue;
            }
            let Some((_, ip_payload)) = netsim_packets::Ipv4Header::parse(eth_payload) else {
                continue;
            };
            let Some((_tcp_header, tcp_payload)) = netsim_packets::TcpHeader::parse(ip_payload)
            else {
                continue;
            };
            if tcp_payload.starts_with(b"mock_target: hello") {
                data_reply_packet = Some(p);
                break;
            }
        }
    }

    assert!(data_reply_packet.is_some(), "Failed to receive echoed data from mock SOCKS5 target");
}

#[cfg(target_os = "linux")]
fn create_icmp_packet(
    src_ip: Ipv4Addr,
    dst_ip: Ipv4Addr,
    icmp_type: u8,
    icmp_code: u8,
    id: u16,
    seq: u16,
    payload: &[u8],
) -> Bytes {
    use netsim_packets::{IP_P_ICMP, IcmpHeader};
    use zerocopy::FromBytes;

    let icmp_header_len = std::mem::size_of::<IcmpHeader>();
    let total_icmp_len = icmp_header_len + payload.len();

    let mut icmp_data = vec![0u8; total_icmp_len];
    let (icmp_hdr_slice, echo_payload_slice) = icmp_data.split_at_mut(icmp_header_len);
    let icmp_header = IcmpHeader::mut_from_bytes(icmp_hdr_slice).unwrap();
    icmp_header.icmp_type = icmp_type;
    icmp_header.icmp_code = icmp_code;
    icmp_header.icmp_checksum = 0.into();

    icmp_header.rest[..2].copy_from_slice(&id.to_be_bytes());
    icmp_header.rest[2..].copy_from_slice(&seq.to_be_bytes());
    echo_payload_slice.copy_from_slice(payload);

    let checksum = netsim_packets::ipv4_checksum(&icmp_data);
    let icmp_header_mut = IcmpHeader::mut_from_bytes(&mut icmp_data[..icmp_header_len]).unwrap();
    icmp_header_mut.icmp_checksum.set(checksum);

    // Now wrap in IP and Ethernet!
    let mut ip_data = vec![0u8; 20 + total_icmp_len];
    let mut ipv4_builder = Ipv4Builder::new(&mut ip_data, IP_P_ICMP, src_ip, dst_ip).unwrap();
    ipv4_builder.payload(&icmp_data).unwrap();
    ipv4_builder.build().unwrap();

    let mut eth_data = vec![0u8; 14 + ip_data.len()];
    let (eth_header_slice, eth_payload_slice) = eth_data.split_at_mut(14);
    let eth_frame = EthernetFrame::mut_from_bytes(eth_header_slice).unwrap();
    eth_frame.dst_addr = MacAddr { bytes: [0xde, 0xad, 0xbe, 0xef, 0x12, 0x35] }; // gateway
    eth_frame.src_addr = MacAddr { bytes: [0xde, 0xad, 0xbe, 0xef, 0x12, 0x34] }; // guest
    eth_frame.ethertype = 0x0800.into();
    eth_payload_slice.copy_from_slice(&ip_data);

    Bytes::from(eth_data)
}

// Linux-only: the ICMP proxy uses a ping socket (SOCK_DGRAM/IPPROTO_ICMP) and
// relies on the kernel rewriting the echo ID to the socket's local port. macOS
// does not provide those semantics.
#[cfg(target_os = "linux")]
#[tokio::test]
async fn test_icmp_proxy_integration() {
    // The ICMP proxy opens a ping socket, which the kernel only permits when
    // the running gid falls inside net.ipv4.ping_group_range. Workstations open
    // that range, but build sandboxes commonly leave it empty, so skip rather
    // than fail where the socket cannot be created.
    if socket2::Socket::new(
        socket2::Domain::IPV4,
        socket2::Type::DGRAM,
        Some(socket2::Protocol::from(1)),
    )
    .is_err()
    {
        eprintln!("skipping test_icmp_proxy_integration: ICMP ping sockets are not permitted");
        return;
    }

    // 1. Create config
    let config = Config::default();

    // 2. Spin up TokioHost and Slirp task
    let (slirp_request_sender, mut slirp_request_receiver) = mpsc::channel(100);
    let (slirp_response_sender, slirp_response_receiver) = mpsc::channel(100);
    let (guest_packet_sender, _guest_packet_receiver) = mpsc::channel(100);
    let (test_response_sender, mut test_response_receiver) = mpsc::channel(100);

    let mut slirp = Slirp::new(config.clone());
    let slirp_response_sender_clone = slirp_response_sender.clone();
    tokio::spawn(async move {
        while let Some(request) = slirp_request_receiver.recv().await {
            let responses = slirp.handle_request(request);
            for response in responses {
                slirp_response_sender_clone.send(response).await.ok();
            }
        }
    });

    let mut host = TokioHost::from_channels(
        slirp_request_sender.clone(),
        slirp_response_receiver,
        guest_packet_sender,
        Some(test_response_sender),
        config.clone(),
    );

    tokio::spawn(async move {
        host.run().await;
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    // 3. Guest sends ICMP Echo Request to 127.0.0.1!
    let ping_id = 0x1234;
    let ping_seq = 1;
    let ping_payload = b"hello_ping_world";
    let ping_packet = create_icmp_packet(
        config.guest_ipv4,
        Ipv4Addr::new(127, 0, 0, 1),
        8, // Echo Request
        0,
        ping_id,
        ping_seq,
        ping_payload,
    );

    // Send to Slirp!
    slirp_request_sender.send(SlirpRequest::Packet(ping_packet)).await.unwrap();

    // 4. Verify host establishes connection and opens socket
    let resp1 = tokio::time::timeout(Duration::from_millis(2000), test_response_receiver.recv())
        .await
        .unwrap()
        .unwrap();

    let _conn_id = match resp1 {
        SlirpResponse::EstablishConnection(id, ConnectionArgs::Icmp(args)) => {
            assert_eq!(args.destination, IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)));
            assert_eq!(args.guest_ip, IpAddr::V4(config.guest_ipv4));
            assert_eq!(args.guest_id, ping_id);
            id
        }
        _ => panic!("Expected EstablishConnection for ICMP, got {resp1:?}"),
    };

    // 5. Wait for the ICMP Echo Reply to arrive at the guest!
    let mut reply_packet = None;
    for _ in 0..10 {
        if let Ok(Some(SlirpResponse::Packet(p))) =
            tokio::time::timeout(Duration::from_millis(1000), test_response_receiver.recv()).await
        {
            // Parse and check if it is the ICMP Echo Reply from 127.0.0.1
            let Some((eth_frame, eth_payload)) = EthernetFrame::parse(&p) else {
                continue;
            };
            if eth_frame.ethertype.get() != 0x0800 {
                continue;
            }
            let Some((_, ip_payload)) = netsim_packets::Ipv4Header::parse(eth_payload) else {
                continue;
            };
            if let Some((icmp_header, icmp_payload)) = netsim_packets::IcmpHeader::parse(ip_payload)
            {
                if icmp_header.icmp_type == 0 {
                    // Echo Reply
                    let identifier = u16::from_be_bytes([icmp_header.rest[0], icmp_header.rest[1]]);
                    let data = icmp_payload;
                    if identifier == ping_id && data.starts_with(ping_payload) {
                        reply_packet = Some(p);
                        break;
                    }
                }
            }
        }
    }

    assert!(reply_packet.is_some(), "Failed to receive real ICMP Echo Reply from 127.0.0.1!");

    // Verify fields of the reply packet!
    let reply = reply_packet.unwrap();
    let (_, eth_payload) = EthernetFrame::parse(&reply).unwrap();
    let (ip_header, ip_payload) = netsim_packets::Ipv4Header::parse(eth_payload).unwrap();
    assert_eq!(Ipv4Addr::from(ip_header.source_addr), Ipv4Addr::new(127, 0, 0, 1));
    assert_eq!(Ipv4Addr::from(ip_header.dest_addr), config.guest_ipv4);

    let (icmp_header, icmp_payload) = netsim_packets::IcmpHeader::parse(ip_payload).unwrap();
    assert_eq!(icmp_header.icmp_type, 0); // Echo Reply
    assert_eq!(icmp_header.icmp_code, 0);

    let identifier = u16::from_be_bytes([icmp_header.rest[0], icmp_header.rest[1]]);
    let sequence_number = u16::from_be_bytes([icmp_header.rest[2], icmp_header.rest[3]]);
    assert_eq!(identifier, ping_id);
    assert_eq!(sequence_number, ping_seq);
    assert_eq!(icmp_payload, ping_payload);

    // Verify ICMP checksum is valid!
    let computed_checksum = netsim_packets::ipv4_checksum(ip_payload);
    assert_eq!(computed_checksum, 0, "ICMP reply checksum is invalid!");
}

struct World {
    config: Config,
    slirp_request_sender: mpsc::Sender<SlirpRequest>,
    test_response_receiver: mpsc::Receiver<SlirpResponse>,
    tasks: Vec<tokio::task::JoinHandle<()>>,
    guest_ip: IpAddr,
    gateway_ip: IpAddr,
    guest_port: u16,
    host_port: u16,
    guest_iss: u32,
    gateway_iss: u32,
}

impl Drop for World {
    fn drop(&mut self) {
        for task in &self.tasks {
            task.abort();
        }
    }
}

impl World {
    async fn new() -> Self {
        let config = Config::default();
        let (slirp_request_sender, mut slirp_request_receiver) = mpsc::channel(100);
        let (slirp_response_sender, slirp_response_receiver) = mpsc::channel(100);
        let (guest_packet_sender, _guest_packet_receiver) = mpsc::channel(100);
        let (test_response_sender, test_response_receiver) = mpsc::channel(100);

        let mut slirp = Slirp::new(config.clone());
        let slirp_response_sender_clone = slirp_response_sender.clone();
        let slirp_task = tokio::spawn(async move {
            while let Some(request) = slirp_request_receiver.recv().await {
                let responses = slirp.handle_request(request);
                for response in responses {
                    slirp_response_sender_clone.send(response).await.ok();
                }
            }
        });

        let mut host = TokioHost::from_channels(
            slirp_request_sender.clone(),
            slirp_response_receiver,
            guest_packet_sender,
            Some(test_response_sender),
            config.clone(),
        );
        let host_task = tokio::spawn(async move {
            host.run().await;
        });
        tokio::time::sleep(Duration::from_millis(50)).await;

        Self {
            config,
            slirp_request_sender,
            test_response_receiver,
            tasks: vec![slirp_task, host_task],
            guest_ip: IpAddr::V4(Ipv4Addr::UNSPECIFIED),
            gateway_ip: IpAddr::V4(Ipv4Addr::UNSPECIFIED),
            guest_port: 0,
            host_port: 0,
            guest_iss: 1000,
            gateway_iss: 0,
        }
    }

    async fn given_ipv4_tcp_echo_server(&mut self) -> u16 {
        use tokio::net::TcpListener;
        let listener =
            TcpListener::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0)).await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let echo_task = tokio::spawn(run_echo_tcp_server(listener));
        self.tasks.push(echo_task);
        self.guest_ip = IpAddr::V4(self.config.guest_ipv4);
        self.gateway_ip = IpAddr::V4(self.config.host_ipv4);
        self.host_port = port;
        port
    }

    async fn given_ipv6_tcp_echo_server(&mut self) -> u16 {
        use tokio::net::TcpListener;
        let listener =
            TcpListener::bind(SocketAddr::new(IpAddr::V6(Ipv6Addr::LOCALHOST), 0)).await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let echo_task = tokio::spawn(run_echo_tcp_server(listener));
        self.tasks.push(echo_task);
        self.guest_ip = IpAddr::V6(self.config.guest_ipv6);
        self.gateway_ip = IpAddr::V6(self.config.host_ipv6);
        self.host_port = port;
        port
    }

    async fn when_guest_connects_tcp_to_gateway(&mut self, guest_port: u16, dst_port: u16) {
        self.guest_port = guest_port;
        self.host_port = dst_port;
        let guest_tcp_addr = SocketAddr::new(self.guest_ip, self.guest_port);
        let gateway_tcp_addr = SocketAddr::new(self.gateway_ip, self.host_port);
        let syn_packet = create_dual_stack_tcp_packet(
            guest_tcp_addr,
            gateway_tcp_addr,
            self.guest_iss,
            0,
            netsim_packets::TCP_FLAG_SYN,
            &[],
        );
        self.slirp_request_sender.send(SlirpRequest::Packet(syn_packet)).await.unwrap();
    }

    async fn then_guest_receives_tcp_syn_ack_from_gateway(&mut self, expected_src_port: u16) {
        let expected_ethertype =
            if self.guest_ip.is_ipv4() { ether_type::IPV4 } else { ether_type::IPV6 };
        let gateway_ip = self.gateway_ip;
        let gateway_iss = recv_matching_packet(&mut self.test_response_receiver, |p| {
            let (eth_frame, eth_payload) = EthernetFrame::parse(p)?;
            if eth_frame.ethertype.get() != expected_ethertype {
                return None;
            }
            let (src_ip, _dst_ip, l4) = parse_ip_endpoints(expected_ethertype, eth_payload)?;
            let (tcp_header, _) = netsim_packets::TcpHeader::parse(l4)?;
            if tcp_header.syn()
                && tcp_header.ack()
                && tcp_header.source_port.get() == expected_src_port
            {
                assert_eq!(src_ip, gateway_ip);
                Some(tcp_header.sequence_num.get())
            } else {
                None
            }
        })
        .await
        .expect("Failed to receive SYN-ACK for gateway TCP");
        self.gateway_iss = gateway_iss;
    }

    async fn when_guest_completes_tcp_handshake_and_sends(&mut self, payload: &[u8]) {
        let guest_tcp_addr = SocketAddr::new(self.guest_ip, self.guest_port);
        let gateway_tcp_addr = SocketAddr::new(self.gateway_ip, self.host_port);
        let ack_packet = create_dual_stack_tcp_packet(
            guest_tcp_addr,
            gateway_tcp_addr,
            self.guest_iss + 1,
            self.gateway_iss + 1,
            netsim_packets::TCP_FLAG_ACK,
            &[],
        );
        self.slirp_request_sender.send(SlirpRequest::Packet(ack_packet)).await.unwrap();
        tokio::time::sleep(Duration::from_millis(50)).await;

        let data_packet = create_dual_stack_tcp_packet(
            guest_tcp_addr,
            gateway_tcp_addr,
            self.guest_iss + 1,
            self.gateway_iss + 1,
            netsim_packets::TCP_FLAG_ACK | netsim_packets::TCP_FLAG_PSH,
            payload,
        );
        self.slirp_request_sender.send(SlirpRequest::Packet(data_packet)).await.unwrap();
    }

    async fn then_guest_receives_tcp_echo_from_gateway(&mut self, expected_payload: &[u8]) {
        let expected_ethertype =
            if self.guest_ip.is_ipv4() { ether_type::IPV4 } else { ether_type::IPV6 };
        let gateway_ip = self.gateway_ip;
        let guest_ip = self.guest_ip;
        let host_port = self.host_port;
        let expected_payload_vec = expected_payload.to_vec();
        let tcp_echo_received = recv_matching_packet(&mut self.test_response_receiver, |p| {
            let (eth_frame, eth_payload) = EthernetFrame::parse(p)?;
            if eth_frame.ethertype.get() != expected_ethertype {
                return None;
            }
            let (src_ip, dst_ip, l4) = parse_ip_endpoints(expected_ethertype, eth_payload)?;
            let (tcp_header, payload) = netsim_packets::TcpHeader::parse(l4)?;
            if tcp_header.source_port.get() == host_port && payload == expected_payload_vec {
                assert_eq!(src_ip, gateway_ip);
                assert_eq!(dst_ip, guest_ip);
                Some(())
            } else {
                None
            }
        })
        .await
        .is_some();
        assert!(
            tcp_echo_received,
            "Failed to receive echoed TCP data from gateway loopback server"
        );
    }
}

async fn recv_matching_packet<T>(
    receiver: &mut mpsc::Receiver<SlirpResponse>,
    predicate: impl Fn(&Bytes) -> Option<T>,
) -> Option<T> {
    for _ in 0..20 {
        let Ok(Some(resp)) =
            tokio::time::timeout(Duration::from_millis(2000), receiver.recv()).await
        else {
            return None;
        };
        if let SlirpResponse::Packet(p) = resp
            && let Some(res) = predicate(&p)
        {
            return Some(res);
        }
    }
    None
}

fn wrap_ethernet(ethertype: u16, ip_bytes: &[u8]) -> Bytes {
    let mut eth_data = vec![0u8; 14 + ip_bytes.len()];
    let (eth_header_slice, eth_payload_slice) = eth_data.split_at_mut(14);
    let eth_frame = EthernetFrame::mut_from_bytes(eth_header_slice).unwrap();
    eth_frame.dst_addr = MacAddr { bytes: [0xde, 0xad, 0xbe, 0xef, 0x12, 0x35] }; // gateway
    eth_frame.src_addr = MacAddr { bytes: [0xde, 0xad, 0xbe, 0xef, 0x12, 0x34] }; // guest
    eth_frame.ethertype = ethertype.into();
    eth_payload_slice.copy_from_slice(ip_bytes);

    Bytes::from(eth_data)
}

fn create_dual_stack_tcp_packet(
    src: SocketAddr,
    dst: SocketAddr,
    seq: u32,
    ack: u32,
    flags: u16,
    payload: &[u8],
) -> Bytes {
    let (ethertype, ip_bytes) = match (src, dst) {
        (SocketAddr::V4(s), SocketAddr::V4(d)) => {
            let mut tcp_data = vec![0u8; 20 + payload.len()];
            let mut tcp_builder = TcpBuilder::new(&mut tcp_data, *s.ip(), *d.ip()).unwrap();
            tcp_builder
                .source_port(s.port())
                .dest_port(d.port())
                .sequence_num(seq)
                .ack_num(ack)
                .flags(flags)
                .window_size(8192);
            tcp_builder.payload(payload).unwrap();
            tcp_builder.build().unwrap();

            let mut ip_data = vec![0u8; 20 + tcp_data.len()];
            let mut ipv4_builder =
                Ipv4Builder::new(&mut ip_data, IP_P_TCP, *s.ip(), *d.ip()).unwrap();
            ipv4_builder.payload(&tcp_data).unwrap();
            ipv4_builder.build().unwrap();
            (ether_type::IPV4, ip_data)
        }
        (SocketAddr::V6(s), SocketAddr::V6(d)) => {
            let mut tcp_data = vec![0u8; 20 + payload.len()];
            let mut tcp_builder = TcpBuilder::new_v6(&mut tcp_data, *s.ip(), *d.ip()).unwrap();
            tcp_builder
                .source_port(s.port())
                .dest_port(d.port())
                .sequence_num(seq)
                .ack_num(ack)
                .flags(flags)
                .window_size(8192);
            tcp_builder.payload(payload).unwrap();
            tcp_builder.build().unwrap();

            let mut ip_data = vec![0u8; 40 + tcp_data.len()];
            let mut ipv6_builder =
                Ipv6Builder::new(&mut ip_data, IP_P_TCP, *s.ip(), *d.ip()).unwrap();
            ipv6_builder.payload(&tcp_data).unwrap();
            ipv6_builder.build().unwrap();
            (ether_type::IPV6, ip_data)
        }
        _ => panic!("Mismatched IP versions for TCP packet: src={src}, dst={dst}"),
    };

    wrap_ethernet(ethertype, &ip_bytes)
}

async fn run_echo_tcp_server(listener: tokio::net::TcpListener) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    while let Ok((mut stream, _)) = listener.accept().await {
        tokio::spawn(async move {
            let mut buf = [0u8; 1024];
            while let Ok(n) = stream.read(&mut buf).await {
                if n == 0 {
                    break;
                }
                if stream.write_all(&buf[..n]).await.is_err() {
                    break;
                }
            }
        });
    }
}

fn parse_ip_endpoints(ethertype: u16, payload: &[u8]) -> Option<(IpAddr, IpAddr, &[u8])> {
    match ethertype {
        ether_type::IPV4 => {
            let (hdr, l4) = netsim_packets::Ipv4Header::parse(payload)?;
            Some((IpAddr::V4(hdr.source_addr.into()), IpAddr::V4(hdr.dest_addr.into()), l4))
        }
        ether_type::IPV6 => {
            let (hdr, l4) = netsim_packets::Ipv6Header::parse(payload)?;
            Some((IpAddr::V6(hdr.source_addr.into()), IpAddr::V6(hdr.dest_addr.into()), l4))
        }
        _ => None,
    }
}

#[tokio::test]
async fn test_gateway_loopback_tcp_ipv4_integration() {
    // Scenario: Guest connects via IPv4 TCP to gateway port and receives echo from host loopback
    // Given an IPv4 TCP echo server on host loopback
    let mut world = World::new().await;
    let host_port = world.given_ipv4_tcp_echo_server().await;

    // When the guest initiates a TCP connection to gateway:<host_port>
    let guest_port = 12345;
    world.when_guest_connects_tcp_to_gateway(guest_port, host_port).await;

    // Then the guest receives a SYN-ACK packet from the gateway
    world.then_guest_receives_tcp_syn_ack_from_gateway(host_port).await;

    // When the guest completes the handshake and sends payload data
    let payload = b"hello_tcp_gateway_loopback";
    world.when_guest_completes_tcp_handshake_and_sends(payload).await;

    // Then the guest receives the echoed payload from the gateway
    world.then_guest_receives_tcp_echo_from_gateway(payload).await;
}

#[tokio::test]
async fn test_gateway_loopback_tcp_ipv6_integration() {
    // Scenario: Guest connects via IPv6 TCP to gateway port and receives echo from host loopback
    // Given an IPv6 TCP echo server on host loopback
    let mut world = World::new().await;
    let host_port = world.given_ipv6_tcp_echo_server().await;

    // When the guest initiates a TCP connection to gateway:<host_port>
    let guest_port = 12345;
    world.when_guest_connects_tcp_to_gateway(guest_port, host_port).await;

    // Then the guest receives a SYN-ACK packet from the gateway
    world.then_guest_receives_tcp_syn_ack_from_gateway(host_port).await;

    // When the guest completes the handshake and sends payload data
    let payload = b"hello_tcp_gateway_loopback";
    world.when_guest_completes_tcp_handshake_and_sends(payload).await;

    // Then the guest receives the echoed payload from the gateway
    world.then_guest_receives_tcp_echo_from_gateway(payload).await;
}
