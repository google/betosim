// Copyright 2023-2025 The Android Open Source Project
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

//! Chip model and management.
//!
//! This module defines the core data structures for representing chips in
//! Netsim, including their types, state, and communication channels. It handles
//! the lifecycle of chips, including creation, updates, and deletion.

use std::fmt;

pub(crate) use netsim_types::ChipKind;
use serde::{Deserialize, Serialize};
use tokio::sync::oneshot;

use crate::{
    chip_error::ChipError,
    client_error::ClientError,
    device::{DeviceId, Pose, api::PoseUpdate},
    stats::NetsimRadioStats,
};

/// Maps the model ChipKind to the stats RadioKind.
pub fn chip_kind_to_radio_kind(kind: ChipKind) -> crate::stats::RadioKind {
    use crate::stats::RadioKind;
    match kind {
        ChipKind::BLUETOOTH => RadioKind::BluetoothLowEnergy,
        ChipKind::WIFI => RadioKind::Wifi,
        ChipKind::UWB => RadioKind::Uwb,
        ChipKind::NFC => RadioKind::Nfc,
        _ => RadioKind::Unspecified,
    }
}
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Radio {
    pub state: Option<bool>,
    pub range: f32,
    pub tx_count: u64,
    pub rx_count: u64,
}

pub use crate::packet_streamer::{PacketSink, PacketStream};

// CHIP SERVICE
//
// This module implements the actor model for managing simulated chips.
// There is one chip service actor for each network type (Bluetooth, UWB,
// Wi-Fi).
//
// The "service" is the actor's message-processing loop, which would be
// implemented in a separate task that owns the `mpsc::Receiver<ChipRequest>`.
//
// The "client" (`ChipClient`) is a handle to the actor that allows other parts
// of the system to send messages to it.

/// A unique identifier for a simulated chip, represented as a u32.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct ChipId(pub u32);

impl From<ChipId> for u32 {
    fn from(id: ChipId) -> Self {
        id.0
    }
}

impl From<u32> for ChipId {
    fn from(id: u32) -> Self {
        ChipId(id)
    }
}
/// A reply channel for sending the result of an operation back to the caller.
pub(crate) type Responder<T> = oneshot::Sender<Result<T, ChipError>>;

/// Defines the message protocol for the chip service actor.
///
/// Each variant corresponds to a specific command that can be sent to the
/// service. For commands that require a response, a `Responder` channel is
/// included.
#[derive(Debug)]
pub enum ChipRequest {
    /// Create a new chip.
    Create {
        /// The ID of the new chip.
        id: ChipId,
        /// The parameters for the new chip.
        params: Box<ChipCreate>,
        /// The channel to send the result.
        respond_to: Responder<()>,
    },
    /// Get the state of a chip.
    Read {
        /// The ID of the chip to retrieve.
        id: ChipId,
        /// The channel to send the chip's state back on.
        respond_to: Responder<Chip>,
    },
    /// Update an existing chip.
    Update {
        /// The ID of the chip to patch.
        id: ChipId,
        /// The patch to apply to the chip.
        patch: Box<ChipUpdate>,
        /// The channel to send the updated chip state back on.
        respond_to: Responder<Chip>,
    },
    /// Delete a chip.
    Delete {
        /// The ID of the chip to delete.
        id: ChipId,
        /// The channel to send the operation result back on.
        respond_to: Responder<()>,
    },
    /// Reset all event counters for a chip.
    Reset {
        /// The ID of the chip to reset.
        id: ChipId,
        /// The channel to send the updated chip state back on.
        respond_to: Responder<Chip>,
    },
    /// Get radio statistics for all chips.
    GetStatistics {
        /// The channel to send the statistics back on.
        respond_to: Responder<Box<[NetsimRadioStats]>>,
    },
    /// Get the total number of chips for testing purposes.
    GetCountForTesting {
        /// The channel to send the count back on.
        respond_to: Responder<usize>,
    },
    /// Command the service to shut down gracefully.
    Shutdown,
}

