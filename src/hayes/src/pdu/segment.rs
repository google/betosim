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

//! 3GPP TS 23.040 § 9.2.3.24 TP-User-Data framing and text message segmentation.

use super::{DeliverPdu, EncodeError};
use crate::pdu::charset::{
    Alphabet, DataCodingScheme, encode_gsm7_septets, encode_ucs2_bytes, pack_septets_with_fill,
};

/// 3GPP TS 23.040 § 9.2.3.24.1: Concatenation header for one part of a multi-part message
/// (8-bit reference number).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ConcatHeader {
    reference: u8,
    total: u8,
    sequence: u8,
}

impl ConcatHeader {
    /// UDH Length (5 octets follow the UDHL byte).
    const UDH_LEN: u8 = (super::charset::CONCAT_UDH_OCTETS - 1) as u8;
    /// Information Element Identifier `0x00`: concatenated short messages, 8-bit reference number (3GPP TS 23.040 § 9.2.3.24.1).
    const IEI_CONCAT_8BIT: u8 = 0x00;
    /// Information Element Data Length (3 octets: reference, total parts, sequence number).
    const IEDL_CONCAT_8BIT: u8 = 0x03;
    /// Fill bits required to align 7-bit septets to the 7-septet (49-bit) boundary after a 48-bit UDH.
    const GSM7_FILL_BITS: u8 =
        (super::charset::CONCAT_UDH_SEPTETS * 7 - super::charset::CONCAT_UDH_OCTETS * 8) as u8;

    fn new(reference: u8, total: u8, sequence: u8) -> Self {
        Self { reference, total, sequence }
    }

    fn to_bytes(self) -> [u8; super::charset::CONCAT_UDH_OCTETS] {
        [
            Self::UDH_LEN,
            Self::IEI_CONCAT_8BIT,
            Self::IEDL_CONCAT_8BIT,
            self.reference,
            self.total,
            self.sequence,
        ]
    }
}

/// Encoded user data payload of an SMS TPDU.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct EncodedUserData {
    dcs: DataCodingScheme,
    has_udh: bool,
    udl: u8,
    payload: Vec<u8>,
}

impl EncodedUserData {
    /// Encodes unsegmented plain text into a TP-User-Data field using the provided alphabet.
    fn encode_unsegmented(text: &str, alphabet: Alphabet) -> Self {
        match alphabet {
            Alphabet::Gsm7 => {
                let septets = encode_gsm7_septets(text).unwrap_or_default();
                let udl = septets.len() as u8;
                let payload = pack_septets_with_fill(&septets, 0);
                Self { dcs: alphabet.dcs(), has_udh: false, udl, payload }
            }
            Alphabet::Ucs2 => {
                let payload = encode_ucs2_bytes(text);
                let udl = payload.len() as u8;
                Self { dcs: alphabet.dcs(), has_udh: false, udl, payload }
            }
        }
    }

    /// Encodes one segment of a concatenated message with a User Data Header.
    fn encode_concat_segment(text: &str, alphabet: Alphabet, header: ConcatHeader) -> Self {
        let header_bytes = header.to_bytes();
        match alphabet {
            Alphabet::Gsm7 => {
                let septets = encode_gsm7_septets(text).unwrap_or_default();
                let udl = (super::charset::CONCAT_UDH_SEPTETS + septets.len()) as u8;
                let mut payload = Vec::with_capacity(
                    header_bytes.len()
                        + (septets.len() * 7 + ConcatHeader::GSM7_FILL_BITS as usize).div_ceil(8),
                );
                payload.extend_from_slice(&header_bytes);
                payload.extend(pack_septets_with_fill(&septets, ConcatHeader::GSM7_FILL_BITS));
                Self { dcs: alphabet.dcs(), has_udh: true, udl, payload }
            }
            Alphabet::Ucs2 => {
                let text_bytes = encode_ucs2_bytes(text);
                let mut payload = Vec::with_capacity(header_bytes.len() + text_bytes.len());
                payload.extend_from_slice(&header_bytes);
                payload.extend(text_bytes);
                let udl = payload.len() as u8;
                Self { dcs: alphabet.dcs(), has_udh: true, udl, payload }
            }
        }
    }

