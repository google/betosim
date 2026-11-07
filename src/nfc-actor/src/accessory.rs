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

//! Trait definitions for Casimir RF accessories.

use casimir::packets::rf;
use netsim_model::{NfcTag, NfcTagError, NfcTagUpdate};

/// Abstract sans-I/O RF accessory interface for emulated NFC tags and peripherals.
pub trait RfAccessory: Send {
    fn set_casimir_id(&mut self, casimir_rf_id: u16);
    fn handle_packet(&mut self, packet: &rf::RfPacket) -> Option<rf::RfPacket>;
    fn deactivate_active_reader(&mut self, reason: rf::DeactivateReason) -> Option<rf::RfPacket>;
    fn active_reader(&self) -> Option<u16>;
    fn apply_update(&mut self, patch: &NfcTagUpdate) -> Result<(), NfcTagError>;
    fn live_ndef_bytes(&self) -> &[u8];
    fn sync_to_model(&self, tag: &mut NfcTag);
    fn is_visible(&self) -> bool;
    fn is_read_only(&self) -> bool;
    fn is_formatted(&self) -> bool;
    fn tx_count(&self) -> u32;
    fn rx_count(&self) -> u32;
}
