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

//! Capture Client

use std::ops::Deref;

use actor_framework::ResourceClient;
use bytes::Bytes;
use capture_api::{
    CaptureAction, CaptureActionResult, CaptureCreate, CaptureInfo, CaptureSender, Direction,
};
use futures::TryFutureExt;
use netsim_model::{ChipId, ClientError};

use crate::CaptureActor;

/// A client for interacting with the Capture Actor.
///
/// This client provides a type-safe API for managing packet captures.
#[derive(Clone, Debug)]
pub struct CaptureClient {
    pub(crate) inner: ResourceClient<CaptureActor>,
}

#[async_trait::async_trait]
impl CaptureSender for CaptureClient {
    async fn create_capture(
        &self,
        chip_id: ChipId,
        create: CaptureCreate,
    ) -> Result<(), ClientError> {
        self.inner.create_with_id(chip_id, create).err_into::<ClientError>().await.map(|_| ())
    }

    async fn packet_sender(
        &self,
        chip_id: ChipId,
    ) -> Result<
        tokio::sync::mpsc::UnboundedSender<(std::time::SystemTime, Direction, Bytes)>,
        ClientError,
    > {
        let result = self
            .inner
            .perform_action(Some(chip_id), CaptureAction::GetPacketSender)
            .err_into::<ClientError>()
            .await?;

        match result {
            CaptureActionResult::PacketSender(sender) => Ok(sender),
            _ => Err(ClientError::Recv("Unexpected result from GetPacketSender".into())),
        }
    }
}

impl CaptureClient {
    pub fn new(inner: ResourceClient<CaptureActor>) -> Self {
        Self { inner }
    }

    /// Creates a new capture session for a chip.
    pub async fn create_capture(
        &self,
        chip_id: ChipId,
        params: CaptureCreate,
    ) -> Result<ChipId, ClientError> {
        self.inner.create_with_id(chip_id, params).err_into::<ClientError>().await
    }

    /// Updates the capture state (enable/disable) for a chip.
    pub async fn update_capture(
        &self,
        chip_id: ChipId,
        enabled: bool,
    ) -> Result<CaptureInfo, ClientError> {
        self.inner.update(chip_id, enabled).err_into::<ClientError>().await
    }

    /// Gets the capture info for a chip.
    pub async fn get_capture(&self, chip_id: ChipId) -> Result<Option<CaptureInfo>, ClientError> {
        self.inner.get(chip_id).err_into::<ClientError>().await
    }

    /// Deletes a capture session.
    pub async fn delete_capture(&self, chip_id: ChipId) -> Result<(), ClientError> {
        self.inner.delete(chip_id).err_into::<ClientError>().await
    }

    /// Lists all active captures.
    pub async fn list_captures(&self) -> Result<Vec<CaptureInfo>, ClientError> {
        self.inner.list().err_into::<ClientError>().await
    }

    /// Patches a capture (alias for update_capture to match old client).
    pub async fn patch_capture(&self, chip_id: ChipId, enabled: bool) -> Result<(), ClientError> {
        self.update_capture(chip_id, enabled).err_into::<ClientError>().await.map(|_| ())
    }

    /// Subscribes to a live PCAP stream of a chip's packets.
    ///
    /// The first message is the PCAP global header, then one message per
    /// record. Works whether or not file capture is enabled. The stream ends
    /// when the capture is deleted; drop the receiver to unsubscribe.
    pub async fn stream_capture(
        &self,
        chip_id: ChipId,
    ) -> Result<tokio::sync::mpsc::Receiver<Bytes>, ClientError> {
        match self
            .inner
            .perform_action(Some(chip_id), CaptureAction::Subscribe)
            .err_into::<ClientError>()
            .await?
        {
            CaptureActionResult::Stream(receiver) => Ok(receiver),
            res => Err(ClientError::Recv(format!("Unexpected result from Subscribe: {res:?}"))),
        }
    }
}

impl Deref for CaptureClient {
    type Target = ResourceClient<CaptureActor>;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}
