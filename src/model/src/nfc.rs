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

use std::{
    fmt::{self, Write as _},
    str::FromStr,
};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::device::Position;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Nfc {
    #[serde(flatten)]
    pub radio: crate::chip::Radio,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct NfcUpdate {
    #[serde(flatten)]
    pub radio: crate::chip::RadioUpdate,
}

impl NfcUpdate {
    pub fn apply(&self, nfc: &mut Nfc) {
        self.radio.apply(&mut nfc.radio);
    }
}

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
pub struct NfcCreate {
    // Future NFC specific properties.
}

impl From<NfcCreate> for Nfc {
    fn from(_create: NfcCreate) -> Self {
        Nfc { radio: crate::chip::Radio::default() }
    }
}

// --- Virtual NFC Tag Data Model (/v1/nfc/tags) ---

/// Default 5-byte prefix for auto-generated 7-byte ISO 14443-3A UIDs (`0x04` is
/// NXP's ISO/IEC 7816-6 manufacturer code `UID0`), combined with a 2-byte
/// big-endian tag ID suffix.
pub const DEFAULT_NFC_TAG_UID_PREFIX: [u8; 5] = [0x04, 0x01, 0x02, 0x03, 0x04];

/// Maximum Type 4A NDEF Elementary File (`0xE104`) capacity in bytes
/// (including 2-byte `NLEN` header).
pub const MAX_T4AT_NDEF_FILE_CAPACITY: usize = 4096;

/// Maximum raw NDEF message size in bytes for Type 4A tags (excluding
/// the 2-byte big-endian `NLEN` header).
pub const MAX_T4AT_NDEF_MESSAGE_BYTES: usize = MAX_T4AT_NDEF_FILE_CAPACITY - 2;

/// Total user memory capacity in bytes for Type 2 tags (`MifareUltralight` /
/// NTAG216).
pub const MAX_T2T_USER_MEMORY_BYTES: usize = 888;

/// Maximum raw NDEF message size in bytes for Type 2 tags (accounting for the
/// 4-byte 3-byte-length NDEF TLV header `03 FF len_hi len_lo` and 1-byte `0xFE`
/// terminator TLV).
pub const MAX_T2T_NDEF_MESSAGE_BYTES: usize = MAX_T2T_USER_MEMORY_BYTES - 5;

/// Unique identifier for a virtual NFC tag managed by `NfcActor`.
#[derive(
    Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[serde(transparent)]
pub struct NfcTagId(pub u32);

impl From<u32> for NfcTagId {
    fn from(id: u32) -> Self {
        Self(id)
    }
}

impl From<NfcTagId> for u32 {
    fn from(id: NfcTagId) -> Self {
        id.0
    }
}

impl fmt::Display for NfcTagId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl FromStr for NfcTagId {
    type Err = NfcTagError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let parsed: u32 = s
            .trim()
            .parse()
            .map_err(|source| NfcTagError::InvalidId { input: s.to_owned(), source })?;
        Ok(Self(parsed))
    }
}

/// Contactless RF tag protocol profile emulated by a virtual NFC tag.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NfcTagProtocol {
    #[default]
    #[serde(
        rename = "T4AT",
        alias = "t4at",
        alias = "ISO_DEP",
        alias = "iso_dep",
        alias = "ISO-DEP",
        alias = "iso-dep"
    )]
    T4at,
    #[serde(
        rename = "T2T",
        alias = "t2t",
        alias = "MIFARE_ULTRALIGHT",
        alias = "mifare_ultralight",
        alias = "MIFARE-ULTRALIGHT",
        alias = "mifare-ultralight"
    )]
    T2t,
}

impl NfcTagProtocol {
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::T4at => "T4AT",
            Self::T2t => "T2T",
        }
    }

    #[must_use]
    pub const fn max_ndef_message_bytes(&self) -> usize {
        match self {
            Self::T4at => MAX_T4AT_NDEF_MESSAGE_BYTES,
            Self::T2t => MAX_T2T_NDEF_MESSAGE_BYTES,
        }
    }
}

impl fmt::Display for NfcTagProtocol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for NfcTagProtocol {
    type Err = NfcTagError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let trimmed = s.trim();
        if ["t4at", "iso_dep", "iso-dep"].iter().any(|alias| trimmed.eq_ignore_ascii_case(alias)) {
            Ok(Self::T4at)
        } else if ["t2t", "mifare_ultralight", "mifare-ultralight"]
            .iter()
            .any(|alias| trimmed.eq_ignore_ascii_case(alias))
        {
            Ok(Self::T2t)
        } else {
            Err(NfcTagError::UnknownProtocol(s.to_owned()))
        }
    }
}

/// Validation and encoding errors for virtual NFC tag configurations and NDEF
/// payloads.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum NfcTagError {
    #[error("invalid NFC tag ID `{input}`: {source}")]
    InvalidId {
        input: String,
        #[source]
        source: std::num::ParseIntError,
    },
    #[error("invalid hex in `{field}`: {reason}")]
    InvalidHex { field: &'static str, reason: String },
    #[error("invalid ISO 14443-3A UID length: got {len_bytes} bytes, expected 4, 7, or 10 bytes")]
    InvalidUidLength { len_bytes: usize },
    #[error("invalid ISO 14443-3A UID: first byte (UID0) cannot be cascade tag 0x88")]
    InvalidUidCascadeByte,
    #[error("NDEF message length ({len_bytes} bytes) exceeds maximum capacity ({max_bytes} bytes)")]
    NdefTooLarge { len_bytes: usize, max_bytes: usize },
    #[error("invalid NDEF text language code `{lang}`: must be 1..=63 ASCII bytes")]
    InvalidLanguageCode { lang: String },
    #[error("unknown NFC tag protocol `{0}`: expected `T4AT` or `T2T`")]
    UnknownProtocol(String),
}

