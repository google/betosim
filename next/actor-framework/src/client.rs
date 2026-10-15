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

//! # Generic Client
//!
//! This module defines the generic client for communicating with actors.

use tokio::sync::{mpsc, oneshot};

use crate::{error::FrameworkError, message::ResourceRequest, service::ActorService};

/// A type-safe client for interacting with a `ResourceActor`.
// ResourceClient manual Clone implementation to avoid T: Clone bound
impl<T: ActorService> Clone for ResourceClient<T> {
    fn clone(&self) -> Self {
        Self { sender: self.sender.clone() }
    }
}
/// ## ResourceClient
///
/// The `ResourceClient<T>` provides a type‑safe, async API for interacting with
/// a `ResourceActor<T>`. It forwards CRUD + Action requests over a Tokio mpsc
/// channel and returns results via oneshot channels. The client is cheap to
/// clone and can be shared across tasks.
///
/// * **Cloneable** – holds only a sender, so cloning is inexpensive.
/// * **Async API** – all methods return `Future`s that resolve to `Result<…,
///   FrameworkError>`.
/// * **Generic** – works with any resource that implements `ActorService`.
pub struct ResourceClient<T: ActorService> {
    sender: mpsc::Sender<ResourceRequest<T>>,
}

impl<T: ActorService> std::fmt::Debug for ResourceClient<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ResourceClient").field("sender", &"mpsc::Sender").finish()
    }
}

impl<T: ActorService> ResourceClient<T> {
    pub fn new(sender: mpsc::Sender<ResourceRequest<T>>) -> Self {
        Self { sender }
    }

    pub async fn create(&self, params: T::Create) -> Result<T::Id, FrameworkError<T::Error>> {
        self.create_internal(params, None).await
    }

    pub async fn create_with_id(
        &self,
        id: T::Id,
        params: T::Create,
    ) -> Result<T::Id, FrameworkError<T::Error>> {
        self.create_internal(params, Some(id)).await
    }

    async fn create_internal(
        &self,
        params: T::Create,
        id: Option<T::Id>,
    ) -> Result<T::Id, FrameworkError<T::Error>> {
        let (respond_to, response) = oneshot::channel();
        // Construct the Create message with or without ID depending on message.rs
        // availability.
        self.sender
            .send(ResourceRequest::Create { params, id, respond_to })
            .await
            .map_err(|err| FrameworkError::ActorClosed(Box::new(err)))?;
        response.await.map_err(FrameworkError::ActorDropped)?.map_err(FrameworkError::ServiceError)
    }

    pub async fn get(&self, id: T::Id) -> Result<Option<T::Entity>, FrameworkError<T::Error>> {
        let (respond_to, response) = oneshot::channel();
        self.sender
            .send(ResourceRequest::Get { id, respond_to })
            .await
            .map_err(|err| FrameworkError::ActorClosed(Box::new(err)))?;
        response.await.map_err(FrameworkError::ActorDropped)?.map_err(FrameworkError::ServiceError)
    }

    pub async fn update(
        &self,
        id: T::Id,
        update: T::Update,
    ) -> Result<T::Entity, FrameworkError<T::Error>> {
        let (respond_to, response) = oneshot::channel();
        self.sender
            .send(ResourceRequest::Update { id, update, respond_to })
            .await
            .map_err(|err| FrameworkError::ActorClosed(Box::new(err)))?;
        response.await.map_err(FrameworkError::ActorDropped)?.map_err(FrameworkError::ServiceError)
    }

    pub async fn delete(&self, id: T::Id) -> Result<(), FrameworkError<T::Error>> {
        let (respond_to, response) = oneshot::channel();
        self.sender
            .send(ResourceRequest::Delete { id, respond_to })
            .await
            .map_err(|err| FrameworkError::ActorClosed(Box::new(err)))?;
        response.await.map_err(FrameworkError::ActorDropped)?.map_err(FrameworkError::ServiceError)
    }

    pub async fn perform_action(
        &self,
        id: Option<T::Id>,
        action: T::Action,
    ) -> Result<T::ActionResult, FrameworkError<T::Error>> {
        let (respond_to, response) = oneshot::channel();
        self.sender
            .send(ResourceRequest::Action { id, action, respond_to })
            .await
            .map_err(|err| FrameworkError::ActorClosed(Box::new(err)))?;
        response.await.map_err(FrameworkError::ActorDropped)?.map_err(FrameworkError::ServiceError)
    }

