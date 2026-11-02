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

//! This module provides the functionality for creating and managing Bluetooth
//! scanner chips.
//!
//! The `create` function initializes a new scanner by enabling scanning.
//! Placeholder functions for updating and retrieving chip information are also
//! included.
//!
//! Future Features:
//! * **External Link Layer API:** Currently used for testing, this mode may
//!   expose an external API in the future to convert Rootcanal LL packets to
//!   standard Bluetooth LL packets for capture.

use netsim_model::{ChipError, ChipId, ScannerParams};
use netsim_packets::{
    Enable, LeScanType, LeScanningFilterPolicy, LeSetEventMask, LeSetScanEnable,
    LeSetScanParameters, OwnAddressType, SetEventMask,
};
use rootcanal::Rootcanal;
use tracing::debug;
use zerocopy::{U16, U64};

use crate::utils::send_hci_command;

const SCAN_INTERVAL_SLOTS: u16 = 0x0010;
const SCAN_WINDOW_SLOTS: u16 = 0x0010;
const EVENT_MASK_ALL: u64 = u64::MAX;
const LE_EVENT_MASK_LOW_OCTET: u64 = 0x0000_0000_0000_00FF;

/// Creates a new `ScannerChip`.
pub(crate) fn create(
    rootcanal: &Rootcanal,
    chip_id: ChipId,
    params: &ScannerParams,
) -> Result<(), ChipError> {
    debug!("[{chip_id}] Setting up scanner chip");

    send_hci_command(rootcanal, chip_id, SetEventMask { event_mask: U64::new(EVENT_MASK_ALL) })?;

    send_hci_command(
        rootcanal,
        chip_id,
        LeSetEventMask { le_event_mask: U64::new(LE_EVENT_MASK_LOW_OCTET) },
    )?;

    send_hci_command(
        rootcanal,
        chip_id,
        LeSetScanParameters {
            le_scan_type: if params.active { LeScanType::ACTIVE } else { LeScanType::PASSIVE },
            le_scan_interval: U16::new(SCAN_INTERVAL_SLOTS),
            le_scan_window: U16::new(SCAN_WINDOW_SLOTS),
            own_address_type: OwnAddressType::PUBLIC_DEVICE_ADDRESS,
            scanning_filter_policy: LeScanningFilterPolicy::ACCEPT_ALL,
        },
    )?;

    send_hci_command(
        rootcanal,
        chip_id,
        LeSetScanEnable { le_scan_enable: Enable::ENABLED, filter_duplicates: Enable::DISABLED },
    )?;

    Ok(())
}
