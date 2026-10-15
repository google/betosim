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

//! # Netsim Capture API
//!
//! This crate defines the public API for the Netsim packet capture system.
//! It includes the core data structures, traits, and actions necessary for
//! interacting with the `capture-actor`.
//!
//! ## Architecture
//!
//! ### Structure
//!
//! The capture system is structured as follows:
//!
//! - **`netsim-model`**: Remains the base layer for data models (e.g.,
//!   `ChipId`, `ChipKind`). It does not depend on `capture-api`.
//! - **`capture-api`**: Defines the behavioral contract for the capture
//!   service.
//!     - Depends on: `netsim-model`
//!     - Contains: `CaptureAction` (enum of supported operations),
//!       `CaptureActionResult`, `CaptureSender` trait, and re-exports of
//!       relevant models.
//! - **`capture-actor`**: Implements the capture service.
//!     - Depends on: `capture-api`, `netsim-model`.
//!     - Implements the handling logic for `CaptureAction`.
//! - **`netsim-client`**: Provides a client-side wrapper for the capture
//!   service.
//!     - Depends on: `capture-api`, `netsim-model`.
//! - **Consumer Crates** (e.g., `device-actor`, `daemon`):
//!     - Depend on: `capture-api`, `netsim-client` (and `netsim-model` if
//!       needed).
//!     - Do not depend directly on `capture-actor` for API definitions.
//!
//! ### Dependency Graph (Simplified)
//!
//! ```text
//! graph TD
//!     ConsumerCrates[Consumer Crates<br>(device-actor, daemon)] --> NetsimClient[netsim-client]
//!     ConsumerCrates --> CaptureAPI[capture-api]
//!     NetsimClient --> CaptureAPI
//!     CaptureActor[capture-actor] --> CaptureAPI
//!     CaptureActor --> NetsimModel[netsim-model]
//!     CaptureAPI --> NetsimModel
//!     NetsimClient --> NetsimModel
//! ```
//!
//! ## API Definition
//!
//! The `capture-api` crate defines the following key components:
//!
//! ### `CaptureAction`
//!
//! An enum representing the actions that can be performed on the capture
//! service. This serves as the primary interface for consumers.
//!
//! ### `CaptureSender`
//!
//! A trait defining the interface for sending packets to the capture system.
//! This allows for dependency injection and easier testing.
//!
//! ## Benefits
//!
//! 1. **Clearer Boundaries**: The separation between API and implementation is
//!    now explicit.
//! 2. **Reduced Coupling**: Consumer crates only depend on the API, not the
//!    implementation.
//! 3. **Improved Build Times**: Changes to the `capture-actor` implementation
//!    do not require recompiling consumer crates, as long as the API remains
//!    stable.
//! 4. **Easier Testing**: Mocking the capture service is now a matter of
//!    implementing the `CaptureAction` contract or the `CaptureSender` trait.

pub mod io;

use std::{
    path::PathBuf,
    sync::{Arc, atomic::AtomicBool},
};

use async_trait::async_trait;
use bytes::Bytes;
use netsim_model::{ChipId, ChipKind, ClientError};
use serde::{Deserialize, Serialize};

#[async_trait]
pub trait CaptureSender: Send + Sync {
    /// Creates a new capture for a chip.
    async fn create_capture(
        &self,
        chip_id: ChipId,
        create: CaptureCreate,
    ) -> Result<(), ClientError>;

    /// Captures a single packet.
    ///
    /// # Arguments
    /// * `chip_id` - The ID of the chip.
    /// * `direction` - The direction of the packet.
    ///
    /// Returns a channel to send packet bytes.
    async fn packet_sender(
        &self,
        chip_id: ChipId,
    ) -> Result<
        tokio::sync::mpsc::UnboundedSender<(std::time::SystemTime, Direction, Bytes)>,
        ClientError,
    >;
}

/// Direction of the packet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Direction {
    /// Packet was sent from the device.
    Sent,
    /// Packet was received by the device.
    Received,
}

/// Parameters for creating a new capture.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaptureCreate {
    /// The kind of chip.
    pub chip_kind: ChipKind,
    /// The name of the device.
    pub device_name: String,
    /// Optional flag to be updated when capture status changes.
    #[serde(skip)]
    pub enabled_flag: Arc<AtomicBool>,
}

/// Actions that can be performed on a capture.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum CaptureAction {
    /// Gets a packet sender for capture.
    GetPacketSender,
    /// Patch a capture (e.g., enable/disable).
    Patch { chip_id: ChipId, enabled: bool },
    /// Create a new capture.
    Create { chip_id: ChipId, chip_kind: ChipKind, device_name: String },
    /// Delete a capture.
    Delete { chip_id: ChipId },
    /// Set default capture directory.
    SetCaptureDirectory { path: PathBuf },
    /// Subscribe to a live PCAP stream of the chip's packets. Independent of
    /// whether file capture is enabled.
    Subscribe,
}

/// Result of an action performed on a capture.
///
/// Not `Clone`: [`CaptureActionResult::Stream`] owns a channel receiver.
#[derive(Debug, Serialize, Deserialize)]
pub enum CaptureActionResult {
    /// Action succeeded.
    Success,
    /// The action resulted in an update and returns the new state.
    Updated(CaptureInfo),
    /// Returns a high-throughput packet sender.
    #[serde(skip)]
    PacketSender(tokio::sync::mpsc::UnboundedSender<(std::time::SystemTime, Direction, Bytes)>),
    /// Live PCAP stream: the first message is the PCAP global header, then one
    /// message per record (record header and payload). The stream ends when
    /// the capture is deleted.
    #[serde(skip)]
    Stream(tokio::sync::mpsc::Receiver<Bytes>),
}

/// Information about a capture.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CaptureInfo {
    /// The ID of the chip.
    pub chip_id: ChipId,
    /// The kind of chip.
    pub chip_kind: ChipKind,
    /// The name of the device.
    pub device_name: String,
    /// Whether capture is enabled.
    pub enabled: bool,
    /// Number of records written.
    pub records_written: u64,
    /// Number of bytes written.
    pub bytes_written: u64,
    /// Timestamp (seconds since Unix epoch) when capture was started.
    pub seconds: i64,
    /// Subsecond nanoseconds when capture was started.
    pub nanos: i32,
    /// Path to the capture file on disk.
    pub filepath: Option<std::path::PathBuf>,
}
