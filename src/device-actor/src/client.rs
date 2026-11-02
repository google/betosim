// Copyright (C) 2025 The Android Open Source Project
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

//! Device Client
//!
//! Provides the [`DeviceClient`] struct for interacting with Device actors.

use actor_framework::ActorClient;
use device_api::{DeviceAction, DeviceActionResult, DeviceCreate, DeviceId};
use futures::TryFutureExt;
use netsim_model::ClientError;
use tracing::debug;

use crate::DeviceActor;

#[derive(Clone)]
pub struct DeviceClient {
    pub(crate) inner: Box<dyn ActorClient<DeviceActor>>,
}

impl std::fmt::Debug for DeviceClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeviceClient").finish_non_exhaustive()
    }
}

impl DeviceClient {
    pub fn new(inner: Box<dyn ActorClient<DeviceActor>>) -> Self {
        Self { inner }
    }
}

impl DeviceClient {
    /// Creates a new device with the given parameters.
    pub async fn create_device(&self, params: DeviceCreate) -> Result<DeviceId, ClientError> {
        debug!("Sending create device request");
        self.inner.create(params).err_into::<ClientError>().await
    }

    pub async fn get(&self, id: DeviceId) -> Result<Option<device_api::Device>, ClientError> {
        debug!("Sending get request for device {}", id);
        self.inner.get(id).err_into::<ClientError>().await
    }

    pub async fn list(&self) -> Result<Vec<device_api::Device>, ClientError> {
        debug!("Sending list devices request");
        self.inner.list().err_into::<ClientError>().await
    }

    /// Resolves a device ID from a name.
    ///
    /// This method lists all devices and finds the one with the matching name.
    /// If multiple devices have the same name, it returns the first one found.
    pub async fn resolve_id_by_name(&self, name: &str) -> Result<DeviceId, ClientError> {
        debug!("Resolving device ID for name: {}", name);
        let devices = self.list().await?;
        devices
            .into_iter()
            .find(|d| d.name == name)
            .map(|d| DeviceId(d.id))
            .ok_or_else(|| ClientError::Framework(Box::from("device not found".to_string())))
    }

    /// Patches a device by ID or Name.
    ///
    /// If `id` is provided (non-zero), it is used.
    /// Otherwise, `name` is used to resolve the ID.
    pub async fn patch(
        &self,
        id: Option<u32>,
        name: Option<&str>,
        mut update: device_api::DeviceUpdate,
    ) -> Result<(), ClientError> {
        let device_id = if let Some(id) = id.filter(|&v| v != 0) {
            DeviceId(id)
        } else if let Some(name) = name {
            self.resolve_id_by_name(name).await?
        } else {
            return Err(ClientError::Send("Device ID or Name must be provided".to_string()));
        };
        update.id = device_id.0;
        self.update(device_id, update).await.map(|_| ())
    }

    /// Updates an existing device's properties.
    pub async fn update(
        &self,
        id: DeviceId,
        update: device_api::DeviceUpdate,
    ) -> Result<device_api::Device, ClientError> {
        debug!("Sending update request for device {}", id);
        self.inner.update(id, update).err_into::<ClientError>().await
    }

    /// Resets a device to its default state. If `id` is `None`, performs a
    /// global reset.
    pub async fn reset(&self, id: Option<DeviceId>) -> Result<(), ClientError> {
        debug!("Sending reset request for device {:?}", id);
        match self.inner.perform_action(id, DeviceAction::Reset).err_into::<ClientError>().await? {
            DeviceActionResult::Success => Ok(()),
            _other => Err(ClientError::Recv("Unexpected action result".to_string())),
        }
    }

    /// Notifies the device that one of its chips has been removed.
    ///
    /// This is used to maintain consistency between the device and its chips.
    pub async fn notify_chip_removed(
        &self,
        id: DeviceId,
        chip_id: netsim_model::ChipId,
    ) -> Result<(), ClientError> {
        debug!("Sending notify_chip_removed request for device {} chip {}", id, chip_id);
        match self
            .inner
            .perform_action(Some(id), DeviceAction::NotifyChipRemoved(id, chip_id))
            .err_into::<ClientError>()
            .await?
        {
            DeviceActionResult::Success => Ok(()),
            _other => Err(ClientError::Recv("Unexpected action result".to_string())),
        }
    }

    /// Fetches the latest radio statistics from all associated chip clients.
    pub async fn get_radio_stats(
        &self,
    ) -> Result<Vec<netsim_model::NetsimRadioStats>, ClientError> {
        self.inner
            .perform_action(None, DeviceAction::GetRadioStats)
            .err_into::<ClientError>()
            .await
            .map(|res| match res {
                DeviceActionResult::Statistics(stats) => stats,
                _ => vec![],
            })
    }

    /// Deletes a device (restricted to internal devices).
    pub async fn delete_device(&self, id: DeviceId) -> Result<(), ClientError> {
        debug!("Sending DeleteDevice request for device {}", id);
        match self
            .inner
            .perform_action(None, DeviceAction::DeleteDevice(id))
            .err_into::<ClientError>()
            .await?
        {
            DeviceActionResult::Success => Ok(()),
            _other => Err(ClientError::Recv("Unexpected action result".to_string())),
        }
    }

    /// Deletes a device (unrestricted, for cleanup).
    pub async fn delete(&self, id: DeviceId) -> Result<(), ClientError> {
        debug!("Sending delete request for device {}", id);
        self.inner.delete(id).err_into::<ClientError>().await
    }

    /// Creates or updates a device based on PacketStream parameters and returns
    /// both the device ID and the newly assigned chip ID.
    ///
    /// Uses `AddChipByGuid` to atomically find/create a device by GUID.
    pub async fn add_chip_with_id(
        &self,
        params: netsim_model::DeviceAddChip,
    ) -> Result<(DeviceId, netsim_model::ChipId), ClientError> {
        let result = self
            .inner
            .perform_action(
                None, // Global action
                DeviceAction::AddChipByGuid { params: Box::new(params) },
            )
            .err_into::<ClientError>()
            .await?;

        match result {
            DeviceActionResult::AddChipByGuidSuccess { device_id, chip_id } => {
                Ok((device_id, chip_id))
            }
            _ => Err(ClientError::Recv("Unexpected action result for AddChipByGuid".to_string())),
        }
    }

    /// Creates or updates a device based on PacketStream parameters.
    ///
    /// Uses `AddChipByGuid` to atomically find/create a device by GUID.
    pub async fn add_chip(
        &self,
        params: netsim_model::DeviceAddChip,
    ) -> Result<DeviceId, ClientError> {
        self.add_chip_with_id(params).await.map(|(device_id, _)| device_id)
    }

    /// Shuts down the device actor.
    pub async fn shutdown(&self) -> Result<(), ClientError> {
        debug!("Sending shutdown request");
        self.inner.shutdown().err_into::<ClientError>().await
    }

    /// Triggers a persistence of the current statistics to disk.
    pub async fn save_stats(&self) -> Result<(), ClientError> {
        self.inner
            .perform_action(None, DeviceAction::SaveStats)
            .err_into::<ClientError>()
            .await
            .map(|_| ())
    }
}
