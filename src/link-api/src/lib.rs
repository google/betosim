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

//! # Link API
//!
//! This crate defines the public API for the Link Actor in Netsim.
//! It provides types and messages for managing links between simulated chips.
//!
//! ## Design
//!
//! The Link API is designed to be used by other actors (e.g., Device Actor,
//! Frontend) to create, modify, and query links. It uses the
//! `actor-framework`'s resource pattern, where `Link` is the resource and
//! `LinkAction` defines global operations.
//!
//! Note that `LinkAction` only contains domain-specific actions (e.g.,
//! notifying chip addition). Standard CRUD operations (Create, Read, Update,
//! Delete) are provided by the `actor-framework` and are available to all
//! actors.
//!
//! ## Usage
//!
//! ```rust
//! use link_api::{ChipId, ChipKind, LinkAction, LinkCreate};
//! // Create a link
//! let create_params = LinkCreate { sender: ChipId(0), receiver: ChipId(1), rssi: -50 };
//! // Send an action
//! let action = LinkAction::NotifyChipAdded(ChipId(0), ChipKind::BLUETOOTH);
//! ```

pub(crate) mod action;
pub(crate) mod error;
pub use action::LinkAction;
pub use error::LinkError;
use netsim_model::ClientError;
pub use netsim_model::{ChipId, ChipKind, Link, LinkCreate, LinkId, LinkUpdate};

#[cfg_attr(any(test, feature = "testing"), mockall::automock)]
#[async_trait::async_trait]
pub trait LinkClient: std::fmt::Debug + Send + Sync {
    async fn list(&self) -> Result<Vec<Link>, ClientError>;
    async fn create(&self, params: LinkCreate) -> Result<LinkId, ClientError>;
    async fn update(&self, id: LinkId, patch: LinkUpdate) -> Result<(), ClientError>;
    async fn delete(&self, id: LinkId) -> Result<(), ClientError>;
    async fn action(&self, id: Option<LinkId>, action: LinkAction) -> Result<(), ClientError>;
    async fn notify_chip_added(&self, chip_id: ChipId, kind: ChipKind) -> Result<(), ClientError>;
    async fn notify_chip_removed(&self, chip_id: ChipId) -> Result<(), ClientError>;
    async fn reset(&self) -> Result<(), ClientError>;
    async fn shutdown(&self) -> Result<(), ClientError>;
}
