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
// src/transport/types.rs - Transport type enumeration and factories
//=============================================================================

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use tokio::net::TcpStream;

use crate::{
    error::{PacketStreamError, Result},
    transport::{
        adapters::TcpTransportListener,
        traits::{PacketSink, PacketStream, into_packet_stream_sink},
    },
};

/// Transport configuration that supports multiple connection types.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum TransportType {
    Tcp {
        addr: String,
        port: u16,
    },
    #[cfg(unix)]
    Uds {
        path: String,
    },
    Fd {
        in_fd: i32,
        out_fd: Option<i32>,
    },
}

impl TransportType {
    pub fn tcp(addr: impl Into<String>, port: u16) -> Self {
        TransportType::Tcp { addr: addr.into(), port }
    }

    #[cfg(unix)]
    pub fn uds(path: impl Into<String>) -> Self {
        TransportType::Uds { path: path.into() }
    }

    pub fn fd(in_fd: i32, out_fd: Option<i32>) -> Self {
        TransportType::Fd { in_fd, out_fd }
    }

    pub(crate) async fn create_listener(&self) -> Result<super::Listener> {
        match self {
            TransportType::Tcp { addr, port } => {
                let listener = TcpTransportListener::bind(addr, *port).await?;
                Ok(super::Listener::Tcp(listener))
            }
            #[cfg(unix)]
            TransportType::Uds { path } => {
                let listener = crate::transport::adapters::UdsTransportListener::bind(path)?;
                Ok(super::Listener::Uds(listener))
            }
            TransportType::Fd { .. } => Err(PacketStreamError::InvalidConfig(
                "File descriptors cannot create listeners, use create_stream() instead".to_string(),
            )),
        }
    }

    pub(crate) async fn create_stream(&self) -> Result<(PacketStream, PacketSink)> {
        match self {
            TransportType::Tcp { addr, port } => {
                let stream = TcpStream::connect(format!("{addr}:{port}")).await?;
                Ok(into_packet_stream_sink(stream))
            }
            #[cfg(unix)]
            TransportType::Uds { path } => {
                let stream = tokio::net::UnixStream::connect(path).await?;
                Ok(into_packet_stream_sink(stream))
            }
            TransportType::Fd { .. } => Err(PacketStreamError::InvalidConfig(
                "FD transport cannot be created directly, it must be inherited".to_string(),
            )),
        }
    }

    pub fn description(&self) -> String {
        match self {
            TransportType::Tcp { addr, port } => format!("TCP {addr}:{port}"),
            #[cfg(unix)]
            TransportType::Uds { path } => format!("UDS {path}"),
            TransportType::Fd { in_fd, out_fd } => match out_fd {
                Some(out_fd) => format!("FD {in_fd}:{out_fd}"),
                None => format!("FD {in_fd}"),
            },
        }
    }

    pub fn supports_listener(&self) -> bool {
        #[cfg(unix)]
        {
            matches!(self, TransportType::Tcp { .. } | TransportType::Uds { .. })
        }
        #[cfg(not(unix))]
        {
            matches!(self, TransportType::Tcp { .. })
        }
    }

    pub fn supports_stream(&self) -> bool {
        true
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListenerConfig {
    pub listeners: HashMap<String, TransportType>,
}

impl Default for ListenerConfig {
    fn default() -> Self {
        Self::new()
    }
}

impl ListenerConfig {
    pub fn new() -> Self {
        Self { listeners: HashMap::new() }
    }

    pub fn add_listener(&mut self, name: impl Into<String>, transport_type: TransportType) {
        self.listeners.insert(name.into(), transport_type);
    }

    pub fn default_config() -> Self {
        let mut config = Self::new();
        config.add_listener("tcp", TransportType::tcp("localhost", 8080));
        #[cfg(unix)]
        config.add_listener("uds", TransportType::uds("/tmp/packetstream.sock"));
        config
    }
}
