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
use async_trait::async_trait;
use futures::TryFutureExt;
use netsim_model::{
    Chip, ChipClient, ChipCreate, ChipId, ChipUpdate, ClientError, NetsimRadioStats,
};

use crate::NfcActor;

#[derive(Clone, Debug)]
pub struct NfcClient {
    pub client: ResourceClient<NfcActor>,
    pub stats: std::sync::Arc<crate::stats::NfcStats>,
    pub service_stats: std::sync::Arc<crate::stats::NfcServiceStats>,
}

impl NfcClient {
    pub fn new(client: ResourceClient<NfcActor>) -> Self {
        Self {
            client,
            stats: std::sync::Arc::new(crate::stats::NfcStats::new()),
            service_stats: std::sync::Arc::new(crate::stats::NfcServiceStats::new()),
        }
    }
    pub async fn list(&self) -> Result<Vec<Chip>, ClientError> {
        self.client.list().err_into::<ClientError>().await
    }

    pub fn stats(&self) -> std::sync::Arc<crate::stats::NfcStats> {
        self.stats.clone()
    }
    pub fn service_stats(&self) -> std::sync::Arc<crate::stats::NfcServiceStats> {
        self.service_stats.clone()
    }

    pub fn incr_get_status_count(&self) {
        self.service_stats.incr_get_status_count();
    }
    pub fn incr_set_power_count(&self) {
        self.service_stats.incr_set_power_count();
    }
    pub fn incr_poll_count(&self) {
        self.service_stats.incr_poll_count();
    }
    pub fn incr_send_apdu_count(&self) {
        self.service_stats.incr_send_apdu_count();
    }
}

#[async_trait]
impl ChipClient for NfcClient {
    async fn create(&self, id: ChipId, params: ChipCreate) -> Result<(), ClientError> {
        self.client.create_with_id(id, params).err_into::<ClientError>().await.map(|_| ())
    }

    async fn read(&self, id: ChipId) -> Result<Chip, ClientError> {
        self.client
            .get(id)
            .err_into::<ClientError>()
            .await?
            .ok_or(ClientError::Chip(netsim_model::ChipError::ChipNotFound(id)))
    }

    async fn update(&self, id: ChipId, patch: ChipUpdate) -> Result<Chip, ClientError> {
        self.client.update(id, patch).err_into::<ClientError>().await
    }

    async fn delete(&self, id: ChipId) -> Result<(), ClientError> {
        self.client.delete(id).err_into::<ClientError>().await
    }

    async fn read_statistics(&self) -> Result<Box<[NetsimRadioStats]>, ClientError> {
        match self.client.perform_action(None, crate::nfc_actor::NfcAction::GetStatistics).await {
            Ok(crate::nfc_actor::NfcActionResult::Statistics(stats)) => Ok(stats),
            Ok(_) => Err(ClientError::Recv("Unexpected action result".into())),
            Err(e) => Err(ClientError::Send(e.to_string())),
        }
    }

    async fn read_count_for_testing(&self) -> Result<usize, ClientError> {
        self.client.list().err_into::<ClientError>().await.map(|chips| chips.len())
    }

    async fn shutdown(&self) -> Result<(), ClientError> {
        self.client.shutdown().err_into::<ClientError>().await
    }

    async fn reset(&self, id: ChipId) -> Result<Chip, ClientError> {
        // Not implemented
        self.read(id).await
    }

    async fn get_global_stats(&self) -> Result<Option<Vec<u8>>, ClientError> {
        use netsim_proto::protobuf::Message;
        self.stats
            .to_ipc_proto()
            .write_to_bytes()
            .map(Some)
            .map_err(|e| ClientError::Recv(e.to_string()))
    }

    async fn get_service_stats(&self) -> Result<Option<Vec<u8>>, ClientError> {
        use netsim_proto::protobuf::Message;
        self.service_stats
            .to_proto()
            .write_to_bytes()
            .map(Some)
            .map_err(|e| ClientError::Recv(e.to_string()))
    }

    fn clone_box(&self) -> Box<dyn ChipClient> {
        Box::new(self.clone())
    }
}

impl NfcClient {
    pub async fn create_control_channel(
        &self,
    ) -> Result<(tokio::io::DuplexStream, u16), ClientError> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        let action = crate::nfc_actor::NfcAction::CreateControlChannel { respond_to: tx };

        self.client
            .perform_action(None, action)
            .await
            .map_err(|e| ClientError::Send(e.to_string()))?;

        rx.await
            .map_err(|e| ClientError::Recv(e.to_string()))?
            .map_err(|e| ClientError::Chip(netsim_model::ChipError::Internal(Box::new(e))))
    }
}
