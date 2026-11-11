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

use std::{
    net::{Ipv4Addr, SocketAddr, SocketAddrV4},
    time::Duration,
};

use bytes::Bytes;
use netsim_packets::{IP_P_UDP, PacketBuilder, ether_type};
use tokio::sync::mpsc::UnboundedSender;
use tracing::{debug, info, warn};

const MDNS_IP: Ipv4Addr = Ipv4Addr::new(224, 0, 0, 251);
const MDNS_PORT: u16 = 5353;
const MDNS_MULTICAST_MAC: [u8; 6] = [0x01, 0x00, 0x5E, 0x00, 0x00, 0xFB];
const MDNS_SRC_MAC: [u8; 6] = [0x02, 0x15, 0xB2, 0x00, 0x00, 0x01];
const SLIRP_GATEWAY_IP: Ipv4Addr = Ipv4Addr::new(10, 0, 2, 2);

const MDNS_MAX_PACKET_SIZE: usize = 9000;
const RECV_ERR_RETRY_DELAY: Duration = Duration::from_millis(50);
const RECV_ERR_LOG_INTERVAL: Duration = Duration::from_secs(5);

/// Rewrites host/container socket source IPv4 addresses to the SLIRP virtual
/// gateway IP (`10.0.2.2`) so guest Wi-Fi clients see an on-link sender whose
/// follow-up unicast responses route back to the host/container loopback.
fn resolve_mdns_source_ip(_src_ip: Ipv4Addr) -> Ipv4Addr {
    SLIRP_GATEWAY_IP
}

fn create_mdns_frame(packet: &[u8], ip_addr: Ipv4Addr) -> Vec<u8> {
    let src_ip = resolve_mdns_source_ip(ip_addr);
    PacketBuilder::new(MDNS_MULTICAST_MAC, MDNS_SRC_MAC, ether_type::IPV4)
        .ipv4(src_ip.octets(), MDNS_IP.octets(), IP_P_UDP)
        .udp(MDNS_PORT, MDNS_PORT)
        .payload(packet)
}

#[cfg(windows)]
const WSAEBADF: i32 = 10009;
#[cfg(windows)]
const WSAENOTSOCK: i32 = 10038;
#[cfg(windows)]
const ERROR_INVALID_HANDLE: i32 = 6;

fn is_fatal_recv_error(error: &std::io::Error) -> bool {
    #[cfg(unix)]
    {
        matches!(error.raw_os_error(), Some(libc::EBADF | libc::ENOTSOCK))
    }
    #[cfg(windows)]
    {
        matches!(error.raw_os_error(), Some(WSAEBADF | WSAENOTSOCK | ERROR_INVALID_HANDLE))
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = error;
        false
    }
}

trait MdnsRecvSocket: Send {
    fn recv_from(
        &mut self,
        buf: &mut [u8],
    ) -> impl std::future::Future<Output = std::io::Result<(usize, SocketAddr)>> + Send;
}

impl MdnsRecvSocket for tokio::net::UdpSocket {
    async fn recv_from(&mut self, buf: &mut [u8]) -> std::io::Result<(usize, SocketAddr)> {
        tokio::net::UdpSocket::recv_from(self, buf).await
    }
}

async fn forward_mdns_loop<S: MdnsRecvSocket>(
    socket: &mut S,
    tx: UnboundedSender<Bytes>,
    retry_delay: Duration,
) -> Result<(), String> {
    let mut buf = vec![0u8; MDNS_MAX_PACKET_SIZE];
    let mut last_warn_time: Option<std::time::Instant> = None;
    let mut suppressed_errors: u64 = 0;

    loop {
        let recv_result = tokio::select! {
            () = tx.closed() => {
                info!("mDNS channel closed, stopping forwarder");
                return Ok(());
            }
            res = socket.recv_from(&mut buf) => res,
        };

        match recv_result {
            Ok((size, src_addr)) => {
                let Some(packet) = buf.get(..size) else {
                    continue;
                };
                if let SocketAddr::V4(socket_addr_v4) = src_addr {
                    debug!("Received {} bytes from {:?}", packet.len(), socket_addr_v4);
                    let ethernet_frame = create_mdns_frame(packet, *socket_addr_v4.ip());
                    if tx.send(Bytes::from(ethernet_frame)).is_err() {
                        warn!(
                            code = crate::err::Code::MdnsTxSendFailed as u32,
                            "Failed to send packet to channel (receiver closed)"
                        );
                        break;
                    }
                } else {
                    warn!(
                        code = crate::err::Code::MdnsIpv6NotSupported as u32,
                        "Forwarding mDNS from IPv6 is not supported: {:?}", src_addr
                    );
                }
            }
            Err(error) => {
                if is_fatal_recv_error(&error) {
                    return Err(format!("recv_from failed: {:?}", error));
                }
                let now = std::time::Instant::now();
                if last_warn_time
                    .is_none_or(|last| now.duration_since(last) >= RECV_ERR_LOG_INTERVAL)
                {
                    warn!(
                        code = crate::err::Code::MdnsSocketRecvFailed as u32,
                        ?error,
                        suppressed_errors,
                        "recv_from failed"
                    );
                    last_warn_time = Some(now);
                    suppressed_errors = 0;
                } else {
                    suppressed_errors += 1;
                    debug!(?error, suppressed_errors, "recv_from failed (rate-limited)");
                }
                tokio::select! {
                    () = tx.closed() => {
                        info!("mDNS channel closed during retry delay, stopping forwarder");
                        return Ok(());
                    }
                    () = tokio::time::sleep(retry_delay) => {}
                }
            }
        }
    }
    Ok(())
}

