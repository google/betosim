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

//! Sans-I/O NFC Forum Type 4A / DESFire tag state machine.

use casimir::packets::rf;
use netsim_model::{
    MAX_T4AT_NDEF_FILE_CAPACITY, MAX_T4AT_NDEF_MESSAGE_BYTES, NfcTag, NfcTagError, NfcTagProtocol,
    NfcTagUpdate, decode_ndef_hex, normalize_uid_hex,
};

use crate::accessory::RfAccessory;

const NLEN_HEADER_BYTES: usize = 2;
const RF_POWER_LEVEL_NOMINAL: u8 = 10;

/// Sans-I/O state machine emulating an NFC Forum Type 4A / DESFire tag.
#[derive(Debug)]
pub struct T4atTag {
    casimir_rf_id: u16,
    uid: Vec<u8>,
    visible: bool,
    read_only: bool,
    formatted: bool,
    disconnect_on_transceive: bool,
    tx_count: u32,
    rx_count: u32,
    active_reader: Option<u16>,
    e104_file: [u8; MAX_T4AT_NDEF_FILE_CAPACITY],
}

impl T4atTag {
    fn clamped_e104_end(&self) -> usize {
        let nlen = usize::from(u16::from_be_bytes([self.e104_file[0], self.e104_file[1]]));
        self.e104_file.len().min(NLEN_HEADER_BYTES.saturating_add(nlen))
    }

    fn write_e104_ndef(&mut self, raw_ndef: &[u8]) -> Result<(), NfcTagError> {
        if raw_ndef.len() > MAX_T4AT_NDEF_MESSAGE_BYTES {
            return Err(NfcTagError::NdefTooLarge {
                len_bytes: raw_ndef.len(),
                max_bytes: MAX_T4AT_NDEF_MESSAGE_BYTES,
            });
        }
        self.e104_file.fill(0);
        let [nlen_hi, nlen_lo] = (raw_ndef.len() as u16).to_be_bytes();
        self.e104_file[0] = nlen_hi;
        self.e104_file[1] = nlen_lo;
        let ndef_end = NLEN_HEADER_BYTES.saturating_add(raw_ndef.len());
        if let Some(dest) = self.e104_file.get_mut(NLEN_HEADER_BYTES..ndef_end) {
            dest.copy_from_slice(raw_ndef);
        }
        Ok(())
    }

    fn reset_session_state(&mut self) {}

    /// Creates a validated Type 4A tag state machine from model parameters.
    pub fn try_from_tag(tag: &NfcTag, casimir_rf_id: u16) -> Result<Self, NfcTagError> {
        if tag.protocol != NfcTagProtocol::T4at {
            return Err(NfcTagError::UnknownProtocol(tag.protocol.to_string()));
        }
        let normalized_uid = normalize_uid_hex(&tag.uid)?;
        let uid = decode_ndef_hex(&normalized_uid, NfcTagProtocol::T4at)?;
        let ndef_bytes = decode_ndef_hex(&tag.ndef_bytes, NfcTagProtocol::T4at)?;
        let mut this = Self {
            casimir_rf_id,
            uid,
            visible: tag.visible,
            read_only: tag.read_only,
            formatted: tag.formatted,
            disconnect_on_transceive: tag.disconnect_on_transceive,
            tx_count: 0,
            rx_count: 0,
            active_reader: None,
            e104_file: [0u8; MAX_T4AT_NDEF_FILE_CAPACITY],
        };
        if tag.formatted {
            this.write_e104_ndef(&ndef_bytes)?;
        }
        Ok(this)
    }

    /// Returns the normalized UID bytes of the tag.
    pub fn uid_bytes(&self) -> &[u8] {
        &self.uid
    }
}

impl RfAccessory for T4atTag {
    fn set_casimir_id(&mut self, casimir_rf_id: u16) {
        self.casimir_rf_id = casimir_rf_id;
    }

    fn handle_packet(&mut self, packet: &rf::RfPacket) -> Option<rf::RfPacket> {
        if !self.visible
            || packet.sender() == self.casimir_rf_id
            || (packet.receiver() != u16::MAX && packet.receiver() != self.casimir_rf_id)
        {
            return None;
        }
        None
    }

    fn deactivate_active_reader(&mut self, reason: rf::DeactivateReason) -> Option<rf::RfPacket> {
        let reader_id = self.active_reader.take()?;
        self.reset_session_state();
        self.tx_count = self.tx_count.saturating_add(1);
        rf::DeactivateNotification {
            sender: self.casimir_rf_id,
            receiver: reader_id,
            technology: rf::Technology::NfcA,
            protocol: rf::Protocol::IsoDep,
            bitrate: rf::BitRate::BitRate106KbitS,
            power_level: RF_POWER_LEVEL_NOMINAL,
            type_: rf::DeactivateType::Discovery,
            reason,
        }
        .try_into()
        .map_err(|error| tracing::debug!(%error, "failed to encode deactivate notification"))
        .ok()
    }

