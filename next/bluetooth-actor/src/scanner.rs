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
use rootcanal::Rootcanal;
use tracing::debug;
use zerocopy::{Immutable, IntoBytes, KnownLayout, Unaligned};

use crate::utils::ToChipError;

const HCI_COMMAND_PACKET: u8 = 0x01;
const HCI_SET_EVENT_MASK: u16 = 0x0C01;
const HCI_LE_SET_EVENT_MASK: u16 = 0x2001;
const HCI_LE_SET_SCAN_PARAMETERS: u16 = 0x200b;
const HCI_LE_SET_SCAN_ENABLE: u16 = 0x200c;

/// Creates a new `ScannerChip`.
pub(crate) fn create(
    rootcanal: &Rootcanal,
    chip_id: ChipId,
    params: &ScannerParams,
) -> Result<(), ChipError> {
    debug!("[{chip_id}] Setting up scanner chip");
    // Enable scanning on the new controller.
    debug!("[{chip_id}] Enabling scanning");

    // HCI_Set_Event_Mask
    // Enable LE Meta Event (0x3E) which is controlled by this mask.
    // LE Meta Event is bit 61 (0x3E - 1? No, event codes are 1-indexed?)
    // Event Code 0x3E = 62.
    // Byte 7, bit 6 (0x40).
    // Let's enable ALL standard events to be safe.
    #[rustfmt::skip]
    let hci_event_mask = vec![
        HCI_COMMAND_PACKET,       // Command Packet
        (HCI_SET_EVENT_MASK & 0xFF) as u8,
        (HCI_SET_EVENT_MASK >> 8) as u8, // Opcode: HCI_Set_Event_Mask (0x0C01)
        8,          // Parameter Length
        0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, // Mask: Enable all
    ];
    rootcanal.receive_hci(chip_id.into(), hci_event_mask.into()).to_chip_error()?;

    // HCI_LE_Set_Event_Mask
    // https://cs.android.com/android/platform/superproject/+/main:packages/modules/Bluetooth/tools/rootcanal/include/hci/hci_packets.h;l=6058
    #[repr(C, packed)]
    #[derive(IntoBytes, Immutable, KnownLayout, Unaligned)]
    struct HciLeSetEventMask {
        packet_type: u8,
        opcode: u16,
        param_len: u8,
        mask: u64,
    }

    let event_mask = HciLeSetEventMask {
        packet_type: HCI_COMMAND_PACKET,
        opcode: HCI_LE_SET_EVENT_MASK,
        param_len: 8,
        mask: 0x00000000000000FF, /* Enable everything (first byte 0xFF covers Adv Report which
                                   * is bit 1) */
    };
    rootcanal.receive_hci(chip_id.into(), event_mask.as_bytes().to_vec().into()).to_chip_error()?;

    // HCI_LE_Set_Scan_Parameters
    // https://cs.android.com/android/platform/superproject/+/main:packages/modules/Bluetooth/tools/rootcanal/include/hci/hci_packets.h;l=6121
    #[repr(C, packed)]
    #[derive(IntoBytes, Immutable, KnownLayout, Unaligned)]
    struct HciLeSetScanParameters {
        packet_type: u8,
        opcode: u16,
        param_len: u8,
        le_scan_type: u8,
        le_scan_interval: u16,
        le_scan_window: u16,
        own_address_type: u8,
        scanning_filter_policy: u8,
    }

    let scan_params = HciLeSetScanParameters {
        packet_type: HCI_COMMAND_PACKET,
        opcode: HCI_LE_SET_SCAN_PARAMETERS,
        param_len: 7,
        le_scan_type: if params.active { 0x01 } else { 0x00 }, // 0x01 for Active, 0x00 for Passive
        le_scan_interval: 0x0010,                              // 10ms
        le_scan_window: 0x0010,                                // 10ms
        own_address_type: 0x00,                                // Public
        scanning_filter_policy: 0x00,                          // Accept all
    };
    rootcanal
        .receive_hci(chip_id.into(), scan_params.as_bytes().to_vec().into())
        .to_chip_error()?;

    // HCI_LE_Set_Scan_Enable
    // https://cs.android.com/android/platform/superproject/+/main:packages/modules/Bluetooth/tools/rootcanal/include/hci/hci_packets.h;l=6264
    // * Enable: 0x01 (Enable)
    // * Filter Duplicates: 0x00 (Disable) - Capture every advertisement
    #[rustfmt::skip]
    let scan_enable = vec![
        HCI_COMMAND_PACKET,       // Command Packet
        (HCI_LE_SET_SCAN_ENABLE & 0xFF) as u8, (HCI_LE_SET_SCAN_ENABLE >> 8) as u8, // Opcode: HCI_LE_Set_Scan_Enable (0x200c)
        2,          // Parameter Length
        0x01,       // Enable: True
        0x00,       // Filter Duplicates: False
    ];
    rootcanal.receive_hci(chip_id.into(), scan_enable.into()).to_chip_error()?;
    Ok(())
}