/// The top-level parameters for creating any kind of chip.
///
/// This struct provides all the necessary information for creating a new
/// simulated chip, including its packet transport, and technology-specific
/// configurations. It is used in the [`ChipRequest::Create`] variant and
/// passed to the chip service through the [`ChipClient::create`] method.
pub struct ChipCreate {
    /// The transport for packet input.
    pub packet_stream: Option<PacketStream>,
    /// The transport for packet output.
    pub packet_sink: Option<PacketSink>,
    /// The standard Chip representation.
    pub chip: Chip,
}

impl fmt::Debug for ChipCreate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ChipCreate").field("chip", &self.chip).finish_non_exhaustive()
    }
}

pub(crate) use crate::{
    bluetooth::{BeaconParams, BluetoothCreate, BluetoothMode, beacon::BleBeacon},
    cell::CellCreate,
    nfc::{Nfc, NfcUpdate},
    uwb::{Uwb, UwbCreate, UwbMode},
    wifi::WifiCreate,
};

impl fmt::Display for ChipId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

// ======================================================================
// Chip - All stateful fields for a chip
// ======================================================================

/// The generic representation of a simulated chip.
///
/// A major purpose of the `Chip` wrapper class is:
/// 1. To store common fields for all chip kinds.
/// 2. To allow common signatures (the `ChipActor`) for clients.
///
/// # Architecture
///
/// The model follows a **Component - Variant** pattern:
///
/// 1. **`Chip` (The Entity)**: Contains generic fields common to all chips,
///    such as `id`, `name`, `position`, and `device_id`.
/// 2. **`ChipVariant` (The Dispatcher)**: The `variant` field is an enum that
///    strictly owns the technology-specific struct (e.g.,
///    `Bluetooth(Bluetooth)`).
///
/// Use `Chip` for generic operations (positioning, lifecycle) and access
/// `variant` for technology-specific state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Chip {
    pub id: u32,
    pub kind: ChipKind,
    pub name: String,
    pub manufacturer: String,
    pub product_name: String,
    pub pose: Pose,
    pub device_id: DeviceId,
    pub variant: Option<ChipVariant>,
    pub links: Vec<(ChipId, i8)>,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

impl Default for Chip {
    fn default() -> Self {
        Self {
            id: 0,
            kind: ChipKind::default(),
            name: String::default(),
            manufacturer: String::default(),
            product_name: String::default(),
            pose: Pose::default(),
            device_id: DeviceId::default(),
            variant: None,
            links: Vec::default(),
            enabled: true,
        }
    }
}

impl Chip {
    pub fn is_le_enabled(&self) -> bool {
        if let Some(ChipVariant::Bluetooth(bt)) = &self.variant {
            return bt.low_energy.state.unwrap_or(true);
        }
        true
    }

    pub fn is_classic_enabled(&self) -> bool {
        if let Some(ChipVariant::Bluetooth(bt)) = &self.variant {
            return bt.classic.state.unwrap_or(true);
        }
        true
    }

    pub fn is_uwb_enabled(&self) -> bool {
        matches!(
            self.variant,
            Some(ChipVariant::Uwb(Uwb { radio: Radio { state: Some(true) | None, .. }, .. }))
        )
    }

    pub fn is_nfc_enabled(&self) -> bool {
        matches!(
            self.variant,
            Some(ChipVariant::Nfc(Nfc { radio: Radio { state: Some(true) | None, .. } }))
        )
    }
}

#[cfg(any(test, feature = "testing"))]
impl Chip {
    pub fn new_test_uwb(name: impl Into<String>) -> Self {
        Self {
            kind: ChipKind::UWB,
            name: name.into(),
            manufacturer: "Google".into(),
            product_name: "Netsim UWB".into(),
            variant: ChipVariant::try_from(ChipKind::UWB).ok(),
            ..Default::default()
        }
    }

