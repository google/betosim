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
    inner: Box<dyn actor_framework::ActorClient<EthernetActor>>,
}

impl std::fmt::Debug for EthernetClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EthernetClient").finish_non_exhaustive()
    }
}

fn to_send_err(err: impl std::fmt::Display) -> ClientError {
    ClientError::Send(err.to_string())
}

impl EthernetClient {
    pub(crate) fn new(client: ResourceClient<EthernetActor>) -> Self {
        Self { inner: Box::new(client) }
    }

    pub async fn list(&self) -> Result<Vec<Chip>, ClientError> {
        self.inner.list().await.map_err(to_send_err)
    }
}

#[async_trait::async_trait]
impl ChipClient for EthernetClient {
    async fn create(&self, id: ChipId, params: ChipCreate) -> Result<(), ClientError> {
        self.inner.create_with_id(id, params).await.map(|_| ()).map_err(to_send_err)
    }

    async fn read(&self, id: ChipId) -> Result<Chip, ClientError> {
        self.inner
            .get(id)
            .await
            .map_err(to_send_err)?
            .ok_or(ClientError::Chip(ChipError::ChipNotFound(id)))
    }

    async fn update(&self, id: ChipId, patch: ChipUpdate) -> Result<Chip, ClientError> {
        self.inner.update(id, patch).await.map_err(to_send_err)
    }

    async fn delete(&self, id: ChipId) -> Result<(), ClientError> {
        self.inner.delete(id).await.map_err(to_send_err)
    }

    async fn read_statistics(&self) -> Result<Box<[NetsimRadioStats]>, ClientError> {
        match self
            .inner
            .perform_action(None, EthernetReq::GetStatistics)
            .await
            .map_err(to_send_err)?
        {
            EthernetResponse::Statistics(stats) => Ok(stats),
            _ => Err(ClientError::Recv("Unexpected action result".into())),
        }
    }

    async fn read_count_for_testing(&self) -> Result<usize, ClientError> {
        self.inner.list().await.map(|chips| chips.len()).map_err(to_send_err)
    }

    async fn shutdown(&self) -> Result<(), ClientError> {
        self.inner.shutdown().await.map_err(to_send_err)
    }

    async fn reset(&self, id: ChipId) -> Result<Chip, ClientError> {
        match self
            .inner
            .perform_action(Some(id), EthernetReq::Reset { id })
            .await
            .map_err(to_send_err)?
        {
            EthernetResponse::Chip(chip) => Ok(*chip),
            _ => Err(ClientError::Recv("Unexpected action result for reset".into())),
        }
    }

    async fn get_global_stats(&self) -> Result<Option<Vec<u8>>, ClientError> {
        Ok(None)
    }

    fn clone_box(&self) -> Box<dyn ChipClient> {
        Box::new(self.clone())
    }
}

#[cfg(test)]
mod tests {
    use actor_framework::MockActorClient;

    use super::*;

    const TEST_CHIP_ID: ChipId = ChipId(5);

    #[tokio::test]
    async fn test_read_statistics_rejects_unexpected_response_variant() {
        let mut mock = MockActorClient::<EthernetActor>::new();
        mock.expect_perform_action()
            .withf(|id, req| id.is_none() && matches!(req, EthernetReq::GetStatistics))
            .times(1)
            .returning(|_, _| Ok(EthernetResponse::Chip(Box::default())));
        let client = EthernetClient { inner: Box::new(mock) };
        let err = client.read_statistics().await.expect_err("expected Recv error");
        assert!(matches!(err, ClientError::Recv(_)), "unexpected error: {err:?}");
    }

    #[tokio::test]
    async fn test_reset_rejects_unexpected_response_variant() {
        let mut mock = MockActorClient::<EthernetActor>::new();
        mock.expect_perform_action()
            .withf(|id, req| {
                *id == Some(TEST_CHIP_ID)
                    && matches!(req, EthernetReq::Reset { id } if *id == TEST_CHIP_ID)
            })
            .times(1)
            .returning(|_, _| Ok(EthernetResponse::Statistics(Box::default())));
        let client = EthernetClient { inner: Box::new(mock) };
        let err = client.reset(TEST_CHIP_ID).await.expect_err("expected Recv error");
        assert!(matches!(err, ClientError::Recv(_)), "unexpected error: {err:?}");
    }
}
