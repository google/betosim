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

//! # Network utilities for netsim TCP listeners.

use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};

use socket2::{Domain, Protocol, Socket, Type};
use tokio::net::TcpListener;
use tracing::warn;

#[cfg(windows)]
const WSAEAFNOSUPPORT: i32 = 10047;
#[cfg(windows)]
const WSAEPROTONOSUPPORT: i32 = 10043;

/// Checks if an error indicates that IPv4 is unsupported on the host
/// environment.
fn is_ipv4_unsupported(err: &std::io::Error) -> bool {
    if matches!(err.kind(), std::io::ErrorKind::AddrNotAvailable | std::io::ErrorKind::Unsupported)
    {
        return true;
    }
    #[cfg(unix)]
    if matches!(err.raw_os_error(), Some(libc::EAFNOSUPPORT | libc::EPROTONOSUPPORT)) {
        return true;
    }
    #[cfg(windows)]
    if matches!(err.raw_os_error(), Some(WSAEAFNOSUPPORT | WSAEPROTONOSUPPORT)) {
        return true;
    }
    false
}

fn create_listener(domain: Domain, addr: SocketAddr) -> std::io::Result<TcpListener> {
    let socket = Socket::new(domain, Type::STREAM, Some(Protocol::TCP))?;
    if domain == Domain::IPV6 {
        let _ = socket.set_only_v6(true);
    }
    #[cfg(not(windows))]
    socket.set_reuse_address(true)?;
    socket.set_nonblocking(true)?;
    socket.bind(&addr.into())?;
    socket.listen(128)?;
    let std_listener: std::net::TcpListener = socket.into();
    TcpListener::from_std(std_listener)
}

/// Binds a Tokio TCP listener to localhost on the given port in non-blocking
/// mode with `SO_REUSEADDR` enabled on non-Windows platforms.
///
/// Tries IPv4 loopback (`127.0.0.1:<port>`) first. If and only if IPv4 is
/// unsupported on the host (e.g. IPv6-only container environments), falls back
/// to IPv6 loopback (`[::1]:<port>`).
///
/// Fails fast on port conflicts (`ErrorKind::AddrInUse`).
pub fn bind_tcp_loopback(port: u16) -> std::io::Result<TcpListener> {
    match create_listener(Domain::IPV4, SocketAddr::from((Ipv4Addr::LOCALHOST, port))) {
        Ok(l) => Ok(l),
        Err(e) => {
            if is_ipv4_unsupported(&e) {
                warn!(error = %e, "IPv4 loopback unavailable on host, trying [::1]:{port}");
                create_listener(Domain::IPV6, SocketAddr::from((Ipv6Addr::LOCALHOST, port)))
            } else {
                Err(e)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    use super::*;

    #[test]
    fn test_is_ipv4_unsupported_classification() {
        assert!(is_ipv4_unsupported(&std::io::Error::new(
            std::io::ErrorKind::AddrNotAvailable,
            "test"
        )));
        assert!(is_ipv4_unsupported(&std::io::Error::new(std::io::ErrorKind::Unsupported, "test")));

        #[cfg(unix)]
        {
            assert!(is_ipv4_unsupported(&std::io::Error::from_raw_os_error(libc::EAFNOSUPPORT)));
            assert!(is_ipv4_unsupported(&std::io::Error::from_raw_os_error(libc::EPROTONOSUPPORT)));
            assert!(!is_ipv4_unsupported(&std::io::Error::from_raw_os_error(libc::EADDRINUSE)));
            assert!(!is_ipv4_unsupported(&std::io::Error::from_raw_os_error(libc::ECONNREFUSED)));
        }

        #[cfg(windows)]
        {
            assert!(is_ipv4_unsupported(&std::io::Error::from_raw_os_error(WSAEAFNOSUPPORT)));
            assert!(is_ipv4_unsupported(&std::io::Error::from_raw_os_error(WSAEPROTONOSUPPORT)));
            assert!(!is_ipv4_unsupported(&std::io::Error::from_raw_os_error(10048)));
            assert!(!is_ipv4_unsupported(&std::io::Error::from_raw_os_error(10061)));
        }

        // Ensure non-fallback errors return false
        assert!(!is_ipv4_unsupported(&std::io::Error::new(std::io::ErrorKind::AddrInUse, "test")));
        assert!(!is_ipv4_unsupported(&std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "test"
        )));
        assert!(!is_ipv4_unsupported(&std::io::Error::new(
            std::io::ErrorKind::ConnectionRefused,
            "test"
        )));
    }

    #[tokio::test]
    async fn test_bind_tcp_loopback_ephemeral() {
        let listener = bind_tcp_loopback(0).expect("Failed to bind ephemeral port");
        let port = listener.local_addr().unwrap().port();
        assert!(port > 0);
    }

    #[tokio::test]
    async fn test_bind_tcp_loopback_conflict() {
        let _listener = bind_tcp_loopback(0).expect("Failed to bind ephemeral port");
        let port = _listener.local_addr().unwrap().port();
        // Second bind to the same fixed port should fail fast on AddrInUse
        let conflict = bind_tcp_loopback(port);
        assert!(conflict.is_err());
        assert_eq!(conflict.unwrap_err().kind(), std::io::ErrorKind::AddrInUse);
    }

    #[cfg(not(windows))]
    #[tokio::test]
    async fn test_bind_tcp_loopback_reuseaddr() {
        let listener = bind_tcp_loopback(0).expect("Failed to bind ephemeral port");
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        // Immediately rebinding to the recently closed port should succeed due to
        // SO_REUSEADDR on Unix
        let rebound = bind_tcp_loopback(port);
        assert!(rebound.is_ok());
    }

    #[tokio::test]
    async fn test_bind_tcp_loopback_stream_roundtrip() {
        let listener = bind_tcp_loopback(0).expect("Failed to bind ephemeral port");
        let addr = listener.local_addr().unwrap();

        let client_task = tokio::spawn(async move {
            tokio::net::TcpStream::connect(addr).await.expect("Client failed to connect")
        });

        let (mut server_stream, _) = listener.accept().await.expect("Server failed to accept");
        let mut client_stream = client_task.await.expect("Client task panicked");

        client_stream.write_all(b"ping").await.unwrap();
        let mut buf = [0u8; 4];
        server_stream.read_exact(&mut buf).await.unwrap();
        assert_eq!(&buf, b"ping");
    }
}
