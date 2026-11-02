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

//! The bluetooth crate provides a simulation environment for Bluetooth devices.
//!
//! This crate is responsible for managing the lifecycle of simulated Bluetooth
//! chips, handling HCI communication, and interacting with the `rootcanal`
//! Bluetooth emulator simulation backend.
//!
//! # Getting Started
//!
//! The main entry point for interacting with this crate is the [`new`]
//! function.
//!
//! To use the bluetooth actor:
//! 1. Create a new instance using [`new()`], which returns the `ResourceActor`
//!    and a `ChipClient`.
//! 2. Spawn the `actor.run(context)` method into a Tokio task to start its
//!    event loop.
//! 3. Use the `ChipClient` to send commands to the running actor.
//!
//! ```no_run
//! use tokio::{self, sync::mpsc};
//!
//! #[tokio::main]
//! async fn main() {
//!     let (device_tx, _device_rx) = mpsc::channel(10);
//!     let resource_client = actor_framework::ResourceClient::new(device_tx);
//!
//!     let (actor, client) = bluetooth_actor::new();
//!     let device_client = device_actor::DeviceClient::new(Box::new(resource_client));
//!     let bluetooth_actor = bluetooth_actor::BluetoothActor::new(device_client, false);
//!     tokio::spawn(async move {
//!         actor.run(bluetooth_actor).await;
//!     });
//!
//!     // Use the client to interact with the actor, e.g., create chips.
//!     // client.create(...).await;
//!
//!     // Keep the main task alive for a duration or until a shutdown signal.
//!     tokio::time::sleep(tokio::time::Duration::from_secs(60)).await;
//! }
//! ```
//!
//! # Commands
//!
//! The actor processes commands sent via the `ChipClient`. The available
//! commands are defined in the [`netsim_model::ChipRequest`] enum. The
//! `ChipClient` provides a convenient method for each command variant.
//!
//! The following commands are supported:
//! - [`Create`](netsim_model::ChipClient::create): Creates a new Bluetooth
//!   chip.
//! - [`Read`](netsim_model::ChipClient::read): Retrieves information about a
//!   Bluetooth chip.
//! - [`Update`](netsim_model::ChipClient::update): Updates an existing
//!   Bluetooth chip.
//! - [`Delete`](netsim_model::ChipClient::delete): Deletes a Bluetooth chip.
//! - [`Reset`](netsim_model::ChipClient::reset): Resets a Bluetooth chip.
//! - [`GetStatistics`](netsim_model::ChipRequest::GetStatistics): Retrieves
//!   statistics for all Bluetooth chips.
//! - [`GetCountForTesting`](netsim_model::ChipRequest::GetCountForTesting):
//!   Retrieves the total number of chips for testing purposes.
//! - [`Shutdown`](netsim_model::ChipClient::shutdown): Shuts down the actor.
//!
//! # Chip Modes
//!
//! The actor can create chips in three different modes, configured via the
//! [`netsim_model::ChipCreate`] struct. The `mode` field within
//! [`netsim_model::BluetoothCreate`] determines the chip's behavior.
//!
//! ## Device Mode
//!
//! A standard Bluetooth controller that can be controlled by an external host,
//! such as an Android Virtual Device or a Bumble test. This is configured
//! using [`netsim_model::BluetoothMode::Device`].
//!
//! ## Beacon Mode
//!
//! A chip that repeatedly broadcasts advertisement packets. This is configured
//! using [`netsim_model::BluetoothMode::Beacon`] with
//! [`netsim_model::BeaconParams`].
//!
//! ## Scanner Mode
//!
//! A chip that listens for and captures all nearby Bluetooth packets. This is
//! configured using [`netsim_model::BluetoothMode::Scanner`].
//!
//! # Features
//!
//! * **Actor-Based State Management:** Implements the actor model, with the
//!   `ResourceActor` as a central actor that serializes all operations to
//!   safely manage the state of multiple Bluetooth chips (Device, Beacon, and
//!   Scanner modes).
//! * **HCI Stream/Sink Bridging:** For each chip, bridges a `PacketStream` (for
//!   incoming HCI commands) and a `PacketSink` (for outgoing HCI events),
//!   routing packets between the host and the `rootcanal` simulation.
//! * **Rootcanal Integration:** Simulates the Bluetooth controller logic using
//!   `rootcanal`.
//!
//! # Future Features
//!
//! * **RSSI Management:** Manage Received Signal Strength Indication (RSSI)
//!   based on chip location.
//! * **Link Layer Capture:** Scanner functionality to convert Rootcanal LL
//!   packets to standard Bluetooth LL packets.
//! * **HCI-based Beacon:** Implement Beacon functionality via HCI commands,
//!   allowing common Android-like advertisement parameters.
#![warn(missing_docs)]
#![allow(clippy::type_complexity)]

use netsim_model::ChipId;
use rootcanal::Phy;

mod actions;
mod beacon;
mod bluetooth_actor;
mod device;
mod error;
mod hci_callbacks;

mod actor_service;
mod lifecycle;
mod ranging;
mod scanner;
mod sniffer;
mod utils;

mod beacon_utils;
mod client;

pub(crate) use actions::{BluetoothAction, BluetoothActionResult};
pub use beacon_utils::{REPORT_ADDR_OFFSET, REPORT_NUM_REPORTS_OFFSET, is_le_advertising_report};
pub use bluetooth_actor::BluetoothActor;
pub use client::BluetoothClient;
pub use error::BluetoothError;

const DEFAULT_ACTOR_CHANNEL_CAPACITY: usize = 32;

#[derive(Debug, Clone)]
/// Events that can be sent to the Bluetooth actor via typed stream.
pub enum BluetoothEvent {
    /// Deliver a link layer packet to a specific chip.
    DeliverPacket {
        /// The destination chip ID.
        receiver_id: ChipId,
        /// The packet payload.
        packet: bytes::Bytes,
        /// The physical layer type (LE or Classic).
        phy: Phy,
        /// The Received Signal Strength Indication.
        rssi: i32,
    },
}

use actor_framework::ResourceActor;

/// Creates a new Bluetooth actor and its client.
pub fn new() -> (ResourceActor<BluetoothActor>, BluetoothClient) {
    let (actor, resource_client) = ResourceActor::new(DEFAULT_ACTOR_CHANNEL_CAPACITY);
    (actor, BluetoothClient(resource_client))
}
