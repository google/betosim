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

//! APDU parsing helpers for ISO/IEC 7816-4.

use std::{borrow::Cow, fmt};

use crate::constants::SW_WRONG_LENGTH;

/// Distinguishes the four standard APDU command-response cases defined in
/// ISO/IEC 7816-4 § 5.1.
#[derive(Debug, PartialEq, Clone)]
pub enum ApduCase<'a> {
    /// Case 1 (ISO/IEC 7816-4 § 5.1): 4-byte header only. No command data sent,
    /// no response data expected.
    CommandOnly,
    /// Case 2 (ISO/IEC 7816-4 § 5.1): Header + Le. No command data sent,
    /// response data of length `expected_length` expected.
    ResponseExpected { expected_length: u8 },
    /// Case 3 (ISO/IEC 7816-4 § 5.1): Header + Lc + Data. Command data payload
    /// sent, no response data expected.
    DataOnly { data: Cow<'a, [u8]> },
    /// Case 4 (ISO/IEC 7816-4 § 5.1): Header + Lc + Data + Le. Command data
    /// sent and response data expected.
    DataAndResponse { data: Cow<'a, [u8]>, expected_length: u8 },
}

/// Represents an ISO/IEC 7816-4 / ETSI TS 102 221 Class (CLA) byte.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub struct Class(pub u8);

impl Class {
    /// Checks whether this class byte is supported according to ETSI TS 102 221
    /// Format 1 and Format 2.
    ///
    /// - Format 1: b7 = 0 (`0x00..=0x3F` or `0x80..=0xBF`).
    /// - Format 2 (Extended Logical Channels): b7 = 1, b6 = 0, b5 = 0
    ///   (`0x40..=0x4F` or `0xC0..=0xCF`).
    pub fn is_supported(self) -> bool {
        let class_bits = self.0 & 0x70;
        class_bits == 0 || class_bits == 0x40
    }

    /// Returns whether this class byte uses ETSI TS 102 221 Format 1.
    pub fn is_format_1(self) -> bool {
        (self.0 & 0x40) == 0
    }

    /// Returns whether this class byte uses ETSI TS 102 221 Format 2.
    pub fn is_format_2(self) -> bool {
        !self.is_format_1()
    }

    /// Extracts the logical channel index from the class byte.
    ///
    /// Supports:
    /// - Format 1: channel in bits 2-1 (0-3).
    /// - Format 2: channel in bits 4-1 (4-19, mapped from value 0-15 + 4).
    pub fn channel(self) -> usize {
        if self.is_format_1() {
            // Format 1: Bits 2-1 encode channel 0-3
            (self.0 & 0x03) as usize
        } else {
            // Format 2: Bits 4-1 encode channel 4-19 (value + 4)
            ((self.0 & 0x0F) + 4) as usize
        }
    }

    /// Normalizes the class byte to channel 0 for XML profile lookups.
    pub fn normalized(self) -> Self {
        if self.is_format_1() {
            // Format 1: Clear channel bits (b2-b1)
            Self(self.0 & 0xFC)
        } else {
            // Format 2: Clear b7 (making it Format 1) and channel bits (b4-b1)
            Self(self.0 & 0xB0)
        }
    }

    /// Heuristic to check if this class byte is likely present in XML profile
    /// commands.
    ///
    /// Returns true if the byte is a Format 1 CLA for channels 0-3 (supported
    /// by the simulator) or 0xFF (used in tests for unsupported classes).
    pub fn is_likely_mapped_cla(self) -> bool {
        matches!(self.0, 0x00..=0x03 | 0x80..=0x83 | 0xFF)
    }
}

#[cfg(test)]
impl PartialEq<u8> for Class {
    fn eq(&self, other: &u8) -> bool {
        self.0 == *other
    }
}

#[cfg(test)]
impl PartialEq<Class> for u8 {
    fn eq(&self, other: &Class) -> bool {
        *self == other.0
    }
}
/// APDU Instructions as defined in ISO/IEC 7816-4 & ETSI TS 102 221.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Instruction {
    ReadBinary,
    ReadRecord,
    UpdateRecord,
    Select,
    UpdateBinary,
    Status,
    ManageChannel,
    GetResponse,
    Unknown(u8),
}