    pub(super) fn dcs(&self) -> DataCodingScheme {
        self.dcs
    }

    pub(super) fn has_udh(&self) -> bool {
        self.has_udh
    }

    pub(super) fn udl(&self) -> u8 {
        self.udl
    }

    pub(super) fn payload(&self) -> &[u8] {
        &self.payload
    }
}

/// A high-level plain-text SMS message prior to PDU encoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextMessage<'a> {
    text: &'a str,
    alphabet: Alphabet,
}

impl<'a> TextMessage<'a> {
    pub fn new(text: &'a str) -> Self {
        let alphabet = Alphabet::classify(text);
        Self { text, alphabet }
    }

    /// Validates that this message fits in a single unsegmented SMS (160 GSM-7
    /// septets or 70 UCS-2 code units).
    pub fn validate_single_part(self) -> Result<(), EncodeError> {
        if self.alphabet.measure(self.text) > self.alphabet.max_unsegmented_capacity() {
            Err(EncodeError::InvalidFormat)
        } else {
            Ok(())
        }
    }

    /// Splits `text` into segment slices of at most `max_concat_segment_capacity()`
    /// (153 GSM-7 septets or 67 UCS-2 code units), never splitting escape sequences
    /// or UTF-16 surrogate pairs.
    pub(crate) fn split_segments(self) -> Result<Vec<&'a str>, EncodeError> {
        if self.alphabet.measure(self.text) <= self.alphabet.max_unsegmented_capacity() {
            return Ok(vec![self.text]);
        }
        let max_capacity = self.alphabet.max_concat_segment_capacity();
        let mut segments = Vec::new();
        let mut start = 0;
        let mut accumulated_cost = 0;

        for (idx, ch) in self.text.char_indices() {
            let cost = self.alphabet.char_cost(ch);
            if accumulated_cost + cost > max_capacity {
                if let Some(segment) = self.text.get(start..idx) {
                    segments.push(segment);
                }
                start = idx;
                accumulated_cost = cost;
            } else {
                accumulated_cost += cost;
            }
        }
        if let Some(segment) = self.text.get(start..) {
            segments.push(segment);
        }
        u8::try_from(segments.len()).map_err(|_err| EncodeError::InvalidFormat)?;
        Ok(segments)
    }

    /// Encodes this message into one or more SMS-DELIVER PDUs.
    pub(super) fn to_deliver_pdus<F>(
        self,
        sender: &str,
        scts_bcd: [u8; 7],
        next_reference: F,
    ) -> Result<Vec<DeliverPdu>, EncodeError>
    where
        F: FnOnce() -> u8,
    {
        let segments = self.split_segments()?;
        if segments.len() <= 1 {
            let user_data = EncodedUserData::encode_unsegmented(self.text, self.alphabet);
            return Ok(vec![DeliverPdu::new(sender.to_string(), scts_bcd, user_data)]);
        }

        let total = segments.len() as u8;
        let reference = next_reference();
        let mut pdus = Vec::with_capacity(segments.len());

        for (sequence, segment) in (1..=total).zip(segments) {
            let header = ConcatHeader::new(reference, total, sequence);
            let user_data = EncodedUserData::encode_concat_segment(segment, self.alphabet, header);
            pdus.push(DeliverPdu::new(sender.to_string(), scts_bcd, user_data));
        }

        Ok(pdus)
    }
}

#[cfg(test)]
mod tests {
    use super::{super::EncodeError, *};

    #[test]
    fn test_encode_unsegmented_gsm7() {
        let user_data = EncodedUserData::encode_unsegmented("hello", Alphabet::Gsm7);
        assert!(!user_data.has_udh());
        assert_eq!(user_data.dcs(), DataCodingScheme::GSM_7BIT_DEFAULT);
        assert_eq!(user_data.udl(), 5);
        assert_eq!(hex::encode_upper(user_data.payload()), "E8329BFD06");
    }