    fn active_reader(&self) -> Option<u16> {
        self.active_reader
    }

    fn apply_update(&mut self, patch: &NfcTagUpdate) -> Result<(), NfcTagError> {
        let decoded_ndef = patch
            .ndef_bytes
            .as_deref()
            .map(|hex| decode_ndef_hex(hex, NfcTagProtocol::T4at))
            .transpose()?;

        if let Some(ref raw_ndef) = decoded_ndef
            && raw_ndef.len() > MAX_T4AT_NDEF_MESSAGE_BYTES
        {
            return Err(NfcTagError::NdefTooLarge {
                len_bytes: raw_ndef.len(),
                max_bytes: MAX_T4AT_NDEF_MESSAGE_BYTES,
            });
        }

        if let Some(visible) = patch.visible {
            self.visible = visible;
            if !visible {
                self.reset_session_state();
                self.active_reader = None;
            }
        }
        if let Some(disconnect) = patch.disconnect_on_transceive {
            self.disconnect_on_transceive = disconnect;
        }
        if let Some(read_only) = patch.read_only {
            self.read_only = read_only;
        }
        if let Some(formatted) = patch.formatted {
            self.formatted = formatted;
            if !formatted {
                self.e104_file.fill(0);
                self.reset_session_state();
            }
        }
        if let (true, Some(raw_ndef)) = (self.formatted, decoded_ndef) {
            self.write_e104_ndef(&raw_ndef)?;
        }

        Ok(())
    }

    fn live_ndef_bytes(&self) -> &[u8] {
        if !self.formatted {
            return &[];
        }
        let end = self.clamped_e104_end();
        if end > NLEN_HEADER_BYTES {
            self.e104_file.get(NLEN_HEADER_BYTES..end).unwrap_or(&[])
        } else {
            &[]
        }
    }

    fn sync_to_model(&self, tag: &mut NfcTag) {
        tag.visible = self.visible;
        tag.read_only = self.read_only;
        tag.formatted = self.formatted;
        tag.disconnect_on_transceive = self.disconnect_on_transceive;
        tag.tx_count = self.tx_count;
        tag.rx_count = self.rx_count;
        if self.formatted {
            let ndef = self.live_ndef_bytes();
            tag.set_ndef_bytes(ndef);
        } else {
            tag.set_ndef_bytes(&[]);
        }
    }

    fn is_visible(&self) -> bool {
        self.visible
    }

    fn is_read_only(&self) -> bool {
        self.read_only
    }

    fn is_formatted(&self) -> bool {
        self.formatted
    }

    fn tx_count(&self) -> u32 {
        self.tx_count
    }

    fn rx_count(&self) -> u32 {
        self.rx_count
    }
}

#[cfg(test)]
mod tests {
    use netsim_model::{NfcTagId, Position, encode_hex_lower, encode_ndef_text_record};

    use super::*;

    fn make_test_tag(formatted: bool, read_only: bool, ndef_text: &str) -> NfcTag {
        let ndef_bytes = if ndef_text.is_empty() {
            Vec::new()
        } else {
            encode_ndef_text_record(ndef_text, "en").expect("valid ndef text")
        };
        NfcTag {
            id: NfcTagId(1),
            name: "test-tag".to_string(),
            protocol: NfcTagProtocol::T4at,
            uid: "04010203040506".to_string(),
            position: Position::default(),
            visible: true,
            ndef_bytes: encode_hex_lower(&ndef_bytes),
            payload_text: if ndef_text.is_empty() { None } else { Some(ndef_text.to_string()) },
            read_only,
            formatted,
            disconnect_on_transceive: false,
            tx_count: 0,
            rx_count: 0,
        }
    }

    #[test]
    fn uid_normalization_and_e104_initialization_with_clamped_nlen() {
        let mut model_4b = make_test_tag(true, false, "Hello");
        model_4b.uid = "01020304".to_string();
        let mut model_7b = make_test_tag(true, false, "Hello");
        model_7b.uid = "04010203040506".to_string();
        let mut model_10b = make_test_tag(true, false, "Hello");
        model_10b.uid = "0102030405060708090A".to_string();

        let tag_4b = T4atTag::try_from_tag(&model_4b, 10).expect("valid 4B tag");
        let tag_7b = T4atTag::try_from_tag(&model_7b, 11).expect("valid 7B tag");
        let tag_10b = T4atTag::try_from_tag(&model_10b, 12).expect("valid 10B tag");

        assert_eq!(tag_4b.uid_bytes(), &[0x01, 0x02, 0x03, 0x04]);
        assert_eq!(tag_7b.uid_bytes(), &[0x04, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06]);
        assert_eq!(
            tag_10b.uid_bytes(),
            &[0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A]
        );

        let expected_ndef = encode_ndef_text_record("Hello", "en").expect("valid ndef text");
        assert_eq!(tag_7b.live_ndef_bytes(), expected_ndef.as_slice());
    }