impl Instruction {
    pub const fn is_update(self) -> bool {
        matches!(self, Self::UpdateBinary | Self::UpdateRecord)
    }

    pub const fn as_u8(self) -> u8 {
        match self {
            Self::ReadBinary => 0xB0,
            Self::ReadRecord => 0xB2,
            Self::UpdateRecord => 0xDC,
            Self::Select => 0xA4,
            Self::UpdateBinary => 0xD6,
            Self::Status => 0xF2,
            Self::ManageChannel => 0x70,
            Self::GetResponse => 0xC0,
            Self::Unknown(val) => val,
        }
    }
}

impl From<Instruction> for u8 {
    fn from(ins: Instruction) -> Self {
        ins.as_u8()
    }
}

impl fmt::Display for Instruction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_u8())
    }
}

impl fmt::UpperHex for Instruction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::UpperHex::fmt(&self.as_u8(), f)
    }
}

impl fmt::LowerHex for Instruction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::LowerHex::fmt(&self.as_u8(), f)
    }
}

impl From<u8> for Instruction {
    fn from(val: u8) -> Self {
        match val {
            0xB0 => Self::ReadBinary,
            0xB2 => Self::ReadRecord,
            0xDC => Self::UpdateRecord,
            0xA4 => Self::Select,
            0xD6 => Self::UpdateBinary,
            0xF2 => Self::Status,
            0x70 => Self::ManageChannel,
            0xC0 => Self::GetResponse,
            other => Self::Unknown(other),
        }
    }
}

/// SELECT command P1 parameters as defined in ISO/IEC 7816-4 § 6.11.1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(clippy::enum_variant_names)]
#[repr(u8)]
pub enum SelectP1 {
    ByFid = 0x00,
    ByDfName = 0x04,
    ByPathFromMf = 0x08,
}

impl TryFrom<u8> for SelectP1 {
    type Error = ();

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0x00 => Ok(Self::ByFid),
            0x04 => Ok(Self::ByDfName),
            0x08 => Ok(Self::ByPathFromMf),
            _ => Err(()),
        }
    }
}

/// SELECT command P2 parameters (FCI template control) as defined in ISO/IEC
/// 7816-4 Table 40.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum SelectP2 {
    ReturnFcp = 0x00,
    DoNotReturnFcp = 0x04,
    ReturnProprietary = 0x08, // Unsupported by simulator
    ReturnFcpNoProprietary = 0x0C,
}

impl TryFrom<u8> for SelectP2 {
    type Error = ();
    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0x00 => Ok(Self::ReturnFcp),
            0x04 => Ok(Self::DoNotReturnFcp),
            0x08 => Ok(Self::ReturnProprietary),
            0x0C => Ok(Self::ReturnFcpNoProprietary),
            _ => Err(()),
        }
    }
}

/// READ/UPDATE RECORD command P2 parameters (Mode) as defined in ETSI TS 102
/// 221 Table 11.5.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum RecordMode {
    NextRecord = 0x02,     // Unsupported by simulator
    PreviousRecord = 0x03, // Unsupported by simulator
    AbsoluteMode = 0x04,
}

impl RecordMode {
    pub const fn is_absolute(self) -> bool {
        matches!(self, Self::AbsoluteMode)
    }
}

impl TryFrom<u8> for RecordMode {
    type Error = ();
    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value & 0x07 {
            0x02 => Ok(Self::NextRecord),
            0x03 => Ok(Self::PreviousRecord),
            0x04 => Ok(Self::AbsoluteMode),
            _ => Err(()),
        }
    }
}