fn default_true() -> bool {
    true
}

/// Virtual NFC tag managed by `NfcActor`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NfcTag {
    pub id: NfcTagId,
    pub name: String,
    pub protocol: NfcTagProtocol,
    pub uid: String,
    pub position: Position,
    #[serde(default = "default_true")]
    pub visible: bool,
    pub ndef_bytes: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payload_text: Option<String>,
    pub read_only: bool,
    #[serde(default = "default_true")]
    pub formatted: bool,
    pub disconnect_on_transceive: bool,
    #[serde(default)]
    pub tx_count: u32,
    #[serde(default)]
    pub rx_count: u32,
}

impl NfcTag {
    /// Returns the resolved tag name, defaulting to `nfc-tag-{id}` if empty
    /// or whitespace.
    fn resolve_name(id: NfcTagId, candidate: String) -> String {
        if candidate.trim().is_empty() { Self::default_name(id) } else { candidate }
    }

    /// Returns the default name for a virtual tag (`nfc-tag-{id}`).
    #[must_use]
    pub fn default_name(id: NfcTagId) -> String {
        format!("nfc-tag-{id}")
    }

    /// Generates a default 7-byte ISO 14443-3A UID combining
    /// [`DEFAULT_NFC_TAG_UID_PREFIX`] with the lower 16 bits of the tag ID.
    #[must_use]
    pub fn default_uid(id: NfcTagId) -> String {
        let [.., hi, lo] = id.0.to_be_bytes();
        let [p0, p1, p2, p3, p4] = DEFAULT_NFC_TAG_UID_PREFIX;
        encode_hex_lower(&[p0, p1, p2, p3, p4, hi, lo])
    }

    /// Creates a validated [`NfcTag`] from [`NfcTagCreate`] parameters.
    ///
    /// # Errors
    ///
    /// Returns [`NfcTagError`] if the UID or NDEF bytes fail validation.
    pub fn try_from_create(id: NfcTagId, create: NfcTagCreate) -> Result<Self, NfcTagError> {
        let NfcTagCreate {
            name,
            protocol,
            uid,
            position,
            visible,
            ndef_bytes: raw_ndef_bytes,
            read_only,
            formatted,
            disconnect_on_transceive,
        } = create;

        let name = Self::resolve_name(id, name);

        let uid =
            if uid.trim().is_empty() { Self::default_uid(id) } else { normalize_uid_hex(&uid)? };

        let decoded_ndef = decode_ndef_hex(&raw_ndef_bytes, protocol)?;
        let ndef_bytes = encode_hex_lower(&decoded_ndef);
        let payload_text = decode_ndef_payload_text(&decoded_ndef);

        Ok(Self {
            id,
            name,
            protocol,
            uid,
            position,
            visible,
            ndef_bytes,
            payload_text,
            read_only,
            formatted,
            disconnect_on_transceive,
            tx_count: 0,
            rx_count: 0,
        })
    }

    /// Updates NDEF bytes and synchronizes decoded payload text atomically.
    pub fn set_ndef_bytes(&mut self, raw_ndef: &[u8]) {
        self.ndef_bytes = encode_hex_lower(raw_ndef);
        self.payload_text = decode_ndef_payload_text(raw_ndef);
    }
}

/// Parameters for creating a virtual NFC tag.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct NfcTagCreate {
    pub name: String,
    pub protocol: NfcTagProtocol,
    pub uid: String,
    pub position: Position,
    pub visible: bool,
    pub ndef_bytes: String,
    pub read_only: bool,
    pub formatted: bool,
    pub disconnect_on_transceive: bool,
}

impl Default for NfcTagCreate {
    fn default() -> Self {
        Self {
            name: String::new(),
            protocol: NfcTagProtocol::T4at,
            uid: String::new(),
            position: Position::default(),
            visible: true,
            ndef_bytes: String::new(),
            read_only: false,
            formatted: true,
            disconnect_on_transceive: false,
        }
    }
}

/// Parameters for updating a virtual NFC tag.
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
pub struct NfcTagUpdate {
    pub name: Option<String>,
    pub position: Option<Position>,
    pub visible: Option<bool>,
    pub ndef_bytes: Option<String>,
    pub read_only: Option<bool>,
    pub formatted: Option<bool>,
    pub disconnect_on_transceive: Option<bool>,
}

impl NfcTagUpdate {
    /// Applies updates to an existing [`NfcTag`].
    ///
    /// # Errors
    ///
    /// Returns [`NfcTagError`] if `ndef_bytes` contains invalid hex or exceeds
    /// the protocol maximum capacity.
    pub fn apply(&self, tag: &mut NfcTag) -> Result<(), NfcTagError> {
        let Self {
            name,
            position,
            visible,
            ndef_bytes,
            read_only,
            formatted,
            disconnect_on_transceive,
        } = self;

        let decoded_ndef =
            ndef_bytes.as_deref().map(|raw| decode_ndef_hex(raw, tag.protocol)).transpose()?;

        if let Some(name) = name {
            tag.name = NfcTag::resolve_name(tag.id, name.clone());
        }
        if let &Some(pos) = position {
            tag.position = pos;
        }
        if let &Some(visible) = visible {
            tag.visible = visible;
        }
        if let &Some(read_only) = read_only {
            tag.read_only = read_only;
        }
        if let &Some(formatted) = formatted {
            tag.formatted = formatted;
        }
        if let &Some(disconnect) = disconnect_on_transceive {
            tag.disconnect_on_transceive = disconnect;
        }
        if let Some(bytes) = decoded_ndef {
            tag.set_ndef_bytes(&bytes);
        }

        Ok(())
    }
}

