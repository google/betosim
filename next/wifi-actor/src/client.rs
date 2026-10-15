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

use actor_framework::ResourceClient;
use netsim_model::{
    Chip, ChipClient, ChipCreate, ChipError, ChipId, ChipUpdate, ChipVariantUpdate, ClientError,
    NetsimRadioStats,
};
use netsim_proto::protobuf::Message;

use crate::wifi_actor::WifiActor;

#[derive(Clone)]
pub struct WifiClient {
    pub(crate) inner: Box<dyn actor_framework::ActorClient<WifiActor>>,
}
impl std::fmt::Debug for WifiClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WifiClient").finish_non_exhaustive()
    }
}

impl WifiClient {
    pub fn new(client: ResourceClient<WifiActor>) -> Self {
        Self { inner: Box::new(client) }
    }

    /// Creates a new WifiClient with a custom internal client (e.g. for
    /// mocking).
    pub fn from_client(client: Box<dyn actor_framework::ActorClient<WifiActor>>) -> Self {
        Self { inner: client }
    }

    pub async fn list(&self) -> Result<Vec<Chip>, ClientError> {
        self.inner.list().await.map_err(|e| ClientError::Send(e.to_string()))
    }

    pub async fn set_rf_state(&self, id: ChipId, enabled: bool) -> Result<(), ClientError> {
        let patch = ChipUpdate {
            variant: Some(ChipVariantUpdate::Wifi(Default::default())),
            enabled: Some(enabled),
            ..Default::default()
        };
        self.inner.update(id, patch).await.map(|_| ()).map_err(|e| ClientError::Send(e.to_string()))
    }

    pub async fn get_global_stats_proto(
        &self,
    ) -> Result<netsim_proto::stats::WifiStats, ClientError> {
        match self.inner.perform_action(None, crate::wifi_actor::WifiReq::GetGlobalStats).await {
            Ok(crate::wifi_actor::WifiResponse::GlobalStats(stats)) => {
                let mut ipc_stats = *stats;
                Ok(ipc_stats.wifi_stats.take().unwrap_or_default())
            }
            Ok(_) => Err(ClientError::Recv("Unexpected action result".into())),
            Err(e) => Err(ClientError::Send(e.to_string())),
        }
    }
}

#[async_trait::async_trait]
impl ChipClient for WifiClient {
    async fn create(&self, id: ChipId, params: ChipCreate) -> Result<(), ClientError> {
        self.inner
            .create_with_id(id, params)
            .await
            .map(|_| ())
            .map_err(|e| ClientError::Send(e.to_string()))
    }

    async fn read(&self, id: ChipId) -> Result<Chip, ClientError> {
        self.inner
            .get(id)
            .await
            .map_err(|e| ClientError::Send(e.to_string()))?
            .ok_or(ClientError::Chip(ChipError::ChipNotFound(id)))
    }

    async fn update(&self, id: ChipId, patch: ChipUpdate) -> Result<Chip, ClientError> {
        self.inner.update(id, patch).await.map_err(|e| ClientError::Send(e.to_string()))
    }

    async fn delete(&self, id: ChipId) -> Result<(), ClientError> {
        self.inner.delete(id).await.map_err(|e| ClientError::Send(e.to_string()))
    }

    async fn read_statistics(&self) -> Result<Box<[NetsimRadioStats]>, ClientError> {
        match self.inner.perform_action(None, crate::wifi_actor::WifiReq::GetStatistics).await {
            Ok(crate::wifi_actor::WifiResponse::Statistics(stats)) => Ok(stats),
            Ok(_) => Err(ClientError::Recv("Unexpected action result".into())),
            Err(e) => Err(ClientError::Send(e.to_string())),
        }
    }

    async fn read_count_for_testing(&self) -> Result<usize, ClientError> {
        self.inner
            .list()
            .await
            .map(|chips| chips.len())
            .map_err(|e| ClientError::Send(e.to_string()))
    }

    async fn shutdown(&self) -> Result<(), ClientError> {
        self.inner.shutdown().await.map_err(|e| ClientError::Send(e.to_string()))
    }

    async fn reset(&self, id: ChipId) -> Result<Chip, ClientError> {
        match self.inner.perform_action(Some(id), crate::wifi_actor::WifiReq::Reset { id }).await {
            Ok(crate::wifi_actor::WifiResponse::Chip(chip)) => Ok(*chip),
            Ok(_) => Err(ClientError::Recv("Unexpected action result for reset".into())),
            Err(e) => Err(ClientError::Send(e.to_string())),
        }
    }

    async fn get_global_stats(&self) -> Result<Option<Vec<u8>>, ClientError> {
        match self.inner.perform_action(None, crate::wifi_actor::WifiReq::GetGlobalStats).await {
            Ok(crate::wifi_actor::WifiResponse::GlobalStats(stats)) => {
                stats.write_to_bytes().map(Some).map_err(|e| ClientError::Recv(e.to_string()))
            }
            Ok(_) => Err(ClientError::Recv("Unexpected action result".into())),
            Err(e) => Err(ClientError::Send(e.to_string())),
        }
    }

    fn clone_box(&self) -> Box<dyn ChipClient> {
        Box::new(self.clone())
    }
}
