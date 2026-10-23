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

//! A shared, dependency-free crate that defines the shared data structures
//! and command enums for the netsim control plane.

//! # Netsim Model
//!
//! This crate defines the core data structures and types used across the Netsim
//! project. It serves as the base layer for the dependency graph.
//!
//! ## Dependency Rules
//!
//! - **Base Layer**: This crate should not depend on other Netsim crates (e.g.,
//!   `*-api`, `*-actor`).
//! - **Shared Types**: It contains shared types like `ChipId`, `Position`, and
//!   `Device` that are used by both API and implementation crates.
//!
//! ## Dependency Hierarchy
//!
//! The Netsim project uses a layered architecture to prevent circular
//! dependencies:
//!
//! 1. **Level 0: Base Layer** (`netsim-model`)
//!     - Shared data types (e.g., `ChipId`, `Position`).
//!     - No dependencies on other Netsim crates.
//! 2. **Level 1: API Layer** (`*-api`)
//!     - Behavioral contracts (Actions, Results, Traits).
//!     - Depends on: `netsim-model`.
//! 3. **Level 2: Implementation & Client Layer**
//!     - `*-actor`: Implements the contracts. Depends on `*-api`,
//!       `netsim-model`.
//!     - `netsim-client`: Client-side wrappers. Depends on `*-api`,
//!       `netsim-model`.
//! 4. **Level 3: Consumer Layer** (`daemon`, `wifi`, `cell`, etc.)
//!     - Uses the services. Depends on `netsim-client`, `*-api`.
//!
//! By maintaining this structure, we ensure a stable foundation and prevent
//! circular dependencies.

pub(crate) mod ap;
pub(crate) mod bluetooth;
pub(crate) mod cell;
pub(crate) mod cellular_data;
pub(crate) mod chip;
pub(crate) mod chip_error;
pub(crate) mod client_error;
pub(crate) mod device;
pub(crate) mod device_error;
pub(crate) mod ethernet;
pub(crate) mod initial_info;
pub(crate) mod link;
pub(crate) mod macros;
pub(crate) mod nfc;
pub(crate) mod packet_streamer;
pub(crate) mod stats;
pub(crate) mod uwb;
pub(crate) mod wifi;

// Explicit Facade Exports

// From chip.rs
// From ap.rs
// From bluetooth.rs
// From cell.rs
#[cfg(any(test, feature = "testing"))]
pub use crate::chip::MockChipClient;
// From client_error.rs
pub use crate::client_error::ClientError;
// From device::api
pub use crate::device::api::{
    ChipCreateVariant, DeviceChipCreate, DeviceCreate, DeviceUpdate, PoseUpdate,
};
// From device.rs
pub use crate::device::{
    Device, DeviceAddChip, DeviceConfig, DeviceId, DeviceInfo, Orientation, Pose, Position,
};
// From device_error.rs
pub use crate::device_error::DeviceError;
// From initial_info.rs
pub use crate::initial_info::{
    BtControllerFeatures, BtControllerProperties, BtControllerQuirks, BtControllerVendorFeatures,
    ChipInfo, ChipKind,
};
// From link.rs
pub use crate::link::{Link, LinkCreate, LinkId, LinkUpdate};
// From stats.rs
pub use crate::stats::{
    FrontendStats, InvalidPacket, InvalidPacketReason, NetsimDeviceStats, NetsimFrontendStats,
    NetsimRadioStats, RadioKind, WifiStats,
};
pub use crate::{
    ap::{Ap, ApCreate, ApUpdate, DEFAULT_WIFI_BSSID, DEFAULT_WIFI_SSID, WifiMode},
    bluetooth::{
        BeaconParams, Bluetooth, BluetoothCreate, BluetoothMode, BluetoothUpdate, Controller,
        DeviceParams, PacketType, ScannerParams, SnifferParams,
        beacon::{
            AdvertiseData, AdvertiseMode, AdvertiseSettings, AdvertiseTxPower, BleBeacon, Interval,
            Service, TxPower,
        },
    },
    cell::{
        Call, CallDirection, CallState, Cell, CellCreate, CellNetworkConfig, CellUpdate,
        MODEM_STATE_DOWN, MODEM_STATE_IDLE, MODEM_STATE_RINGING, ModemAction, Quirks,
        RadioTechnology, RegistrationStatus,
    },
    cellular_data::{CellularData, CellularDataCreate, CellularDataUpdate},
    chip::{
        Chip, ChipClient, ChipCreate, ChipId, ChipRequest, ChipUpdate, ChipVariant,
        ChipVariantUpdate, PacketSink, PacketStream, Radio, RadioChipClient, RadioUpdate,
        chip_kind_to_radio_kind,
    },
    chip_error::ChipError,
    ethernet::{Ethernet, EthernetCreate, EthernetUpdate},
    nfc::{
        DEFAULT_NFC_TAG_UID_PREFIX, MAX_T2T_NDEF_MESSAGE_BYTES, MAX_T2T_USER_MEMORY_BYTES,
        MAX_T4AT_NDEF_FILE_CAPACITY, MAX_T4AT_NDEF_MESSAGE_BYTES, Nfc, NfcCreate, NfcTag,
        NfcTagCreate, NfcTagError, NfcTagId, NfcTagProtocol, NfcTagUpdate, NfcUpdate,
        decode_ndef_hex, decode_ndef_payload_text, encode_hex_lower, encode_ndef_text_record,
        encode_ndef_uri_record, normalize_uid_hex,
    },
    uwb::{Uwb, UwbCreate, UwbMode, UwbUpdate},
    wifi::{Wifi, WifiCreate, WifiUpdate},
};