/// MANAGE CHANNEL command P1 parameters (Action) as defined in ISO/IEC 7816-4 §
/// 6.13.1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ManageChannelAction {
    Open = 0x00,
    Close = 0x80,
}

impl TryFrom<u8> for ManageChannelAction {
    type Error = ();
    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0x00 => Ok(Self::Open),
            0x80 => Ok(Self::Close),
            _ => Err(()),
        }
    }
}

/// Parsed APDU components according to ISO/IEC 7816-4.
#[derive(Debug, PartialEq, Clone)]
pub struct ParsedApdu<'a> {
    pub class: Class,
    pub ins: Instruction,
    pub p1: u8,
    pub p2: u8,
    pub case: ApduCase<'a>,
}

impl<'a> ParsedApdu<'a> {
    /// Returns the APDU command data payload if present (`DataOnly` or
    /// `DataAndResponse`), or an empty slice.
    pub fn data(&self) -> &[u8] {
        match &self.case {
            ApduCase::DataOnly { data } | ApduCase::DataAndResponse { data, .. } => data.as_ref(),
            _ => &[],
        }
    }

    /// Returns the expected response length `Le` if present (`ResponseExpected`
    /// or `DataAndResponse`).
    pub fn expected_length(&self) -> Option<u8> {
        match self.case {
            ApduCase::ResponseExpected { expected_length }
            | ApduCase::DataAndResponse { expected_length, .. } => Some(expected_length),
            _ => None,
        }
    }

    /// Parses raw APDU bytes into structured [`ParsedApdu`] components.
    ///
    /// Supports short length APDUs as defined in ISO/IEC 7816-4:
    /// - Case 1: CLA INS P1 P2 (4 bytes)
    /// - Case 2: CLA INS P1 P2 Le (5 bytes)
    /// - Case 3: CLA INS P1 P2 Lc Data (5 + Lc bytes)
    /// - Case 4: CLA INS P1 P2 Lc Data Le (6 + Lc bytes)
    ///
    /// Returns `SW_WRONG_LENGTH` (0x6700) if the structure is invalid.
    pub fn parse(apdu_bytes: &'a [u8]) -> Result<Self, u16> {
        let (cla, ins, p1, p2, case) = match *apdu_bytes {
            [cla, ins, p1, p2] => (cla, ins, p1, p2, ApduCase::CommandOnly),
            [cla, ins, p1, p2, expected_length] => {
                (cla, ins, p1, p2, ApduCase::ResponseExpected { expected_length })
            }
            [cla, ins, p1, p2, lc, ref rest @ ..] => {
                let len = lc as usize;
                match rest.split_at_checked(len) {
                    Some((data, [])) => {
                        (cla, ins, p1, p2, ApduCase::DataOnly { data: Cow::Borrowed(data) })
                    }
                    Some((data, &[expected_length])) => (
                        cla,
                        ins,
                        p1,
                        p2,
                        ApduCase::DataAndResponse { data: Cow::Borrowed(data), expected_length },
                    ),
                    _ => return Err(SW_WRONG_LENGTH),
                }
            }
            _ => return Err(SW_WRONG_LENGTH),
        };
        Ok(Self { class: Class(cla), ins: Instruction::from(ins), p1, p2, case })
    }
}

