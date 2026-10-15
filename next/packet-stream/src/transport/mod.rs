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
// src/transport/mod.rs - Transport abstraction layer
//=============================================================================

//! Transport abstraction layer for packet streaming.

// Public API modules
#[cfg(all(target_os = "linux", feature = "cuttlefish"))]
pub mod dual_fd;
pub mod h4;
pub mod modem;
pub mod nci;
pub mod traits;
pub mod types;
pub mod uci;

// Platform-specific socket implementations
#[cfg(unix)]
pub mod unix;
#[cfg(windows)]
pub mod windows;

// Internal implementation modules
pub(crate) mod adapters;

// Re-export public API only
use std::path::PathBuf;

use async_trait::async_trait;
#[cfg(all(target_os = "linux", feature = "cuttlefish"))]
pub use dual_fd::{DualFdConfig, DualFdListener};
use futures::{SinkExt, StreamExt};
pub use h4::H4Codec;
pub use modem::ModemCodec;
pub use nci::NciCodec;
use serde::{Deserialize, Serialize};
use tokio_util::codec::{Framed, LengthDelimitedCodec};
#[cfg(windows)]
use tracing::warn;
use traits::{PacketSink, PacketStream, TransportListener};
pub use types::{ListenerConfig, TransportType};
pub use uci::UciCodec;