    #[test]
    fn test_encode_unsegmented_ucs2() {
        let user_data = EncodedUserData::encode_unsegmented("👋", Alphabet::Ucs2);
        assert!(!user_data.has_udh());
        assert_eq!(user_data.dcs(), DataCodingScheme::UCS2);
        assert_eq!(user_data.udl(), 4);
        assert_eq!(hex::encode_upper(user_data.payload()), "D83DDC4B");
    }

    #[test]
    fn test_text_message_validate_single_part_gsm7() {
        let exact_160 = "A".repeat(160);
        let msg_ok = TextMessage::new(&exact_160);
        assert!(msg_ok.validate_single_part().is_ok());

        let exceed_161 = "A".repeat(161);
        let msg_err = TextMessage::new(&exceed_161);
        assert_eq!(msg_err.validate_single_part(), Err(EncodeError::InvalidFormat));

        // 80 extension chars = 160 septets
        let ext_80 = "{".repeat(80);
        assert!(TextMessage::new(&ext_80).validate_single_part().is_ok());
        let ext_81 = "{".repeat(81);
        assert_eq!(
            TextMessage::new(&ext_81).validate_single_part(),
            Err(EncodeError::InvalidFormat)
        );
    }

    #[test]
    fn test_text_message_validate_single_part_ucs2() {
        // 70 code units: 35 surrogate pairs (each pair = 2 code units)
        let exact_35_emoji = "🚀".repeat(35);
        assert_eq!(exact_35_emoji.encode_utf16().count(), 70);
        assert!(TextMessage::new(&exact_35_emoji).validate_single_part().is_ok());

        let exceed_36_emoji = "🚀".repeat(36);
        assert_eq!(exceed_36_emoji.encode_utf16().count(), 72);
        assert_eq!(
            TextMessage::new(&exceed_36_emoji).validate_single_part(),
            Err(EncodeError::InvalidFormat)
        );
    }

    #[test]
    fn test_split_segments_empty_and_single_part() {
        assert_eq!(TextMessage::new("").split_segments(), Ok(vec![""]));
        assert_eq!(TextMessage::new("hello").split_segments(), Ok(vec!["hello"]));

        let exact_160 = "A".repeat(160);
        assert_eq!(TextMessage::new(&exact_160).split_segments(), Ok(vec![exact_160.as_str()]));

        let exact_70_ucs2 = "你".repeat(70);
        assert_eq!(
            TextMessage::new(&exact_70_ucs2).split_segments(),
            Ok(vec![exact_70_ucs2.as_str()])
        );
    }

    #[test]
    fn test_split_segments_gsm7_boundaries() {
        // 160 vs 161 boundary
        let gsm_160 = "A".repeat(160);
        assert_eq!(TextMessage::new(&gsm_160).split_segments().unwrap().len(), 1);

        let gsm_161 = "A".repeat(161);
        let parts_161 = TextMessage::new(&gsm_161).split_segments().unwrap();
        assert_eq!(parts_161, vec![&"A".repeat(153), &"A".repeat(8)]);

        // 306 vs 307 boundary (153 * 2 = 306)
        let gsm_306 = "A".repeat(306);
        let parts_306 = TextMessage::new(&gsm_306).split_segments().unwrap();
        assert_eq!(parts_306, vec![&"A".repeat(153), &"A".repeat(153)]);

        let gsm_307 = "A".repeat(307);
        let parts_307 = TextMessage::new(&gsm_307).split_segments().unwrap();
        assert_eq!(parts_307, vec![&"A".repeat(153), &"A".repeat(153), "A"]);
    }

    #[test]
    fn test_split_segments_gsm7_extension_char_boundary() {
        // 152 basic chars + '{' (costs 2 septets)
        // 152 + 2 = 154 > 153, so '{' must not be split across segments
        let mut text = "A".repeat(152);
        text.push('{');
        text.push_str(&"B".repeat(10));

        let parts = TextMessage::new(&text).split_segments().unwrap();
        assert_eq!(parts.len(), 2);
        assert_eq!(parts.first().copied(), Some("A".repeat(152).as_str()));
        assert!(parts.get(1).copied().unwrap_or("").starts_with('{'));
    }