fn parse_hex_bytes(s: &str, field: &'static str) -> Result<Vec<u8>, NfcTagError> {
    let nibbles: Result<Vec<u8>, NfcTagError> = s
        .chars()
        .filter(|&c| !c.is_ascii_whitespace() && !matches!(c, ':' | '-'))
        .map(|c| {
            c.to_digit(16).and_then(|d| u8::try_from(d).ok()).ok_or_else(|| {
                NfcTagError::InvalidHex {
                    field,
                    reason: format!("invalid character `{c}` in `{s}`"),
                }
            })
        })
        .collect();
    let nibbles = nibbles?;

    if !nibbles.len().is_multiple_of(2) {
        return Err(NfcTagError::InvalidHex {
            field,
            reason: format!("odd number of hex digits ({}) in `{s}`", nibbles.len()),
        });
    }

    let bytes = nibbles
        .chunks_exact(2)
        .map(|pair| pair.iter().fold(0u8, |acc, &n| (acc << 4) | n))
        .collect();
    Ok(bytes)
}

/// Validates and normalizes an ISO 14443-3A UID string (4, 7, or 10 bytes).
///
/// # Errors
///
/// Returns [`NfcTagError::InvalidHex`] if `uid` contains invalid hex or an odd
/// number of digits. Returns [`NfcTagError::InvalidUidCascadeByte`] if the
/// first byte is cascade tag `0x88`. Returns [`NfcTagError::InvalidUidLength`]
/// if the decoded length is not 4, 7, or 10 bytes.
pub fn normalize_uid_hex(uid: &str) -> Result<String, NfcTagError> {
    let bytes = parse_hex_bytes(uid, "uid")?;
    match bytes.len() {
        4 | 7 | 10 => {
            if bytes.first() == Some(&0x88) {
                Err(NfcTagError::InvalidUidCascadeByte)
            } else {
                Ok(encode_hex_lower(&bytes))
            }
        }
        len_bytes => Err(NfcTagError::InvalidUidLength { len_bytes }),
    }
}

/// Encodes raw bytes as a canonical lowercase hexadecimal string.
#[must_use]
pub fn encode_hex_lower(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(out, "{b:02x}");
    }
    out
}

/// Decodes a hexadecimal string into NDEF bytes, enforcing protocol limits.
///
/// # Errors
///
/// Returns [`NfcTagError::InvalidHex`] if `raw_hex` contains invalid hex, or
/// [`NfcTagError::NdefTooLarge`] if decoded bytes exceed the protocol maximum.
pub fn decode_ndef_hex(raw_hex: &str, protocol: NfcTagProtocol) -> Result<Vec<u8>, NfcTagError> {
    let bytes = parse_hex_bytes(raw_hex, "ndef_bytes")?;
    let max_bytes = protocol.max_ndef_message_bytes();
    if bytes.len() > max_bytes {
        return Err(NfcTagError::NdefTooLarge { len_bytes: bytes.len(), max_bytes });
    }
    Ok(bytes)
}

fn encode_well_known_record(record_type: u8, payload: &[u8]) -> Result<Vec<u8>, NfcTagError> {
    let short_len = u8::try_from(payload.len()).ok();
    let header_len: usize = if short_len.is_some() { 4 } else { 7 };
    let total_len = header_len.saturating_add(payload.len());
    if total_len > MAX_T4AT_NDEF_MESSAGE_BYTES {
        return Err(NfcTagError::NdefTooLarge {
            len_bytes: total_len,
            max_bytes: MAX_T4AT_NDEF_MESSAGE_BYTES,
        });
    }

    let mut record = Vec::with_capacity(total_len);
    if let Some(len) = short_len {
        // MB=1, ME=1, CF=0, SR=1, IL=0, TNF=0x01
        record.extend_from_slice(&[0xD1, 1, len, record_type]);
    } else {
        // MB=1, ME=1, CF=0, SR=0, IL=0, TNF=0x01
        let [b0, b1, b2, b3] = (payload.len() as u32).to_be_bytes();
        record.extend_from_slice(&[0xC1, 1, b0, b1, b2, b3, record_type]);
    }

    record.extend_from_slice(payload);
    Ok(record)
}

/// Encodes an NFC Forum RTD-Text record with the specified language code.
///
/// # Errors
///
/// Returns [`NfcTagError::InvalidLanguageCode`] if `lang` is empty, longer
/// than 63 bytes, or not ASCII. Returns [`NfcTagError::NdefTooLarge`] if
/// the encoded record exceeds [`MAX_T4AT_NDEF_MESSAGE_BYTES`].
pub fn encode_ndef_text_record(text: &str, lang: &str) -> Result<Vec<u8>, NfcTagError> {
    let status_byte = u8::try_from(lang.len())
        .ok()
        .filter(|&len| (1..=63).contains(&len) && lang.is_ascii())
        .ok_or_else(|| NfcTagError::InvalidLanguageCode { lang: lang.to_owned() })?;

    let lang_bytes = lang.as_bytes();
    let text_bytes = text.as_bytes();
    let mut payload = Vec::with_capacity(1 + lang_bytes.len() + text_bytes.len());
    payload.push(status_byte);
    payload.extend_from_slice(lang_bytes);
    payload.extend_from_slice(text_bytes);

    encode_well_known_record(b'T', &payload)
}