    #[test]
    fn oversized_nlen_in_e104_file_clamps_live_ndef_bytes_to_capacity() {
        let model = make_test_tag(true, false, "Initial");
        let mut tag = T4atTag::try_from_tag(&model, 15).expect("valid tag");

        if let Some(dest) = tag.e104_file.get_mut(..2) {
            dest.copy_from_slice(&[0xFF, 0xFF]);
        }

        assert_eq!(tag.live_ndef_bytes().len(), MAX_T4AT_NDEF_FILE_CAPACITY - 2);
    }

    #[test]
    fn invalid_uid_hex_returns_invalid_hex_error() {
        let mut model = make_test_tag(true, false, "Hello");
        model.uid = "123".to_string();
        let result = T4atTag::try_from_tag(&model, 16);
        assert!(matches!(result, Err(NfcTagError::InvalidHex { field: "uid", .. })));
    }

    #[test]
    fn unformatted_tag_with_invalid_ndef_hex_returns_invalid_hex_error() {
        let mut model = make_test_tag(false, false, "");
        model.ndef_bytes = "123".to_string();
        let result = T4atTag::try_from_tag(&model, 17);
        assert!(matches!(result, Err(NfcTagError::InvalidHex { field: "ndef_bytes", .. })));
    }

    #[test]
    fn unformatted_tag_initializes_with_empty_ndef_and_zeroed_file() {
        let model = make_test_tag(false, false, "");
        let tag = T4atTag::try_from_tag(&model, 20).expect("valid unformatted tag");
        assert!(!tag.is_formatted());
        assert!(tag.live_ndef_bytes().is_empty());
    }

    #[test]
    fn apply_update_unformat_preserves_active_reader() {
        let model = make_test_tag(true, false, "Initial");
        let mut tag = T4atTag::try_from_tag(&model, 30).expect("valid tag");
        tag.active_reader = Some(42);

        let unformat_patch = NfcTagUpdate { formatted: Some(false), ..Default::default() };
        tag.apply_update(&unformat_patch).expect("unformat succeeds");

        assert!(!tag.is_formatted());
        assert!(tag.live_ndef_bytes().is_empty());
        assert_eq!(tag.active_reader(), Some(42));
    }

    #[test]
    fn apply_update_hide_clears_active_reader() {
        let model = make_test_tag(true, false, "Initial");
        let mut tag = T4atTag::try_from_tag(&model, 31).expect("valid tag");
        tag.active_reader = Some(42);

        let hide_patch = NfcTagUpdate { visible: Some(false), ..Default::default() };
        tag.apply_update(&hide_patch).expect("hide succeeds");

        assert!(!tag.is_visible());
        assert_eq!(tag.active_reader(), None);
    }

    #[test]
    fn apply_update_modifies_fields_and_syncs_to_model() {
        let model = make_test_tag(false, false, "");
        let mut tag = T4atTag::try_from_tag(&model, 32).expect("valid tag");

        let new_ndef = encode_ndef_text_record("Updated", "en").expect("valid ndef");
        let restore_patch = NfcTagUpdate {
            visible: Some(true),
            formatted: Some(true),
            ndef_bytes: Some(encode_hex_lower(&new_ndef)),
            read_only: Some(true),
            disconnect_on_transceive: Some(true),
            ..Default::default()
        };
        tag.apply_update(&restore_patch).expect("restore succeeds");

        assert!(tag.is_visible());
        assert!(tag.is_formatted());
        assert!(tag.is_read_only());
        assert_eq!(tag.live_ndef_bytes(), new_ndef.as_slice());

        let mut synced = model;
        tag.sync_to_model(&mut synced);
        assert!(synced.disconnect_on_transceive);
        assert_eq!(synced.ndef_bytes, encode_hex_lower(&new_ndef));
    }

    #[test]
    fn apply_update_with_invalid_ndef_hex_returns_err_without_partial_mutation() {
        let model = make_test_tag(true, false, "Valid");
        let mut tag = T4atTag::try_from_tag(&model, 33).expect("valid tag");

        let bad_patch = NfcTagUpdate {
            visible: Some(false),
            ndef_bytes: Some("xyz_invalid".to_string()),
            ..Default::default()
        };
        let result = tag.apply_update(&bad_patch);

        assert!(matches!(result, Err(NfcTagError::InvalidHex { field: "ndef_bytes", .. })));
        assert!(tag.is_visible());
    }