    #[test]
    fn test_split_segments_ucs2_boundaries() {
        // 70 vs 71 boundary
        let ucs2_70 = "你".repeat(70);
        assert_eq!(TextMessage::new(&ucs2_70).split_segments().unwrap().len(), 1);

        let ucs2_71 = "你".repeat(71);
        let parts_71 = TextMessage::new(&ucs2_71).split_segments().unwrap();
        assert_eq!(parts_71, vec![&"你".repeat(67), &"你".repeat(4)]);

        // 134 vs 135 boundary (67 * 2 = 134)
        let ucs2_134 = "你".repeat(134);
        let parts_134 = TextMessage::new(&ucs2_134).split_segments().unwrap();
        assert_eq!(parts_134, vec![&"你".repeat(67), &"你".repeat(67)]);

        let ucs2_135 = "你".repeat(135);
        let parts_135 = TextMessage::new(&ucs2_135).split_segments().unwrap();
        assert_eq!(parts_135, vec![&"你".repeat(67), &"你".repeat(67), "你"]);
    }

    #[test]
    fn test_split_segments_surrogate_pair_boundary() {
        // 66 single-code-unit UCS-2 chars + '🚀' (costs 2 code units)
        // 66 + 2 = 68 > 67, so '🚀' must start segment 2
        let mut text = "你".repeat(66);
        text.push('🚀');
        text.push_str(&"你".repeat(5));

        let parts = TextMessage::new(&text).split_segments().unwrap();
        assert_eq!(parts.len(), 2);
        assert_eq!(parts.first().copied(), Some("你".repeat(66).as_str()));
        assert!(parts.get(1).copied().unwrap_or("").starts_with('🚀'));
    }

    #[test]
    fn test_split_segments_too_many_parts() {
        // 255 parts of 153 chars fits
        let text_max = "A".repeat(255 * 153);
        let parts = TextMessage::new(&text_max).split_segments().unwrap();
        assert_eq!(parts.len(), 255);

        // 255 * 153 + 1 requires 256 parts -> exceeds u8::MAX (255)
        let text_over = "A".repeat(255 * 153 + 1);
        assert_eq!(TextMessage::new(&text_over).split_segments(), Err(EncodeError::InvalidFormat));
    }

    #[test]
    fn test_encode_concat_segment_gsm7_and_ucs2() {
        let header = ConcatHeader::new(0x2A, 2, 1);
        let segment_gsm7 = EncodedUserData::encode_concat_segment("A", Alphabet::Gsm7, header);
        assert!(segment_gsm7.has_udh());
        assert_eq!(segment_gsm7.dcs(), DataCodingScheme::GSM_7BIT_DEFAULT);
        // 7 septets of header + 1 septet of text = 8 septets UDL
        assert_eq!(segment_gsm7.udl(), 8);
        // Header: 05 00 03 2A 02 01, 'A' with fill_bits=1: 0x41 << 1 = 0x82
        assert_eq!(hex::encode_upper(segment_gsm7.payload()), "0500032A020182");

        let segment_ucs2 = EncodedUserData::encode_concat_segment("👋", Alphabet::Ucs2, header);
        assert!(segment_ucs2.has_udh());
        assert_eq!(segment_ucs2.dcs(), DataCodingScheme::UCS2);
        // 6 octets header + 4 octets UTF-16 = 10 octets UDL
        assert_eq!(segment_ucs2.udl(), 10);
        assert_eq!(hex::encode_upper(segment_ucs2.payload()), "0500032A0201D83DDC4B");
    }

    #[test]
    fn test_concat_header_to_bytes() {
        let header = ConcatHeader::new(0x42, 5, 3);
        assert_eq!(header.to_bytes(), [0x05, 0x00, 0x03, 0x42, 0x05, 0x03]);
    }
}
