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
use tracing::debug;

use crate::{
    accessory::RfAccessory,
    apdu::{ParsedApdu, SW_WRONG_LENGTH, parse_apdu},
};

const SW_OK: [u8; 2] = [0x90, 0x00];
const SW_FILE_NOT_FOUND: [u8; 2] = [0x6A, 0x82];
const SW_INCORRECT_P1P2: [u8; 2] = [0x6A, 0x86];
const SW_WRONG_P1P2: [u8; 2] = [0x6B, 0x00];
const SW_INS_NOT_SUPPORTED: [u8; 2] = [0x6D, 0x00];
const SW_CLA_NOT_SUPPORTED: [u8; 2] = [0x6E, 0x00];

// Casimir's `IsoDepT4ATSelectResponse.rats_response` excludes the leading ISO/IEC 14443-4 `TL`
// length byte (starting at Byte 2: T0, TA(1), TB(1), TC(1), historical byte 0x80) because
// `casimir::controller` prepends `TL` automatically when building the NCI activation payload.
const DESFIRE_ATS_RESPONSE: [u8; 5] = [0x75, 0x77, 0x81, 0x02, 0x80];

const NLEN_HEADER_BYTES: usize = 2;
const RF_POWER_LEVEL_NOMINAL: u8 = 10;
const SINGLE_SIZE_UID_LEN: usize = 4;
const BIT_FRAME_SDD_SINGLE_SIZE_UID: u8 = 0x08;
const BIT_FRAME_SDD_CASCADE_REQUIRED: u8 = 0x04;
const INT_PROTOCOL_ISO_DEP: u8 = 0b01;

const CC_FILE_LEN: usize = 15;
const CC_WRITE_ACCESS_OFFSET: usize = 14;

const CC_FILE_ID: [u8; 2] = [0xE1, 0x03];
const NDEF_FILE_ID: [u8; 2] = [0xE1, 0x04];
const T4T_V2_APP_ID: [u8; 7] = [0xD2, 0x76, 0x00, 0x00, 0x85, 0x01, 0x01];
const T4T_V1_APP_ID: [u8; 7] = [0xD2, 0x76, 0x00, 0x00, 0x85, 0x01, 0x00];
const DESFIRE_NDEF_APP_ID: [u8; 5] = [0xF0, 0x4E, 0x44, 0x45, 0x46];

const CC_LEN_BYTES: [u8; 2] = (CC_FILE_LEN as u16).to_be_bytes();
const T4T_MAX_R_APDU_DATA_BYTES: [u8; 2] = 0x00FFu16.to_be_bytes();
const T4T_MAX_C_APDU_DATA_BYTES: [u8; 2] = 0x00FFu16.to_be_bytes();
const MAX_NDEF_SIZE_BYTES: [u8; 2] = (MAX_T4AT_NDEF_FILE_CAPACITY as u16).to_be_bytes();

const T4T_SPEC_VERSION_2_0: u8 = 0x20;
const T4T_NDEF_TLV_TAG: u8 = 0x04;
const T4T_NDEF_TLV_LEN: u8 = 0x06;

const CC_ACCESS_GRANTED: u8 = 0x00;
const CC_ACCESS_DISABLED: u8 = 0xFF;

const CLA_STANDARD: u8 = 0x00;
const CLA_PROPRIETARY: u8 = 0x80;
const INS_SELECT: u8 = 0xA4;
const INS_READ_BINARY: u8 = 0xB0;

const P1_SELECT_BY_DF_NAME: u8 = 0x04;
const P1_SELECT_BY_EF_ID: u8 = 0x00;
const P2_SELECT_FIRST_OR_ONLY: u8 = 0x00;
const P2_SELECT_NO_FCI: u8 = 0x0C;

const EF_FILE_ID_LEN: usize = 2;
const CARD_STATE_READ_ONLY: u8 = 0x01;
const CARD_STATE_READ_WRITE: u8 = 0x02;
const DEFAULT_READ_BINARY_LE: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SelectedFile {
    None,
    App,
    Cc,
    Ndef,
}