    pub fn new_test_uwb_anchor(name: impl Into<String>, address: Option<String>) -> Self {
        Self {
            kind: ChipKind::UWB,
            name: name.into(),
            manufacturer: "Google".into(),
            product_name: "Netsim UWB Anchor".into(),
            variant: Some(ChipVariant::Uwb(Uwb {
                radio: Default::default(),
                address: address.unwrap_or_default(),
                mode: UwbMode::Anchor,
            })),
            ..Default::default()
        }
    }

    pub fn new_test_ble(name: impl Into<String>) -> Self {
        Self {
            kind: ChipKind::BLUETOOTH,
            name: name.into(),
            manufacturer: "Google".into(),
            product_name: "Netsim BLE".into(),
            variant: ChipVariant::try_from(ChipKind::BLUETOOTH).ok(),
            ..Default::default()
        }
    }

    pub fn new_test_cell(name: impl Into<String>) -> Self {
        Self {
            kind: ChipKind::CELLULAR,
            name: name.into(),
            manufacturer: "Google".into(),
            product_name: "Netsim Cell".into(),
            variant: ChipVariant::try_from(ChipKind::CELLULAR).ok(),
            ..Default::default()
        }
    }

    pub fn new_test_nfc(name: impl Into<String>) -> Self {
        Self {
            kind: ChipKind::NFC,
            name: name.into(),
            manufacturer: "Google".into(),
            product_name: "Netsim NFC".into(),
            variant: ChipVariant::try_from(ChipKind::NFC).ok(),
            ..Default::default()
        }
    }
}

fn default_enabled() -> bool {
    true
}

/// Information about a chip, including technology-specific details.
///
/// # Component Composition
///
/// Each variant owns a dedicated struct from a technology-specific module.
/// These structs often compose or wrap the generic `Radio` struct.
///
/// - **Bluetooth**: Has two `Radio` components (`classic` and `low_energy`).
/// - **Wifi**: Wraps a single `Radio` component.
/// - **Uwb**: Wraps a single `Radio` component.
/// - **Cell**: Contains cellular-specific state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ChipVariant {
    Bluetooth(Box<crate::bluetooth::Bluetooth>),
    Wifi(crate::wifi::Wifi),
    Uwb(crate::uwb::Uwb),
    Cell(crate::cell::Cell),
    CellularData(crate::cellular_data::CellularData),
    Ethernet(crate::ethernet::Ethernet),
    Nfc(Nfc),
}

/// Builds the default variant for a chip kind.
///
/// The match is exhaustive so that adding a `ChipKind` without a radio
/// implementation fails to compile rather than silently picking another radio.
impl TryFrom<ChipKind> for ChipVariant {
    type Error = ChipError;

    fn try_from(kind: ChipKind) -> Result<Self, Self::Error> {
        let variant = match kind {
            ChipKind::BLUETOOTH => ChipVariant::Bluetooth(Box::new(crate::bluetooth::Bluetooth {
                low_energy: Default::default(),
                classic: Default::default(),
                address: String::new(),
                bt_properties: Default::default(),
                mode: crate::chip::BluetoothMode::Device(Default::default()),
                preset: None,
            })),
            ChipKind::WIFI => ChipVariant::Wifi(Default::default()),
            ChipKind::UWB => ChipVariant::Uwb(Default::default()),
            ChipKind::CELLULAR => ChipVariant::Cell(crate::cell::Cell {
                radio: Default::default(),
                state: "unknown".into(),
                sim_type: None,
                quirks: Default::default(),
                ..Default::default()
            }),
            ChipKind::CELLULAR_DATA => ChipVariant::CellularData(Default::default()),
            ChipKind::ETHERNET => ChipVariant::Ethernet(Default::default()),
            ChipKind::NFC => ChipVariant::Nfc(Default::default()),
            ChipKind::UNSPECIFIED => return Err(ChipError::UnsupportedChipKind(kind)),
        };
        Ok(variant)
    }
}