use crate::{
    error::{PacketStreamError, Result, SocketError},
    types::ChipInfo,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SocketType {
    Unix(PathBuf),
    NamedPipe(String),
    Tcp(std::net::SocketAddr),
}

#[derive(Debug, Clone)]
pub enum SocketConfig {
    Auto,
    Manual(SocketType),
    /// Optimized for containerized environments
    Container {
        /// Prefer volume-mounted Unix sockets for performance
        prefer_volumes: bool,
        /// Fallback to host networking if available
        allow_host_networking: bool,
    },
}

pub enum CrossPlatformListener {
    #[cfg(unix)]
    Unix(unix::UnixSocketListener),
    #[cfg(windows)]
    Windows(windows::WindowsListener),
    Tcp(tokio::net::TcpListener),
}

pub enum CrossPlatformStream {
    #[cfg(unix)]
    Unix(tokio::net::UnixStream),
    Tcp(tokio::net::TcpStream),
    #[cfg(windows)]
    Windows(tokio::net::windows::named_pipe::NamedPipeServer),
}

impl CrossPlatformListener {
    pub async fn bind(config: SocketConfig) -> Result<Self> {
        match config {
            SocketConfig::Auto => Self::bind_auto().await,
            SocketConfig::Manual(socket_type) => Self::bind_manual(socket_type).await,
            SocketConfig::Container { prefer_volumes, allow_host_networking } => {
                Self::bind_container(prefer_volumes, allow_host_networking).await
            }
        }
    }

    async fn bind_container(prefer_volumes: bool, allow_host_networking: bool) -> Result<Self> {
        if prefer_volumes {
            #[cfg(unix)]
            {
                let volume_socket = PathBuf::from("/shared/sockets/packetstream.sock");
                if volume_socket.parent().is_some_and(|p| p.exists())
                    && let Ok(listener) = unix::UnixSocketListener::bind(&volume_socket).await
                {
                    return Ok(CrossPlatformListener::Unix(listener));
                }
            }
        }

        if allow_host_networking {
            #[cfg(unix)]
            {
                let host_socket = PathBuf::from("/tmp/packetstream-host.sock");
                if let Ok(listener) = unix::UnixSocketListener::bind(&host_socket).await {
                    return Ok(CrossPlatformListener::Unix(listener));
                }
            }
        }

        let addr: std::net::SocketAddr = "localhost:8080".parse().map_err(SocketError::from)?;
        Ok(CrossPlatformListener::Tcp(tokio::net::TcpListener::bind(addr).await?))
    }

    async fn bind_auto() -> Result<Self> {
        #[cfg(unix)]
        {
            let socket_path =
                std::env::temp_dir().join(format!("packetstream_{}.sock", std::process::id()));
            let listener = unix::UnixSocketListener::bind(&socket_path).await?;
            Ok(CrossPlatformListener::Unix(listener))
        }

        #[cfg(windows)]
        {
            let pipe_name = "packetstream";
            let fallback_addr: std::net::SocketAddr =
                "localhost:0".parse().map_err(SocketError::from)?;
            match windows::WindowsListener::bind_named_pipe(pipe_name).await {
                Ok(listener) => Ok(CrossPlatformListener::Windows(listener)),
                Err(err) => {
                    warn!("Failed to create named pipe, falling back to tcp: {err}");
                    let listener = tokio::net::TcpListener::bind(fallback_addr).await?;
                    Ok(CrossPlatformListener::Tcp(listener))
                }
            }
        }

        #[cfg(not(any(unix, windows)))]
        {
            let addr = "localhost:0".parse().map_err(SocketError::from)?;
            let listener = tokio::net::TcpListener::bind(addr).await?;
            Ok(CrossPlatformListener::Tcp(listener))
        }
    }

    async fn bind_manual(socket_type: SocketType) -> Result<Self> {
        match socket_type {
            #[cfg(unix)]
            SocketType::Unix(path) => {
                let listener = unix::UnixSocketListener::bind(path).await?;
                Ok(CrossPlatformListener::Unix(listener))
            }
            #[cfg(not(unix))]
            SocketType::Unix(_) => {
                Err(PacketStreamError::Socket(SocketError::UnsupportedPlatform(
                    "Unix sockets not supported on this platform".to_string(),
                )))
            }
            #[cfg(windows)]
            SocketType::NamedPipe(name) => {
                let listener = windows::WindowsListener::bind_named_pipe(&name).await?;
                Ok(CrossPlatformListener::Windows(listener))
            }
            #[cfg(not(windows))]
            SocketType::NamedPipe(_) => {
                Err(PacketStreamError::Socket(SocketError::UnsupportedPlatform(
                    "Named pipes not supported on this platform".to_string(),
                )))
            }
            SocketType::Tcp(addr) => {
                let listener = tokio::net::TcpListener::bind(addr).await?;
                Ok(CrossPlatformListener::Tcp(listener))
            }
        }
    }

    pub async fn accept(&mut self) -> Result<(PacketStream, PacketSink)> {
        match self {
            #[cfg(unix)]
            CrossPlatformListener::Unix(listener) => {
                let stream = listener.accept().await?;
                let framed = Framed::new(stream, LengthDelimitedCodec::new());
                let (sink, stream) = framed.split();
                let stream =
                    stream.map(|item| item.map(|b| b.freeze()).map_err(PacketStreamError::Io));
                let sink = sink.sink_map_err(PacketStreamError::Io);
                Ok((Box::pin(stream), Box::pin(sink)))
            }
            #[cfg(windows)]
            CrossPlatformListener::Windows(listener) => {
                let stream = listener.accept().await?;
                let framed = Framed::new(stream, LengthDelimitedCodec::new());
                let (sink, stream) = framed.split();
                let stream =
                    stream.map(|item| item.map(|b| b.freeze()).map_err(PacketStreamError::Io));
                let sink = sink.sink_map_err(PacketStreamError::Io);
                Ok((Box::pin(stream), Box::pin(sink)))
            }
            CrossPlatformListener::Tcp(listener) => {
                let (stream, _) = listener.accept().await?;
                let framed = Framed::new(stream, LengthDelimitedCodec::new());
                let (sink, stream) = framed.split();
                let stream =
                    stream.map(|item| item.map(|b| b.freeze()).map_err(PacketStreamError::Io));
                let sink = sink.sink_map_err(PacketStreamError::Io);
                Ok((Box::pin(stream), Box::pin(sink)))
            }
        }
    }

    pub fn local_addr(&self) -> Result<String> {
        match self {
            #[cfg(unix)]
            CrossPlatformListener::Unix(listener) => Ok(listener.path().display().to_string()),
            #[cfg(windows)]
            CrossPlatformListener::Windows(listener) => listener.local_addr(),
            CrossPlatformListener::Tcp(listener) => Ok(listener.local_addr()?.to_string()),
        }
    }
}

pub enum Listener {
    Tcp(adapters::TcpTransportListener),
    #[cfg(unix)]
    Uds(adapters::UdsTransportListener),
    #[cfg(all(target_os = "linux", feature = "cuttlefish"))]
    DualFd(DualFdListener),
}

#[async_trait]
impl TransportListener for Listener {
    async fn accept(&mut self) -> Result<(PacketStream, PacketSink, ChipInfo, String)> {
        match self {
            Listener::Tcp(l) => l.accept().await,
            #[cfg(unix)]
            Listener::Uds(l) => l.accept().await,
            #[cfg(all(target_os = "linux", feature = "cuttlefish"))]
            Listener::DualFd(l) => l.accept().await,
        }
    }

    fn local_addr(&self) -> Result<crate::types::StreamAddress> {
        match self {
            Listener::Tcp(l) => l.local_addr(),
            #[cfg(unix)]
            Listener::Uds(l) => l.local_addr(),
            #[cfg(all(target_os = "linux", feature = "cuttlefish"))]
            Listener::DualFd(l) => l.local_addr(),
        }
    }

    async fn shutdown(&mut self) -> Result<()> {
        match self {
            Listener::Tcp(l) => l.shutdown().await,
            #[cfg(unix)]
            Listener::Uds(l) => l.shutdown().await,
            #[cfg(all(target_os = "linux", feature = "cuttlefish"))]
            Listener::DualFd(l) => l.shutdown().await,
        }
    }
}
