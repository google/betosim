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

use actor_framework::ResourceClient;
use netsim_model::{ChipId, ClientError, PacketSink, PacketStream};
use tokio::sync::{mpsc::UnboundedSender, oneshot};

use crate::slirp_actor::{SlirpActor, SlirpCreate, SlirpLease, SlirpReq};

#[derive(Clone, Debug)]
pub struct SlirpClient {
    client: ResourceClient<SlirpActor>,
}

impl SlirpClient {
    pub fn new(client: ResourceClient<SlirpActor>) -> Self {
        Self { client }
    }

    pub async fn create(
        &self,
        packet_stream: Option<PacketStream>,
        packet_sink: Option<PacketSink>,
    ) -> Result<(), ClientError> {
        self.client
            .create(SlirpCreate { packet_stream, packet_sink })
            .await
            .map(|_| ())
            .map_err(|e| ClientError::Send(e.to_string()))
    }

    pub async fn send_packet(&self, packet: bytes::Bytes) {
        let _ = self.client.perform_action(None, SlirpReq::SendPacket(packet)).await;
    }

    pub async fn register(
        &self,
        client_id: u32,
        stream: std::pin::Pin<Box<dyn tokio_stream::Stream<Item = bytes::Bytes> + Sync + Send>>,
        sink: PacketSink,
        notifier: Option<UnboundedSender<ChipId>>,
        isolated: bool,
    ) -> Result<(), ClientError> {
        self.client
            .perform_action(
                None,
                SlirpReq::Register { client_id, stream, sink, notifier, isolated },
            )
            .await
            .map_err(|e| ClientError::Send(e.to_string()))?;
        Ok(())
    }

    pub async fn allocate_lease(&self, chip_id: ChipId) -> Result<Option<SlirpLease>, ClientError> {
        let (tx, rx) = oneshot::channel();
        self.client
            .perform_action(None, SlirpReq::AllocateLease { chip_id, respond_to: tx })
            .await
            .map_err(|e| ClientError::Send(e.to_string()))?;
        rx.await.map_err(|e| ClientError::Send(e.to_string()))
    }

    pub async fn release_lease(&self, chip_id: ChipId) -> Result<(), ClientError> {
        self.client
            .perform_action(None, SlirpReq::ReleaseLease { chip_id })
            .await
            .map_err(|e| ClientError::Send(e.to_string()))?;
        Ok(())
    }

    pub async fn unregister(&self, client_id: u32) -> Result<(), ClientError> {
        self.client
            .perform_action(None, SlirpReq::Unregister { client_id })
            .await
            .map_err(|e| ClientError::Send(e.to_string()))?;
        Ok(())
    }

    pub async fn switch_backend(
        &self,
        backend: crate::slirp_actor::SlirpBackend,
    ) -> Result<(), ClientError> {
        self.client
            .perform_action(None, SlirpReq::SwitchBackend(backend))
            .await
            .map_err(|e| ClientError::Send(e.to_string()))?;
        Ok(())
    }

    pub async fn shutdown(&self) -> Result<(), ClientError> {
        self.client.shutdown().await.map_err(|e| ClientError::Send(e.to_string()))
    }
}