// ======================================================================
// ChipUpdate - All patchable fields for a chip
// ======================================================================

/// This struct represents the partial, optional set of changes to a
/// Chip provided by the client.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ChipUpdate {
    pub id: Option<ChipId>,
    pub name: Option<String>,
    pub manufacturer: Option<String>,
    pub product_name: Option<String>,
    #[serde(default)]
    pub pose: PoseUpdate,
    pub variant: Option<ChipVariantUpdate>,
    pub links: Option<Vec<(ChipId, i8)>>,
    pub enabled: Option<bool>,
}

impl ChipUpdate {
    /// Replaces the fields on `Chip` with the fields on `ChipUpdate` if they
    /// are `Some`.
    pub fn apply(&self, chip: &mut Chip) {
        let ChipUpdate { id, name, manufacturer, product_name, pose, variant, links, enabled } =
            self;

        if let Some(id) = id {
            chip.id = (*id).into();
        }
        if let Some(name) = name {
            chip.name = name.clone();
        }
        if let Some(manufacturer) = manufacturer {
            chip.manufacturer = manufacturer.clone();
        }
        if let Some(product_name) = product_name {
            chip.product_name = product_name.clone();
        }
        pose.apply(&mut chip.pose);
        if let Some(enabled) = enabled {
            chip.enabled = *enabled;
        }
        if let Some(links) = links {
            chip.links = links.clone();
        }
        if let (Some(update_variant), Some(chip_variant)) = (variant, &mut chip.variant) {
            update_variant.apply(chip_variant);
        }
    }
}

/// Generic radio chip update (Bluetooth, Wi-Fi, UWB).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct RadioUpdate {
    pub state: Option<bool>,
}

impl RadioUpdate {
    pub fn apply(&self, radio: &mut Radio) {
        if let Some(state) = self.state {
            radio.state = Some(state);
        }
    }
}

/// The technology variant specific fields
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ChipVariantUpdate {
    Bluetooth(crate::bluetooth::BluetoothUpdate),
    Wifi(crate::wifi::WifiUpdate),
    Uwb(crate::uwb::UwbUpdate),
    Cell(crate::cell::CellUpdate),
    CellularData(crate::cellular_data::CellularDataUpdate),
    Ethernet(crate::ethernet::EthernetUpdate),
    Nfc(NfcUpdate),
}

impl ChipVariantUpdate {
    pub fn kind(&self) -> ChipKind {
        match self {
            ChipVariantUpdate::Bluetooth(_) => ChipKind::BLUETOOTH,
            ChipVariantUpdate::Wifi(_) => ChipKind::WIFI,
            ChipVariantUpdate::Uwb(_) => ChipKind::UWB,
            ChipVariantUpdate::Cell(_) => ChipKind::CELLULAR,
            ChipVariantUpdate::CellularData(_) => ChipKind::CELLULAR_DATA,
            ChipVariantUpdate::Ethernet(_) => ChipKind::ETHERNET,
            ChipVariantUpdate::Nfc(_) => ChipKind::NFC,
        }
    }

    pub fn apply(&self, variant: &mut ChipVariant) {
        match (self, variant) {
            (ChipVariantUpdate::Bluetooth(update), ChipVariant::Bluetooth(bt)) => {
                update.apply(bt);
            }
            (ChipVariantUpdate::Wifi(update), ChipVariant::Wifi(wifi)) => {
                update.apply(wifi);
            }
            (ChipVariantUpdate::Uwb(update), ChipVariant::Uwb(uwb)) => {
                update.apply(uwb);
            }
            (ChipVariantUpdate::Cell(update), ChipVariant::Cell(cell)) => {
                update.apply(cell);
            }
            (ChipVariantUpdate::CellularData(update), ChipVariant::CellularData(cellular_data)) => {
                update.apply(cellular_data);
            }
            (ChipVariantUpdate::Ethernet(update), ChipVariant::Ethernet(ethernet)) => {
                update.apply(ethernet);
            }
            (ChipVariantUpdate::Nfc(update), ChipVariant::Nfc(nfc)) => {
                update.apply(nfc);
            }
            (u, v) => {
                tracing::warn!("ChipVariantUpdate mismatch with ChipVariant: {:?} vs {:?}", u, v);
            }
        }
    }
}

