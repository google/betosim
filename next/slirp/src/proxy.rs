// Copyright 2024-2026 The Android Open Source Project
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
    io,
    net::{SocketAddr, TcpStream},
};

/// Trait for handling proxy connection results.
pub trait ProxyConnect: Send {
    /// Notifies slirp about the result of a proxy connection attempt.
    fn proxy_connect(&self, fd: i32, addr: SocketAddr);

    /// Returns `true` if this handler accepts an owned [`TcpStream`] via
    /// [`Self::proxy_connect_stream`] instead of a raw descriptor via
    /// [`Self::proxy_connect`].
    fn accepts_stream(&self) -> bool {
        false
    }

    /// Notifies slirp about the result of a proxy connection attempt with an
    /// owned [`TcpStream`].
    ///
    /// On success, the handed-off [`TcpStream`] is in blocking mode (matching
    /// the raw-descriptor path). Callers requiring an asynchronous stream
    /// (such as `tokio::net::TcpStream::from_std`) must explicitly call
    /// [`TcpStream::set_nonblocking`] with `true`.
    fn proxy_connect_stream(&self, _stream: io::Result<TcpStream>, _addr: SocketAddr) {}
}

/// HTTP Proxy callback trait
pub trait ProxyManager: Send + Sync {
    /// Attempts to establish a connection through the proxy.
    fn try_connect(
        &self,
        sockaddr: SocketAddr,
        connect_id: usize,
        connect_func: Box<dyn ProxyConnect + Send>,
    ) -> bool;
    /// Removes a proxy connection.
    fn remove(&self, connect_id: usize);
}

#[cfg(test)]
mod tests {
    use super::*;

    struct DefaultProxyConnect;
    impl ProxyConnect for DefaultProxyConnect {
        fn proxy_connect(&self, _fd: i32, _addr: SocketAddr) {}
    }

    #[test]
    fn test_proxy_connect_default_methods() {
        let connect = DefaultProxyConnect;
        assert!(!connect.accepts_stream());
        let addr: SocketAddr = "127.0.0.1:80".parse().expect("parse addr");
        connect.proxy_connect_stream(Err(io::Error::other("test")), addr);
    }
}
