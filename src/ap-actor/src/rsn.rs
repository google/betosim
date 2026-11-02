// Copyright 2025-2026 The Android Open Source Project
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

use netsim_packets::{tags, write_ie};
use zerocopy::{IntoBytes, LittleEndian, U16};

use crate::ap_actor::ApConfig;

/// Builds the RSN Information Element (IE) for WPA2-PSK-CCMP.
pub(crate) fn build_rsn_ie(config: &ApConfig) -> Vec<u8> {
    if config.wpa_passphrase.is_none() {
        return Vec::new(); // Open network, no RSN IE.
    }

    let mut body = Vec::new();
    body.extend_from_slice(U16::<LittleEndian>::new(tags::RSN_VER).as_bytes()); // Version

    // Group Cipher Suite: CCMP
    body.extend_from_slice(tags::CIPHER_CCMP);

    // Pairwise Cipher Suite Count: 1
    body.extend_from_slice(U16::<LittleEndian>::new(1).as_bytes());
    // Pairwise Cipher Suite: CCMP
    body.extend_from_slice(tags::CIPHER_CCMP);

    // AKM Suite Count: 1
    body.extend_from_slice(U16::<LittleEndian>::new(1).as_bytes());
    // AKM Suite: PSK
    body.extend_from_slice(tags::AKM_PSK);

    // RSN Capabilities: 0x0000 (No pre-auth, no pairwise pre-auth)
    body.extend_from_slice(U16::<LittleEndian>::new(0).as_bytes());

    // Element ID: RSN
    let mut ie = Vec::new();
    write_ie(&mut ie, tags::RSN, &body);

    ie
}

/// Builds a GTK (Group Temporal Key) KDE (Key Data Encapsulation).
/// Format: Type(0xDD) Len OUI(00 0F AC) DataType(1) KeyID/Tx/Rsvd(1) Rsvd(1)
/// GTK(N)
pub(crate) fn build_gtk_kde(gtk: &[u8], key_id: u8) -> Vec<u8> {
    let mut kde = Vec::new();
    kde.push(0xDD); // Element ID: Vendor Specific

    let mut body = Vec::new();
    // OUI: 00-0F-AC (802.11)
    body.extend_from_slice(&[0x00, 0x0F, 0xAC]);
    // Data Type: 1 (GTK)
    body.push(0x01);

    // Key ID (bits 0-1), Tx (bit 2), Reserved (3-7)
    // Tx bit (bit 2) shall be set. Key ID (bits 0-1).
    let ky_bits = (key_id & 0x03) | (1 << 2);
    body.push(ky_bits);

    body.push(0x00); // Reserved

    // GTK
    body.extend_from_slice(gtk);

    // Key Data padding is handled during encryption (AES Key Wrap).

    kde.push(body.len() as u8);
    kde.extend_from_slice(&body);

    kde
}