    #[test]
    fn sync_to_model_synchronizes_state_and_live_ndef_bytes() {
        let model = make_test_tag(true, false, "SyncTest");
        let tag = T4atTag::try_from_tag(&model, 40).expect("valid tag");

        let mut synced = model;
        synced.ndef_bytes.clear();
        tag.sync_to_model(&mut synced);

        let expected_ndef = encode_ndef_text_record("SyncTest", "en").expect("valid ndef");
        assert_eq!(synced.ndef_bytes, encode_hex_lower(&expected_ndef));
        assert!(synced.visible);
        assert!(!synced.read_only);
        assert!(synced.formatted);
    }

    #[test]
    fn deactivate_active_reader_emits_discovery_deactivate_notification() {
        let model = make_test_tag(true, false, "LinkLoss");
        let mut tag = T4atTag::try_from_tag(&model, 50).expect("valid tag");
        tag.active_reader = Some(99);

        let maybe_pkt = tag.deactivate_active_reader(rf::DeactivateReason::RfLinkLoss);
        assert_eq!(tag.active_reader(), None);
        let pkt = maybe_pkt.expect("must return DeactivateNotification packet");
        match pkt.specialize().expect("specializes to child") {
            rf::RfPacketChild::DeactivateNotification(notif) => {
                assert_eq!(notif.sender(), 50);
                assert_eq!(notif.receiver(), 99);
                assert_eq!(notif.type_(), rf::DeactivateType::Discovery);
                assert_eq!(notif.reason(), rf::DeactivateReason::RfLinkLoss);
            }
            other => panic!("expected DeactivateNotification, got {other:?}"),
        }
    }

    #[test]
    fn try_from_tag_with_oversized_ndef_returns_ndef_too_large_error() {
        let mut model = make_test_tag(true, false, "");
        let oversized_payload = [0xA5u8; 4095];
        model.ndef_bytes = encode_hex_lower(&oversized_payload);
        let result = T4atTag::try_from_tag(&model, 100);
        assert!(matches!(
            result,
            Err(NfcTagError::NdefTooLarge { len_bytes: 4095, max_bytes: 4094 })
        ));
    }

    #[test]
    fn try_from_tag_with_unsupported_protocol_returns_error() {
        let mut model = make_test_tag(true, false, "Hello");
        model.protocol = NfcTagProtocol::T2t;
        let result = T4atTag::try_from_tag(&model, 102);
        assert!(matches!(result, Err(NfcTagError::UnknownProtocol(_))));
    }

    #[test]
    fn try_from_tag_with_exact_capacity_4094_bytes_succeeds() {
        let mut model = make_test_tag(true, false, "");
        let exact_payload = [0x5Au8; 4094];
        model.ndef_bytes = encode_hex_lower(&exact_payload);
        let tag = T4atTag::try_from_tag(&model, 101).expect("exact capacity payload succeeds");
        assert_eq!(tag.e104_file.get(..2), Some(&[0x0F, 0xFE][..]));
        assert_eq!(tag.live_ndef_bytes(), exact_payload.as_slice());
    }

    #[test]
    fn write_e104_ndef_with_oversized_slice_returns_error() {
        let model = make_test_tag(false, false, "");
        let mut tag = T4atTag::try_from_tag(&model, 103).expect("valid tag");
        let oversized = [0x3Cu8; 4095];
        let result = tag.write_e104_ndef(&oversized);
        assert!(matches!(
            result,
            Err(NfcTagError::NdefTooLarge { len_bytes: 4095, max_bytes: 4094 })
        ));
        assert!(tag.e104_file.iter().all(|&b| b == 0));
    }

    #[test]
    fn try_from_tag_with_empty_ndef_bytes_formatted_succeeds() {
        let model = make_test_tag(true, false, "");
        let tag = T4atTag::try_from_tag(&model, 104).expect("formatted empty ndef succeeds");
        assert!(tag.is_formatted());
        assert_eq!(tag.e104_file.get(..2), Some(&[0x00, 0x00][..]));
        assert!(tag.live_ndef_bytes().is_empty());
    }

    #[test]
    fn apply_update_with_oversized_ndef_returns_error() {
        let model = make_test_tag(true, false, "Initial");
        let mut tag = T4atTag::try_from_tag(&model, 105).expect("valid tag");
        assert!(tag.is_visible());

        let oversized_patch = NfcTagUpdate {
            visible: Some(false),
            ndef_bytes: Some("00".repeat(4095)),
            ..Default::default()
        };
        let err = tag.apply_update(&oversized_patch).expect_err("oversized payload must fail");
        assert!(matches!(err, NfcTagError::NdefTooLarge { len_bytes: 4095, max_bytes: 4094 }));
        assert!(tag.is_visible(), "tag visibility must not be mutated on failed update");
    }
}
