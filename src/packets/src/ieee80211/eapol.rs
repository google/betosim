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

//! EAPOL (Extensible Authentication Protocol over LAN) definitions.

use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout, Unaligned};

/// Represents the EAPOL Header.
#[derive(FromBytes, IntoBytes, Immutable, KnownLayout, Unaligned, Debug, Clone, Copy, Default)]
#[repr(C, packed)]
pub struct EapolHeader {
    /// Protocol Version (e.g., 1 or 2).
    pub version: u8,
    /// Packet Type (e.g., 3 = Key).
    pub packet_type: u8,
    /// Packet Body Length (Big Endian).
    pub length: [u8; 2],
}

impl EapolHeader {
    /// Creates a new EAPOL header.
    pub fn new(version: u8, packet_type: u8, length: u16) -> Self {
        Self { version, packet_type, length: length.to_be_bytes() }
    }
}

/// Represents the EAPOL-Key Frame (used in WPA/WPA2/RSN).
#[derive(FromBytes, IntoBytes, Immutable, KnownLayout, Unaligned, Debug, Clone, Copy, Default)]
#[repr(C, packed)]
pub struct EapolKeyFrame {
    /// Key Descriptor Type (1 = RC4, 2 = RSN (WPA2), 254 = WPA).
    pub descriptor_type: u8,
    /// Key Information Flags (Big Endian).
    pub key_info: [u8; 2],
    /// Key Length (Big Endian).
    pub key_len: [u8; 2],
    /// Replay Counter (Big Endian).
    pub replay_counter: [u8; 8],
    /// Key Nonce.
    pub key_nonce: [u8; 32],
    /// Key IV.
    pub key_iv: [u8; 16],
    /// Key RSC.
    pub key_rsc: [u8; 8],
    /// Key ID (Reserved).
    pub key_id: [u8; 8],
    /// Message Integrity Code (MIC).
    pub mic: [u8; 16],
    /// Key Data Length (Big Endian).
    pub key_data_len: [u8; 2],
    // Data follows
}

impl EapolKeyFrame {
    #[allow(clippy::too_many_arguments)]
    /// Creates a new EAPOL-Key frame with common defaults.
    pub fn new(
        descriptor_type: u8,
        key_info: u16,
        key_len: u16,
        replay_counter: u64,
        key_nonce: [u8; 32],
        key_iv: [u8; 16],
        key_rsc: u64,
        mic: [u8; 16],
        key_data_len: u16,
    ) -> Self {
        Self {
            descriptor_type,
            key_info: key_info.to_be_bytes(),
            key_len: key_len.to_be_bytes(),
            replay_counter: replay_counter.to_be_bytes(),
            key_nonce,
            key_iv,
            key_rsc: key_rsc.to_be_bytes(),
            key_id: [0; 8], // Reserved
            mic,
            key_data_len: key_data_len.to_be_bytes(),
        }
    }
}

/// EAPOL Version 1.
pub const EAPOL_VERSION: u8 = 1;
/// EAPOL Packet Type: Key.
pub const EAPOL_TYPE_KEY: u8 = 3;
/// EAPOL Packet Type: EAPOL-Start (1).
pub const EAPOL_TYPE_START: u8 = 1;
/// Key Descriptor Type: RSN (WPA2).
pub const EAPOL_KEY_DESC_TYPE_RSN: u8 = 2;

/// EAPOL Packet Type: EAP-Packet (0).
pub const EAPOL_TYPE_PACKET: u8 = 0;

/// EAP Code: Request (1).
pub const EAP_CODE_REQUEST: u8 = 1;
/// EAP Code: Response (2).
pub const EAP_CODE_RESPONSE: u8 = 2;
/// EAP Code: Success (3).
pub const EAP_CODE_SUCCESS: u8 = 3;
/// EAP Code: Failure (4).
pub const EAP_CODE_FAILURE: u8 = 4;

/// EAP Type: Identity (1).
pub const EAP_TYPE_IDENTITY: u8 = 1;

/// Represents a generic EAP Packet (Code, ID, Length, Type, Data).
/// Note: EAP Success/Failure do not have a Type field, just Code/ID/Length.
/// We'll use a generic byte layout or specific struct.
/// Header: Code(1), ID(1), Length(2)
#[derive(FromBytes, IntoBytes, Immutable, KnownLayout, Unaligned, Debug, Clone, Copy, Default)]
#[repr(C, packed)]
pub struct EapHeader {
    pub code: u8,
    pub id: u8,
    pub length: [u8; 2],
}

impl EapHeader {
    pub fn new(code: u8, id: u8, length: u16) -> Self {
        Self { code, id, length: length.to_be_bytes() }
    }
}