impl<'a> ParsedApdu<'a> {
    /// Compares this APDU with another APDU for matching overrides in XML
    /// profiles.
    ///
    /// It compares the normalized CLA, INS, P1, P2, and the case payload (data
    /// and expected length).
    pub fn matches(&self, other: &ParsedApdu<'_>) -> bool {
        if self.class.normalized() != other.class.normalized() {
            return false;
        }

        let mapped_channel = self.class.channel();
        if mapped_channel != 0 && mapped_channel != other.class.channel() {
            return false;
        }

        self.ins == other.ins
            && self.p1 == other.p1
            && self.p2 == other.p2
            && match (&self.case, &other.case) {
                (ApduCase::CommandOnly, ApduCase::CommandOnly) => true,
                (
                    ApduCase::ResponseExpected { expected_length: l1 },
                    ApduCase::ResponseExpected { expected_length: l2 },
                ) => l1 == l2,
                (ApduCase::DataOnly { data: d1 }, ApduCase::DataOnly { data: d2 }) => {
                    d1.as_ref() == d2.as_ref()
                }
                (
                    ApduCase::DataAndResponse { data: d1, expected_length: l1 },
                    ApduCase::DataAndResponse { data: d2, expected_length: l2 },
                ) => d1.as_ref() == d2.as_ref() && l1 == l2,
                _ => false,
            }
    }
}

impl ParsedApdu<'static> {
    /// Parses an APDU command mapping from an XML profile.
    ///
    /// Note: Some XML profiles omit the `0x00` CLA byte because the APDU
    /// commands were historically serialized as integers (stripping leading
    /// zeros).
    ///
    /// Heuristic: If the first byte matches a supported Format 1 CLA (channels
    /// 0-3: `0x00..=0x03` or `0x80..=0x83`) or `0xFF` (for testing
    /// unsupported classes), we assume the CLA is present. Otherwise, we
    /// assume `0x00` CLA was omitted and prepend it.
    pub fn parse_mapped(s: &str) -> Result<Self, String> {
        let cleaned = s.trim();
        let cleaned = if let Some(start) = cleaned.find('"')
            && let Some(end) = cleaned.rfind('"')
            && let Some(sub) = cleaned.get(start + 1..end)
        {
            sub
        } else {
            cleaned
        };

        let bytes = hex::decode(cleaned).map_err(|e| e.to_string())?;

        let Some(&first_byte) = bytes.first() else {
            return Err("Empty APDU command".to_string());
        };
        let class = Class(first_byte);

        // Heuristic: Check if first byte is a likely CLA in XML commands.
        let has_cla = class.is_likely_mapped_cla();

        let (cla, ins, p1, p2, rest) = if has_cla {
            let &[cla, ins, p1, p2, ref rest @ ..] = bytes.as_slice() else {
                return Err(
                    "APDU command too short (expected at least 4 bytes for header)".to_string()
                );
            };
            (cla, ins, p1, p2, rest)
        } else {
            let &[ins, p1, p2, ref rest @ ..] = bytes.as_slice() else {
                return Err(
                    "CLA-less APDU command too short (expected at least 3 bytes for header)"
                        .to_string(),
                );
            };
            (0x00, ins, p1, p2, rest) // Assume CLA 00 (normalized)
        };

        let case = match *rest {
            [] => ApduCase::CommandOnly,
            [expected_length] => ApduCase::ResponseExpected { expected_length },
            [lc, ref data_rest @ ..] => {
                let lc_len = lc as usize;
                match data_rest.split_at_checked(lc_len) {
                    Some((data, [])) => ApduCase::DataOnly { data: Cow::Owned(data.to_vec()) },
                    Some((data, &[expected_length])) => ApduCase::DataAndResponse {
                        data: Cow::Owned(data.to_vec()),
                        expected_length,
                    },
                    _ => {
                        return Err(format!(
                            "Invalid APDU length: Lc is {lc_len}, but remaining bytes length is {}",
                            data_rest.len()
                        ));
                    }
                }
            }
        };

        Ok(Self { class: Class(cla), ins: Instruction::from(ins), p1, p2, case })
    }
}