// =============================================================================
// Chip Actor CLIENT
// =============================================================================

/// A client handle for interacting with the chip server actor.
///
/// There is one chip server actor for each network type (Bluetooth, UWB,
/// Wi-Fi). This client provides a high-level API for sending `ChipRequest`
/// messages to the server over an `mpsc` channel. It abstracts away the channel
/// and `oneshot` responder boilerplate for each command.
///
/// A generic client for interacting with any chip server actor (UWB, WiFi,
/// Cell).
#[derive(Clone)]
pub struct RadioChipClient {
    sender: tokio::sync::mpsc::Sender<ChipRequest>,
}

impl RadioChipClient {
    pub fn new(sender: tokio::sync::mpsc::Sender<ChipRequest>) -> Self {
        Self { sender }
    }
}

impl std::fmt::Debug for RadioChipClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RadioChipClient").finish_non_exhaustive()
    }
}

#[cfg_attr(any(test, feature = "testing"), mockall::automock)]
#[async_trait::async_trait]
impl ChipClient for RadioChipClient {
    async fn create(&self, id: ChipId, params: ChipCreate) -> Result<(), ClientError> {
        let (tx, rx) = oneshot::channel();
        self.sender
            .send(ChipRequest::Create { id, params: Box::new(params), respond_to: tx })
            .await
            .map_err(|e| ClientError::Send(e.to_string()))?;
        rx.await.map_err(|e| ClientError::Recv(e.to_string()))?.map_err(ClientError::Chip)
    }

    async fn read(&self, id: ChipId) -> Result<Chip, ClientError> {
        let (tx, rx) = oneshot::channel();
        self.sender
            .send(ChipRequest::Read { id, respond_to: tx })
            .await
            .map_err(|e| ClientError::Send(e.to_string()))?;
        rx.await.map_err(|e| ClientError::Recv(e.to_string()))?.map_err(ClientError::Chip)
    }

    async fn update(&self, id: ChipId, patch: ChipUpdate) -> Result<Chip, ClientError> {
        let (tx, rx) = oneshot::channel();
        self.sender
            .send(ChipRequest::Update { id, patch: Box::new(patch), respond_to: tx })
            .await
            .map_err(|e| ClientError::Send(e.to_string()))?;
        rx.await.map_err(|e| ClientError::Recv(e.to_string()))?.map_err(ClientError::Chip)
    }

    async fn delete(&self, id: ChipId) -> Result<(), ClientError> {
        let (tx, rx) = oneshot::channel();
        self.sender
            .send(ChipRequest::Delete { id, respond_to: tx })
            .await
            .map_err(|e| ClientError::Send(e.to_string()))?;
        rx.await.map_err(|e| ClientError::Recv(e.to_string()))?.map_err(ClientError::Chip)
    }

    async fn read_statistics(&self) -> Result<Box<[NetsimRadioStats]>, ClientError> {
        let (tx, rx) = oneshot::channel();
        self.sender
            .send(ChipRequest::GetStatistics { respond_to: tx })
            .await
            .map_err(|e| ClientError::Send(e.to_string()))?;
        rx.await.map_err(|e| ClientError::Recv(e.to_string()))?.map_err(ClientError::Chip)
    }

    async fn read_count_for_testing(&self) -> Result<usize, ClientError> {
        let (tx, rx) = oneshot::channel();
        self.sender
            .send(ChipRequest::GetCountForTesting { respond_to: tx })
            .await
            .map_err(|e| ClientError::Send(e.to_string()))?;
        rx.await.map_err(|e| ClientError::Recv(e.to_string()))?.map_err(ClientError::Chip)
    }