const URI_PREFIX_TABLE: [&str; 36] = [
    "",                           // 0x00
    "http://www.",                // 0x01
    "https://www.",               // 0x02
    "http://",                    // 0x03
    "https://",                   // 0x04
    "tel:",                       // 0x05
    "mailto:",                    // 0x06
    "ftp://anonymous:anonymous@", // 0x07
    "ftp://ftp.",                 // 0x08
    "ftps://",                    // 0x09
    "sftp://",                    // 0x0A
    "smb://",                     // 0x0B
    "nfs://",                     // 0x0C
    "ftp://",                     // 0x0D
    "dav://",                     // 0x0E
    "news:",                      // 0x0F
    "telnet://",                  // 0x10
    "imap:",                      // 0x11
    "rtsp://",                    // 0x12
    "urn:",                       // 0x13
    "pop:",                       // 0x14
    "sip:",                       // 0x15
    "sips:",                      // 0x16
    "tftp:",                      // 0x17
    "btspp://",                   // 0x18
    "btl2cap://",                 // 0x19
    "btgoep://",                  // 0x1A
    "tcpobex://",                 // 0x1B
    "irdaobex://",                // 0x1C
    "file://",                    // 0x1D
    "urn:epc:id:",                // 0x1E
    "urn:epc:tag:",               // 0x1F
    "urn:epc:pat:",               // 0x20
    "urn:epc:raw:",               // 0x21
    "urn:epc:",                   // 0x22
    "urn:nfc:",                   // 0x23
];

/// Encodes an NFC Forum RTD-URI record using longest-prefix code matching.
///
/// # Errors
///
/// Returns [`NfcTagError::NdefTooLarge`] if the encoded record exceeds
/// [`MAX_T4AT_NDEF_MESSAGE_BYTES`].
pub fn encode_ndef_uri_record(uri: &str) -> Result<Vec<u8>, NfcTagError> {
    let (best_code, remainder) = URI_PREFIX_TABLE
        .iter()
        .zip(0u8..)
        .skip(1)
        .filter_map(|(&prefix, code)| uri.strip_prefix(prefix).map(|rest| (code, rest)))
        .min_by_key(|(_, rest)| rest.len())
        .unwrap_or((0x00, uri));

    let remainder_bytes = remainder.as_bytes();
    let mut payload = Vec::with_capacity(1 + remainder_bytes.len());
    payload.push(best_code);
    payload.extend_from_slice(remainder_bytes);

    encode_well_known_record(b'U', &payload)
}

