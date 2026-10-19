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
// src/streams.rs - Multi-transport connection manager
//=============================================================================

use std::collections::HashMap;

use bytes::Bytes;
use futures::SinkExt;
use serde::{Deserialize, Serialize};
use tokio::{
    sync::{broadcast, mpsc},
    task::JoinHandle,
};
use tracing::{error, info, warn};

use crate::{
    error::{PacketStreamError, Result},
    transport::{
        ListenerConfig, TransportType,
        traits::{PacketSink, PacketStream, TransportListener},
    },
    types::{ChipInfo, StreamAddress},
};

/// Init info message for transport handshake.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InitInfo {
    pub chip_info: ChipInfo,
    pub transport_type: String,
}

/// Manages multiple transport listeners and stream connections.
#[derive(Debug)]
pub struct Streams {
    listener_tasks: HashMap<String, JoinHandle<()>>,
    listener_addresses: HashMap<String, StreamAddress>,
    shutdown_tx: broadcast::Sender<()>,
    connection_rx: mpsc::UnboundedReceiver<(String, (PacketStream, PacketSink, ChipInfo, String))>,
    connection_tx: mpsc::UnboundedSender<(String, (PacketStream, PacketSink, ChipInfo, String))>,
}

impl Default for Streams {
    fn default() -> Self {
        Self::new()
    }
}

impl Streams {
    pub fn new() -> Self {
        let (shutdown_tx, _) = broadcast::channel(16);
        let (connection_tx, connection_rx) = mpsc::unbounded_channel();

        Self {
            listener_tasks: HashMap::new(),
            listener_addresses: HashMap::new(),
            shutdown_tx,
            connection_rx,
            connection_tx,
        }
    }

    pub fn listener_address(&self, name: &str) -> Option<&StreamAddress> {
        self.listener_addresses.get(name)
    }

    pub async fn start_listener(
        &mut self,
        name: impl Into<String>,
        transport_type: TransportType,
    ) -> Result<()> {
        let name = name.into();
        if !transport_type.supports_listener() {
            return Err(crate::error::PacketStreamError::InvalidConfig(format!(
                "Stream type {} does not support listeners",
                transport_type.description()
            )));
        }

        let mut listener = transport_type.create_listener().await?;
        let addr = listener.local_addr()?;
        info!("Starting {name} listener on {addr}");

        self.listener_addresses.insert(name.clone(), addr.clone());

        let listener_name = name.clone();
        let connection_tx = self.connection_tx.clone();
        let mut shutdown_rx = self.shutdown_tx.subscribe();

        let task = tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = shutdown_rx.recv() => {
                        info!("Shutting down {listener_name} listener");
                        let _ = listener.shutdown().await;
                        break;
                    }
                    result = listener.accept() => {
                        match result {
                            Ok(stream_tuple) => {
                                if connection_tx.send((listener_name.clone(), stream_tuple)).is_err() {
                                    break;
                                }
                            }
                            Err(error) => {
                                error!(%error, "Listener {listener_name} accept error");
                            }
                        }
                    }
                }
            }
        });

        self.listener_tasks.insert(name.clone(), task);
        Ok(())
    }

    pub fn add_listener(
        &mut self,
        name: impl Into<String>,
        mut listener: Box<dyn TransportListener + Send>,
    ) -> Result<()> {
        let name = name.into();
        let addr = listener.local_addr()?;
        info!("Adding {name} listener on {addr}");

        self.listener_addresses.insert(name.clone(), addr.clone());

        let listener_name = name.clone();
        let connection_tx = self.connection_tx.clone();
        let mut shutdown_rx = self.shutdown_tx.subscribe();

        let task = tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = shutdown_rx.recv() => {
                        info!("Shutting down {listener_name} listener");
                        let _ = listener.shutdown().await;
                        break;
                    }
                    result = listener.accept() => {
                        match result {
                            Ok(stream_tuple) => {
                                if connection_tx.send((listener_name.clone(), stream_tuple)).is_err() {
                                    break;
                                }
                            }
                            Err(error) => {
                                error!(%error, "Listener {listener_name} accept error");
                            }
                        }
                    }
                }
            }
        });

        self.listener_tasks.insert(name.clone(), task);
        Ok(())
    }

    pub async fn start_from_config(&mut self, config: &ListenerConfig) -> Result<()> {
        for (name, transport) in &config.listeners {
            self.start_listener(name, transport.clone()).await?;
        }
        Ok(())
    }

    pub async fn accept_any(
        &mut self,
    ) -> Result<(String, (PacketStream, PacketSink, ChipInfo, String))> {
        match self.connection_rx.recv().await {
            Some((listener_name, stream_tuple)) => Ok((listener_name, stream_tuple)),
            None => Err(crate::error::PacketStreamError::ConnectionClosed),
        }
    }

    pub fn listener_names(&self) -> Vec<String> {
        self.listener_tasks.keys().cloned().collect()
    }

    pub fn is_listener_running(&self, name: impl AsRef<str>) -> bool {
        self.listener_tasks.contains_key(name.as_ref())
    }

    pub fn stop_listener(&mut self, name: impl AsRef<str>) -> Result<()> {
        let name = name.as_ref();
        if let Some(task) = self.listener_tasks.remove(name) {
            task.abort();
            info!("Stopped {name} listener");
        }
        Ok(())
    }

    pub async fn shutdown_all(&mut self) -> Result<()> {
        info!("Shutting down all listeners...");
        let _ = self.shutdown_tx.send(());
        let tasks: Vec<_> = self.listener_tasks.drain().collect();
        for (name, task) in tasks {
            match tokio::time::timeout(std::time::Duration::from_secs(5), task).await {
                Ok(_) => info!("Listener {name} shutdown complete"),
                Err(_) => {
                    warn!("Listener {name} shutdown timed out, aborting");
                }
            }
        }
        Ok(())
    }

    pub async fn connect(
        &self,
        transport_type: TransportType,
        chip_info: ChipInfo,
    ) -> Result<(PacketStream, PacketSink)> {
        let (stream, mut sink) = transport_type.create_stream().await?;
        let is_dual_fd = matches!(transport_type, TransportType::Fd { .. });

        if !is_dual_fd {
            let init_info = InitInfo { chip_info, transport_type: transport_type.description() };
            let init_json = serde_json::to_vec(&init_info).map_err(|e| {
                PacketStreamError::Protocol(crate::error::ProtocolError::InvalidFormat(format!(
                    "Failed to serialize init_info: {e}"
                )))
            })?;
            sink.send(Bytes::from(init_json)).await?;
        }

        Ok((stream, sink))
    }
}

impl Drop for Streams {
    fn drop(&mut self) {
        for (_, task) in self.listener_tasks.drain() {
            task.abort();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_streams_creation() {
        let streams = Streams::new();
        assert_eq!(streams.listener_names().len(), 0);
        assert!(!streams.is_listener_running("tcp"));
    }
}