    /// List all entities.
    pub async fn list(&self) -> Result<Vec<T::Entity>, FrameworkError<T::Error>> {
        let (respond_to, response) = oneshot::channel();
        self.sender
            .send(ResourceRequest::List { respond_to })
            .await
            .map_err(|err| FrameworkError::ActorClosed(Box::new(err)))?;
        response.await.map_err(FrameworkError::ActorDropped)?.map_err(FrameworkError::ServiceError)
    }

    /// Shut down the actor.
    pub async fn shutdown(&self) -> Result<(), FrameworkError<T::Error>> {
        let (respond_to, response) = oneshot::channel();
        self.sender
            .send(ResourceRequest::Shutdown { respond_to })
            .await
            .map_err(|err| FrameworkError::ActorClosed(Box::new(err)))?;
        response.await.map_err(FrameworkError::ActorDropped)?.map_err(FrameworkError::ServiceError)
    }
}

/// A trait for interacting with an actor.
///
/// This trait abstracts over the `ResourceClient` to allow for mocking.
#[cfg_attr(any(test, feature = "testing"), mockall::automock)]
#[async_trait::async_trait]
pub trait ActorClient<T: ActorService>: Send + Sync {
    async fn create(&self, params: T::Create) -> Result<T::Id, FrameworkError<T::Error>>;
    async fn create_with_id(
        &self,
        id: T::Id,
        params: T::Create,
    ) -> Result<T::Id, FrameworkError<T::Error>>;
    async fn get(&self, id: T::Id) -> Result<Option<T::Entity>, FrameworkError<T::Error>>;
    async fn update(
        &self,
        id: T::Id,
        update: T::Update,
    ) -> Result<T::Entity, FrameworkError<T::Error>>;
    async fn delete(&self, id: T::Id) -> Result<(), FrameworkError<T::Error>>;
    async fn perform_action(
        &self,
        id: Option<T::Id>,
        action: T::Action,
    ) -> Result<T::ActionResult, FrameworkError<T::Error>>;
    async fn list(&self) -> Result<Vec<T::Entity>, FrameworkError<T::Error>>;
    async fn shutdown(&self) -> Result<(), FrameworkError<T::Error>>;
    fn clone_box(&self) -> Box<dyn ActorClient<T>>;
}

#[async_trait::async_trait]
impl<T: ActorService + Send + Sync> ActorClient<T> for ResourceClient<T> {
    async fn create(&self, params: T::Create) -> Result<T::Id, FrameworkError<T::Error>> {
        self.create(params).await
    }
    async fn create_with_id(
        &self,
        id: T::Id,
        params: T::Create,
    ) -> Result<T::Id, FrameworkError<T::Error>> {
        self.create_with_id(id, params).await
    }
    async fn get(&self, id: T::Id) -> Result<Option<T::Entity>, FrameworkError<T::Error>> {
        self.get(id).await
    }
    async fn update(
        &self,
        id: T::Id,
        update: T::Update,
    ) -> Result<T::Entity, FrameworkError<T::Error>> {
        self.update(id, update).await
    }
    async fn delete(&self, id: T::Id) -> Result<(), FrameworkError<T::Error>> {
        self.delete(id).await
    }
    async fn perform_action(
        &self,
        id: Option<T::Id>,
        action: T::Action,
    ) -> Result<T::ActionResult, FrameworkError<T::Error>> {
        self.perform_action(id, action).await
    }
    async fn list(&self) -> Result<Vec<T::Entity>, FrameworkError<T::Error>> {
        self.list().await
    }
    async fn shutdown(&self) -> Result<(), FrameworkError<T::Error>> {
        self.shutdown().await
    }
    fn clone_box(&self) -> Box<dyn ActorClient<T>> {
        Box::new(self.clone())
    }
}

impl<T: ActorService> Clone for Box<dyn ActorClient<T>> {
    fn clone(&self) -> Box<dyn ActorClient<T>> {
        self.clone_box()
    }
}
