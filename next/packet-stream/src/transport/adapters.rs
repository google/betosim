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
// src/transport/adapters.rs - Adapters for existing PacketStream
// implementations
//=============================================================================

use std::net::SocketAddr;

use async_trait::async_trait;
use futures::{SinkExt, stream::StreamExt};
use tokio::net::TcpListener;
use tokio_util::codec::{Framed, LengthDelimitedCodec};

use crate::{
    error::{PacketStreamError, Result},
    streams::InitInfo,
    transport::traits::{PacketSink, PacketStream, TransportListener},
    types::{ChipInfo, StreamAddress},
};

/// TCP listener adapter
pub struct TcpTransportListener {
    inner: TcpListener,
    local_addr: SocketAddr,
}

impl TcpTransportListener {
    pub async fn bind(addr: &str, port: u16) -> Result<Self> {
        let bind_addr = format!("{addr}:{port}");
        let listener = TcpListener::bind(&bind_addr).await?;
        let local_addr = listener.local_addr()?;

        Ok(Self { inner: listener, local_addr })
    }
}

#[async_trait]
impl TransportListener for TcpTransportListener {
    async fn accept(&mut self) -> Result<(PacketStream, PacketSink, ChipInfo, String)> {
        let (stream, peer_addr) = self.inner.accept().await?;
        let guid = peer_addr.to_string();
        let framed = Framed::new(stream, LengthDelimitedCodec::new());
        let (sink, mut stream) = framed.split();

        // Perform the init_info handshake
        let first_message = stream.next().await.ok_or(PacketStreamError::ConnectionClosed)??;
        let init_info: InitInfo = serde_json::from_slice(&first_message).map_err(|e| {
            PacketStreamError::Protocol(crate::error::ProtocolError::InvalidFormat(format!(
                "Failed to parse init_info: {e}"
            )))
        })?;

        let stream = stream.map(|item| item.map(|b| b.freeze()).map_err(PacketStreamError::Io));
        let sink = sink.sink_map_err(PacketStreamError::Io);

        Ok((Box::pin(stream), Box::pin(sink), init_info.chip_info, guid))
    }

    fn local_addr(&self) -> Result<StreamAddress> {
        Ok(StreamAddress::Tcp(self.local_addr))
    }

    async fn shutdown(&mut self) -> Result<()> {
        Ok(())
    }
}

/// Unix Domain Socket listener adapter
#[cfg(unix)]
pub struct UdsTransportListener {
    inner: tokio::net::UnixListener,
    path: std::path::PathBuf,
    conn_counter: std::sync::Arc<std::sync::Mutex<u64>>,
}

#[cfg(unix)]
impl UdsTransportListener {
    pub fn bind(path: &str) -> Result<Self> {
        let path_buf = std::path::PathBuf::from(path);
        if path_buf.exists() {
            std::fs::remove_file(&path_buf)?;
        }
        let listener = tokio::net::UnixListener::bind(&path_buf)?;
        Ok(Self {
            inner: listener,
            path: path_buf,
            conn_counter: std::sync::Arc::new(std::sync::Mutex::new(0)),
        })
    }
}

#[cfg(unix)]
#[async_trait]
impl TransportListener for UdsTransportListener {
    async fn accept(&mut self) -> Result<(PacketStream, PacketSink, ChipInfo, String)> {
        let (stream, _) = self.inner.accept().await?;
        let framed = Framed::new(stream, LengthDelimitedCodec::new());
        let (sink, mut stream) = framed.split();

        // Perform the init_info handshake
        let first_message = stream.next().await.ok_or(PacketStreamError::ConnectionClosed)??;
        let init_info: InitInfo = serde_json::from_slice(&first_message).map_err(|e| {
            PacketStreamError::Protocol(crate::error::ProtocolError::InvalidFormat(format!(
                "Failed to parse init_info: {e}"
            )))
        })?;

        let stream = stream.map(|item| item.map(|b| b.freeze()).map_err(PacketStreamError::Io));
        let sink = sink.sink_map_err(PacketStreamError::Io);

        let mut count = self.conn_counter.lock().unwrap();
        let guid = format!("uds-{}", *count);
        *count += 1;

        Ok((Box::pin(stream), Box::pin(sink), init_info.chip_info, guid))
    }

    fn local_addr(&self) -> Result<StreamAddress> {
        Ok(StreamAddress::Uds(self.path.clone()))
    }

    async fn shutdown(&mut self) -> Result<()> {
        if self.path.exists() {
            let _ = std::fs::remove_file(&self.path);
        }
        Ok(())
    }
}