/// Decodes the payload of the first NFC Forum Text ("T") or URI ("U") record
/// in an NDEF message.
///
/// Returns `None` if:
/// - The message is empty or contains truncated record headers / fields.
/// - The Chunk Flag (`CF=1`) is set (chunked records are unsupported).
/// - No TNF=0x01 (Well-Known type) Text or URI record is found before the
///   Message End (`ME=1`) or end of input.
/// - A Text record has bit 7 set (UTF-16 encoding) or bit 6 set (RFU bit).
/// - A Text record's language code length exceeds the payload length.
/// - A Text or URI payload contains invalid UTF-8.
/// - A URI record contains an unknown prefix code (outside `0x00..=0x23`).
#[must_use]
pub fn decode_ndef_payload_text(ndef_bytes: &[u8]) -> Option<String> {
    let mut remaining = ndef_bytes;

    while let Some((&header, rest)) = remaining.split_first() {
        // CF=1 (Chunk Flag) is unsupported for direct text decoding
        if (header & 0x20) != 0 {
            return None;
        }

        let is_me = (header & 0x40) != 0;
        let is_sr = (header & 0x10) != 0;
        let is_il = (header & 0x08) != 0;
        let tnf = header & 0x07;

        let (&type_len, rest) = rest.split_first()?;
        let type_len = usize::from(type_len);

        let (payload_len, rest) = if is_sr {
            let (&len_u8, rest) = rest.split_first()?;
            (usize::from(len_u8), rest)
        } else {
            let (len_bytes, rest) = rest.split_first_chunk::<4>()?;
            let len = usize::try_from(u32::from_be_bytes(*len_bytes)).ok()?;
            (len, rest)
        };

        let (id_len, rest) = if is_il {
            let (&len_u8, rest) = rest.split_first()?;
            (usize::from(len_u8), rest)
        } else {
            (0, rest)
        };

        let (type_bytes, rest) = rest.split_at_checked(type_len)?;
        let (_id_bytes, rest) = rest.split_at_checked(id_len)?;
        let (payload, rest) = rest.split_at_checked(payload_len)?;

        if tnf == 0x01 {
            if type_bytes == b"T" {
                let (&status, text_payload) = payload.split_first()?;
                // Reject UTF-16 (bit 7 = 0x80) and reserved RFU (bit 6 = 0x40)
                if (status & 0xC0) != 0 {
                    return None;
                }
                let lang_len = usize::from(status & 0x3F);
                let text_bytes = text_payload.get(lang_len..)?;
                let text_str = std::str::from_utf8(text_bytes).ok()?;
                return Some(text_str.to_owned());
            } else if type_bytes == b"U" {
                let (&prefix_code, uri_suffix_bytes) = payload.split_first()?;
                let prefix = *URI_PREFIX_TABLE.get(usize::from(prefix_code))?;
                let suffix_str = std::str::from_utf8(uri_suffix_bytes).ok()?;
                return Some(format!("{prefix}{suffix_str}"));
            }
        }

        if is_me {
            break;
        }
        remaining = rest;
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tag_id_from_str() {
        let parsed_valid: Result<NfcTagId, _> = "42".parse();
        let id = parsed_valid.expect("valid u32 string must parse");
        assert_eq!(id, NfcTagId(42));
        assert_eq!(id.to_string(), "42");

        let parsed_abc: Result<NfcTagId, _> = "abc".parse();
        assert!(matches!(
            parsed_abc,
            Err(NfcTagError::InvalidId { ref input, .. }) if input == "abc"
        ));

        let parsed_neg: Result<NfcTagId, _> = "-1".parse();
        assert!(matches!(
            parsed_neg,
            Err(NfcTagError::InvalidId { ref input, .. }) if input == "-1"
        ));
    }

    #[test]
    fn tag_protocol_from_str() {
        let parsed_t4at: Result<NfcTagProtocol, _> = "T4AT".parse();
        assert_eq!(parsed_t4at.expect("T4AT"), NfcTagProtocol::T4at);
        assert_eq!(NfcTagProtocol::T4at.to_string(), "T4AT");

        let parsed_t4at_lower: Result<NfcTagProtocol, _> = "t4at".parse();
        assert_eq!(parsed_t4at_lower.expect("t4at"), NfcTagProtocol::T4at);

        let parsed_iso_dep: Result<NfcTagProtocol, _> = "iso_dep".parse();
        assert_eq!(parsed_iso_dep.expect("iso_dep"), NfcTagProtocol::T4at);

        let parsed_iso_dep_dash: Result<NfcTagProtocol, _> = "iso-dep".parse();
        assert_eq!(parsed_iso_dep_dash.expect("iso-dep"), NfcTagProtocol::T4at);

        let parsed_t2t: Result<NfcTagProtocol, _> = "T2T".parse();
        assert_eq!(parsed_t2t.expect("T2T"), NfcTagProtocol::T2t);
        assert_eq!(NfcTagProtocol::T2t.to_string(), "T2T");

        let parsed_t2t_lower: Result<NfcTagProtocol, _> = "t2t".parse();
        assert_eq!(parsed_t2t_lower.expect("t2t"), NfcTagProtocol::T2t);

        let parsed_mifare: Result<NfcTagProtocol, _> = "mifare_ultralight".parse();
        assert_eq!(parsed_mifare.expect("mifare"), NfcTagProtocol::T2t);

        let parsed_mifare_dash: Result<NfcTagProtocol, _> = "mifare-ultralight".parse();
        assert_eq!(parsed_mifare_dash.expect("mifare-dash"), NfcTagProtocol::T2t);

        let parsed_err: Result<NfcTagProtocol, _> = "unknown".parse();
        assert!(matches!(
            parsed_err,
            Err(NfcTagError::UnknownProtocol(s)) if s == "unknown"
        ));
    }

    #[test]
    fn default_uid_generation() {
        assert_eq!(NfcTag::default_uid(NfcTagId(0)), "04010203040000");
        assert_eq!(NfcTag::default_uid(NfcTagId(1)), "04010203040001");
        assert_eq!(NfcTag::default_uid(NfcTagId(256)), "04010203040100");
        assert_eq!(NfcTag::default_uid(NfcTagId(65535)), "0401020304ffff");
    }

    #[test]
    fn normalize_uid_hex_valid() {
        // 4-byte single-size UID
        assert_eq!(normalize_uid_hex("01020304").expect("4-byte UID"), "01020304");
        // 7-byte double-size UID with delimiters
        assert_eq!(
            normalize_uid_hex("04:01:02:03:04:05:06").expect("7-byte UID with colons"),
            "04010203040506"
        );
        assert_eq!(
            normalize_uid_hex("04-01-02 03-04-05 06").expect("7-byte UID with hyphens/spaces"),
            "04010203040506"
        );
        // 10-byte triple-size UID
        assert_eq!(
            normalize_uid_hex("04010203040506070809").expect("10-byte UID"),
            "04010203040506070809"
        );
    }

    #[test]
    fn normalize_uid_hex_invalid() {
        // 4-byte starting with cascade tag 0x88 (rejected per ISO 14443-3
        // Section 6.5.4)
        assert_eq!(normalize_uid_hex("88010203"), Err(NfcTagError::InvalidUidCascadeByte));
        // 7-byte starting with cascade tag 0x88
        assert_eq!(normalize_uid_hex("88010203040506"), Err(NfcTagError::InvalidUidCascadeByte));
        // 10-byte starting with cascade tag 0x88
        assert_eq!(
            normalize_uid_hex("88010203040506070809"),
            Err(NfcTagError::InvalidUidCascadeByte)
        );
        // Invalid lengths (3, 5 bytes)
        assert_eq!(
            normalize_uid_hex("010203"),
            Err(NfcTagError::InvalidUidLength { len_bytes: 3 })
        );
        assert_eq!(
            normalize_uid_hex("0102030405"),
            Err(NfcTagError::InvalidUidLength { len_bytes: 5 })
        );
        // Odd nibbles
        assert!(matches!(
            normalize_uid_hex("123"),
            Err(NfcTagError::InvalidHex { field: "uid", .. })
        ));
        // Non-hex chars
        assert!(matches!(
            normalize_uid_hex("0401020304050G"),
            Err(NfcTagError::InvalidHex { field: "uid", .. })
        ));
    }

    #[test]
    fn try_from_create_defaults() {
        let tag = NfcTag::try_from_create(NfcTagId(1), NfcTagCreate::default())
            .expect("default create must succeed");

        assert_eq!(tag.id, NfcTagId(1));
        assert_eq!(tag.name, "nfc-tag-1");
        assert_eq!(tag.protocol, NfcTagProtocol::T4at);
        assert_eq!(tag.uid, "04010203040001");
        assert_eq!(tag.position, Position::default());
        assert!(tag.visible);
        assert_eq!(tag.ndef_bytes, "");
        assert!(tag.payload_text.is_none());
        assert!(!tag.read_only);
        assert!(tag.formatted);
        assert!(!tag.disconnect_on_transceive);
        assert_eq!(tag.tx_count, 0);
        assert_eq!(tag.rx_count, 0);
    }

    #[test]
    fn try_from_create_custom() {
        let text_record =
            encode_ndef_text_record("Hello, world!", "en").expect("text record must encode");
        let ndef_hex = encode_hex_lower(&text_record);

        let create = NfcTagCreate {
            name: "my-custom-tag".to_owned(),
            protocol: NfcTagProtocol::T2t,
            uid: "04:02:03:04:05:06:07".to_owned(),
            position: Position { x: 1.0, y: 2.0, z: 3.0 },
            visible: false,
            ndef_bytes: ndef_hex,
            read_only: true,
            formatted: true,
            disconnect_on_transceive: true,
        };

        let tag = NfcTag::try_from_create(NfcTagId(5), create).expect("custom create must succeed");

        assert_eq!(tag.id, NfcTagId(5));
        assert_eq!(tag.name, "my-custom-tag");
        assert_eq!(tag.protocol, NfcTagProtocol::T2t);
        assert_eq!(tag.uid, "04020304050607");
        assert_eq!(tag.position, Position { x: 1.0, y: 2.0, z: 3.0 });
        assert!(!tag.visible);
        assert_eq!(tag.payload_text.as_deref(), Some("Hello, world!"));
        assert!(tag.read_only);
        assert!(tag.formatted);
        assert!(tag.disconnect_on_transceive);
        assert_eq!(tag.tx_count, 0);
        assert_eq!(tag.rx_count, 0);
    }

    #[test]
    fn ndef_text_record_roundtrip() {
        // Short text record (SR=1)
        let short_bytes =
            encode_ndef_text_record("Short message", "en").expect("short text record");
        assert_eq!(short_bytes.first(), Some(&0xD1));
        assert_eq!(decode_ndef_payload_text(&short_bytes).as_deref(), Some("Short message"));

        // Long text record (SR=0, > 255 payload bytes)
        let long_text = "NFC ".repeat(100);
        let long_bytes = encode_ndef_text_record(&long_text, "en").expect("long text record");
        assert_eq!(long_bytes.first(), Some(&0xC1));
        assert_eq!(decode_ndef_payload_text(&long_bytes).as_deref(), Some(long_text.as_str()));

        // Language code validations
        assert!(matches!(
            encode_ndef_text_record("Test", ""),
            Err(NfcTagError::InvalidLanguageCode { .. })
        ));
        assert!(matches!(
            encode_ndef_text_record("Test", &"e".repeat(64)),
            Err(NfcTagError::InvalidLanguageCode { .. })
        ));
        assert!(matches!(
            encode_ndef_text_record("Test", "e\u{00F1}"),
            Err(NfcTagError::InvalidLanguageCode { .. })
        ));
    }

    #[test]
    fn ndef_uri_record_roundtrip_all_prefixes() {
        for (code, &prefix) in URI_PREFIX_TABLE.iter().enumerate().skip(1) {
            let test_uri = format!("{prefix}example.com/path/{code}");
            let encoded = encode_ndef_uri_record(&test_uri).expect("URI record must encode");
            assert_eq!(encoded.get(4).copied(), u8::try_from(code).ok());
            let decoded = decode_ndef_payload_text(&encoded);
            assert_eq!(decoded.as_deref(), Some(test_uri.as_str()));
        }

        // Longest-prefix matching: 0x02 vs 0x04
        let https_www = "https://www.google.com";
        let enc_https_www = encode_ndef_uri_record(https_www).expect("https_www");
        assert_eq!(enc_https_www.get(4), Some(&0x02));
        assert_eq!(decode_ndef_payload_text(&enc_https_www).as_deref(), Some(https_www));

        let https_bare = "https://google.com";
        let enc_https_bare = encode_ndef_uri_record(https_bare).expect("https_bare");
        assert_eq!(enc_https_bare.get(4), Some(&0x04));
        assert_eq!(decode_ndef_payload_text(&enc_https_bare).as_deref(), Some(https_bare));

        // Longest-prefix matching: 0x07 vs 0x0D
        let ftp_anon = "ftp://anonymous:anonymous@ftp.example.com";
        let enc_ftp_anon = encode_ndef_uri_record(ftp_anon).expect("ftp_anon");
        assert_eq!(enc_ftp_anon.get(4), Some(&0x07));
        assert_eq!(decode_ndef_payload_text(&enc_ftp_anon).as_deref(), Some(ftp_anon));

        let ftp_bare = "ftp://ftp.example.com";
        let enc_ftp_bare = encode_ndef_uri_record(ftp_bare).expect("ftp_bare");
        assert_eq!(enc_ftp_bare.get(4), Some(&0x08));
        assert_eq!(decode_ndef_payload_text(&enc_ftp_bare).as_deref(), Some(ftp_bare));

        // Longest-prefix matching: 0x1E vs 0x22 vs 0x13
        let urn_epc_id = "urn:epc:id:sgtin-96";
        let enc_epc_id = encode_ndef_uri_record(urn_epc_id).expect("epc_id");
        assert_eq!(enc_epc_id.get(4), Some(&0x1E));
        assert_eq!(decode_ndef_payload_text(&enc_epc_id).as_deref(), Some(urn_epc_id));

        let urn_epc_raw = "urn:epc:raw:96";
        let enc_epc_raw = encode_ndef_uri_record(urn_epc_raw).expect("epc_raw");
        assert_eq!(enc_epc_raw.get(4), Some(&0x21));
        assert_eq!(decode_ndef_payload_text(&enc_epc_raw).as_deref(), Some(urn_epc_raw));

        let urn_epc = "urn:epc:other";
        let enc_epc = encode_ndef_uri_record(urn_epc).expect("epc");
        assert_eq!(enc_epc.get(4), Some(&0x22));
        assert_eq!(decode_ndef_payload_text(&enc_epc).as_deref(), Some(urn_epc));

        let urn_bare = "urn:ietf:rfc:2141";
        let enc_urn = encode_ndef_uri_record(urn_bare).expect("urn");
        assert_eq!(enc_urn.get(4), Some(&0x13));
        assert_eq!(decode_ndef_payload_text(&enc_urn).as_deref(), Some(urn_bare));

        // 0x1D file://
        let file_uri = "file:///sdcard/Download/test.apk";
        let enc_file = encode_ndef_uri_record(file_uri).expect("file_uri");
        assert_eq!(enc_file.get(4), Some(&0x1D));
        assert_eq!(decode_ndef_payload_text(&enc_file).as_deref(), Some(file_uri));

        // Fallback prefix 0x00
        let custom_uri = "custom://my.app/deep/link";
        let enc_custom = encode_ndef_uri_record(custom_uri).expect("custom_uri");
        assert_eq!(enc_custom.get(4), Some(&0x00));
        assert_eq!(decode_ndef_payload_text(&enc_custom).as_deref(), Some(custom_uri));
    }

    #[test]
    fn decode_ndef_payload_text_resilience() {
        // IL=1: Record with ID Length byte and ID bytes
        let il_record =
            [0xD9, 1, 8, 3, b'T', b'i', b'd', b'1', 0x02, b'e', b'n', b'H', b'e', b'l', b'l', b'o'];
        assert_eq!(decode_ndef_payload_text(&il_record).as_deref(), Some("Hello"));

        // CF=1: Chunk Flag set -> None
        let mut cf_record = il_record;
        if let Some(h) = cf_record.first_mut() {
            *h |= 0x20;
        }
        assert_eq!(decode_ndef_payload_text(&cf_record), None);

        // UTF-16 bit set (status = 0x82) -> None
        let utf16_record = [0xD1, 1, 6, b'T', 0x82, b'e', b'n', 0x00, 0x48, 0x00];
        assert_eq!(decode_ndef_payload_text(&utf16_record), None);

        // Unknown URI prefix code 0x24 -> None
        let unknown_uri_prefix = [0xD1, 1, 5, b'U', 0x24, b't', b'e', b's', b't'];
        assert_eq!(decode_ndef_payload_text(&unknown_uri_prefix), None);

        // Truncated headers -> None
        assert_eq!(decode_ndef_payload_text(&[]), None);
        assert_eq!(decode_ndef_payload_text(&[0xD1]), None);
        assert_eq!(decode_ndef_payload_text(&[0xD1, 1, 50, b'T', 0x02]), None);

        // Out-of-bounds language code length in Text record -> None
        let invalid_lang_len = [0xD1, 1, 2, b'T', 0x05, b'e'];
        assert_eq!(decode_ndef_payload_text(&invalid_lang_len), None);
    }

    #[test]
    fn multi_record_ndef_message_skips_unsupported_first_record() {
        // Record 1: MB=1, ME=0, SR=1, IL=0, TNF=1, Type="X", payload="abc"
        // Record 2: MB=0, ME=1, SR=1, IL=0, TNF=1, Type="T",
        // payload=[0x02, 'e', 'n', 'H', 'i']
        let multi =
            [0x91, 1, 3, b'X', b'a', b'b', b'c', 0x51, 1, 5, b'T', 0x02, b'e', b'n', b'H', b'i'];

        assert_eq!(decode_ndef_payload_text(&multi).as_deref(), Some("Hi"));
    }

    #[test]
    fn encode_ndef_records_reject_oversized_payload() {
        let oversized_text = "A".repeat(MAX_T4AT_NDEF_MESSAGE_BYTES);
        assert!(matches!(
            encode_ndef_text_record(&oversized_text, "en"),
            Err(NfcTagError::NdefTooLarge { .. })
        ));

        let oversized_uri = format!("https://{}", "a".repeat(MAX_T4AT_NDEF_MESSAGE_BYTES));
        assert!(matches!(
            encode_ndef_uri_record(&oversized_uri),
            Err(NfcTagError::NdefTooLarge { .. })
        ));
    }

    #[test]
    fn decode_ndef_payload_text_rejects_reserved_bit_6() {
        // Text status byte with bit 6 set (0x42: RFU bit set, lang_len=2)
        let reserved_bit_record = [0xD1, 1, 7, b'T', 0x42, b'e', b'n', b't', b'e', b's', b't'];
        assert_eq!(decode_ndef_payload_text(&reserved_bit_record), None);
    }

    #[test]
    fn serde_json_wire_representation() {
        let create: NfcTagCreate =
            serde_json::from_str("{}").expect("create from empty JSON must deserialize");
        assert!(create.visible);
        assert!(create.formatted);
        assert_eq!(create.name, "");

        let json = r#"{
            "id": 1,
            "name": "tag-1",
            "protocol": "T4AT",
            "uid": "04010203040001",
            "position": {"x": 0.0, "y": 0.0, "z": 0.0},
            "visible": true,
            "ndef_bytes": "",
            "read_only": false,
            "formatted": true,
            "disconnect_on_transceive": false
        }"#;
        let tag: NfcTag = serde_json::from_str(json).expect("tag deserialize without counts");
        assert_eq!(tag.tx_count, 0);
        assert_eq!(tag.rx_count, 0);
        assert_eq!(tag.payload_text, None);

        let serialized = serde_json::to_string(&tag).expect("tag serialize");
        assert!(!serialized.contains("payload_text"));
    }

    #[test]
    fn update_apply_atomicity_and_mutation() {
        let mut tag =
            NfcTag::try_from_create(NfcTagId(1), NfcTagCreate::default()).expect("create tag");

        // 1. Invalid hex in update -> Error and tag completely untouched
        let bad_hex_update = NfcTagUpdate {
            name: Some("new-name-should-fail".to_owned()),
            ndef_bytes: Some("invalid_hex".to_owned()),
            ..Default::default()
        };
        assert!(bad_hex_update.apply(&mut tag).is_err());
        assert_eq!(tag.name, "nfc-tag-1");
        assert_eq!(tag.ndef_bytes, "");

        // 2. Oversized NDEF for T4at (> 4094 bytes) -> Error and tag untouched
        let oversized_hex = "aa".repeat(MAX_T4AT_NDEF_MESSAGE_BYTES + 1);
        let oversized_update = NfcTagUpdate {
            name: Some("new-name-should-fail".to_owned()),
            ndef_bytes: Some(oversized_hex),
            ..Default::default()
        };
        assert_eq!(
            oversized_update.apply(&mut tag),
            Err(NfcTagError::NdefTooLarge {
                len_bytes: MAX_T4AT_NDEF_MESSAGE_BYTES + 1,
                max_bytes: MAX_T4AT_NDEF_MESSAGE_BYTES,
            })
        );
        assert_eq!(tag.name, "nfc-tag-1");

        // 3. Oversized NDEF for T2t (> 883 bytes) -> Error and tag untouched
        let mut t2t_tag = NfcTag::try_from_create(
            NfcTagId(2),
            NfcTagCreate { protocol: NfcTagProtocol::T2t, ..Default::default() },
        )
        .expect("create t2t tag");
        let t2t_oversized_hex = "aa".repeat(MAX_T2T_NDEF_MESSAGE_BYTES + 1);
        let t2t_oversized_update = NfcTagUpdate {
            name: Some("new-name-should-fail".to_owned()),
            ndef_bytes: Some(t2t_oversized_hex),
            ..Default::default()
        };
        assert_eq!(
            t2t_oversized_update.apply(&mut t2t_tag),
            Err(NfcTagError::NdefTooLarge {
                len_bytes: MAX_T2T_NDEF_MESSAGE_BYTES + 1,
                max_bytes: MAX_T2T_NDEF_MESSAGE_BYTES,
            })
        );
        assert_eq!(t2t_tag.name, "nfc-tag-2");

        // 4. Empty name resets to "nfc-tag-{id}"
        let empty_name_update = NfcTagUpdate { name: Some("   ".to_owned()), ..Default::default() };
        empty_name_update.apply(&mut tag).expect("empty name apply");
        assert_eq!(tag.name, "nfc-tag-1");

        // 5. Valid updates mutate fields, and set_ndef_bytes updates
        // payload_text.
        let text_record = encode_ndef_text_record("Updated payload", "en").expect("text record");
        let valid_update = NfcTagUpdate {
            name: Some("updated-tag".to_owned()),
            position: Some(Position { x: 5.0, y: 6.0, z: 7.0 }),
            visible: Some(false),
            ndef_bytes: Some(encode_hex_lower(&text_record)),
            read_only: Some(true),
            formatted: Some(true),
            disconnect_on_transceive: Some(true),
        };
        valid_update.apply(&mut tag).expect("valid update apply");
        assert_eq!(tag.name, "updated-tag");
        assert_eq!(tag.position, Position { x: 5.0, y: 6.0, z: 7.0 });
        assert!(!tag.visible);
        assert_eq!(tag.ndef_bytes, encode_hex_lower(&text_record));
        assert_eq!(tag.payload_text.as_deref(), Some("Updated payload"));
        assert!(tag.read_only);
        assert!(tag.formatted);
        assert!(tag.disconnect_on_transceive);
    }
}
