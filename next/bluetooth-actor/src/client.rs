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

//! Bluetooth Client
//!
//! This module provides the [`BluetoothClient`] struct, which is a wrapper
//! around [`ResourceClient<BluetoothActor>`].

use std::ops::Deref;

use actor_framework::ResourceClient;
use netsim_model::{ChipClient, ChipCreate, ChipId, ClientError};

use crate::{BluetoothAction, BluetoothActionResult, BluetoothActor};

/// A client for the Bluetooth actor.
#[derive(Clone, Debug)]
pub struct BluetoothClient(pub ResourceClient<BluetoothActor>);

impl Deref for BluetoothClient {
    type Target = ResourceClient<BluetoothActor>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

// TODO: Consider generic impl<T> ChipClient for ResourceClient<T>.
#[async_trait::async_trait]
impl ChipClient for BluetoothClient {
    async fn create(&self, id: ChipId, params: ChipCreate) -> Result<(), ClientError> {
        self.0
            .create_with_id(id, params)
            .await
            .map(|_| ())
            .map_err(|e| ClientError::Send(e.to_string()))
    }

    async fn read(&self, id: ChipId) -> Result<netsim_model::Chip, ClientError> {
        self.0
            .get(id)
            .await
            .map_err(|e| ClientError::Send(e.to_string()))?
            .ok_or(ClientError::Chip(netsim_model::ChipError::ChipNotFound(id)))
    }

    async fn update(
        &self,
        id: ChipId,
        patch: netsim_model::ChipUpdate,
    ) -> Result<netsim_model::Chip, ClientError> {
        self.0.update(id, patch).await.map_err(|e| ClientError::Send(e.to_string()))
    }

    async fn delete(&self, id: ChipId) -> Result<(), ClientError> {
        self.0.delete(id).await.map_err(|e| ClientError::Send(e.to_string()))
    }

    async fn read_statistics(&self) -> Result<Box<[netsim_model::NetsimRadioStats]>, ClientError> {
        match self.0.perform_action(None, BluetoothAction::GetStatistics).await {
            Ok(BluetoothActionResult::Statistics(stats)) => Ok(stats),
            Ok(_) => Err(ClientError::Recv("Unexpected action result".into())),
            Err(e) => Err(ClientError::Send(e.to_string())),
        }
    }

    async fn read_count_for_testing(&self) -> Result<usize, ClientError> {
        self.0.list().await.map(|chips| chips.len()).map_err(|e| ClientError::Send(e.to_string()))
    }

    async fn shutdown(&self) -> Result<(), ClientError> {
        self.0.shutdown().await.map_err(|e| ClientError::Send(e.to_string()))
    }

    async fn reset(&self, id: ChipId) -> Result<netsim_model::Chip, ClientError> {
        match self.0.perform_action(Some(id), BluetoothAction::Reset { id }).await {
            Ok(BluetoothActionResult::Chip(chip)) => Ok(*chip),
            Ok(_) => Err(ClientError::Recv("Unexpected action result for reset".into())),
            Err(e) => Err(ClientError::Send(e.to_string())),
        }
    }

    fn clone_box(&self) -> Box<dyn ChipClient> {
        Box::new(self.clone())
    }
}
