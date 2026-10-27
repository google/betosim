// Copyright 2025 The Android Open Source Project
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
//=============================================================================
// src/types.rs - Core data types for PacketStream
//=============================================================================

use std::{net::SocketAddr, path::PathBuf};

/// Stream addressing for different connection types.
///
/// This enum provides a unified way to represent addresses across different
/// transport mechanisms, allowing the same API to work with TCP sockets,
/// Unix domain sockets, gRPC endpoints, and raw file descriptors.
///
/// # Examples
///
/// ```rust
/// use std::path::PathBuf;
///
/// use packet_stream::StreamAddress;
///
/// // TCP address
/// let tcp_addr = StreamAddress::Tcp("127.0.0.1:8080".parse().unwrap());
///
/// // Unix domain socket
/// let uds_addr = StreamAddress::Uds(PathBuf::from("/tmp/socket.sock"));
/// ```
#[derive(Debug, Clone)]
pub enum StreamAddress {
    /// TCP socket address
    Tcp(SocketAddr),
    /// Unix Domain Socket path
    Uds(PathBuf),
    /// Raw file descriptors (from cuttlefish)
    Fd { in_fd: i32, out_fd: Option<i32> },
    /// gRPC endpoint address
    Grpc(SocketAddr),
}

impl std::fmt::Display for StreamAddress {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StreamAddress::Uds(path) => write!(f, "UDS:{}", path.display()),
            StreamAddress::Tcp(addr) => write!(f, "TCP:{}", addr),
            StreamAddress::Fd { in_fd, out_fd } => match out_fd {
                Some(out_fd) => write!(f, "fd://{in_fd}:{out_fd}"),
                None => write!(f, "fd://{in_fd}"),
            },
            StreamAddress::Grpc(addr) => write!(f, "GRPC:{}", addr),
        }
    }
}

pub use netsim_types::{BtControllerProperties, Chip, ChipInfo, ChipKind, DeviceInfo};