impl<'de> serde::Deserialize<'de> for ParsedApdu<'static> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Self::parse_mapped(&s).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_apdu_cases() {
        // Case 1
        let parsed = ParsedApdu::parse(&[0x00, 0xA4, 0x00, 0x04]).unwrap();
        assert_eq!(
            parsed,
            ParsedApdu {
                class: Class(0x00),
                ins: Instruction::Select,
                p1: 0x00,
                p2: 0x04,
                case: ApduCase::CommandOnly
            }
        );
        assert!(parsed.data().is_empty());
        assert_eq!(parsed.expected_length(), None);

        // Case 2
        let parsed = ParsedApdu::parse(&[0x00, 0xC0, 0x00, 0x00, 0x05]).unwrap();
        assert_eq!(
            parsed,
            ParsedApdu {
                class: Class(0x00),
                ins: Instruction::GetResponse,
                p1: 0x00,
                p2: 0x00,
                case: ApduCase::ResponseExpected { expected_length: 5 }
            }
        );
        assert!(parsed.data().is_empty());
        assert_eq!(parsed.expected_length(), Some(5));

        // Case 3
        let parsed = ParsedApdu::parse(&[0x00, 0xD6, 0x00, 0x00, 0x02, 0x11, 0x22]).unwrap();
        assert_eq!(
            parsed,
            ParsedApdu {
                class: Class(0x00),
                ins: Instruction::UpdateBinary,
                p1: 0x00,
                p2: 0x00,
                case: ApduCase::DataOnly { data: Cow::Borrowed(&[0x11, 0x22]) }
            }
        );
        assert_eq!(parsed.data(), &[0x11, 0x22]);
        assert_eq!(parsed.expected_length(), None);

        // Case 4 with Lc=0 (empty data) and Le present
        let parsed = ParsedApdu::parse(&[0x00, 0xA4, 0x00, 0x04, 0x00, 0x05]).unwrap();
        assert_eq!(
            parsed,
            ParsedApdu {
                class: Class(0x00),
                ins: Instruction::Select,
                p1: 0x00,
                p2: 0x04,
                case: ApduCase::DataAndResponse { data: Cow::Borrowed(&[]), expected_length: 5 }
            }
        );
        assert!(parsed.data().is_empty());
        assert_eq!(parsed.expected_length(), Some(5));

        // Invalid length
        assert_eq!(ParsedApdu::parse(&[0x00, 0xA4, 0x00]), Err(SW_WRONG_LENGTH));
        assert_eq!(ParsedApdu::parse(&[0x00, 0xD6, 0x00, 0x00, 0x02, 0x11]), Err(SW_WRONG_LENGTH));
        assert_eq!(ParsedApdu::parse(&[]), Err(SW_WRONG_LENGTH));
        assert_eq!(ParsedApdu::parse(&[0x00]), Err(SW_WRONG_LENGTH));
    }

    #[test]
    fn test_class() {
        // Format 1 supported cases and PartialEq implementations (both directions)
        let class_0 = Class(0x00);
        assert_eq!(class_0, 0x00_u8);
        assert_eq!(0x00_u8, class_0);
        assert!(class_0.is_supported());
        assert!(class_0.is_format_1());
        assert!(!class_0.is_format_2());

        let class_80 = Class(0x80);
        assert!(class_80.is_supported());
        assert!(class_80.is_format_1());
        assert!(!class_80.is_format_2());

        // Format 2 supported cases
        let class_40 = Class(0x40);
        assert!(class_40.is_supported());
        assert!(!class_40.is_format_1());
        assert!(class_40.is_format_2());

        let class_c0 = Class(0xC0);
        assert!(class_c0.is_supported());
        assert!(!class_c0.is_format_1());
        assert!(class_c0.is_format_2());

        // Unsupported RFU and invalid class bytes
        assert!(!Class(0xFF).is_supported());
        assert!(!Class(0x20).is_supported()); // b5=1 in Format 1 is RFU/unsupported
        assert!(!Class(0x60).is_supported()); // b6=1 in Format 2 is RFU/unsupported
    }

    #[test]
    fn test_channel_extraction() {
        // Format 1 channels (0..3)
        assert_eq!(Class(0x00).channel(), 0);
        assert_eq!(Class(0x01).channel(), 1);
        assert_eq!(Class(0x82).channel(), 2);
        assert_eq!(Class(0x03).channel(), 3);
        assert_eq!(Class(0x80).channel(), 0);
        assert_eq!(Class(0x83).channel(), 3);

        // Format 2 channels (4..19)
        assert_eq!(Class(0x40).channel(), 4);
        assert_eq!(Class(0xC1).channel(), 5);
        assert_eq!(Class(0x4F).channel(), 19);
        assert_eq!(Class(0xC0).channel(), 4);
        assert_eq!(Class(0xCF).channel(), 19);
    }

    #[test]
    fn test_normalize_class() {
        // Format 1 normalization
        assert_eq!(Class(0x00).normalized(), Class(0x00));
        assert_eq!(Class(0x01).normalized(), Class(0x00));
        assert_eq!(Class(0x80).normalized(), Class(0x80));
        assert_eq!(Class(0x82).normalized(), Class(0x80));
        assert_eq!(Class(0x83).normalized(), Class(0x80));

        // Format 2 normalization
        assert_eq!(Class(0x40).normalized(), Class(0x00));
        assert_eq!(Class(0x45).normalized(), Class(0x00));
        assert_eq!(Class(0xC3).normalized(), Class(0x80));
        assert_eq!(Class(0x4F).normalized(), Class(0x00));
        assert_eq!(Class(0xCF).normalized(), Class(0x80));
    }

    #[test]
    fn test_parse_mapped_apdu() {
        // Format 1 with CLA (normalized to 00)
        assert_eq!(
            ParsedApdu::parse_mapped("00A40004022FE2").unwrap(),
            ParsedApdu {
                class: Class(0x00),
                ins: Instruction::Select,
                p1: 0x00,
                p2: 0x04,
                case: ApduCase::DataOnly { data: Cow::Owned(vec![0x2F, 0xE2]) }
            }
        );

        // Format 1 with channel 1 CLA (unnormalized)
        assert_eq!(
            ParsedApdu::parse_mapped("81F2FF0000").unwrap(),
            ParsedApdu {
                class: Class(0x81),
                ins: Instruction::Status,
                p1: 0xFF,
                p2: 0x00,
                case: ApduCase::ResponseExpected { expected_length: 0 }
            }
        );

        // CLA-less SELECT (prepends 00, normalized to 00)
        assert_eq!(
            ParsedApdu::parse_mapped("A40004025031").unwrap(),
            ParsedApdu {
                class: Class(0x00),
                ins: Instruction::Select,
                p1: 0x00,
                p2: 0x04,
                case: ApduCase::DataOnly { data: Cow::Owned(vec![0x50, 0x31]) }
            }
        );

        // CLA-less GET RESPONSE (prepends 00, normalized to 00)
        assert_eq!(
            ParsedApdu::parse_mapped("C0000024").unwrap(),
            ParsedApdu {
                class: Class(0x00),
                ins: Instruction::GetResponse,
                p1: 0x00,
                p2: 0x00,
                case: ApduCase::ResponseExpected { expected_length: 0x24 }
            }
        );

        // Legacy format with quotes inside (unnormalized CLA)
        assert_eq!(
            ParsedApdu::parse_mapped("1,14,\"81F2FF0000\"").unwrap(),
            ParsedApdu {
                class: Class(0x81),
                ins: Instruction::Status,
                p1: 0xFF,
                p2: 0x00,
                case: ApduCase::ResponseExpected { expected_length: 0 }
            }
        );

        // Invalid hex
        assert!(ParsedApdu::parse_mapped("XXA4").is_err());
        // Too short
        assert!(ParsedApdu::parse_mapped("0").is_err());
    }

    #[test]
    fn test_matches_apdu() {
        let select_ef_dir_mapped = ParsedApdu::parse_mapped("A40004022FE2").unwrap();

        // Exact match (channel 0)
        let select_channel_0 =
            ParsedApdu::parse(&[0x00, 0xA4, 0x00, 0x04, 0x02, 0x2F, 0xE2]).unwrap();
        assert!(select_ef_dir_mapped.matches(&select_channel_0));

        // Match on channel 1 (Format 1) - CLA 01 normalizes to 00
        let select_channel_1 =
            ParsedApdu::parse(&[0x01, 0xA4, 0x00, 0x04, 0x02, 0x2F, 0xE2]).unwrap();
        assert!(select_ef_dir_mapped.matches(&select_channel_1));

        // Match on channel 4 (Format 2) - CLA 40 normalizes to 00
        let select_channel_4 =
            ParsedApdu::parse(&[0x40, 0xA4, 0x00, 0x04, 0x02, 0x2F, 0xE2]).unwrap();
        assert!(select_ef_dir_mapped.matches(&select_channel_4));

        // Mismatched data
        let select_other = ParsedApdu::parse(&[0x00, 0xA4, 0x00, 0x04, 0x02, 0x50, 0x31]).unwrap();
        assert!(!select_ef_dir_mapped.matches(&select_other));

        // Mapped command with explicit CLA 81 (channel 1, Format 1)
        let status_channel_1_mapped = ParsedApdu::parse_mapped("81F2FF0000").unwrap();

        // Match guest command on channel 1 (CLA 81 normalizes to 80)
        let status_channel_1_guest = ParsedApdu::parse(&[0x81, 0xF2, 0xFF, 0x00, 0x00]).unwrap();
        assert!(status_channel_1_mapped.matches(&status_channel_1_guest));

        // Do not match guest command on channel 0 (XML mapping to channel 1 is strict)
        let status_channel_0_guest = ParsedApdu::parse(&[0x80, 0xF2, 0xFF, 0x00, 0x00]).unwrap();
        assert!(!status_channel_1_mapped.matches(&status_channel_0_guest));
    }

    #[test]
    fn test_select_p1_try_from() {
        assert_eq!(SelectP1::try_from(0x00), Ok(SelectP1::ByFid));
        assert_eq!(SelectP1::try_from(0x04), Ok(SelectP1::ByDfName));
        assert_eq!(SelectP1::try_from(0x08), Ok(SelectP1::ByPathFromMf));
        assert_eq!(SelectP1::try_from(0x01), Err(()));
        assert_eq!(SelectP1::try_from(0x03), Err(()));
        assert_eq!(SelectP1::try_from(0x09), Err(()));
        assert_eq!(SelectP1::try_from(0x02), Err(()));
        assert_eq!(SelectP1::try_from(0x0A), Err(()));
    }

    #[test]
    fn test_select_p2_try_from() {
        assert_eq!(SelectP2::try_from(0x00), Ok(SelectP2::ReturnFcp));
        assert_eq!(SelectP2::try_from(0x04), Ok(SelectP2::DoNotReturnFcp));
        assert_eq!(SelectP2::try_from(0x08), Ok(SelectP2::ReturnProprietary));
        assert_eq!(SelectP2::try_from(0x0C), Ok(SelectP2::ReturnFcpNoProprietary));
        assert_eq!(SelectP2::try_from(0x01), Err(()));
    }

    #[test]
    fn test_record_mode_try_from() {
        assert_eq!(RecordMode::try_from(0x02), Ok(RecordMode::NextRecord));
        assert_eq!(RecordMode::try_from(0x03), Ok(RecordMode::PreviousRecord));
        assert_eq!(RecordMode::try_from(0x04), Ok(RecordMode::AbsoluteMode));
        assert_eq!(RecordMode::try_from(0x14), Ok(RecordMode::AbsoluteMode));
        assert_eq!(RecordMode::try_from(0x00), Err(()));
    }

    #[test]
    fn test_instruction_from_u8() {
        assert_eq!(Instruction::from(0xC0), Instruction::GetResponse);
    }
}
