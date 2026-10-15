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
use netsim_model::{
    Chip, ChipClient, ChipCreate, ChipError, ChipId, ChipUpdate, ClientError, NetsimRadioStats,
};

use crate::ethernet_actor::{EthernetActor, EthernetReq, EthernetResponse};

#[derive(Clone)]
pub struct EthernetClient {
    pub(crate) inner: Box<dyn actor_framework::ActorClient<EthernetActor>>,
}

impl std::fmt::Debug for EthernetClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EthernetClient").finish_non_exhaustive()
    }
}

impl EthernetClient {
    pub fn new(client: ResourceClient<EthernetActor>) -> Self {
        Self { inner: Box::new(client) }
    }
}

#[async_trait::async_trait]
impl ChipClient for EthernetClient {
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
        match self.inner.perform_action(None, EthernetReq::GetStatistics).await {
            Ok(EthernetResponse::Statistics(stats)) => Ok(stats),
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
        match self.inner.perform_action(Some(id), EthernetReq::Reset { id }).await {
            Ok(EthernetResponse::Chip(chip)) => Ok(*chip),
            Ok(_) => Err(ClientError::Recv("Unexpected action result for reset".into())),
            Err(e) => Err(ClientError::Send(e.to_string())),
        }
    }

    async fn get_global_stats(&self) -> Result<Option<Vec<u8>>, ClientError> {
        Ok(None)
    }

    fn clone_box(&self) -> Box<dyn ChipClient> {
        Box::new(self.clone())
    }
}