fn encode<T: TryInto<rf::RfPacket>>(pkt: T, what: &str) -> Option<rf::RfPacket>
where
    T::Error: std::fmt::Display,
{
    match pkt.try_into() {
        Ok(packet) => Some(packet),
        Err(error) => {
            debug!(%error, "failed to encode T4AT {what}");
            None
        }
    }
}

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
    cc_file: [u8; CC_FILE_LEN],
    e104_file: [u8; MAX_T4AT_NDEF_FILE_CAPACITY],
    selected_file: SelectedFile,
}

impl T4atTag {
    fn build_cc_file(read_only: bool) -> [u8; CC_FILE_LEN] {
        let write_access = if read_only { CC_ACCESS_DISABLED } else { CC_ACCESS_GRANTED };
        let [cc_len_hi, cc_len_lo] = CC_LEN_BYTES;
        let [mle_hi, mle_lo] = T4T_MAX_R_APDU_DATA_BYTES;
        let [mlc_hi, mlc_lo] = T4T_MAX_C_APDU_DATA_BYTES;
        let [ndef_id_hi, ndef_id_lo] = NDEF_FILE_ID;
        let [max_ndef_hi, max_ndef_lo] = MAX_NDEF_SIZE_BYTES;
        [
            cc_len_hi,
            cc_len_lo,
            T4T_SPEC_VERSION_2_0,
            mle_hi,
            mle_lo,
            mlc_hi,
            mlc_lo,
            T4T_NDEF_TLV_TAG,
            T4T_NDEF_TLV_LEN,
            ndef_id_hi,
            ndef_id_lo,
            max_ndef_hi,
            max_ndef_lo,
            CC_ACCESS_GRANTED,
            write_access,
        ]
    }

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

    fn reset_session_state(&mut self) {
        self.selected_file = SelectedFile::None;
    }