    async fn shutdown(&self) -> Result<(), ClientError> {
        self.sender
            .send(ChipRequest::Shutdown)
            .await
            .map_err(|e| ClientError::Send(e.to_string()))?;
        Ok(())
    }

    async fn reset(&self, id: ChipId) -> Result<Chip, ClientError> {
        let (tx, rx) = oneshot::channel();
        self.sender
            .send(ChipRequest::Reset { id, respond_to: tx })
            .await
            .map_err(|e| ClientError::Send(e.to_string()))?;
        rx.await.map_err(|e| ClientError::Recv(e.to_string()))?.map_err(ClientError::Chip)
    }

    fn clone_box(&self) -> Box<dyn ChipClient> {
        Box::new(self.clone())
    }
}

/// A client handle for interacting with the chip server actor.
///
/// There is one chip server actor for each network type (Bluetooth, UWB,
/// Wi-Fi). This client provides a high-level API for sending `ChipRequest`
/// messages to the server over an `mpsc` channel. It abstracts away the channel
/// and `oneshot` responder boilerplate for each command.
#[cfg_attr(any(test, feature = "testing"), mockall::automock)]
#[async_trait::async_trait]
pub trait ChipClient: std::fmt::Debug + Send + Sync {
    async fn create(&self, id: ChipId, params: ChipCreate) -> Result<(), ClientError>;
    async fn read(&self, id: ChipId) -> Result<Chip, ClientError>;
    async fn update(&self, id: ChipId, patch: ChipUpdate) -> Result<Chip, ClientError>;
    async fn delete(&self, id: ChipId) -> Result<(), ClientError>;
    async fn read_statistics(&self) -> Result<Box<[NetsimRadioStats]>, ClientError>;
    async fn read_count_for_testing(&self) -> Result<usize, ClientError>;
    async fn shutdown(&self) -> Result<(), ClientError>;
    /// Resets the state of the specified chip.
    async fn reset(&self, id: ChipId) -> Result<Chip, ClientError>;
    fn clone_box(&self) -> Box<dyn ChipClient>;
    async fn get_global_stats(&self) -> Result<Option<Vec<u8>>, ClientError> {
        Ok(None)
    }
    async fn get_service_stats(&self) -> Result<Option<Vec<u8>>, ClientError> {
        Ok(None)
    }
}

impl Clone for Box<dyn ChipClient> {
    fn clone(&self) -> Box<dyn ChipClient> {
        self.clone_box()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chip_default_enabled() {
        let chip = Chip::default();
        assert!(chip.enabled);
    }

    #[test]
    fn test_chip_variant_try_from_unspecified_is_rejected() {
        let err = ChipVariant::try_from(ChipKind::UNSPECIFIED).unwrap_err();
        assert!(matches!(err, ChipError::UnsupportedChipKind(ChipKind::UNSPECIFIED)));
        assert_eq!(err.to_string(), "Unsupported chip kind: UNSPECIFIED");
    }

    #[test]
    fn test_chip_variant_try_from_maps_each_kind_to_its_own_radio() {
        let cases = [
            (ChipKind::BLUETOOTH, ChipVariant::Bluetooth(Box::default())),
            (ChipKind::WIFI, ChipVariant::Wifi(Default::default())),
            (ChipKind::UWB, ChipVariant::Uwb(Default::default())),
            (ChipKind::CELLULAR, ChipVariant::Cell(Default::default())),
            (ChipKind::CELLULAR_DATA, ChipVariant::CellularData(Default::default())),
            (ChipKind::ETHERNET, ChipVariant::Ethernet(Default::default())),
            (ChipKind::NFC, ChipVariant::Nfc(Default::default())),
        ];
        for (kind, expected) in cases {
            let actual = ChipVariant::try_from(kind).unwrap();
            assert_eq!(
                std::mem::discriminant(&actual),
                std::mem::discriminant(&expected),
                "{kind} built {actual:?}"
            );
        }
    }
}
