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

use std::net::SocketAddr;

/// Trait for handling proxy connection results.
pub trait ProxyConnect: Send {
    /// Notifies slirp about the result of a proxy connection attempt.
    fn proxy_connect(&self, fd: i32, addr: SocketAddr);
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