    /// Creates a validated Type 4A tag state machine from model parameters.
    pub fn try_from_tag(tag: &NfcTag, casimir_rf_id: u16) -> Result<Self, NfcTagError> {
        if tag.protocol != NfcTagProtocol::T4at {
            return Err(NfcTagError::UnknownProtocol(tag.protocol.to_string()));
        }
        let normalized_uid = normalize_uid_hex(&tag.uid)?;
        let uid = decode_ndef_hex(&normalized_uid, NfcTagProtocol::T4at)?;
        let ndef_bytes = decode_ndef_hex(&tag.ndef_bytes, NfcTagProtocol::T4at)?;
        let cc_file = Self::build_cc_file(tag.read_only);
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
            cc_file,
            e104_file: [0u8; MAX_T4AT_NDEF_FILE_CAPACITY],
            selected_file: SelectedFile::None,
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

    fn handle_poll_command(&mut self, cmd: &rf::PollCommand) -> Option<rf::RfPacket> {
        if cmd.technology() != rf::Technology::NfcA {
            return None;
        }
        if self.active_reader.is_some_and(|active| active != cmd.sender()) {
            return None;
        }
        if self.active_reader == Some(cmd.sender()) {
            self.active_reader = None;
            self.reset_session_state();
        }

        let bit_frame_sdd = if self.uid.len() == SINGLE_SIZE_UID_LEN {
            BIT_FRAME_SDD_SINGLE_SIZE_UID
        } else {
            BIT_FRAME_SDD_CASCADE_REQUIRED
        };
        self.rx_count = self.rx_count.saturating_add(1);
        self.tx_count = self.tx_count.saturating_add(1);

        encode(
            rf::NfcAPollResponse {
                sender: self.casimir_rf_id,
                receiver: cmd.sender(),
                protocol: rf::Protocol::IsoDep,
                bitrate: rf::BitRate::BitRate106KbitS,
                power_level: RF_POWER_LEVEL_NOMINAL,
                nfcid1: self.uid.clone(),
                int_protocol: INT_PROTOCOL_ISO_DEP,
                bit_frame_sdd,
            },
            "poll response",
        )
    }

    fn handle_select_command(&mut self, cmd: &rf::IsoDepT4ATSelectCommand) -> Option<rf::RfPacket> {
        if self.active_reader.is_some_and(|active| active != cmd.sender()) {
            return None;
        }
        self.active_reader = Some(cmd.sender());
        self.reset_session_state();
        self.rx_count = self.rx_count.saturating_add(1);
        self.tx_count = self.tx_count.saturating_add(1);

        encode(
            rf::IsoDepT4ATSelectResponse {
                sender: self.casimir_rf_id,
                receiver: cmd.sender(),
                bitrate: rf::BitRate::BitRate106KbitS,
                power_level: RF_POWER_LEVEL_NOMINAL,
                rats_response: DESFIRE_ATS_RESPONSE.to_vec(),
            },
            "select response",
        )
    }

    fn handle_deactivate_notification(
        &mut self,
        cmd: &rf::DeactivateNotification,
    ) -> Option<rf::RfPacket> {
        if self.active_reader == Some(cmd.sender()) {
            self.rx_count = self.rx_count.saturating_add(1);
            self.active_reader = None;
            self.reset_session_state();
        }
        None
    }

    fn handle_select_apdu(&mut self, apdu: &ParsedApdu<'_>) -> Vec<u8> {
        if apdu.p2 != P2_SELECT_FIRST_OR_ONLY && apdu.p2 != P2_SELECT_NO_FCI {
            return SW_INCORRECT_P1P2.to_vec();
        }
        if apdu.p1 == P1_SELECT_BY_DF_NAME {
            if !self.formatted {
                return SW_FILE_NOT_FOUND.to_vec();
            }
            if apdu.data == T4T_V2_APP_ID || apdu.data == T4T_V1_APP_ID {
                self.selected_file = SelectedFile::App;
                SW_OK.to_vec()
            } else if apdu.data == DESFIRE_NDEF_APP_ID {
                self.selected_file = SelectedFile::Ndef;
                let card_state =
                    if self.read_only { CARD_STATE_READ_ONLY } else { CARD_STATE_READ_WRITE };
                let end = self.clamped_e104_end();
                let mut resp = Vec::with_capacity(1 + end + 2);
                resp.push(card_state);
                resp.extend_from_slice(self.e104_file.get(..end).unwrap_or(&[]));
                resp.extend_from_slice(&SW_OK);
                resp
            } else {
                SW_FILE_NOT_FOUND.to_vec()
            }
        } else if apdu.p1 == P1_SELECT_BY_EF_ID {
            if apdu.data.len() != EF_FILE_ID_LEN {
                return SW_WRONG_LENGTH.to_vec();
            }
            if !self.formatted {
                return SW_FILE_NOT_FOUND.to_vec();
            }
            if apdu.data == CC_FILE_ID {
                self.selected_file = SelectedFile::Cc;
                SW_OK.to_vec()
            } else if apdu.data == NDEF_FILE_ID {
                self.selected_file = SelectedFile::Ndef;
                SW_OK.to_vec()
            } else {
                SW_FILE_NOT_FOUND.to_vec()
            }
        } else {
            SW_INCORRECT_P1P2.to_vec()
        }
    }

    fn handle_read_binary_apdu(&mut self, apdu: &ParsedApdu<'_>) -> Vec<u8> {
        if !self.formatted {
            return SW_FILE_NOT_FOUND.to_vec();
        }
        if !apdu.data.is_empty() {
            return SW_WRONG_LENGTH.to_vec();
        }
        let target_file: &[u8] = match self.selected_file {
            SelectedFile::Cc => &self.cc_file,
            SelectedFile::Ndef => &self.e104_file,
            SelectedFile::None | SelectedFile::App => return SW_FILE_NOT_FOUND.to_vec(),
        };
        let offset = usize::from(u16::from_be_bytes([apdu.p1, apdu.p2]));
        if offset >= target_file.len() {
            return SW_WRONG_P1P2.to_vec();
        }
        let req_le = apdu.le.unwrap_or(DEFAULT_READ_BINARY_LE);
        let end = target_file.len().min(offset.saturating_add(req_le));
        let slice = target_file.get(offset..end).unwrap_or(&[]);
        let mut resp = Vec::with_capacity(slice.len() + 2);
        resp.extend_from_slice(slice);
        resp.extend_from_slice(&SW_OK);
        resp
    }

    fn process_apdu(&mut self, parsed: &ParsedApdu<'_>) -> Vec<u8> {
        if parsed.cla == CLA_STANDARD || parsed.cla == CLA_PROPRIETARY {
            match parsed.ins {
                INS_SELECT => self.handle_select_apdu(parsed),
                INS_READ_BINARY => self.handle_read_binary_apdu(parsed),
                _ => SW_INS_NOT_SUPPORTED.to_vec(),
            }
        } else {
            SW_CLA_NOT_SUPPORTED.to_vec()
        }
    }

    fn handle_data_command(&mut self, cmd: &rf::Data) -> Option<rf::RfPacket> {
        if self.active_reader != Some(cmd.sender())
            || cmd.technology() != rf::Technology::NfcA
            || cmd.protocol() != rf::Protocol::IsoDep
        {
            return None;
        }

        self.rx_count = self.rx_count.saturating_add(1);
        let payload = if cmd.data().is_empty() {
            Vec::new()
        } else {
            match parse_apdu(cmd.data()) {
                Ok(ref parsed) => self.process_apdu(parsed),
                Err(sw) => sw.to_vec(),
            }
        };
        self.tx_count = self.tx_count.saturating_add(1);

        encode(
            rf::Data {
                sender: self.casimir_rf_id,
                receiver: cmd.sender(),
                technology: rf::Technology::NfcA,
                protocol: rf::Protocol::IsoDep,
                bitrate: rf::BitRate::BitRate106KbitS,
                power_level: RF_POWER_LEVEL_NOMINAL,
                data: payload,
            },
            "data packet",
        )
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
        let child = match packet.specialize() {
            Ok(c) => c,
            Err(error) => {
                debug!(%error, "failed to specialize RF packet");
                return None;
            }
        };

        match child {
            rf::RfPacketChild::PollCommand(cmd) => self.handle_poll_command(&cmd),
            rf::RfPacketChild::IsoDepT4ATSelectCommand(cmd) => self.handle_select_command(&cmd),
            rf::RfPacketChild::DeactivateNotification(cmd) => {
                self.handle_deactivate_notification(&cmd)
            }
            rf::RfPacketChild::Data(cmd) => self.handle_data_command(&cmd),
            _ => None,
        }
    }

    fn deactivate_active_reader(&mut self, reason: rf::DeactivateReason) -> Option<rf::RfPacket> {
        let reader_id = self.active_reader.take()?;
        self.reset_session_state();
        self.tx_count = self.tx_count.saturating_add(1);
        encode(
            rf::DeactivateNotification {
                sender: self.casimir_rf_id,
                receiver: reader_id,
                technology: rf::Technology::NfcA,
                protocol: rf::Protocol::IsoDep,
                bitrate: rf::BitRate::BitRate106KbitS,
                power_level: RF_POWER_LEVEL_NOMINAL,
                type_: rf::DeactivateType::Discovery,
                reason,
            },
            "deactivate notification",
        )
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
            self.cc_file[CC_WRITE_ACCESS_OFFSET] =
                if read_only { CC_ACCESS_DISABLED } else { CC_ACCESS_GRANTED };
        }
        if let Some(formatted) = patch.formatted {
            let was_formatted = self.formatted;
            self.formatted = formatted;
            if !formatted {
                self.cc_file = Self::build_cc_file(self.read_only);
                self.e104_file.fill(0);
                self.reset_session_state();
            } else if !was_formatted {
                self.cc_file = Self::build_cc_file(self.read_only);
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

    struct T4atWorld {
        tag: T4atTag,
        last_action: Option<rf::RfPacket>,
    }

    impl T4atWorld {
        fn given_formatted_tag(casimir_id: u16, uid_hex: &str, ndef_text: &str) -> Self {
            let mut model = make_test_tag(true, false, ndef_text);
            model.uid = uid_hex.to_string();
            let tag = T4atTag::try_from_tag(&model, casimir_id).expect("valid tag");
            Self { tag, last_action: None }
        }

        fn given_unformatted_tag(casimir_id: u16, uid_hex: &str) -> Self {
            let mut model = make_test_tag(false, false, "");
            model.uid = uid_hex.to_string();
            let tag = T4atTag::try_from_tag(&model, casimir_id).expect("valid tag");
            Self { tag, last_action: None }
        }

        fn given_activated_tag(casimir_id: u16, uid_hex: &str, ndef_text: &str) -> Self {
            let mut world = Self::given_formatted_tag(casimir_id, uid_hex, ndef_text);
            world.when_poll_received(1, rf::Technology::NfcA);
            world.when_rats_received(1, casimir_id);
            world.last_action = None;
            world
        }

        fn given_activated_unformatted_tag(casimir_id: u16, uid_hex: &str) -> Self {
            let mut world = Self::given_unformatted_tag(casimir_id, uid_hex);
            world.when_poll_received(1, rf::Technology::NfcA);
            world.when_rats_received(1, casimir_id);
            world.last_action = None;
            world
        }

        fn given_invisible_tag(casimir_id: u16, uid_hex: &str) -> Self {
            let mut model = make_test_tag(true, false, "Hello");
            model.uid = uid_hex.to_string();
            model.visible = false;
            let tag = T4atTag::try_from_tag(&model, casimir_id).expect("valid tag");
            Self { tag, last_action: None }
        }

        fn when_poll_received(&mut self, sender: u16, tech: rf::Technology) {
            let pkt: rf::RfPacket = rf::PollCommand {
                sender,
                receiver: u16::MAX,
                technology: tech,
                protocol: rf::Protocol::Undetermined,
                bitrate: rf::BitRate::BitRate106KbitS,
                power_level: 10,
                format: rf::PollingFrameFormat::Short,
                payload: vec![],
            }
            .try_into()
            .expect("valid poll pkt");
            self.last_action = self.tag.handle_packet(&pkt);
        }

        fn when_rats_received(&mut self, sender: u16, receiver: u16) {
            let pkt: rf::RfPacket = rf::IsoDepT4ATSelectCommand {
                sender,
                receiver,
                bitrate: rf::BitRate::BitRate106KbitS,
                power_level: 10,
                param: 0x80,
            }
            .try_into()
            .expect("valid rats pkt");
            self.last_action = self.tag.handle_packet(&pkt);
        }

        fn when_data_received(&mut self, sender: u16, receiver: u16, data: &[u8]) {
            let pkt: rf::RfPacket = rf::Data {
                sender,
                receiver,
                technology: rf::Technology::NfcA,
                protocol: rf::Protocol::IsoDep,
                bitrate: rf::BitRate::BitRate106KbitS,
                power_level: 10,
                data: data.to_vec(),
            }
            .try_into()
            .expect("valid data pkt");
            self.last_action = self.tag.handle_packet(&pkt);
        }

        fn when_deactivate_received(&mut self, sender: u16, receiver: u16) {
            let pkt: rf::RfPacket = rf::DeactivateNotification {
                sender,
                receiver,
                technology: rf::Technology::NfcA,
                protocol: rf::Protocol::IsoDep,
                bitrate: rf::BitRate::BitRate106KbitS,
                power_level: 10,
                type_: rf::DeactivateType::Discovery,
                reason: rf::DeactivateReason::EndpointRequest,
            }
            .try_into()
            .expect("valid deactivate pkt");
            self.last_action = self.tag.handle_packet(&pkt);
        }

        fn then_poll_response_matches(
            &self,
            expected_sender: u16,
            expected_receiver: u16,
            expected_bit_frame_sdd: u8,
            expected_nfcid1: &[u8],
        ) {
            match &self.last_action {
                Some(pkt) => match pkt.specialize().expect("specializes to child") {
                    rf::RfPacketChild::NfcAPollResponse(resp) => {
                        assert_eq!(resp.sender(), expected_sender);
                        assert_eq!(resp.receiver(), expected_receiver);
                        assert_eq!(resp.protocol(), rf::Protocol::IsoDep);
                        assert_eq!(resp.int_protocol(), 0b01);
                        assert_eq!(resp.bit_frame_sdd(), expected_bit_frame_sdd);
                        assert_eq!(resp.nfcid1(), expected_nfcid1);
                    }
                    other => panic!("expected NfcAPollResponse, got {other:?}"),
                },
                None => panic!("expected poll response, got None"),
            }
        }

        fn then_ats_response_matches(
            &self,
            expected_sender: u16,
            expected_receiver: u16,
            expected_ats: &[u8],
        ) {
            match &self.last_action {
                Some(pkt) => match pkt.specialize().expect("specializes to child") {
                    rf::RfPacketChild::IsoDepT4ATSelectResponse(resp) => {
                        assert_eq!(resp.sender(), expected_sender);
                        assert_eq!(resp.receiver(), expected_receiver);
                        assert_eq!(resp.rats_response(), expected_ats);
                    }
                    other => panic!("expected IsoDepT4ATSelectResponse, got {other:?}"),
                },
                None => panic!("expected IsoDepT4ATSelectResponse, got None"),
            }
        }

        fn then_action_is_none(&self) {
            assert_eq!(self.last_action, None);
        }

        fn then_data_matches(&self, expected: &[u8]) {
            match &self.last_action {
                Some(pkt) => match pkt.specialize().expect("specializes to child") {
                    rf::RfPacketChild::Data(data) => {
                        assert_eq!(data.data(), expected);
                    }
                    other => panic!("expected rf::Data, got {other:?}"),
                },
                None => panic!("expected rf::Data, got None"),
            }
        }

        fn then_data_length_is(&self, expected_len: usize) {
            match &self.last_action {
                Some(pkt) => match pkt.specialize().expect("specializes to child") {
                    rf::RfPacketChild::Data(data) => {
                        assert_eq!(data.data().len(), expected_len);
                    }
                    other => panic!("expected rf::Data, got {other:?}"),
                },
                None => panic!("expected rf::Data, got None"),
            }
        }

        fn then_data_ends_with(&self, expected_suffix: &[u8]) {
            match &self.last_action {
                Some(pkt) => match pkt.specialize().expect("specializes to child") {
                    rf::RfPacketChild::Data(data) => {
                        assert!(
                            data.data().ends_with(expected_suffix),
                            "expected data to end with {expected_suffix:?}, got {:?}",
                            data.data()
                        );
                    }
                    other => panic!("expected rf::Data, got {other:?}"),
                },
                None => panic!("expected rf::Data, got None"),
            }
        }

        fn then_active_reader_is(&self, expected: Option<u16>) {
            assert_eq!(self.tag.active_reader(), expected);
        }
    }

    const CMD_SELECT_AID_V2: [u8; 13] =
        [0x00, 0xA4, 0x04, 0x00, 0x07, 0xD2, 0x76, 0x00, 0x00, 0x85, 0x01, 0x01, 0x00];
    const CMD_SELECT_AID_V1: [u8; 12] =
        [0x00, 0xA4, 0x04, 0x00, 0x07, 0xD2, 0x76, 0x00, 0x00, 0x85, 0x01, 0x00];
    const CMD_SELECT_AID_LEGACY: [u8; 10] =
        [0x00, 0xA4, 0x04, 0x00, 0x05, 0xF0, 0x4E, 0x44, 0x45, 0x46];
    const CMD_SELECT_CC: [u8; 7] = [0x00, 0xA4, 0x00, 0x00, 0x02, 0xE1, 0x03];
    const CMD_SELECT_NDEF: [u8; 7] = [0x00, 0xA4, 0x00, 0x00, 0x02, 0xE1, 0x04];
    const CMD_READ_CC_ALL: [u8; 5] = [0x00, 0xB0, 0x00, 0x00, 0x0F];
    const CMD_READ_NLEN: [u8; 5] = [0x00, 0xB0, 0x00, 0x00, 0x02];

    #[test]
    fn poll_and_rats_activation_on_7_byte_uid_returns_iso_dep_and_desfire_ats() {
        let mut world = T4atWorld::given_formatted_tag(100, "04010203040506", "Hello");
        world.when_poll_received(1, rf::Technology::NfcA);
        world.then_poll_response_matches(100, 1, 0x04, &[0x04, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06]);
        world.when_rats_received(1, 100);
        world.then_ats_response_matches(100, 1, &[0x75, 0x77, 0x81, 0x02, 0x80]);
    }

    #[test]
    fn poll_reports_bit_frame_sdd_0x08_for_4_byte_uid_and_0x04_for_10_byte_uid() {
        let mut world4 = T4atWorld::given_formatted_tag(101, "01020304", "Hello");
        let mut world10 = T4atWorld::given_formatted_tag(102, "0102030405060708090A", "Hello");
        world4.when_poll_received(1, rf::Technology::NfcA);
        world4.then_poll_response_matches(101, 1, 0x08, &[0x01, 0x02, 0x03, 0x04]);
        world10.when_poll_received(1, rf::Technology::NfcA);
        world10.then_poll_response_matches(
            102,
            1,
            0x04,
            &[0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A],
        );
    }

    #[test]
    fn non_nfca_poll_command_is_ignored() {
        let mut world = T4atWorld::given_formatted_tag(103, "04010203040506", "Hello");
        world.when_poll_received(2, rf::Technology::NfcB);
        world.then_action_is_none();
    }

    #[test]
    fn invisible_tag_ignores_poll_command() {
        let mut world = T4atWorld::given_invisible_tag(104, "04010203040506");
        world.when_poll_received(1, rf::Technology::NfcA);
        world.then_action_is_none();
    }

    #[test]
    fn active_reader_arbitration_ignores_competing_reader_until_deactivation() {
        let mut world = T4atWorld::given_formatted_tag(50, "04010203040506", "Arbitration");
        world.when_poll_received(1, rf::Technology::NfcA);
        world.when_rats_received(1, 50);
        world.then_active_reader_is(Some(1));

        world.when_poll_received(2, rf::Technology::NfcA);
        world.then_action_is_none();
        world.when_rats_received(2, 50);
        world.then_action_is_none();
        world.when_data_received(2, 50, &[0x00, 0xA4, 0x04, 0x00]);
        world.then_action_is_none();
        world.then_active_reader_is(Some(1));

        world.when_deactivate_received(1, 50);
        world.then_active_reader_is(None);

        world.when_poll_received(2, rf::Technology::NfcA);
        world.when_rats_received(2, 50);
        world.then_active_reader_is(Some(2));
    }

    #[test]
    fn empty_data_presence_check_is_echoed_by_active_reader() {
        let mut world = T4atWorld::given_formatted_tag(51, "04010203040506", "Echo");
        world.when_poll_received(1, rf::Technology::NfcA);
        world.when_rats_received(1, 51);
        world.when_data_received(1, 51, &[]);
        world.then_data_matches(&[]);
    }

    #[test]
    fn select_aid_v2_v1_and_legacy_verify_alias() {
        let mut world = T4atWorld::given_activated_tag(20, "04010203040506", "Payload");

        world.when_data_received(1, 20, &CMD_SELECT_AID_V2);
        world.then_data_matches(&[0x90, 0x00]);

        world.when_data_received(1, 20, &CMD_SELECT_AID_V1);
        world.then_data_matches(&[0x90, 0x00]);

        world.when_data_received(1, 20, &CMD_SELECT_AID_LEGACY);
        world.then_data_matches(&[
            0x02, 0x00, 0x0E, 0xD1, 0x01, 0x0A, 0x54, 0x02, 0x65, 0x6E, 0x50, 0x61, 0x79, 0x6C,
            0x6F, 0x61, 0x64, 0x90, 0x00,
        ]);
    }

    #[test]
    fn select_unknown_aid_returns_6a82() {
        let mut world = T4atWorld::given_activated_tag(21, "04010203040506", "Payload");

        let unknown = [0x00, 0xA4, 0x04, 0x00, 0x07, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77];
        world.when_data_received(1, 21, &unknown);
        world.then_data_matches(&[0x6A, 0x82]);
    }

    #[test]
    fn read_cc_and_ndef_with_case1_short_and_le_zero_slicing() {
        let mut world = T4atWorld::given_activated_tag(30, "04010203040506", "Hello World");

        world.when_data_received(1, 30, &CMD_SELECT_CC);
        world.then_data_matches(&[0x90, 0x00]);

        world.when_data_received(1, 30, &CMD_READ_CC_ALL);
        world.then_data_matches(&[
            0x00, 0x0F, 0x20, 0x00, 0xFF, 0x00, 0xFF, 0x04, 0x06, 0xE1, 0x04, 0x10, 0x00, 0x00,
            0x00, 0x90, 0x00,
        ]);

        world.when_data_received(1, 30, &CMD_SELECT_NDEF);
        world.then_data_matches(&[0x90, 0x00]);

        world.when_data_received(1, 30, &CMD_READ_NLEN);
        world.then_data_matches(&[0x00, 0x12, 0x90, 0x00]);

        world.when_data_received(1, 30, &[0x00, 0xB0, 0x00, 0x00]);
        world.then_data_length_is(258);
        world.then_data_ends_with(&[0x90, 0x00]);

        world.when_data_received(1, 30, &[0x00, 0xB0, 0x00, 0x00, 0x00]);
        world.then_data_length_is(258);
        world.then_data_ends_with(&[0x90, 0x00]);

        world.when_data_received(1, 30, &[0x00, 0xB0, 0x00, 0x02, 0x05]);
        world.then_data_matches(&[0xD1, 0x01, 0x0E, 0x54, 0x02, 0x90, 0x00]);
    }

    #[test]
    fn unformatted_tag_rejects_select_and_read_binary_with_6a82() {
        let mut world = T4atWorld::given_activated_unformatted_tag(35, "04010203040506");

        world.when_data_received(1, 35, &CMD_SELECT_AID_V2);
        world.then_data_matches(&[0x6A, 0x82]);

        world.when_data_received(1, 35, &CMD_SELECT_NDEF);
        world.then_data_matches(&[0x6A, 0x82]);

        world.when_data_received(1, 35, &CMD_READ_NLEN);
        world.then_data_matches(&[0x6A, 0x82]);
    }

    #[test]
    fn invalid_p1_on_select_returns_6a86() {
        let mut world = T4atWorld::given_activated_tag(91, "04010203040506", "Err");
        world.when_data_received(1, 91, &[0x00, 0xA4, 0x02, 0x00, 0x02, 0xE1, 0x04]);
        world.then_data_matches(&[0x6A, 0x86]);
    }

    #[test]
    fn invalid_p2_on_select_aid_and_select_file_returns_6a86() {
        let mut world = T4atWorld::given_activated_tag(92, "04010203040506", "Err");

        let bad_p2_aid =
            [0x00, 0xA4, 0x04, 0x01, 0x07, 0xD2, 0x76, 0x00, 0x00, 0x85, 0x01, 0x01, 0x00];
        world.when_data_received(1, 92, &bad_p2_aid);
        world.then_data_matches(&[0x6A, 0x86]);

        world.when_data_received(1, 92, &[0x00, 0xA4, 0x00, 0x01, 0x02, 0xE1, 0x04]);
        world.then_data_matches(&[0x6A, 0x86]);
    }

    #[test]
    fn select_file_with_invalid_length_returns_6700() {
        let mut world = T4atWorld::given_activated_unformatted_tag(93, "04010203040506");
        world.when_data_received(1, 93, &[0x00, 0xA4, 0x00, 0x00, 0x01, 0xE1]);
        world.then_data_matches(&[0x67, 0x00]);
    }

    #[test]
    fn select_unknown_file_returns_6a82() {
        let mut world = T4atWorld::given_activated_tag(94, "04010203040506", "Err");
        world.when_data_received(1, 94, &[0x00, 0xA4, 0x00, 0x00, 0x02, 0xE1, 0x05]);
        world.then_data_matches(&[0x6A, 0x82]);
    }

    #[test]
    fn read_binary_with_non_empty_payload_returns_6700() {
        let mut world = T4atWorld::given_activated_tag(95, "04010203040506", "Err");
        world.when_data_received(1, 95, &CMD_SELECT_CC);
        world.then_data_matches(&[0x90, 0x00]);

        world.when_data_received(1, 95, &[0x00, 0xB0, 0x00, 0x00, 0x02, 0xAA, 0xBB]);
        world.then_data_matches(&[0x67, 0x00]);
    }

    #[test]
    fn read_binary_with_out_of_bounds_offset_returns_6b00() {
        let mut world = T4atWorld::given_activated_tag(96, "04010203040506", "Err");
        world.when_data_received(1, 96, &CMD_SELECT_CC);
        world.then_data_matches(&[0x90, 0x00]);

        world.when_data_received(1, 96, &[0x00, 0xB0, 0x13, 0x88, 0x05]);
        world.then_data_matches(&[0x6B, 0x00]);
    }

    #[test]
    fn unsupported_ins_returns_6d00() {
        let mut world = T4atWorld::given_activated_tag(97, "04010203040506", "Err");
        world.when_data_received(1, 97, &[0x00, 0xEE, 0x00, 0x00]);
        world.then_data_matches(&[0x6D, 0x00]);
    }

    #[test]
    fn unsupported_cla_returns_6e00() {
        let mut world = T4atWorld::given_activated_tag(98, "04010203040506", "Err");
        world.when_data_received(1, 98, &[0x55, 0x11, 0x00, 0x00]);
        world.then_data_matches(&[0x6E, 0x00]);
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
