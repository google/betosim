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

//! Link Client
//!
//! This module provides the [`LinkClient`] struct, which is a type-safe wrapper
//! around the generic [`ResourceClient`]. It provides a convenient API for
//! interacting with Link actors.

use std::ops::Deref;

use actor_framework::ResourceClient;
use futures::TryFutureExt;
use netsim_model::{ChipId, ChipKind, ClientError};

use crate::LinkActor;

/// A client for interacting with the Link Actor.
///
/// This client provides a type-safe API for managing chip-to-chip links.
///
/// # Design Philosophy: Stateless Client
///
/// To keep the implementation lightweight, this client is stateless and
/// non-atomic. It does not internally track the existence of links or combine
/// "check-and-create" operations into a single step.
///
/// # Recommended Usage Pattern
///
/// To ensure a link exists with the correct configuration, consumers should use
/// the **Look-then-Act** approach:
///
/// 1. **Find**: Query `list()` for an existing connection.
/// 2. **Path A (Exists)**: Use `update()` with the existing ID.
/// 3. **Path B (Missing)**: Use `create()` with your parameters.
///
/// # Handling Race Conditions
///
/// In environments where multiple scripts or processes may be acting on the
/// same hardware simultaneously, your "Find" results may become stale before
/// you can "Act."
///
/// If you attempt to `create()` a link that was just created by another
/// process, the server will return an `AlreadyExists` error. Robust
/// applications should catch this error and treat it as a signal to re-fetch
/// the link ID and perform an `update()` instead.
#[derive(Clone, Debug)]
pub struct LinkClient {
    pub(crate) inner: ResourceClient<LinkActor>,
}

impl LinkClient {
    pub fn new(inner: ResourceClient<LinkActor>) -> Self {
        Self { inner }
    }
}

#[async_trait::async_trait]
impl link_api::LinkClient for LinkClient {
    async fn list(&self) -> Result<Vec<link_api::Link>, ClientError> {
        self.inner.list().err_into::<ClientError>().await
    }

    async fn create(&self, params: link_api::LinkCreate) -> Result<link_api::LinkId, ClientError> {
        self.inner.create(params).err_into::<ClientError>().await
    }

    async fn update(
        &self,
        id: link_api::LinkId,
        patch: link_api::LinkUpdate,
    ) -> Result<(), ClientError> {
        self.inner.update(id, patch).err_into::<ClientError>().await.map(|_| ())
    }

    async fn delete(&self, id: link_api::LinkId) -> Result<(), ClientError> {
        self.inner.delete(id).err_into::<ClientError>().await
    }

    async fn action(
        &self,
        id: Option<link_api::LinkId>,
        action: link_api::LinkAction,
    ) -> Result<(), ClientError> {
        self.inner.perform_action(id, action).err_into::<ClientError>().await.map(|_| ())
    }

    async fn notify_chip_added(&self, chip_id: ChipId, kind: ChipKind) -> Result<(), ClientError> {
        self.inner
            .perform_action(None, link_api::LinkAction::NotifyChipAdded(chip_id, kind))
            .err_into::<ClientError>()
            .await
            .map(|_| ())
    }

    async fn notify_chip_removed(&self, chip_id: ChipId) -> Result<(), ClientError> {
        self.inner
            .perform_action(None, link_api::LinkAction::NotifyChipRemoved(chip_id))
            .err_into::<ClientError>()
            .await
            .map(|_| ())
    }

    async fn reset(&self) -> Result<(), ClientError> {
        self.inner
            .perform_action(None, link_api::LinkAction::Reset)
            .err_into::<ClientError>()
            .await
            .map(|_| ())
    }

    async fn shutdown(&self) -> Result<(), ClientError> {
        self.inner.shutdown().err_into::<ClientError>().await
    }
}

impl Deref for LinkClient {
    type Target = ResourceClient<LinkActor>;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}