fn to_nonblocking_tokio_udp(socket: socket2::Socket) -> Result<tokio::net::UdpSocket, String> {
    socket.set_nonblocking(true).map_err(|e| format!("set_nonblocking failed: {:?}", e))?;
    let std_socket = std::net::UdpSocket::from(socket);
    tokio::net::UdpSocket::from_std(std_socket)
        .map_err(|e| format!("Failed to convert to tokio socket: {:?}", e))
}

pub(crate) async fn run_mdns_forwarder(tx: UnboundedSender<Bytes>) -> Result<(), String> {
    // Setup socket using socket2 for REUSEPORT/REUSEADDR and JoinMulticast
    let domain = socket2::Domain::IPV4;
    let socket = socket2::Socket::new(domain, socket2::Type::DGRAM, Some(socket2::Protocol::UDP))
        .map_err(|e| format!("create socket failed: {:?}", e))?;

    socket.set_reuse_address(true).map_err(|e| format!("set ReuseAddr failed: {:?}", e))?;
    #[cfg(not(windows))]
    socket.set_reuse_port(true).map_err(|e| format!("set ReusePort failed: {:?}", e))?;

    socket
        .join_multicast_v4(&MDNS_IP, &Ipv4Addr::UNSPECIFIED)
        .map_err(|e| format!("join_multicast_v4 failed: {:?}", e))?;
    socket
        .set_multicast_loop_v4(false)
        .map_err(|e| format!("set_multicast_loop_v4 failed: {:?}", e))?;

    let addr: SocketAddr = SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, MDNS_PORT).into();
    socket.bind(&addr.into()).map_err(|e| format!("socket bind to {} failed: {:?}", addr, e))?;

    let mut socket = to_nonblocking_tokio_udp(socket)?;

    forward_mdns_loop(&mut socket, tx, RECV_ERR_RETRY_DELAY).await
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    };

    use netsim_packets::{EthernetFrame, Ipv4Header, UdpHeader, ipv4_checksum, udp_checksum};
    use socket2::{Domain, Protocol, Socket, Type};
    use tracing::instrument::WithSubscriber;
    use tracing_subscriber::{Layer, layer::SubscriberExt};

    use super::*;

    type MockRecvSender =
        tokio::sync::mpsc::UnboundedSender<std::io::Result<(Vec<u8>, SocketAddr)>>;
    type CallTimestamps = Arc<Mutex<Vec<std::time::Instant>>>;

    struct MockMdnsSocket {
        rx: tokio::sync::mpsc::UnboundedReceiver<std::io::Result<(Vec<u8>, SocketAddr)>>,
        calls: Arc<Mutex<Vec<std::time::Instant>>>,
    }

    impl MockMdnsSocket {
        fn new() -> (Self, MockRecvSender, CallTimestamps) {
            let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
            let calls = Arc::new(Mutex::new(Vec::new()));
            (Self { rx, calls: calls.clone() }, tx, calls)
        }
    }

    impl MdnsRecvSocket for MockMdnsSocket {
        async fn recv_from(&mut self, buf: &mut [u8]) -> std::io::Result<(usize, SocketAddr)> {
            self.calls.lock().unwrap().push(std::time::Instant::now());
            match self.rx.recv().await {
                Some(Ok((packet, addr))) => {
                    let len = packet.len().min(buf.len());
                    buf[..len].copy_from_slice(&packet[..len]);
                    Ok((len, addr))
                }
                Some(Err(e)) => Err(e),
                None => std::future::pending().await,
            }
        }
    }

    struct LogCounter {
        warn_count: Arc<AtomicUsize>,
        debug_count: Arc<AtomicUsize>,
    }

    impl<S: tracing::Subscriber> Layer<S> for LogCounter {
        fn on_event(
            &self,
            event: &tracing::Event<'_>,
            _ctx: tracing_subscriber::layer::Context<'_, S>,
        ) {
            if event.metadata().target() == "wifi_actor::mdns_forwarder" {
                struct MessageVisitor {
                    is_recv_error: bool,
                }
                impl tracing::field::Visit for MessageVisitor {
                    fn record_debug(
                        &mut self,
                        field: &tracing::field::Field,
                        value: &dyn std::fmt::Debug,
                    ) {
                        if field.name() == "message"
                            && format!("{value:?}").contains("recv_from failed")
                        {
                            self.is_recv_error = true;
                        }
                    }
                }
                let mut visitor = MessageVisitor { is_recv_error: false };
                event.record(&mut visitor);
                if visitor.is_recv_error {
                    match *event.metadata().level() {
                        tracing::Level::WARN => {
                            self.warn_count.fetch_add(1, Ordering::SeqCst);
                        }
                        tracing::Level::DEBUG => {
                            self.debug_count.fetch_add(1, Ordering::SeqCst);
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    #[tokio::test]
    async fn test_forward_mdns_loop_recovers_after_transient_and_oversized_errors() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let payload_8972 = vec![0xABu8; 8972];
        let addr = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(192, 168, 1, 50), 5353));

        let (mut socket, queue_tx, calls) = MockMdnsSocket::new();

        queue_tx.send(Err(std::io::Error::from(std::io::ErrorKind::Interrupted))).unwrap();
        queue_tx.send(Err(std::io::Error::other("oversized datagram"))).unwrap();
        queue_tx.send(Err(std::io::Error::from(std::io::ErrorKind::Interrupted))).unwrap();
        queue_tx.send(Err(std::io::Error::from(std::io::ErrorKind::Interrupted))).unwrap();
        queue_tx.send(Err(std::io::Error::from(std::io::ErrorKind::Interrupted))).unwrap();
        queue_tx.send(Err(std::io::Error::from(std::io::ErrorKind::Interrupted))).unwrap();
        queue_tx.send(Ok((payload_8972.clone(), addr))).unwrap();

        let warn_count = Arc::new(AtomicUsize::new(0));
        let debug_count = Arc::new(AtomicUsize::new(0));
        let counter =
            LogCounter { warn_count: warn_count.clone(), debug_count: debug_count.clone() };
        let subscriber = tracing_subscriber::registry().with(counter);
        let _multi_dispatch_guard = tracing::Dispatch::new(tracing_subscriber::registry());

        let retry_delay = Duration::from_millis(10);
        let forward_handle = tokio::spawn(
            async move { forward_mdns_loop(&mut socket, tx, retry_delay).await }
                .with_subscriber(subscriber),
        );

        let frame_bytes = tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .expect("timeout waiting for 9000-byte IPv4 mDNS packet")
            .expect("channel closed unexpectedly");

        let (_eth, ip_bytes) = EthernetFrame::parse(&frame_bytes).expect("valid ethernet frame");
        assert_eq!(ip_bytes.len(), 9000);
        let (_ipv4, udp_bytes) = Ipv4Header::parse(ip_bytes).expect("valid ipv4 header");
        assert_eq!(udp_bytes.len(), 8980);
        let (_udp, udp_payload) = UdpHeader::parse(udp_bytes).expect("valid udp header");
        assert_eq!(udp_payload.len(), 8972);
        assert_eq!(udp_payload, &payload_8972[..]);

        queue_tx.send(Err(std::io::Error::from(std::io::ErrorKind::Interrupted))).unwrap();
        queue_tx.send(Ok((vec![0xCD; 10], addr))).unwrap();

        let second_frame = tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .expect("timeout waiting for second packet after recovery")
            .expect("channel closed unexpectedly");
        let (_eth, ip_bytes) = EthernetFrame::parse(&second_frame).expect("valid ethernet frame");
        let (_ipv4, udp_bytes) = Ipv4Header::parse(ip_bytes).expect("valid ipv4 header");
        let (_udp, udp_payload) = UdpHeader::parse(udp_bytes).expect("valid udp header");
        assert_eq!(udp_payload, &[0xCD; 10]);

        let start = std::time::Instant::now();
        while calls.lock().unwrap().len() < 10 {
            if start.elapsed() > Duration::from_secs(1) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }

        drop(rx);

        let result = tokio::time::timeout(Duration::from_secs(5), forward_handle)
            .await
            .expect("timeout waiting for forward loop to exit on closed channel")
            .expect("forward loop task panicked");
        assert_eq!(result, Ok(()));

        let call_times = calls.lock().unwrap().clone();
        assert_eq!(call_times.len(), 10);
        for pair in call_times[..7].windows(2) {
            assert!(pair[1].duration_since(pair[0]) >= retry_delay);
        }
        assert!(call_times[6].duration_since(call_times[0]) < Duration::from_millis(300));
        assert!(call_times[8].duration_since(call_times[7]) >= retry_delay);
        assert!(call_times[8].duration_since(call_times[7]) < Duration::from_millis(300));

        assert_eq!(warn_count.load(Ordering::SeqCst), 1);
        assert_eq!(debug_count.load(Ordering::SeqCst), 6);
    }

    #[tokio::test]
    async fn test_forward_mdns_loop_returns_err_on_fatal_socket_error() {
        #[cfg(unix)]
        let fatal_codes = [libc::EBADF, libc::ENOTSOCK];
        #[cfg(windows)]
        let fatal_codes = [WSAEBADF, WSAENOTSOCK, ERROR_INVALID_HANDLE];

        #[cfg(any(unix, windows))]
        for code in fatal_codes {
            let err = std::io::Error::from_raw_os_error(code);
            assert!(is_fatal_recv_error(&err), "expected fatal for raw_os_error {}", code);
        }

        for kind in [
            std::io::ErrorKind::PermissionDenied,
            std::io::ErrorKind::AddrNotAvailable,
            std::io::ErrorKind::InvalidInput,
            std::io::ErrorKind::NotConnected,
            std::io::ErrorKind::BrokenPipe,
            std::io::ErrorKind::Unsupported,
            std::io::ErrorKind::WouldBlock,
            std::io::ErrorKind::TimedOut,
            std::io::ErrorKind::Interrupted,
            std::io::ErrorKind::Other,
        ] {
            let err = std::io::Error::from(kind);
            assert!(!is_fatal_recv_error(&err), "expected non-fatal for {:?}", kind);
        }

        #[cfg(any(unix, windows))]
        for code in fatal_codes {
            let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
            let (mut socket, queue_tx, _calls) = MockMdnsSocket::new();
            queue_tx.send(Err(std::io::Error::from_raw_os_error(code))).unwrap();

            let result = tokio::time::timeout(
                Duration::from_secs(5),
                forward_mdns_loop(&mut socket, tx, Duration::from_millis(10)),
            )
            .await;
            match result {
                Ok(Err(err)) => {
                    assert!(err.contains("recv_from failed"));
                }
                Ok(Ok(())) => panic!("expected Err on fatal socket error {}, got Ok", code),
                Err(elapsed) => panic!("timed out on fatal error {}: {}", code, elapsed),
            }
        }
    }

    #[tokio::test]
    async fn test_forward_mdns_loop_exits_promptly_when_receiver_dropped_during_backoff() {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let (mut socket, queue_tx, calls) = MockMdnsSocket::new();

        queue_tx.send(Err(std::io::Error::from(std::io::ErrorKind::Interrupted))).unwrap();

        let retry_delay = Duration::from_secs(5);
        let forward_handle =
            tokio::spawn(async move { forward_mdns_loop(&mut socket, tx, retry_delay).await });

        let start = std::time::Instant::now();
        while calls.lock().unwrap().is_empty() {
            if start.elapsed() > Duration::from_secs(1) {
                panic!("timed out waiting for first recv_from call");
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }

        tokio::time::sleep(Duration::from_millis(20)).await;

        assert!(
            !forward_handle.is_finished(),
            "forward_mdns_loop should still be sleeping in 5s retry delay before receiver is \
             dropped"
        );

        drop(rx);

        let result = tokio::time::timeout(Duration::from_millis(500), forward_handle)
            .await
            .expect(
                "forward_mdns_loop did not exit promptly within 500ms when rx dropped during 5s \
                 retry delay",
            )
            .expect("forward loop task panicked");
        assert_eq!(result, Ok(()));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_forward_mdns_loop_real_socket_shuts_down_promptly_on_receiver_drop() {
        let socket =
            Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP)).expect("create socket");
        let bind_addr: SocketAddr = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0).into();
        socket.bind(&bind_addr.into()).expect("bind socket");

        let mut rx_socket = to_nonblocking_tokio_udp(socket).expect("to_nonblocking_tokio_udp");
        let local_addr = rx_socket.local_addr().expect("local_addr");

        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let handle = tokio::spawn(async move {
            forward_mdns_loop(&mut rx_socket, tx, RECV_ERR_RETRY_DELAY).await
        });

        let sender = tokio::net::UdpSocket::bind("127.0.0.1:0").await.expect("bind sender");
        sender.send_to(b"mdns-probe", local_addr).await.expect("send_to");

        let frame_bytes = tokio::time::timeout(Duration::from_secs(2), rx.recv())
            .await
            .expect("timed out waiting for mDNS packet")
            .expect("mDNS channel closed unexpectedly");
        let (_eth, ip_bytes) = EthernetFrame::parse(&frame_bytes).expect("valid ethernet frame");
        let (_ipv4, udp_bytes) = Ipv4Header::parse(ip_bytes).expect("valid ipv4 header");
        let (_udp, udp_payload) = UdpHeader::parse(udp_bytes).expect("valid udp header");
        assert_eq!(udp_payload, b"mdns-probe");

        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(!handle.is_finished());

        drop(rx);
        assert!(matches!(
            tokio::time::timeout(Duration::from_secs(2), handle).await,
            Ok(Ok(Ok(())))
        ));
    }

    #[test]
    fn test_create_mdns_frame() {
        let packet = b"sample mdns query payload";
        let ip_addr = Ipv4Addr::new(192, 168, 1, 100);
        let frame = create_mdns_frame(packet, ip_addr);

        let (eth, ip_bytes) = EthernetFrame::parse(&frame).expect("valid ethernet frame");
        assert_eq!(eth.dst_addr.bytes, MDNS_MULTICAST_MAC);
        assert_eq!(eth.src_addr.bytes, MDNS_SRC_MAC);
        assert_eq!(eth.src_addr.bytes[0] & 0x01, 0);
        assert!(!eth.src_addr.is_multicast());
        assert_eq!(eth.ethertype.get(), ether_type::IPV4);

        let (ipv4, udp_bytes) = Ipv4Header::parse(ip_bytes).expect("valid ipv4 header");
        assert_eq!(ipv4.version(), 4);
        assert_eq!(ipv4.ihl(), 5);
        assert_eq!(ipv4.ttl, 64);
        assert_eq!(ipv4.protocol, IP_P_UDP);
        assert_eq!(ipv4.source_addr, SLIRP_GATEWAY_IP.octets());
        assert_eq!(ipv4.dest_addr, MDNS_IP.octets());
        assert_eq!(ipv4.total_length.get() as usize, ip_bytes.len());
        assert_ne!(ipv4.header_checksum.get(), 0);
        assert_eq!(ipv4_checksum(&ip_bytes[..ipv4.header_length()]), 0);

        let (udp, udp_payload) = UdpHeader::parse(udp_bytes).expect("valid udp header");
        assert_eq!(udp.source_port.get(), MDNS_PORT);
        assert_eq!(udp.dest_port.get(), MDNS_PORT);
        assert_eq!(udp.length.get() as usize, udp_bytes.len());
        assert_ne!(udp.checksum.get(), 0);
        assert_eq!(udp_checksum(udp_bytes, SLIRP_GATEWAY_IP, MDNS_IP), u16::MAX);
        assert_eq!(udp_payload, packet);
    }

    #[test]
    fn test_mdns_forwarder_isolates_container_source_ip_to_slirp_gateway() {
        let packet = b"container mdns announcement";
        for outer_ip in [
            Ipv4Addr::new(172, 17, 0, 2),
            Ipv4Addr::new(192, 168, 1, 50),
            Ipv4Addr::LOCALHOST,
            SLIRP_GATEWAY_IP,
        ] {
            let frame = create_mdns_frame(packet, outer_ip);
            let (_eth, ip_bytes) = EthernetFrame::parse(&frame).expect("valid ethernet frame");
            let (ipv4, udp_bytes) = Ipv4Header::parse(ip_bytes).expect("valid ipv4 header");
            assert_eq!(ipv4.source_addr, SLIRP_GATEWAY_IP.octets());
            assert_eq!(ipv4_checksum(&ip_bytes[..ipv4.header_length()]), 0);
            assert_eq!(udp_checksum(udp_bytes, SLIRP_GATEWAY_IP, MDNS_IP), u16::MAX);
        }
    }
}
