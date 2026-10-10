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

use crate::uwb_actor::UwbActor;

/// A client for communicating with the UWB Actor.
/// Wraps a generic `ResourceClient` and implements `ChipClient`.
#[derive(Clone, Debug)]
pub struct UwbClient(pub(crate) ResourceClient<UwbActor>);

#[async_trait]
impl ChipClient for UwbClient {
    async fn create(&self, id: ChipId, params: ChipCreate) -> Result<(), ClientError> {
        self.0.create_with_id(id, params).err_into::<ClientError>().await.map(|_| ())
    }

    async fn read(&self, id: ChipId) -> Result<Chip, ClientError> {
        self.0
            .get(id)
            .err_into::<ClientError>()
            .await?
            .ok_or(ClientError::Chip(netsim_model::ChipError::ChipNotFound(id)))
    }

    async fn update(&self, id: ChipId, patch: ChipUpdate) -> Result<Chip, ClientError> {
        self.0.update(id, patch).err_into::<ClientError>().await
    }

    async fn delete(&self, id: ChipId) -> Result<(), ClientError> {
        self.0.delete(id).err_into::<ClientError>().await
    }

    async fn read_statistics(&self) -> Result<Box<[NetsimRadioStats]>, ClientError> {
        match self
            .0
            .perform_action(None, crate::UwbAction::GetStatistics)
            .err_into::<ClientError>()
            .await
        {
            Ok(crate::UwbActionResult::Statistics(stats)) => Ok(stats),
            Ok(_) => Err(ClientError::Recv("Unexpected action result".into())),
            Err(e) => Err(e),
        }
    }

    async fn read_count_for_testing(&self) -> Result<usize, ClientError> {
        self.0.list().err_into::<ClientError>().await.map(|chips| chips.len())
    }

    async fn shutdown(&self) -> Result<(), ClientError> {
        self.0.shutdown().err_into::<ClientError>().await
    }

    async fn reset(&self, id: ChipId) -> Result<Chip, ClientError> {
        match self
            .0
            .perform_action(Some(id), crate::UwbAction::Reset { id })
            .err_into::<ClientError>()
            .await
        {
            Ok(crate::UwbActionResult::Chip(chip)) => Ok(*chip),
            Ok(_) => Err(ClientError::Recv("Unexpected action result for reset".into())),
            Err(e) => Err(e),
        }
    }

    async fn get_global_stats(&self) -> Result<Option<Vec<u8>>, ClientError> {
        match self
            .0
            .perform_action(None, crate::UwbAction::GetGlobalStats)
            .err_into::<ClientError>()
            .await
        {
            Ok(crate::UwbActionResult::GlobalStats(stats)) => Ok(Some(stats)),
            Ok(_) => Err(ClientError::Recv("Unexpected action result for get_global_stats".into())),
            Err(e) => Err(e),
        }
    }

    fn clone_box(&self) -> Box<dyn ChipClient> {
        Box::new(self.clone())
    }
}

#[cfg(any(test, feature = "testing"))]
impl UwbClient {
    /// Starts ranging for the given chip and session.
    pub async fn start_ranging(&self, id: ChipId, session_id: u32) -> Result<(), ClientError> {
        self.0
            .perform_action(Some(id), crate::UwbAction::StartRanging { id, session_id })
            .err_into::<ClientError>()
            .await
            .map(|_| ())
    }

    /// Stops ranging for the given chip and session.
    pub async fn stop_ranging(&self, id: ChipId, session_id: u32) -> Result<(), ClientError> {
        self.0
            .perform_action(Some(id), crate::UwbAction::StopRanging { id, session_id })
            .err_into::<ClientError>()
            .await
            .map(|_| ())
    }
}
