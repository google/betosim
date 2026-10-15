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

use std::{str, time::Duration};

pub use modem_rs_derive::ParsableEnum;
use netsim_model::{Call, CellNetworkConfig, Quirks, RegistrationStatus};
use nom::{
    IResult,
    branch::alt,
    bytes::complete::{tag, take_while, take_while1},
    combinator::{map, opt},
    sequence::{pair, preceded},
};

use crate::{
    call_service::CallResponse,
    constants::{ADN_CAPABILITY_EXT_BYTES, ADN_DIALING_NUMBER_LEN, ADN_FOOTER_LEN},
    data_service::DataResponse,
    misc_service::MiscResponse,
    network_service::NetworkResponse,
    sim_service::SimResponse,
    sms_service::SmsResponse,
    stk_service::StkResponse,
    sup_service::SupResponse,
};

/// Parses a double-quoted string (e.g. `"foo"`) or an unquoted token delimited
/// by comma, semicolon, or carriage return/newline.
pub fn parse_quoted_or_unquoted(input: &[u8]) -> IResult<&[u8], &str> {
    if let Ok((rem, qs)) = QuotedString::parse(input) {
        return Ok((rem, qs.as_str()));
    }
    use nom::{bytes::complete::take_while1, combinator::map_res};
    let (input, content) = map_res(
        take_while1(|c: u8| c != b',' && c != b';' && c != b'\r' && c != b'\n'),
        std::str::from_utf8,
    )(input)?;
    Ok((input, content))
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub struct QuotedString<'a>(pub &'a str);

impl<'a> QuotedString<'a> {
    pub fn as_str(self) -> &'a str {
        self.0
    }
}

impl<'a> Parsable<'a> for QuotedString<'a> {
    fn parse(input: &'a [u8]) -> IResult<&'a [u8], Self> {
        use nom::{
            bytes::complete::{tag, take_while},
            combinator::map_res,
            sequence::delimited,
        };
        let (input, content) = delimited(
            tag(br#"""#),
            map_res(take_while(|c| c != b'"'), std::str::from_utf8),
            tag(br#"""#),
        )(input)?;
        Ok((input, QuotedString(content)))
    }
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub struct PinString<'a>(pub &'a str);

impl<'a> PinString<'a> {
    pub fn as_str(self) -> &'a str {
        self.0
    }
}

impl<'a> Parsable<'a> for PinString<'a> {
    fn parse(input: &'a [u8]) -> IResult<&'a [u8], Self> {
        let (input, content) = parse_quoted_or_unquoted(input)?;
        Ok((input, PinString(content)))
    }
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub struct ApduData<'a>(pub &'a str);

impl<'a> ApduData<'a> {
    pub fn as_str(self) -> &'a str {
        self.0
    }

    pub fn decode_hex(self) -> Result<Vec<u8>, hex::FromHexError> {
        hex::decode(self.0)
    }
}

impl<'a> Parsable<'a> for ApduData<'a> {
    fn parse(input: &'a [u8]) -> IResult<&'a [u8], Self> {
        let (input, content) = parse_quoted_or_unquoted(input)?;
        Ok((input, ApduData(content)))
    }
}

pub trait Parsable<'a>: Sized {
    fn parse(input: &'a [u8]) -> IResult<&'a [u8], Self>;
}

impl Parsable<'_> for u8 {
    fn parse(input: &[u8]) -> IResult<&[u8], Self> {
        nom::combinator::map_res(
            nom::combinator::map_res(nom::character::complete::digit1, str::from_utf8),
            |s: &str| s.parse::<u8>(),
        )(input)
    }
}

impl Parsable<'_> for u16 {
    fn parse(input: &[u8]) -> IResult<&[u8], Self> {
        nom::combinator::map_res(
            nom::combinator::map_res(nom::character::complete::digit1, str::from_utf8),
            |s: &str| s.parse::<u16>(),
        )(input)
    }
}

impl Parsable<'_> for u32 {
    fn parse(input: &[u8]) -> IResult<&[u8], Self> {
        nom::combinator::map_res(
            nom::combinator::map_res(nom::character::complete::digit1, str::from_utf8),
            |s: &str| s.parse::<u32>(),
        )(input)
    }
}

impl Parsable<'_> for usize {
    fn parse(input: &[u8]) -> IResult<&[u8], Self> {
        nom::combinator::map_res(
            nom::combinator::map_res(nom::character::complete::digit1, str::from_utf8),
            |s: &str| s.parse::<usize>(),
        )(input)
    }
}

pub const AT_OK: &[u8] = b"OK\r\n";
pub const AT_ERROR: &[u8] = b"ERROR\r\n";

pub const DEFAULT_PIN: &str = "1234";
pub const DEFAULT_PIN2: &str = "5678";
pub const DEFAULT_PUK2: &str = "12345678";
pub const DEFAULT_BARRING_PASSWORD: &str = "0000";

// A unique identifier for a modem instance.
pub type ModemId = u32;

// Custom error type for the library.
use std::fmt::{self, Write};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModemError {
    DuplicateModemId(ModemId),
    UnknownModemId(ModemId),
    NotFound,
    InvalidConfig(String),
    InvalidProfile(String),
}

impl std::error::Error for ModemError {}

impl fmt::Display for ModemError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ModemError::DuplicateModemId(id) => write!(f, "Duplicate modem ID: {id}"),
            ModemError::UnknownModemId(id) => write!(f, "Unknown modem ID: {id}"),
            ModemError::NotFound => write!(f, "Modem network not found"),
            ModemError::InvalidConfig(msg) => write!(f, "Invalid configuration: {msg}"),
            ModemError::InvalidProfile(msg) => write!(f, "Invalid SIM profile: {msg}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlmnError {
    InvalidLength(usize),
    InvalidDigits(String),
    InvalidMcc(String),
    InvalidMnc(String),
}

impl std::error::Error for PlmnError {}

impl fmt::Display for PlmnError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PlmnError::InvalidLength(len) => {
                write!(
                    f,
                    "PLMN length must be 5 digits (2-digit MNC) or 6 digits (3-digit MNC), got {len}"
                )
            }
            PlmnError::InvalidDigits(s) => {
                write!(f, "PLMN must contain only ASCII decimal digits, got: {s:?}")
            }
            PlmnError::InvalidMcc(s) => {
                write!(f, "MCC must be exactly 3 ASCII decimal digits, got: {s:?}")
            }
            PlmnError::InvalidMnc(s) => {
                write!(f, "MNC must be 2 or 3 ASCII decimal digits, got: {s:?}")
            }
        }
    }
}

/// Represents a Public Land Mobile Network (PLMN) identity per 3GPP TS 23.003
/// §2.2 and ITU-T Recommendation E.212.
///
/// Composed of:
/// - Mobile Country Code (MCC): Exactly 3 decimal digits.
/// - Mobile Network Code (MNC): Either 2 decimal digits (total 5 digits) or 3
///   decimal digits (total 6 digits).
///
/// The distinction between 2- and 3-digit MNCs is governed by carrier
/// assignment and signaled on SIM cards via `EF_AD` (Administrative Data) byte
/// 4 (3GPP TS 31.102 §4.2.18).
#[derive(Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(into = "String", try_from = "String")]
pub enum Plmn {
    /// 5-digit PLMN with a 2-digit Mobile Network Code (3-digit MCC + 2-digit
    /// MNC). Standard in European (ITU-T Region 2) and international GSM
    /// networks (e.g. "20810").
    TwoDigitMnc([u8; 5]),

    /// 6-digit PLMN with a 3-digit Mobile Network Code (3-digit MCC + 3-digit
    /// MNC). Standard in North American (ITU-T Region 3) networks (e.g. US
    /// carriers "310260", "311740").
    ThreeDigitMnc([u8; 6]),
}

impl Plmn {
    /// Validates and constructs a PLMN (must be 5 or 6 ASCII decimal digits).
    pub fn parse(s: &str) -> Result<Self, PlmnError> {
        let trimmed = s.trim();
        let bytes = trimmed.as_bytes();
        if !bytes.iter().all(u8::is_ascii_digit) {
            return Err(PlmnError::InvalidDigits(s.to_string()));
        }
        match bytes.len() {
            5 => {
                let mut b = [0u8; 5];
                b.copy_from_slice(bytes);
                Ok(Self::TwoDigitMnc(b))
            }
            6 => {
                let mut b = [0u8; 6];
                b.copy_from_slice(bytes);
                Ok(Self::ThreeDigitMnc(b))
            }
            len => Err(PlmnError::InvalidLength(len)),
        }
    }

    /// Creates a PLMN from string or string-like slice.
    pub fn new(s: impl AsRef<str>) -> Result<Self, PlmnError> {
        Self::parse(s.as_ref())
    }

    /// Constructs a PLMN from separate MCC and MNC strings.
    pub fn from_mcc_mnc(mcc: &str, mnc: &str) -> Result<Self, PlmnError> {
        let mcc = mcc.trim();
        let mnc = mnc.trim();
        let &[c0, c1, c2] = mcc.as_bytes() else {
            return Err(PlmnError::InvalidMcc(mcc.to_string()));
        };
        if !c0.is_ascii_digit() || !c1.is_ascii_digit() || !c2.is_ascii_digit() {
            return Err(PlmnError::InvalidMcc(mcc.to_string()));
        }
        match *mnc.as_bytes() {
            [n0, n1] if n0.is_ascii_digit() && n1.is_ascii_digit() => {
                Ok(Self::TwoDigitMnc([c0, c1, c2, n0, n1]))
            }
            [n0, n1, n2] if n0.is_ascii_digit() && n1.is_ascii_digit() && n2.is_ascii_digit() => {
                Ok(Self::ThreeDigitMnc([c0, c1, c2, n0, n1, n2]))
            }
            _ => Err(PlmnError::InvalidMnc(mnc.to_string())),
        }
    }

    /// Derives the home PLMN from an IMSI string.
    ///
    /// If `mnc_len` is specified (e.g. from EF_AD byte 4 per 3GPP TS 31.102
    /// §4.2.18), it is honored. Otherwise defaults to a 3-digit MNC if len
    /// is at least 6, or 2-digit MNC if len is 5. Returns `None` if IMSI is
    /// shorter than 5 digits or non-numeric.
    pub fn from_imsi(imsi: &str, mnc_len: Option<usize>) -> Option<Self> {
        let digits = imsi.trim().as_bytes();
        if !digits.iter().all(u8::is_ascii_digit) {
            return None;
        }
        match mnc_len {
            Some(2) => digits.first_chunk::<5>().copied().map(Self::TwoDigitMnc),
            _ => digits
                .first_chunk::<6>()
                .copied()
                .map(Self::ThreeDigitMnc)
                .or_else(|| digits.first_chunk::<5>().copied().map(Self::TwoDigitMnc)),
        }
    }

    /// Returns the raw byte slice of ASCII decimal digits (length 5 or 6).
    pub fn as_bytes(&self) -> &[u8] {
        match self {
            Self::TwoDigitMnc(b) => b,
            Self::ThreeDigitMnc(b) => b,
        }
    }

    /// Returns the PLMN as an ASCII string slice (length 5 or 6).
    pub fn as_str(&self) -> &str {
        str::from_utf8(self.as_bytes()).expect("PLMN bytes are validated ASCII digits")
    }

    /// Returns the 3-digit Mobile Country Code (MCC).
    pub fn mcc(&self) -> &str {
        let (Self::TwoDigitMnc([mcc @ .., _, _]) | Self::ThreeDigitMnc([mcc @ .., _, _, _])) = self;
        str::from_utf8(mcc).unwrap_or("")
    }

    /// Returns the 2- or 3-digit Mobile Network Code (MNC).
    pub fn mnc(&self) -> &str {
        match self {
            Self::TwoDigitMnc([_, _, _, mnc @ ..]) => str::from_utf8(mnc).unwrap_or(""),
            Self::ThreeDigitMnc([_, _, _, mnc @ ..]) => str::from_utf8(mnc).unwrap_or(""),
        }
    }

    /// Returns the length of the Mobile Network Code (2 or 3).
    pub fn mnc_length(&self) -> usize {
        match self {
            Self::TwoDigitMnc(_) => 2,
            Self::ThreeDigitMnc(_) => 3,
        }
    }
}

impl fmt::Display for Plmn {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl fmt::Debug for Plmn {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Plmn(\"{}\")", self.as_str())
    }
}

impl From<Plmn> for String {
    fn from(plmn: Plmn) -> Self {
        plmn.as_str().to_string()
    }
}

impl TryFrom<String> for Plmn {
    type Error = PlmnError;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        Self::parse(&s)
    }
}

impl TryFrom<&str> for Plmn {
    type Error = PlmnError;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        Self::parse(s)
    }
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum ParsePhoneNumberError {
    Empty,
    InvalidCharacters(String),
    TrailingCharacters(String),
}

impl fmt::Display for ParsePhoneNumberError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(f, "phone number cannot be empty"),
            Self::InvalidCharacters(s) => write!(f, "invalid characters in phone number: '{s}'"),
            Self::TrailingCharacters(s) => {
                write!(f, "unparsed trailing characters in phone number: '{s}'")
            }
        }
    }
}

impl std::error::Error for ParsePhoneNumberError {}

#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct PhoneNumber(String);

impl TryFrom<&str> for PhoneNumber {
    type Error = ParsePhoneNumberError;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        if s.is_empty() {
            return Err(ParsePhoneNumberError::Empty);
        }
        let (rem, phone) = Self::parse(s.as_bytes())
            .map_err(|_err| ParsePhoneNumberError::InvalidCharacters(s.to_string()))?;
        if !rem.is_empty() {
            return Err(ParsePhoneNumberError::TrailingCharacters(
                String::from_utf8_lossy(rem).to_string(),
            ));
        }
        Ok(phone)
    }
}

impl TryFrom<String> for PhoneNumber {
    type Error = ParsePhoneNumberError;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        Self::try_from(s.as_str())
    }
}

impl From<PhoneNumber> for String {
    fn from(phone: PhoneNumber) -> Self {
        phone.0
    }
}

impl PhoneNumber {
    #[cfg(any(test, feature = "test-utils", feature = "testing"))]
    pub fn new_for_test(s: impl Into<String>) -> Self {
        Self(s.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn normalized(&self) -> &str {
        self.0.strip_prefix('+').unwrap_or(&self.0)
    }

    pub fn is_international(&self) -> bool {
        self.0.starts_with('+')
    }

    pub fn toa(&self) -> TypeOfAddress {
        TypeOfAddress::from_number(&self.0)
    }

    pub fn is_gprs_dial(&self) -> bool {
        self.0.starts_with("*99")
            && self.0.ends_with('#')
            && self.0.as_bytes().get(3).is_some_and(|&c| c == b'*' || c == b'#')
    }
}

/// Abbreviated Dialling Number (ADN) record per 3GPP TS 31.102 §4.4.2.3 and
/// TS 51.011 §10.5.1.
///
/// Shared linear-fixed record structure for:
/// - EF_MSISDN (0x6F40)
/// - EF_MBDN (0x6FC7)
/// - EF_FDN (0x6F3B)
/// - EF_ADN (0x6F3A / 0x4F3A)
/// - EF_SDN (0x6F49)
/// - EF_BDN (0x6F4D)
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AdnRecord {
    pub alpha_tag: Option<String>,
    pub number: Option<PhoneNumber>,
}

impl AdnRecord {
    #[cfg(test)]
    pub fn new(alpha_tag: Option<impl Into<String>>, number: Option<PhoneNumber>) -> Self {
        Self { alpha_tag: alpha_tag.map(Into::into).filter(|s| !s.is_empty()), number }
    }

    pub fn from_number(number: &PhoneNumber) -> Self {
        Self { alpha_tag: None, number: Some(number.clone()) }
    }

    pub fn encode_from_number(number: &PhoneNumber, record_len: usize) -> Vec<u8> {
        Self::from_number(number).encode(record_len)
    }

    /// Decodes an ADN linear-fixed record slice (length >= 14 bytes).
    ///
    /// Returns None if the record is empty (all 0xFF) or malformed (< 14
    /// bytes).
    pub fn decode(record: &[u8]) -> Option<Self> {
        if record.iter().all(|&b| b == 0xFF) {
            return None;
        }

        let alpha_len = record.len().checked_sub(ADN_FOOTER_LEN)?;
        let (alpha_bytes, after_alpha) = record.split_at_checked(alpha_len)?;
        let trimmed = alpha_bytes
            .iter()
            .rposition(|&b| b != 0xFF)
            .and_then(|last| alpha_bytes.get(..=last))
            .unwrap_or_default();
        let alpha_tag = if !trimmed.is_empty() {
            Some(String::from_utf8_lossy(trimmed).into_owned())
        } else {
            None
        };

        let &[len_byte, ton_npi, ref rest @ ..] = after_alpha else {
            return None;
        };
        if len_byte <= 1 || len_byte == 0xFF {
            return Some(Self { alpha_tag, number: None });
        }

        let bcd_content_len = len_byte as usize;
        if bcd_content_len > 1 + ADN_DIALING_NUMBER_LEN {
            return Some(Self { alpha_tag, number: None });
        }

        let is_international = (ton_npi & 0xF0) == 0x90 || (ton_npi & 0x70) == 0x10;

        let bcd_digits_len = bcd_content_len - 1;
        let bcd_bytes = rest.get(..bcd_digits_len)?;
        let mut digits = crate::pdu::bcd::bcd_to_string(bcd_bytes);

        if is_international && !digits.is_empty() && !digits.starts_with('+') {
            digits.insert(0, '+');
        }

        let number = PhoneNumber::try_from(digits).ok();
        Some(Self { alpha_tag, number })
    }

    /// Encodes this record into a linear-fixed ADN record of `record_len`
    /// bytes.
    pub fn encode(&self, record_len: usize) -> Vec<u8> {
        let Some(alpha_len) = record_len.checked_sub(ADN_FOOTER_LEN) else {
            return vec![0xFF; record_len];
        };
        let mut out = vec![0xFF; record_len];
        let Some((alpha_part, footer_part)) = out.split_at_mut_checked(alpha_len) else {
            return out;
        };

        // 1. Encode Alpha Identifier
        if let Some(ref tag) = self.alpha_tag {
            for (dst, &src) in alpha_part.iter_mut().zip(tag.as_bytes()) {
                *dst = src;
            }
        }

        // 2. Encode Dialing Number
        if let Some(ref phone) = self.number {
            let clean_digits: String = phone
                .as_str()
                .chars()
                .filter(|c| c.is_ascii_digit() || *c == '*' || *c == '#')
                .collect();
            if !clean_digits.is_empty() {
                let ton_npi = if clean_digits.len() == 11 && clean_digits.starts_with('1') {
                    TypeOfAddress::International
                } else {
                    phone.toa()
                };

                let bcd = crate::pdu::bcd::string_to_bcd(&clean_digits);
                let dialing_copy = bcd.len().min(ADN_DIALING_NUMBER_LEN);
                if let [len_slot, ton_slot, digits @ .., _, _] = footer_part {
                    *len_slot = (1 + dialing_copy) as u8;
                    *ton_slot = ton_npi.as_u8();
                    for (dst, &src) in digits.iter_mut().zip(&bcd) {
                        *dst = src;
                    }
                }
            }
        }

        // 3. Capability Configuration & Extension Record IDs (bytes 13 and 14 of
        //    footer)
        if let Some(ext) = footer_part.get_mut(ADN_FOOTER_LEN - 2..) {
            ext.copy_from_slice(&ADN_CAPABILITY_EXT_BYTES);
        }

        out
    }
}

impl fmt::Display for PhoneNumber {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Type of Address (TON/NPI) as defined in 3GPP TS 24.008 Table 10.5.118 and
/// 3GPP TS 23.040 Table 9.1.2.5.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ParsableEnum, Default)]
#[repr(u8)]
pub enum TypeOfAddress {
    /// Non-standard placeholder (0) transmitted by Android Goldfish RIL in
    /// `RadioMessaging::setSmscAddress` (`AT+CSCA=...,0`).
    /// Not a valid 3GPP TOA octet (Bit 8 is 0).
    GoldfishCompat = 0,
    /// Default 3GPP Type of Address (0x81 = 129).
    /// TON = 000 (Unknown), NPI = 0001 (ISDN / telephony E.164).
    /// Mandated by 3GPP TS 27.005 §3.1 when number lacks '+'.
    #[default]
    Unknown = 129,
    /// International numbering plan with E.164 (0x91 = 145).
    /// TON = 001 (International), NPI = 0001 (ISDN / telephony E.164).
    /// Mandated by 3GPP TS 27.005 §3.1 when number starts with '+'.
    International = 145,
}

impl TypeOfAddress {
    pub fn from_number(number: &str) -> Self {
        if number.starts_with('+') { Self::International } else { Self::Unknown }
    }

    pub const fn is_international(self) -> bool {
        matches!(self, Self::International)
    }

    pub const fn as_u8(self) -> u8 {
        self as u8
    }
}

impl fmt::Display for TypeOfAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", *self as u8)
    }
}

impl From<TypeOfAddress> for u8 {
    fn from(toa: TypeOfAddress) -> Self {
        toa as u8
    }
}

impl<'a> Parsable<'a> for PhoneNumber {
    fn parse(input: &'a [u8]) -> IResult<&'a [u8], Self> {
        if let Ok((rem, qs)) = QuotedString::parse(input) {
            let (inner_rem, p) = Self::parse(qs.as_str().as_bytes())?;
            if !inner_rem.is_empty() {
                return Err(nom::Err::Error(nom::error::Error::new(
                    inner_rem,
                    nom::error::ErrorKind::Verify,
                )));
            }
            return Ok((rem, p));
        }

        use nom::{
            bytes::complete::{tag, take_while1},
            combinator::{opt, recognize},
            sequence::pair,
        };

        // ONLY allow clean number characters (digits, *, #, and optional leading +)
        let (remaining, digits) = recognize(pair(
            opt(tag::<_, _, nom::error::Error<&[u8]>>(b"+")),
            take_while1(|c: u8| matches!(c, b'0'..=b'9' | b'*' | b'#')),
        ))(input)?;

        let s = std::str::from_utf8(digits).map_err(|_e| {
            nom::Err::Error(nom::error::Error::new(remaining, nom::error::ErrorKind::MapRes))
        })?;

        Ok((remaining, PhoneNumber(s.to_string())))
    }
}

/// 3GPP TS 27.007 §6.2: Dial string CLIR presentation modifier ('I' for
/// invocation, 'i' for suppression).
fn parse_clir_modifier(input: &[u8]) -> IResult<&[u8], ClirMode> {
    alt((
        map(tag(b"i"), |_bytes| ClirMode::Suppression),
        map(tag(b"I"), |_bytes| ClirMode::Invocation),
    ))(input)
}

/// Emergency call dialing metadata (@<category>,#<clir>).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct EmergencyInfo {
    pub category: Option<u32>,
    pub clir: Option<ClirMode>,
}

impl<'a> Parsable<'a> for EmergencyInfo {
    fn parse(input: &'a [u8]) -> IResult<&'a [u8], Self> {
        let (input, _) = tag(b"@")(input)?;
        let (input, cat_digits) = opt(take_while1(|c: u8| c.is_ascii_digit()))(input)?;
        let category = cat_digits.and_then(|d| std::str::from_utf8(d).ok()?.parse::<u32>().ok());
        let (input, clir) = opt(preceded(tag(b",#"), opt(parse_clir_modifier)))(input)?;
        let clir = clir.flatten();
        Ok((input, Self { category, clir }))
    }
}

/// Parsed representation of an AT dial string per 3GPP TS 27.007 §6.2.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DialString {
    pub number: PhoneNumber,
    pub emergency: Option<EmergencyInfo>,
    clir: Option<ClirMode>,
}

impl<'a> Parsable<'a> for DialString {
    fn parse(input: &'a [u8]) -> IResult<&'a [u8], Self> {
        let (input, number) = PhoneNumber::parse(input)?;
        let (input, clir) = opt(parse_clir_modifier)(input)?;
        let (input, _) = opt(pair(
            tag(b","),
            take_while(|c: u8| matches!(c, b'0'..=b'9' | b'*' | b'#' | b',')),
        ))(input)?;
        let (input, emergency) = opt(EmergencyInfo::parse)(input)?;
        Ok((input, Self { number, clir, emergency }))
    }
}

impl DialString {
    /// Resolves CLIR mode, giving precedence to emergency CLIR override if
    /// present.
    pub fn clir(&self, default: ClirMode) -> ClirMode {
        self.emergency.as_ref().and_then(|e| e.clir).or(self.clir).unwrap_or(default)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DialArgs {
    pub number: PhoneNumber,
    pub clir: ClirMode,
    pub is_emergency: bool,
}

impl<'a> Parsable<'a> for DialArgs {
    fn parse(input: &'a [u8]) -> IResult<&'a [u8], Self> {
        let (tail, content) = crate::parser::parse_until_semicolon(input)?;
        let (rem, dial_str) = DialString::parse(content)?;
        if !rem.is_empty() {
            return Err(nom::Err::Error(nom::error::Error::new(
                rem,
                nom::error::ErrorKind::Verify,
            )));
        }
        let clir = dial_str.clir(ClirMode::SubscriptionDefault);
        let is_emergency = dial_str.emergency.is_some();
        let number = dial_str.number;
        Ok((tail, DialArgs { number, clir, is_emergency }))
    }
}

/// 3GPP TS 27.005 §4.4: New message acknowledgement (<n> parameter).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SmsAck {
    /// Message routing to TE is acknowledged (0 or 1 per TS 27.005 §4.4).
    Success,
    /// Message routing to TE is not acknowledged / rejected (2 per TS 27.005
    /// §4.4).
    Failure,
}

impl SmsAck {
    pub fn is_success(&self) -> bool {
        matches!(self, Self::Success)
    }
}

impl<'a> Parsable<'a> for SmsAck {
    fn parse(input: &'a [u8]) -> IResult<&'a [u8], Self> {
        let (input, val) = u8::parse(input)?;
        match val {
            0 | 1 => Ok((input, Self::Success)),
            2 => Ok((input, Self::Failure)),
            _ => Err(nom::Err::Error(nom::error::Error::new(input, nom::error::ErrorKind::MapRes))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HangupReason {
    Normal,
    Busy,
    NoAnswer,
}

impl HangupReason {
    pub fn as_urc_str(&self, goldfish_ril_37_or_earlier: bool) -> &'static str {
        if goldfish_ril_37_or_earlier {
            "RING\r\n"
        } else {
            match self {
                HangupReason::Busy => "BUSY\r\n",
                HangupReason::NoAnswer => "NO ANSWER\r\n",
                HangupReason::Normal => "NO CARRIER\r\n",
            }
        }
    }
}

// Actions that a command can request to be executed by the
// CellularNetworkSimulator.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandAction {
    InitiateCall(DialArgs),
    AnswerCall(ModemId),
    HangupCall { initiator: ModemId, target_peer: ModemId, reason: HangupReason },
    HoldCall { holder: ModemId, target: ModemId },
    ResumeCall { resumer: ModemId, target: ModemId },
    ReceiveSms { to: Option<String>, pdu: Vec<u8>, status_report: Option<crate::pdu::StatusReport> },
    ReceiveTextSms { to: String, text: String },
    AcknowledgeIncomingSms { ack: SmsAck },
    None,
}

// Callbacks Removed in favor of Sink and NetworkEvent

use std::sync::Arc;

use bytes::Bytes;

/// A sink for sending packets back to the modem client.
#[derive(Clone)]
pub struct ModemSink {
    sender: Arc<dyn Fn(Bytes) -> Result<(), String> + Send + Sync>,
}

impl ModemSink {
    pub fn new<F>(f: F) -> Self
    where
        F: Fn(Bytes) -> Result<(), String> + Send + Sync + 'static,
    {
        Self { sender: Arc::new(f) }
    }

    pub fn send(&self, packet: Bytes) -> Result<(), String> {
        (self.sender)(packet)
    }
}

impl fmt::Debug for ModemSink {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ModemSink").finish()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, ParsableEnum)]
#[repr(u8)]
pub enum CmeeMode {
    #[default]
    Disable = 0, // Returns "ERROR"
    Numeric = 1, // Returns "+CME ERROR: <code>"
    Verbose = 2, // Returns "+CME ERROR: <verbose string>"
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, ParsableEnum)]
#[repr(u8)]
pub enum RegistrationUnsolicitedMode {
    #[default]
    Disable = 0,
    Enable = 1,
    EnableWithLocation = 2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, ParsableEnum)]
#[repr(u8)]
pub enum RadioPowerLevel {
    Minimum = 0,
    #[default]
    Full = 1,
    DisableRf = 4,
}

/// Standardized 3GPP TS 27.007 Section 9.2 Mobile Equipment Error Codes
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CmeError {
    PhoneFailure,
    OperationNotAllowed,
    OperationNotSupported,
    SimNotInserted,
    SimPinRequired,
    SimPukRequired,
    SimFailure,
    SimBusy,
    IncorrectPassword,
    SimPin2Required,
    SimPuk2Required,
    MemoryFull,
    InvalidIndex,
    NotFound,
    MemoryFailure,
    TextStringTooLong,
    InvalidCharacters,
    NoNetworkService,
    /// 3GPP TS 27.007 Section 9.2.1: Network not allowed - emergency calls only
    NetworkNotAllowedEmergencyCallsOnly,
    NetworkNotAttachedDueToMTFunctionalRestrictions,
    NoResources,
    IncorrectParameters,
    FixedDialNumberOnlyAllowed,
    Custom(u32, &'static str),
}

impl fmt::Display for CmeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.verbose_str())
    }
}

impl std::error::Error for CmeError {}

impl CmeError {
    pub fn code(&self) -> u32 {
        match *self {
            Self::PhoneFailure => 0,
            Self::OperationNotAllowed => 3,
            Self::OperationNotSupported => 4,
            Self::SimNotInserted => 10,
            Self::SimPinRequired => 11,
            Self::SimPukRequired => 12,
            Self::SimFailure => 13,
            Self::SimBusy => 14,
            Self::IncorrectPassword => 16,
            Self::SimPin2Required => 17,
            Self::SimPuk2Required => 18,
            Self::MemoryFull => 20,
            Self::InvalidIndex => 21,
            Self::NotFound => 22,
            Self::MemoryFailure => 23,
            Self::TextStringTooLong => 24,
            Self::InvalidCharacters => 25,
            Self::NoNetworkService => 30,
            Self::NetworkNotAllowedEmergencyCallsOnly => 32,
            Self::NetworkNotAttachedDueToMTFunctionalRestrictions => 53,
            Self::NoResources => 142,
            Self::IncorrectParameters => 50,
            Self::FixedDialNumberOnlyAllowed => 56,
            Self::Custom(c, _) => c,
        }
    }

    pub fn verbose_str(&self) -> &'static str {
        match *self {
            Self::PhoneFailure => "phone failure",
            Self::OperationNotAllowed => "operation not allowed",
            Self::OperationNotSupported => "operation not supported",
            Self::SimNotInserted => "SIM not inserted",
            Self::SimPinRequired => "SIM PIN required",
            Self::SimPukRequired => "SIM PUK required",
            Self::SimFailure => "SIM failure",
            Self::SimBusy => "SIM busy",
            Self::IncorrectPassword => "incorrect password",
            Self::SimPin2Required => "SIM PIN2 required",
            Self::SimPuk2Required => "SIM PUK2 required",
            Self::MemoryFull => "memory full",
            Self::InvalidIndex => "invalid index",
            Self::NotFound => "not found",
            Self::MemoryFailure => "memory failure",
            Self::TextStringTooLong => "text string too long",
            Self::InvalidCharacters => "invalid characters in text string",
            Self::NoNetworkService => "no network service",
            Self::NetworkNotAllowedEmergencyCallsOnly => {
                "network not allowed - emergency calls only"
            }
            Self::NetworkNotAttachedDueToMTFunctionalRestrictions => {
                "network not attached due to MT functional restrictions"
            }
            Self::NoResources => "no resources",
            Self::IncorrectParameters => "incorrect parameters",
            Self::FixedDialNumberOnlyAllowed => "fixed dialing number only allowed",
            Self::Custom(_, msg) => msg,
        }
    }

    pub fn format_response(&self, mode: CmeeMode) -> String {
        match mode {
            CmeeMode::Disable => "ERROR\r\n".to_string(),
            CmeeMode::Numeric => format!("+CME ERROR: {}\r\n", self.code()),
            CmeeMode::Verbose => format!("+CME ERROR: {}\r\n", self.verbose_str()),
        }
    }
}

/// Standard 3GPP TS 27.005 Message Service Failure Error Codes (+CMS ERROR).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CmsError {
    InvalidPduParameter,
    InvalidTextModeParameter,
    SimNotInserted,
    SimPinRequired,
    InvalidMemoryIndex,
    MemoryFull,
    NoNetworkService,
}

impl fmt::Display for CmsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.verbose_str())
    }
}

impl std::error::Error for CmsError {}

impl CmsError {
    pub fn code(&self) -> u32 {
        match *self {
            Self::InvalidPduParameter => 304,
            Self::InvalidTextModeParameter => 305,
            Self::SimNotInserted => 310,
            Self::SimPinRequired => 311,
            Self::InvalidMemoryIndex => 321,
            Self::MemoryFull => 322,
            Self::NoNetworkService => 331,
        }
    }

    pub fn verbose_str(&self) -> &'static str {
        match *self {
            Self::InvalidPduParameter => "invalid PDU mode parameter",
            Self::InvalidTextModeParameter => "invalid text mode parameter",
            Self::SimNotInserted => "SIM not inserted",
            Self::SimPinRequired => "SIM PIN required",
            Self::InvalidMemoryIndex => "invalid memory index",
            Self::MemoryFull => "memory full",
            Self::NoNetworkService => "no network service",
        }
    }

    pub fn format_response(&self, mode: CmeeMode) -> std::borrow::Cow<'static, str> {
        match mode {
            CmeeMode::Disable => std::borrow::Cow::Borrowed("ERROR\r\n"),
            CmeeMode::Numeric => {
                std::borrow::Cow::Owned(format!("+CMS ERROR: {}\r\n", self.code()))
            }
            CmeeMode::Verbose => {
                std::borrow::Cow::Owned(format!("+CMS ERROR: {}\r\n", self.verbose_str()))
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, ParsableEnum)]
#[repr(u8)]
pub enum SmsMessageStatus {
    #[default]
    ReceivedUnread = 0,
    ReceivedRead = 1,
    StoredUnsent = 2,
    StoredSent = 3,
}

impl From<SmsMessageStatus> for u8 {
    fn from(val: SmsMessageStatus) -> Self {
        val as u8
    }
}

impl std::fmt::Display for SmsMessageStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", *self as u8)
    }
}

pub type MessageStatus = SmsMessageStatus;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SimSmsMessage {
    pub status: SmsMessageStatus,
    pub pdu: Vec<u8>,
}

impl SimSmsMessage {
    #[cfg(test)]
    pub fn new(status: SmsMessageStatus, pdu: Vec<u8>) -> Self {
        Self { status, pdu }
    }

    /// Marks received unread messages as read (TS 27.005 §3.1 / §3.5.3).
    pub fn mark_read(&mut self) {
        if self.status == SmsMessageStatus::ReceivedUnread {
            self.status = SmsMessageStatus::ReceivedRead;
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Response {
    Sim(SimResponse),
    Call(CallResponse),
    Sms(SmsResponse),
    Network(NetworkResponse),
    Data(DataResponse),
    Misc(MiscResponse),
    Sup(SupResponse),
    Stk(StkResponse),
    Ok,
}

impl fmt::Display for Response {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Sim(resp) => write!(f, "{resp}"),
            Self::Call(resp) => write!(f, "{resp}"),
            Self::Sms(resp) => write!(f, "{resp}"),
            Self::Network(resp) => write!(f, "{resp}"),
            Self::Data(resp) => write!(f, "{resp}"),
            Self::Misc(resp) => write!(f, "{resp}"),
            Self::Sup(resp) => write!(f, "{resp}"),
            Self::Stk(resp) => write!(f, "{resp}"),
            Self::Ok => write!(f, "OK\r\n"),
        }
    }
}

impl From<SimResponse> for Response {
    fn from(r: SimResponse) -> Self {
        Self::Sim(r)
    }
}

impl From<CallResponse> for Response {
    fn from(r: CallResponse) -> Self {
        Self::Call(r)
    }
}

impl From<SmsResponse> for Response {
    fn from(r: SmsResponse) -> Self {
        Self::Sms(r)
    }
}

impl From<NetworkResponse> for Response {
    fn from(r: NetworkResponse) -> Self {
        Self::Network(r)
    }
}

impl From<DataResponse> for Response {
    fn from(r: DataResponse) -> Self {
        Self::Data(r)
    }
}

impl From<MiscResponse> for Response {
    fn from(r: MiscResponse) -> Self {
        Self::Misc(r)
    }
}

impl From<SupResponse> for Response {
    fn from(r: SupResponse) -> Self {
        Self::Sup(r)
    }
}

impl From<StkResponse> for Response {
    fn from(r: StkResponse) -> Self {
        Self::Stk(r)
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct HandledCommand {
    pub responses: Vec<Response>,
    pub actions: Vec<CommandAction>,
}

impl HandledCommand {
    pub fn ok() -> Self {
        Self { responses: vec![Response::Ok], actions: vec![] }
    }

    pub fn ok_with_actions(actions: Vec<CommandAction>) -> Self {
        Self { responses: vec![Response::Ok], actions }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CommandError {
    Cme(CmeError),
    Cms(CmsError),
    #[default]
    Generic,
}

impl fmt::Display for CommandError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cme(err) => write!(f, "{err}"),
            Self::Cms(err) => write!(f, "{err}"),
            Self::Generic => write!(f, "generic error"),
        }
    }
}

impl std::error::Error for CommandError {}

impl CommandError {
    pub fn format_response(&self, mode: CmeeMode) -> std::borrow::Cow<'static, str> {
        match self {
            Self::Cme(err) => match mode {
                CmeeMode::Disable => std::borrow::Cow::Borrowed("ERROR\r\n"),
                _ => std::borrow::Cow::Owned(err.format_response(mode)),
            },
            Self::Cms(err) => err.format_response(mode),
            Self::Generic => std::borrow::Cow::Borrowed("ERROR\r\n"),
        }
    }
}

impl From<CmeError> for CommandError {
    fn from(err: CmeError) -> Self {
        Self::Cme(err)
    }
}

impl From<CmsError> for CommandError {
    fn from(err: CmsError) -> Self {
        Self::Cms(err)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ExecutionResult {
    /// The command was successfully handled, yielding responses and/or action.
    Success(HandledCommand),

    /// The command failed with an explicit error reason and optional URCs.
    Error { error: CommandError, urcs: Vec<Response> },

    /// This command has not been refactored yet and should be handled by the
    /// legacy system.
    Unhandled,
}

impl ExecutionResult {
    pub fn ok() -> Self {
        Self::Success(HandledCommand::ok())
    }

    pub fn ok_with_actions(actions: Vec<CommandAction>) -> Self {
        Self::Success(HandledCommand::ok_with_actions(actions))
    }

    pub fn error() -> Self {
        Self::Error { error: CommandError::Generic, urcs: Vec::new() }
    }

    pub fn error_with_urcs(error: impl Into<CommandError>, urcs: Vec<Response>) -> Self {
        Self::Error { error: error.into(), urcs }
    }

    pub fn cme_error(cme: CmeError) -> Self {
        Self::Error { error: CommandError::Cme(cme), urcs: Vec::new() }
    }

    pub fn cms_error(cms: CmsError) -> Self {
        Self::Error { error: CommandError::Cms(cms), urcs: Vec::new() }
    }

    /// Formats the error and preceding URCs into the target string buffer.
    /// Returns `true` if this was an Error or Unhandled result, `false` on
    /// Success.
    pub fn format_error_into(&self, out: &mut String, mode: CmeeMode) -> bool {
        match self {
            Self::Error { error, urcs } => {
                for r in urcs {
                    let _ = write!(out, "{r}");
                }
                out.push_str(&error.format_response(mode));
                true
            }
            Self::Unhandled => {
                out.push_str("ERROR\r\n");
                true
            }
            Self::Success(_) => false,
        }
    }

    pub fn with_urcs(mut self, mut urcs: Vec<Response>) -> Self {
        match self {
            Self::Error { urcs: ref mut target_urcs, .. } => *target_urcs = urcs,
            Self::Success(ref mut handled) => {
                urcs.append(&mut handled.responses);
                handled.responses = urcs;
            }
            Self::Unhandled => {}
        }
        self
    }
}

impl<T: Into<Response>> From<Option<T>> for ExecutionResult {
    fn from(opt: Option<T>) -> Self {
        match opt.map(Into::into) {
            Some(
                resp @ (Response::Call(CallResponse::Ring) | Response::Data(DataResponse::Connect)),
            ) => Self::Success(HandledCommand { responses: vec![resp], actions: vec![] }),
            Some(Response::Call(CallResponse::Empty)) => Self::Success(HandledCommand::default()),
            Some(Response::Call(CallResponse::WithActions(actions))) => {
                Self::Success(HandledCommand::ok_with_actions(actions))
            }
            Some(resp) => Self::Success(HandledCommand {
                responses: vec![resp, Response::Ok],
                actions: vec![],
            }),
            None => Self::ok(),
        }
    }
}

impl<T: Into<ExecutionResult>> From<Result<T, ExecutionResult>> for ExecutionResult {
    fn from(res: Result<T, ExecutionResult>) -> Self {
        match res {
            Ok(val) => val.into(),
            Err(err) => err,
        }
    }
}

impl From<CmeError> for ExecutionResult {
    fn from(err: CmeError) -> Self {
        Self::cme_error(err)
    }
}

impl From<CmsError> for ExecutionResult {
    fn from(err: CmsError) -> Self {
        Self::cms_error(err)
    }
}

impl From<CommandError> for ExecutionResult {
    fn from(err: CommandError) -> Self {
        Self::Error { error: err, urcs: Vec::new() }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostEvent {
    SinkError(u32),
    TimerRequest { chip_id: u32, duration: Duration },
}

#[derive(Debug, Clone, Default)]
pub struct ModemInfo {
    pub id: u32,
    pub calls: Vec<Call>,
    pub ringing: bool,
    pub sms_count: u32,
    pub quirks: Quirks,
    pub rssi: u32,
    pub ber: u32,
    pub voice_registration: RegistrationStatus,
    pub data_registration: RegistrationStatus,
    pub network_configs: Vec<CellNetworkConfig>,
}

/// Multi-technology composite signal strength (22 fields) required by Android's
/// Radio AIDL (`android.hardware.radio.network.SignalStrength`).
///
/// Note: While standard 3GPP TS 27.007 §8.5 `+CSQ` only defines `<rssi>,<ber>`
/// ([`SignalQuality`]), Android's emulator RIL overloads `+CSQ` to pass this
/// full AIDL parcel (b/206814247).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AidlSignalStrength {
    pub gsm_rssi: i32,
    pub gsm_ber: i32,
    pub cdma_dbm: i32,
    pub cdma_ecio: i32,
    pub evdo_dbm: i32,
    pub evdo_ecio: i32,
    pub evdo_snr: i32,
    pub lte_rssi: i32,
    pub lte_rsrp: i32,
    pub lte_rsrq: i32,
    pub lte_rssnr: i32,
    pub lte_cqi: i32,
    pub lte_ta: i32,
    pub tdscdma_rscp: i32,
    pub wcdma_rssi: i32,
    pub wcdma_ber: i32,
    pub nr_ss_rsrp: i32,
    pub nr_ss_rsrq: i32,
    pub nr_ss_sinr: i32,
    pub nr_csi_rsrp: i32,
    pub nr_csi_rsrq: i32,
    pub nr_csi_sinr: i32,
}

impl Default for AidlSignalStrength {
    fn default() -> Self {
        let max = i32::MAX;
        let unknown = crate::constants::CSQ_SIGNAL_UNKNOWN as i32;
        Self {
            gsm_rssi: unknown,
            gsm_ber: unknown,
            cdma_dbm: max,
            cdma_ecio: max,
            evdo_dbm: max,
            evdo_ecio: max,
            evdo_snr: max,
            lte_rssi: max,
            lte_rsrp: max,
            lte_rsrq: max,
            lte_rssnr: max,
            lte_cqi: max,
            lte_ta: max,
            tdscdma_rscp: max,
            wcdma_rssi: max,
            wcdma_ber: max,
            nr_ss_rsrp: max,
            nr_ss_rsrq: max,
            nr_ss_sinr: max,
            nr_csi_rsrp: max,
            nr_csi_rsrq: max,
            nr_csi_sinr: max,
        }
    }
}

impl AidlSignalStrength {
    pub fn from_quality(quality: SignalQuality, act: AccessTechnology) -> Self {
        let SignalQuality { rssi, ber } = quality;
        let mut ss = Self::default();
        match act {
            AccessTechnology::Gsm => {
                ss.gsm_rssi = rssi as i32;
                ss.gsm_ber = ber as i32;
            }
            AccessTechnology::Wcdma => {
                ss.wcdma_rssi = rssi as i32;
                ss.wcdma_ber = ber as i32;
            }
            AccessTechnology::Lte => {
                ss.lte_rssi = rssi as i32;
                ss.lte_rsrp = Self::rssi_to_rsrp(rssi);
            }
            AccessTechnology::Nr => {
                ss.nr_ss_rsrp = Self::rssi_to_rsrp(rssi);
            }
        }
        ss
    }

    fn rssi_to_rsrp(rssi: u8) -> i32 {
        if rssi == crate::constants::CSQ_SIGNAL_UNKNOWN {
            return i32::MAX;
        }
        let rsrp_dbm = -140 + (rssi as i32 * 3);
        let rsrp_csq = -rsrp_dbm;
        rsrp_csq.clamp(44, 140)
    }
}

impl std::fmt::Display for AidlSignalStrength {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}",
            self.gsm_rssi,
            self.gsm_ber,
            self.cdma_dbm,
            self.cdma_ecio,
            self.evdo_dbm,
            self.evdo_ecio,
            self.evdo_snr,
            self.lte_rssi,
            self.lte_rsrp,
            self.lte_rsrq,
            self.lte_rssnr,
            self.lte_cqi,
            self.lte_ta,
            self.tdscdma_rscp,
            self.wcdma_rssi,
            self.wcdma_ber,
            self.nr_ss_rsrp,
            self.nr_ss_rsrq,
            self.nr_ss_sinr,
            self.nr_csi_rsrp,
            self.nr_csi_rsrq,
            self.nr_csi_sinr
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, ParsableEnum)]
#[repr(u8)]
pub enum CopsMode {
    #[default]
    Automatic = 0,
    Manual = 1,
    Deregister = 2,
    SetFormatOnly = 3,
    ManualAutomatic = 4,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, ParsableEnum)]
#[repr(u8)]
pub enum CopsFormat {
    LongAlphanumeric = 0,
    ShortAlphanumeric = 1,
    #[default]
    Numeric = 2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum OperatorStatus {
    Unknown = 0,
    Available = 1,
    Current = 2,
    Forbidden = 3,
}

impl std::fmt::Display for OperatorStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", *self as u8)
    }
}

/// AT+CHLD Call Hold operations (3GPP TS 27.007 Section 7.22).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum CallHoldAction {
    /// AT+CHLD=0: Releases all held calls or rejects a waiting call.
    ReleaseHeld = 0,
    /// AT+CHLD=1: Releases all active calls and accepts held/waiting call.
    ReleaseAndAccept = 1,
    /// AT+CHLD=2: Places active calls on hold and accepts held/waiting call.
    HoldAndAccept = 2,
    /// AT+CHLD=3: Adds a held call to the active conversation (Conference
    /// call).
    Conference = 3,
    /// AT+CHLD=4: Explicit Call Transfer (ECT).
    Transfer = 4,
    /// AT+CHLD=5: User-to-User Signaling.
    UserToUserSignaling = 5,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CallHoldParam {
    pub op: CallHoldAction,
    pub call_id: Option<u8>,
}

impl<'a> Parsable<'a> for CallHoldParam {
    fn parse(input: &'a [u8]) -> nom::IResult<&'a [u8], Self> {
        let (input, val) = u8::parse(input)?;
        let (op_val, call_id) = if val >= 100 {
            (val / 100, Some(val % 100))
        } else if val >= 10 {
            (val / 10, Some(val % 10))
        } else {
            (val, None)
        };
        let op = match op_val {
            0 => CallHoldAction::ReleaseHeld,
            1 => CallHoldAction::ReleaseAndAccept,
            2 => CallHoldAction::HoldAndAccept,
            3 => CallHoldAction::Conference,
            4 => CallHoldAction::Transfer,
            5 => CallHoldAction::UserToUserSignaling,
            _ => {
                return Err(nom::Err::Error(nom::error::Error::new(
                    input,
                    nom::error::ErrorKind::MapRes,
                )));
            }
        };
        Ok((input, Self { op, call_id }))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ParsableEnum)]
#[repr(u8)]
pub enum FacilityLockMode {
    Unlock = 0,
    Lock = 1,
    QueryStatus = 2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, ParsableEnum)]
#[repr(u8)]
pub enum SmsBroadcastMode {
    #[default]
    Accept = 0,
    Discard = 1,
}

/// Cell broadcast message configuration (3GPP TS 27.005 §3.3.4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BroadcastConfig {
    pub mode: SmsBroadcastMode,
    pub mids: String,
    pub dcss: String,
}

impl Default for BroadcastConfig {
    fn default() -> Self {
        Self { mode: SmsBroadcastMode::Accept, mids: String::new(), dcss: String::new() }
    }
}

/// Radio signal quality parameters (3GPP TS 27.007 §8.5 `+CSQ`: `<rssi>`,
/// `<ber>`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SignalQuality {
    pub rssi: u8,
    pub ber: u8,
}

impl SignalQuality {
    pub const fn new(rssi: u8, ber: u8) -> Self {
        Self { rssi, ber }
    }
}

impl Default for SignalQuality {
    fn default() -> Self {
        Self { rssi: 20, ber: 99 }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, ParsableEnum)]
#[repr(u8)]
pub enum ProductSerialNumberType {
    #[default]
    ImeiWithInfo = 0,
    Imei = 1,
    ImeiWithSvn = 2,
    Svn = 3,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, ParsableEnum)]
#[repr(u8)]
pub enum IcfFormat {
    Data8Stop2 = 1,
    Data8Parity1Stop1 = 2,
    #[default]
    Data8Stop1 = 3,
    Data7Stop2 = 4,
    Data7Parity1Stop1 = 5,
    Data7Stop1 = 6,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, ParsableEnum)]
#[repr(u8)]
pub enum IcfParity {
    Odd = 0,
    Even = 1,
    Mark = 2,
    #[default]
    Space = 3,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, ParsableEnum)]
#[repr(u8)]
pub enum FlowControlMode {
    None = 0,
    XonXoff = 1,
    #[default]
    Hardware = 2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ParsableEnum)]
#[repr(u8)]
pub enum CallMode {
    SingleMode = 0,
    AlternateVoiceData = 1,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, ParsableEnum)]
#[repr(u8)]
pub enum CdmaSubscriptionSource {
    #[default]
    RuimSim = 0,
    Nv = 1,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ParsableEnum)]
#[repr(u8)]
pub enum ClipProvisionStatus {
    NotProvisioned = 0,
    Provisioned = 1,
    Unknown = 2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FormattedNumber<'a> {
    pub number: &'a str,
    pub toa: TypeOfAddress,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum NumberPresentation {
    #[default]
    Allowed = 0,
    Restricted = 1,
    NotAvailable = 2,
}

impl NumberPresentation {
    pub fn format_number<'a>(&self, number: Option<&'a PhoneNumber>) -> FormattedNumber<'a> {
        match self {
            Self::Restricted | Self::NotAvailable => {
                FormattedNumber { number: "", toa: TypeOfAddress::Unknown }
            }
            Self::Allowed => {
                if let Some(num) = number {
                    FormattedNumber { number: num.normalized(), toa: num.toa() }
                } else {
                    FormattedNumber { number: "", toa: TypeOfAddress::Unknown }
                }
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ParsableEnum)]
#[repr(u8)]
pub enum CallWaitingMode {
    Disable = 0,
    Enable = 1,
    Query = 2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ParsableEnum)]
#[repr(u8)]
pub enum CallWaitingStatus {
    NotActive = 0,
    Active = 1,
}

impl<'a> Parsable<'a> for bool {
    fn parse(input: &'a [u8]) -> nom::IResult<&'a [u8], Self> {
        let (input, val) = u8::parse(input)?;
        match val {
            0 => Ok((input, false)),
            1 => Ok((input, true)),
            _ => Err(nom::Err::Error(nom::error::Error::new(input, nom::error::ErrorKind::MapRes))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, ParsableEnum)]
#[repr(u8)]
pub enum ClirMode {
    #[default]
    SubscriptionDefault = 0,
    Invocation = 1,
    Suppression = 2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum CallForwardCondition {
    Unconditional = 0,
    Busy = 1,
    NoReply = 2,
    NotReachable = 3,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, ParsableEnum)]
#[repr(u8)]
pub enum CallForwardingReason {
    Unconditional = 0,
    Busy = 1,
    NoReply = 2,
    NotReachable = 3,
    All = 4,
    AllConditional = 5,
}

impl CallForwardingReason {
    pub const UNCONDITIONAL: &'static [CallForwardCondition] =
        &[CallForwardCondition::Unconditional];
    pub const MOBILE_BUSY: &'static [CallForwardCondition] = &[CallForwardCondition::Busy];
    pub const NO_REPLY: &'static [CallForwardCondition] = &[CallForwardCondition::NoReply];
    pub const NOT_REACHABLE: &'static [CallForwardCondition] =
        &[CallForwardCondition::NotReachable];
    pub const ALL: &'static [CallForwardCondition] = &[
        CallForwardCondition::Unconditional,
        CallForwardCondition::Busy,
        CallForwardCondition::NoReply,
        CallForwardCondition::NotReachable,
    ];
    pub const ALL_CONDITIONAL: &'static [CallForwardCondition] = &[
        CallForwardCondition::Busy,
        CallForwardCondition::NoReply,
        CallForwardCondition::NotReachable,
    ];

    pub fn conditions(self) -> &'static [CallForwardCondition] {
        match self {
            Self::Unconditional => Self::UNCONDITIONAL,
            Self::Busy => Self::MOBILE_BUSY,
            Self::NoReply => Self::NO_REPLY,
            Self::NotReachable => Self::NOT_REACHABLE,
            Self::All => Self::ALL,
            Self::AllConditional => Self::ALL_CONDITIONAL,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ParsableEnum)]
#[repr(u8)]
pub enum CallForwardingMode {
    Disable = 0,
    Enable = 1,
    Query = 2,
    Registration = 3,
    Erasure = 4,
}

/// Call Forwarding no-reply wait time in seconds (1..=30) per 3GPP TS 27.007
/// §7.11.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CallForwardTime(pub u8);

impl<'a> Parsable<'a> for CallForwardTime {
    fn parse(input: &'a [u8]) -> nom::IResult<&'a [u8], Self> {
        let (input, val) = <u8 as Parsable>::parse(input)?;
        if (1..=30).contains(&val) {
            Ok((input, Self(val)))
        } else {
            Err(nom::Err::Error(nom::error::Error::new(input, nom::error::ErrorKind::Verify)))
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, ParsableEnum)]
#[repr(u8)]
pub enum CallForwardNumberType {
    NoValidInfo = 0,
    Uri = 1,
    #[default]
    Number = 2,
}

impl CallForwardNumberType {
    pub const fn as_u8(self) -> u8 {
        self as u8
    }
}

/// Call forwarding destination per 3GPP TS 27.007 §7.35.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CallForwardDestination {
    // Prevent Android PhoneNumberUtils from prepending '+' to URI destinations.
    Uri(String),
    Number { digits: String, ton: CallForwardTon },
}

impl CallForwardDestination {
    pub fn number_type(&self) -> CallForwardNumberType {
        match self {
            Self::Uri(_) => CallForwardNumberType::Uri,
            Self::Number { .. } => CallForwardNumberType::Number,
        }
    }

    pub fn ton(&self) -> CallForwardTon {
        match self {
            Self::Uri(_) => CallForwardTon(0),
            Self::Number { ton, .. } => *ton,
        }
    }

    pub fn as_str(&self) -> &str {
        match self {
            Self::Uri(uri) => uri.as_str(),
            Self::Number { digits, .. } => digits.as_str(),
        }
    }
}

// Accept TS 24.008 address octets (128..=255) passed by Goldfish RIL in the TON slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CallForwardTon(pub u8);

impl CallForwardTon {
    pub const fn as_u8(self) -> u8 {
        self.0
    }

    pub const fn ton(self) -> u8 {
        if self.0 >= 128 { (self.0 >> 4) & 0x07 } else { self.0 }
    }

    pub const fn as_address_octet(self) -> u8 {
        if self.0 >= 128 { self.0 } else { 0x80 | (self.ton() << 4) | 0x01 }
    }

    pub fn from_wire(raw: u8, quirks: Quirks) -> Result<Self, ExecutionResult> {
        match raw {
            0..=7 => Ok(Self(raw)),
            128..=255 if quirks.goldfish_ril_37_or_earlier => Ok(Self(raw)),
            _ => Err(ExecutionResult::cme_error(CmeError::IncorrectParameters)),
        }
    }
}

impl<'a> Parsable<'a> for CallForwardTon {
    fn parse(input: &'a [u8]) -> nom::IResult<&'a [u8], Self> {
        let (rem, val) = <u8 as Parsable>::parse(input)?;
        if val <= 7 || val >= 128 {
            Ok((rem, Self(val)))
        } else {
            Err(nom::Err::Error(nom::error::Error::new(input, nom::error::ErrorKind::Verify)))
        }
    }
}

impl Default for CallForwardTon {
    fn default() -> Self {
        Self(129)
    }
}

impl From<TypeOfAddress> for CallForwardTon {
    fn from(toa: TypeOfAddress) -> Self {
        Self(toa.as_u8())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ParsableEnum)]
#[repr(u8)]
pub enum UssdMode {
    DisableUrc = 0,
    EnableUrc = 1,
    Cancel = 2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ClirStatus {
    NotActive = 0,
    Active = 1,
    Unknown = 2,
    TemporaryRestricted = 3,
    TemporaryAllowed = 4,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum UssdStatus {
    NoActionRequired = 0,
    ActionRequired = 1,
    TerminatedByNetwork = 2,
    OtherClientResponded = 3,
    NotSupported = 4,
    NetworkTimeout = 5,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, ParsableEnum)]
#[repr(u8)]
pub enum SpeakerMuteMode {
    Off = 0,
    #[default]
    OnAndOffOnCarrier = 1,
    AlwaysOn = 2,
    OffDialRingOnConnectOffCarrier = 3,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, ParsableEnum)]
#[repr(u8)]
pub enum CdmaRoamingPreference {
    #[default]
    HomeOnly = 0,
    RoamingOnly = 1,
    Automatic = 2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PdpType {
    Ip,
    Ipv6,
    Ipv4v6,
    Ppp,
    NonIp,
    Cell,
}

impl<'a> Parsable<'a> for PdpType {
    fn parse(input: &'a [u8]) -> nom::IResult<&'a [u8], Self> {
        let (input, quoted) = QuotedString::parse(input)?;
        match quoted.as_str() {
            "IP" => Ok((input, Self::Ip)),
            "IPV6" => Ok((input, Self::Ipv6)),
            "IPV4V6" => Ok((input, Self::Ipv4v6)),
            "PPP" => Ok((input, Self::Ppp)),
            "Non-IP" => Ok((input, Self::NonIp)),
            "Cell" => Ok((input, Self::Cell)),
            _ => Err(nom::Err::Error(nom::error::Error::new(input, nom::error::ErrorKind::MapRes))),
        }
    }
}

impl std::fmt::Display for PdpType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PdpType::Ip => write!(f, "IP"),
            PdpType::Ipv6 => write!(f, "IPV6"),
            PdpType::Ipv4v6 => write!(f, "IPV4V6"),
            PdpType::Ppp => write!(f, "PPP"),
            PdpType::NonIp => write!(f, "Non-IP"),
            PdpType::Cell => write!(f, "Cell"),
        }
    }
}

/// 3GPP TS 27.007 §10.1.12 Layer 2 Protocol `<L2P>` for `AT+CGDATA`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Layer2Protocol {
    #[default]
    Ppp,
    Null,
    Ip,
    Packet,
}
impl std::str::FromStr for Layer2Protocol {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "PPP" => Ok(Self::Ppp),
            "NULL" => Ok(Self::Null),
            "IP" => Ok(Self::Ip),
            "PACKET" => Ok(Self::Packet),
            _ => Err(()),
        }
    }
}

impl std::fmt::Display for CopsMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", *self as u8)
    }
}

impl std::fmt::Display for CopsFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", *self as u8)
    }
}

impl std::fmt::Display for RegistrationUnsolicitedMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", *self as u8)
    }
}

impl std::fmt::Display for RadioPowerLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", *self as u8)
    }
}

impl std::fmt::Display for CmeeMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", *self as u8)
    }
}

impl std::fmt::Display for SmsBroadcastMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", *self as u8)
    }
}

impl std::fmt::Display for IcfFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", *self as u8)
    }
}

impl std::fmt::Display for IcfParity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", *self as u8)
    }
}

impl std::fmt::Display for FlowControlMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", *self as u8)
    }
}

impl std::fmt::Display for CdmaSubscriptionSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", *self as u8)
    }
}

impl std::fmt::Display for CallWaitingStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", *self as u8)
    }
}

impl std::fmt::Display for CallForwardTime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::fmt::Display for ClirMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", *self as u8)
    }
}

impl std::fmt::Display for SpeakerMuteMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", *self as u8)
    }
}

impl std::fmt::Display for ClirStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", *self as u8)
    }
}

impl std::fmt::Display for ClipProvisionStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", *self as u8)
    }
}

impl std::fmt::Display for UssdStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", *self as u8)
    }
}

impl std::fmt::Display for CdmaRoamingPreference {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", *self as u8)
    }
}

/// Radio access technologies modelled by the modem.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, ParsableEnum)]
#[repr(u8)]
pub enum AccessTechnology {
    Gsm = 0,
    Wcdma = 2,
    #[default]
    Lte = 7,
    Nr = 11,
}

impl AccessTechnology {
    pub fn from_wire(wire: u8, quirks: Quirks) -> Result<Self, ExecutionResult> {
        let is_goldfish = quirks.goldfish_ril_37_or_earlier;
        match wire {
            0 | 1 | 8 => Ok(Self::Gsm),
            2 | 4 | 5 => Ok(Self::Wcdma),
            3 if is_goldfish => Ok(Self::Lte),
            3 => Ok(Self::Gsm),
            6 if is_goldfish => Ok(Self::Nr),
            6 => Ok(Self::Wcdma),
            7 | 9 | 10 => Ok(Self::Lte),
            11 | 12 => Ok(Self::Nr),
            _ => Err(ExecutionResult::cme_error(CmeError::IncorrectParameters)),
        }
    }
}

impl std::fmt::Display for AccessTechnology {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", *self as u8)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum CtecTechnology {
    Gsm = 1,
    Wcdma = 2,
    Cdma = 4,
    Evdo = 8,
    Tdscdma = 16,
    #[default]
    Lte = 32,
    Nr = 64,
}

impl CtecTechnology {
    pub const ALL: &'static [CtecTechnology] =
        &[Self::Gsm, Self::Wcdma, Self::Cdma, Self::Evdo, Self::Tdscdma, Self::Lte, Self::Nr];

    pub const fn all_mask() -> u8 {
        let mut mask = 0u8;
        let mut slice = Self::ALL;
        while let [tech, rest @ ..] = slice {
            mask |= *tech as u8;
            slice = rest;
        }
        mask
    }

    /// Converts an `AT+CTEC` wire value to a single active `CtecTechnology`.
    pub fn from_wire(wire: u8) -> Result<Self, ExecutionResult> {
        match wire {
            1 => Ok(Self::Gsm),
            2 => Ok(Self::Wcdma),
            4 => Ok(Self::Cdma),
            8 => Ok(Self::Evdo),
            16 => Ok(Self::Tdscdma),
            32 => Ok(Self::Lte),
            64 => Ok(Self::Nr),
            _ => Err(ExecutionResult::cme_error(CmeError::IncorrectParameters)),
        }
    }
}

impl std::fmt::Display for CtecTechnology {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", *self as u8)
    }
}

/// Facility locks supported by AT+CLCK and AT+CPWD.
///
/// Ref: 3GPP TS 27.007 § 7.4 (Facility lock +CLCK), § 7.5 (Change password
/// +CPWD), 3GPP TS 22.088 (Call Barring supplementary services),
/// 3GPP TS 22.030 § 6.5.6.5 (Supplementary service control codes).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Facility {
    /// SIM lock (PIN1) — "SC" (TS 27.007 § 7.4).
    SimPin,
    /// SIM PIN2 — "P2" (TS 27.007 § 7.5). Note: Used with +CPWD to change PIN2.
    SimPin2,
    /// Fixed Dialing Number — "FD" (TS 27.007 § 7.4).
    FixedDial,
    /// Bar All Outgoing Calls — "AO" (TS 27.007 § 7.4, TS 22.088 BAOC,
    /// activation code 33).
    BarAllOutgoing,
    /// Bar Outgoing International Calls — "OI" (TS 27.007 § 7.4, TS 22.088
    /// BOIC, activation code 331).
    BarOutgoingInternational,
    /// Bar Outgoing International Calls except to Home PLMN — "OX" (TS 27.007 §
    /// 7.4, TS 22.088 BOIC-exHC, activation code 332).
    BarOutgoingInternationalExceptHome,
    /// Bar All Incoming Calls — "AI" (TS 27.007 § 7.4, TS 22.088 BAIC,
    /// activation code 35).
    BarAllIncoming,
    /// Bar Incoming Calls when Roaming outside the home PLMN country — "IR" (TS
    /// 27.007 § 7.4, TS 22.088 BIC-Roam, activation code 351).
    BarIncomingRoaming,
    /// All Barring Services — "AB" (TS 27.007 § 7.4, TS 22.030 § 6.5.6.5,
    /// activation code 330).
    BarAll,
    /// All Outgoing Barring Services — "AG" (TS 27.007 § 7.4, TS 22.030 §
    /// 6.5.6.5, activation code 333).
    BarAllOutgoingServices,
    /// All Incoming Barring Services — "AC" (TS 27.007 § 7.4, TS 22.030 §
    /// 6.5.6.5, activation code 353).
    BarAllIncomingServices,
    /// Any unrecognized facility string.
    Unsupported,
}

impl Facility {
    /// Returns the standard 3GPP 2-character facility code, or "UNSUPPORTED".
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SimPin => "SC",
            Self::SimPin2 => "P2",
            Self::FixedDial => "FD",
            Self::BarAllOutgoing => "AO",
            Self::BarOutgoingInternational => "OI",
            Self::BarOutgoingInternationalExceptHome => "OX",
            Self::BarAllIncoming => "AI",
            Self::BarIncomingRoaming => "IR",
            Self::BarAll => "AB",
            Self::BarAllOutgoingServices => "AG",
            Self::BarAllIncomingServices => "AC",
            Self::Unsupported => "UNSUPPORTED",
        }
    }

    /// Resolves a facility from its 2-character string code.
    pub fn from_str(s: &str) -> Self {
        match s {
            "SC" => Self::SimPin,
            "P2" => Self::SimPin2,
            "FD" => Self::FixedDial,
            "AO" => Self::BarAllOutgoing,
            "OI" => Self::BarOutgoingInternational,
            "OX" => Self::BarOutgoingInternationalExceptHome,
            "AI" => Self::BarAllIncoming,
            "IR" => Self::BarIncomingRoaming,
            "AB" => Self::BarAll,
            "AG" => Self::BarAllOutgoingServices,
            "AC" => Self::BarAllIncomingServices,
            _ => Self::Unsupported,
        }
    }

    /// Returns true if this facility is a 3GPP TS 22.088 / TS 22.030 call
    /// barring supplementary service.
    pub const fn is_call_barring(self) -> bool {
        matches!(
            self,
            Self::BarAll
                | Self::BarAllOutgoingServices
                | Self::BarAllIncomingServices
                | Self::BarAllOutgoing
                | Self::BarOutgoingInternational
                | Self::BarOutgoingInternationalExceptHome
                | Self::BarAllIncoming
                | Self::BarIncomingRoaming
        )
    }
}

impl<'a> Parsable<'a> for Facility {
    fn parse(input: &'a [u8]) -> nom::IResult<&'a [u8], Self> {
        let (input, quoted) = QuotedString::parse(input)?;
        Ok((input, Self::from_str(quoted.as_str())))
    }
}

/// Query selector for AT+CPINR remaining retry queries (3GPP TS 27.007 § 8.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PinType {
    SimPin,
    SimPuk,
    SimPin2,
    SimPuk2,
}

impl PinType {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SimPin => "SIM PIN",
            Self::SimPuk => "SIM PUK",
            Self::SimPin2 => "SIM PIN2",
            Self::SimPuk2 => "SIM PUK2",
        }
    }
}

impl<'a> Parsable<'a> for PinType {
    fn parse(input: &'a [u8]) -> nom::IResult<&'a [u8], Self> {
        let (input, quoted) = QuotedString::parse(input)?;
        let pin_type = match quoted.as_str() {
            "SIM PIN" => Self::SimPin,
            "SIM PUK" => Self::SimPuk,
            "SIM PIN2" => Self::SimPin2,
            "SIM PUK2" => Self::SimPuk2,
            _ => {
                return Err(nom::Err::Error(nom::error::Error::new(
                    input,
                    nom::error::ErrorKind::Tag,
                )));
            }
        };
        Ok((input, pin_type))
    }
}

impl<'a> Parsable<'a> for crate::apdu::Instruction {
    fn parse(input: &'a [u8]) -> nom::IResult<&'a [u8], Self> {
        let (input, val) = u8::parse(input)?;
        Ok((input, crate::apdu::Instruction::from(val)))
    }
}

/// ITU-T V.250 §6.3.6 Speaker volume level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, ParsableEnum)]
#[repr(u8)]
pub enum SpeakerVolume {
    Off = 0,
    #[default]
    Low = 1,
    Medium = 2,
    High = 3,
}

impl std::fmt::Display for SpeakerVolume {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", *self as u8)
    }
}

/// 3GPP TS 27.007 §10.1.13 Packet Domain Event Reporting mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, ParsableEnum)]
#[repr(u8)]
pub enum PacketEventReportingMode {
    #[default]
    Buffer = 0,
    Discard = 1,
    Forward = 2,
}

/// 3GPP TS 27.007 §5.5 TE character set selection (+CSCS).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CharacterSet {
    #[default]
    Gsm,
    Hex,
    Ira,
    Pccp437,
    Iso8859_1,
    Ucs2,
    Utf8,
}

impl<'a> Parsable<'a> for CharacterSet {
    fn parse(input: &'a [u8]) -> nom::IResult<&'a [u8], Self> {
        let (rem, qs) = QuotedString::parse(input)?;
        match qs.as_str() {
            "GSM" => Ok((rem, Self::Gsm)),
            "HEX" => Ok((rem, Self::Hex)),
            "IRA" => Ok((rem, Self::Ira)),
            "PCCP437" => Ok((rem, Self::Pccp437)),
            "8859-1" => Ok((rem, Self::Iso8859_1)),
            "UCS2" => Ok((rem, Self::Ucs2)),
            "UTF-8" => Ok((rem, Self::Utf8)),
            _ => Err(nom::Err::Error(nom::error::Error::new(input, nom::error::ErrorKind::Tag))),
        }
    }
}

/// 3GPP TS 22.004 / TS 27.007 §7.4, §7.11, §7.12 Telecommunication service
/// class `<class>`. Represented as a sum of integers (bitmask).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ServiceClass(pub u8);

impl ServiceClass {
    pub const VOICE: Self = Self(1);
    pub const DATA: Self = Self(2);
    pub const FAX: Self = Self(4);
    pub const VOICE_DATA_FAX: Self = Self(7);
    pub const SHORT_MESSAGE_SERVICE: Self = Self(8);
    pub const DATA_CIRCUIT_SYNC: Self = Self(16);
    pub const DATA_CIRCUIT_ASYNC: Self = Self(32);
    pub const DEDICATED_PACKET_ACCESS: Self = Self(64);
    pub const DEDICATED_PAD_ACCESS: Self = Self(128);
    pub const ALL_SERVICES: Self = Self(255);

    /// Returns `true` if all bits in `other` are set in `self`.
    pub const fn contains(self, other: Self) -> bool {
        (self.0 & other.0) == other.0
    }

    pub const fn as_u8(self) -> u8 {
        self.0
    }
}

impl Default for ServiceClass {
    fn default() -> Self {
        Self::VOICE_DATA_FAX
    }
}

impl<'a> Parsable<'a> for ServiceClass {
    fn parse(input: &'a [u8]) -> nom::IResult<&'a [u8], Self> {
        let (input, val) = <u8 as Parsable>::parse(input)?;
        Ok((input, Self(val)))
    }
}

impl std::fmt::Display for ServiceClass {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// 3GPP TS 27.007 §7.14 DTMF tone character [0-9*#A-Da-d].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DtmfTone(pub u8);

impl std::fmt::Display for DtmfTone {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0 as char)
    }
}

impl<'a> Parsable<'a> for DtmfTone {
    fn parse(input: &'a [u8]) -> nom::IResult<&'a [u8], Self> {
        let (rem, s) = parse_quoted_or_unquoted(input)?;
        let mut chars = s.chars();
        let ch = chars.next().ok_or_else(|| {
            nom::Err::Error(nom::error::Error::new(input, nom::error::ErrorKind::Verify))
        })?;
        if chars.next().is_some() {
            return Err(nom::Err::Error(nom::error::Error::new(
                input,
                nom::error::ErrorKind::Verify,
            )));
        }

        if matches!(ch, '0'..='9' | '*' | '#' | 'A'..='D' | 'a'..='d') {
            Ok((rem, DtmfTone(ch as u8)))
        } else {
            Err(nom::Err::Error(nom::error::Error::new(input, nom::error::ErrorKind::Verify)))
        }
    }
}

/// 3GPP TS 27.007 §7.14 DTMF command arguments (+VTS=<tone>[,<duration>]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DtmfArgs {
    pub tone: DtmfTone,
    pub duration: Option<u32>,
}

impl<'a> Parsable<'a> for DtmfArgs {
    fn parse(input: &'a [u8]) -> nom::IResult<&'a [u8], Self> {
        let (input, tone) = DtmfTone::parse(input)?;
        let (input, duration) = if let Ok((rem, _)) =
            nom::bytes::complete::tag::<_, _, nom::error::Error<&[u8]>>(b",")(input)
        {
            let (rem, dur) = u32::parse(rem)?;
            (rem, Some(dur))
        } else {
            (input, None)
        };
        Ok((input, DtmfArgs { tone, duration }))
    }
}

/// Cuttlefish / Android vendor CTEC 4-byte priority tier bitmask.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CtecPreferredMask(pub u32);

impl CtecPreferredMask {
    pub const VALID_RAT_MASK: u8 = CtecTechnology::all_mask();
}

impl<'a> Parsable<'a> for CtecPreferredMask {
    fn parse(input: &'a [u8]) -> nom::IResult<&'a [u8], Self> {
        let (rem, raw_str) = parse_quoted_or_unquoted(input)?;
        let clean = raw_str.trim();
        let clean = clean.strip_prefix("0x").or_else(|| clean.strip_prefix("0X")).unwrap_or(clean);
        let val = u32::from_str_radix(clean, 16).map_err(|_err| {
            nom::Err::Error(nom::error::Error::new(input, nom::error::ErrorKind::MapRes))
        })?;

        if val.to_le_bytes().iter().any(|&tier| (tier & !Self::VALID_RAT_MASK) != 0) {
            return Err(nom::Err::Error(nom::error::Error::new(
                input,
                nom::error::ErrorKind::Verify,
            )));
        }

        Ok((rem, CtecPreferredMask(val)))
    }
}

/// 3GPP TS 27.007 §11.1.15 UICC Application Identifier (AID) hex string.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub struct ApplicationId<'a>(pub &'a str);

impl<'a> ApplicationId<'a> {
    pub fn as_str(self) -> &'a str {
        self.0
    }
}

impl<'a> Parsable<'a> for ApplicationId<'a> {
    fn parse(input: &'a [u8]) -> nom::IResult<&'a [u8], Self> {
        let (rem, raw_str) = parse_quoted_or_unquoted(input)?;
        if !raw_str.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(nom::Err::Error(nom::error::Error::new(
                input,
                nom::error::ErrorKind::Verify,
            )));
        }
        Ok((rem, ApplicationId(raw_str)))
    }
}

/// 3GPP TS 27.007 §10.1.1 / Goldfish RIL PDP context activation arguments.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PdpContextActivateArgs {
    pub cid: u8,
    pub state: bool,
}

impl<'a> Parsable<'a> for PdpContextActivateArgs {
    fn parse(input: &'a [u8]) -> nom::IResult<&'a [u8], Self> {
        let (input, first) = u8::parse(input)?;
        let (input, _) = nom::bytes::complete::tag(b",")(input)?;
        let (input, second) = u8::parse(input)?;

        // Compatibility hack for legacy Goldfish/Reference RIL.
        // It sends AT+CGACT using non-standard <cid>,<state> format.
        // We detect this by checking if the parsed state is > 1 (which means it's
        // actually the CID) or if the parsed CID is 0 (which means it's the state 0).
        let (cid, state_val) =
            if first > 1 || second == 0 { (first, second) } else { (second, first) };
        if state_val > 1 {
            return Err(nom::Err::Error(nom::error::Error::new(
                input,
                nom::error::ErrorKind::MapRes,
            )));
        }
        Ok((input, PdpContextActivateArgs { cid, state: state_val == 1 }))
    }
}

/// 3GPP TS 27.005 § 3.5.1 Send SMS command arguments (+CMGS).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SendSmsArgs<'a> {
    Text {
        /// Destination address (<da> in TS 27.005 § 3.5.1).
        destination_address: QuotedString<'a>,
        /// Type of destination address (<toda> in TS 27.005 § 3.5.1).
        type_of_destination_address: Option<TypeOfAddress>,
    },
    Pdu {
        /// TPDU length in octets.
        length: usize,
    },
}

impl<'a> Parsable<'a> for SendSmsArgs<'a> {
    fn parse(input: &'a [u8]) -> nom::IResult<&'a [u8], Self> {
        if let Ok((rem, destination_address)) = QuotedString::parse(input) {
            let (rem, type_of_destination_address) = nom::combinator::opt(
                nom::sequence::preceded(nom::bytes::complete::tag(b","), TypeOfAddress::parse),
            )(rem)?;
            return Ok((
                rem,
                SendSmsArgs::Text { destination_address, type_of_destination_address },
            ));
        }
        let (input, length) = nom::combinator::map_res(
            nom::combinator::map_res(nom::character::complete::digit1, std::str::from_utf8),
            |s: &str| s.parse::<usize>(),
        )(input)?;
        Ok((input, SendSmsArgs::Pdu { length }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constants::UiccFileId;

    #[test]
    fn test_call_forward_ton_parse() {
        let (rem, ton) = <CallForwardTon as Parsable>::parse(b"0").unwrap();
        assert_eq!(rem, b"");
        assert_eq!(ton, CallForwardTon(0));

        let (rem, ton) = <CallForwardTon as Parsable>::parse(b"7").unwrap();
        assert_eq!(rem, b"");
        assert_eq!(ton, CallForwardTon(7));

        let (rem, ton) = <CallForwardTon as Parsable>::parse(b"128").unwrap();
        assert_eq!(rem, b"");
        assert_eq!(ton, CallForwardTon(128));

        let (rem, ton) = <CallForwardTon as Parsable>::parse(b"129").unwrap();
        assert_eq!(rem, b"");
        assert_eq!(ton, CallForwardTon(129));

        let (rem, ton) = <CallForwardTon as Parsable>::parse(b"145").unwrap();
        assert_eq!(rem, b"");
        assert_eq!(ton, CallForwardTon(145));

        let (rem, ton) = <CallForwardTon as Parsable>::parse(b"255").unwrap();
        assert_eq!(rem, b"");
        assert_eq!(ton, CallForwardTon(255));

        assert!(<CallForwardTon as Parsable>::parse(b"8").is_err());
        assert!(<CallForwardTon as Parsable>::parse(b"127").is_err());
    }

    #[test]
    fn test_phone_number_parse() {
        let (rem, phone) = PhoneNumber::parse(b"+16505550100").unwrap();
        assert_eq!(rem, b"");
        assert_eq!(phone.as_str(), "+16505550100");

        let (rem, phone) = PhoneNumber::parse(b"12345*678#").unwrap();
        assert_eq!(rem, b"");
        assert_eq!(phone.as_str(), "12345*678#");

        // Prefix parsing behavior: stops at first invalid char
        let (rem, phone) = PhoneNumber::parse(b"123a45").unwrap();
        assert_eq!(rem, b"a45");
        assert_eq!(phone.as_str(), "123");

        // Plus at non-start is invalid and ends digits matching
        let (rem, phone) = PhoneNumber::parse(b"12+34").unwrap();
        assert_eq!(rem, b"+34");
        assert_eq!(phone.as_str(), "12");
    }

    #[test]
    fn test_dial_string_number_parsing() {
        // Valid
        let (rem, dial) = DialString::parse(b"12345").unwrap();
        assert_eq!(rem, b"");
        assert_eq!(dial.number.as_str(), "12345");

        let (rem, dial) = DialString::parse(b"+16505550100").unwrap();
        assert_eq!(rem, b"");
        assert_eq!(dial.number.as_str(), "+16505550100");

        // Strips CLIR and modifiers
        let (rem, dial) = DialString::parse(b"12345i,1234").unwrap();
        assert_eq!(rem, b"");
        assert_eq!(dial.number.as_str(), "12345");

        let (rem, dial) = DialString::parse(b"+12345I,678").unwrap();
        assert_eq!(rem, b"");
        assert_eq!(dial.number.as_str(), "+12345");

        // Remaining unparsed bytes on invalid dial characters (like 'a')
        let (rem, _) = DialString::parse(b"123a45").unwrap();
        assert_eq!(rem, b"a45");
        let (rem, _) = DialString::parse(b"123b45").unwrap();
        assert_eq!(rem, b"b45");
        let (rem, _) = DialString::parse(b"123c45").unwrap();
        assert_eq!(rem, b"c45");
    }

    #[test]
    fn test_dial_string_clir() {
        let (rem, dial) = DialString::parse(b"12345i").unwrap();
        assert_eq!(rem, b"");
        assert_eq!(dial.clir(ClirMode::SubscriptionDefault), ClirMode::Suppression);

        let (rem, dial) = DialString::parse(b"12345I").unwrap();
        assert_eq!(rem, b"");
        assert_eq!(dial.clir(ClirMode::SubscriptionDefault), ClirMode::Invocation);

        let (rem, dial) = DialString::parse(b"12345").unwrap();
        assert_eq!(rem, b"");
        assert_eq!(dial.clir(ClirMode::SubscriptionDefault), ClirMode::SubscriptionDefault);
        assert_eq!(dial.clir(ClirMode::Invocation), ClirMode::Invocation);

        // Modifiers with pause/wait
        let (rem, dial) = DialString::parse(b"12345i,1234").unwrap();
        assert_eq!(rem, b"");
        assert_eq!(dial.clir(ClirMode::SubscriptionDefault), ClirMode::Suppression);

        // Trailing modifier after comma pause is unparsed remainder
        let (rem, dial2) = DialString::parse(b"12345,1234i").unwrap();
        assert_eq!(rem, b"i");
        assert_eq!(dial2.clir(ClirMode::SubscriptionDefault), ClirMode::SubscriptionDefault);

        let (rem, dial) = DialString::parse(b"12345I@1,#").unwrap();
        assert_eq!(rem, b"");
        assert_eq!(dial.clir(ClirMode::SubscriptionDefault), ClirMode::Invocation);

        // Emergency dial string with CLIR suffix after @
        let (rem, dial) = DialString::parse(b"911@1,#i").unwrap();
        assert_eq!(rem, b"");
        assert_eq!(dial.clir(ClirMode::SubscriptionDefault), ClirMode::Suppression);
    }

    #[test]
    fn test_dial_args_parse() {
        let (rem, args) = DialArgs::parse(b"12345i;\r\n").unwrap();
        assert_eq!(rem, b"\r\n");
        assert_eq!(args.number.as_str(), "12345");
        assert_eq!(args.clir, ClirMode::Suppression);
        assert!(!args.is_emergency);

        let (rem, args) = DialArgs::parse(b"12345I\r\n").unwrap();
        assert_eq!(rem, b"\r\n");
        assert_eq!(args.number.as_str(), "12345");
        assert_eq!(args.clir, ClirMode::Invocation);
        assert!(!args.is_emergency);

        let (rem, args) = DialArgs::parse(b"911;\r\n").unwrap();
        assert_eq!(rem, b"\r\n");
        assert_eq!(args.number.as_str(), "911");
        assert!(!args.is_emergency);

        let (rem, args) = DialArgs::parse(b"911@;\r\n").unwrap();
        assert_eq!(rem, b"\r\n");
        assert_eq!(args.number.as_str(), "911");
        assert!(args.is_emergency);

        let (rem, args) = DialArgs::parse(b"911@1,#I;\r\n").unwrap();
        assert_eq!(rem, b"\r\n");
        assert_eq!(args.number.as_str(), "911");
        assert_eq!(args.clir, ClirMode::Invocation);
        assert!(args.is_emergency);

        let (rem, args) = DialArgs::parse(b"911@1,#i;\r\n").unwrap();
        assert_eq!(rem, b"\r\n");
        assert_eq!(args.number.as_str(), "911");
        assert_eq!(args.clir, ClirMode::Suppression);
        assert!(args.is_emergency);

        let (rem, args) = DialArgs::parse(b"911@1,#;\r\n").unwrap();
        assert_eq!(rem, b"\r\n");
        assert_eq!(args.number.as_str(), "911");
        assert_eq!(args.clir, ClirMode::SubscriptionDefault);
        assert!(args.is_emergency);

        let (rem, args) = DialArgs::parse(b"911@,#I;\r\n").unwrap();
        assert_eq!(rem, b"\r\n");
        assert_eq!(args.number.as_str(), "911");
        assert_eq!(args.clir, ClirMode::Invocation);
        assert!(args.is_emergency);

        let (rem, args) = DialArgs::parse(b"911@,#;\r\n").unwrap();
        assert_eq!(rem, b"\r\n");
        assert_eq!(args.number.as_str(), "911");
        assert_eq!(args.clir, ClirMode::SubscriptionDefault);
        assert!(args.is_emergency);

        let (rem, args) = DialArgs::parse(b"999@1,#I;\r\n").unwrap();
        assert_eq!(rem, b"\r\n");
        assert_eq!(args.number.as_str(), "999");
        assert_eq!(args.clir, ClirMode::Invocation);
        assert!(args.is_emergency);

        let (rem, args) = DialArgs::parse(b"12345@1,#i;\r\n").unwrap();
        assert_eq!(rem, b"\r\n");
        assert_eq!(args.number.as_str(), "12345");
        assert_eq!(args.clir, ClirMode::Suppression);
        assert!(args.is_emergency);

        let (rem, args) = DialArgs::parse(b"12345@0;\r\n").unwrap();
        assert_eq!(rem, b"\r\n");
        assert_eq!(args.number.as_str(), "12345");
        assert!(args.is_emergency);

        // Verifies error span points to the invalid remainder within the dial content
        let err = DialArgs::parse(b"12345extra;\r\n").unwrap_err();
        match err {
            nom::Err::Error(e) => {
                assert_eq!(e.input, b"extra");
                assert_eq!(e.code, nom::error::ErrorKind::Verify);
            }
            _ => panic!("Expected nom::Err::Error"),
        }
    }

    #[test]
    fn test_phone_number_is_gprs_dial() {
        assert!(PhoneNumber::new_for_test("*99#").is_gprs_dial());
        assert!(PhoneNumber::new_for_test("*99*1#").is_gprs_dial());
        assert!(PhoneNumber::new_for_test("*99***1#").is_gprs_dial());
        assert!(!PhoneNumber::new_for_test("12345").is_gprs_dial());
        assert!(!PhoneNumber::new_for_test("*99").is_gprs_dial());
        assert!(!PhoneNumber::new_for_test("*99#1").is_gprs_dial());
    }

    #[test]
    fn test_phone_number_toa() {
        assert_eq!(PhoneNumber::new_for_test("+16505550100").toa(), TypeOfAddress::International);
        assert_eq!(PhoneNumber::new_for_test("16505550100").toa(), TypeOfAddress::Unknown);
        assert_eq!(PhoneNumber::new_for_test("12345").toa(), TypeOfAddress::Unknown);
    }

    #[test]
    fn test_number_presentation_format_number() {
        let phone = PhoneNumber::new_for_test("12345");

        // Allowed
        let formatted = NumberPresentation::Allowed.format_number(Some(&phone));
        assert_eq!(formatted.number, "12345");
        assert_eq!(formatted.toa, TypeOfAddress::Unknown);

        let int_phone = PhoneNumber::new_for_test("+12345");
        let formatted = NumberPresentation::Allowed.format_number(Some(&int_phone));
        assert_eq!(formatted.number, "12345");
        assert_eq!(formatted.toa, TypeOfAddress::International);

        let formatted = NumberPresentation::Allowed.format_number(None);
        assert_eq!(formatted.number, "");
        assert_eq!(formatted.toa, TypeOfAddress::Unknown);

        // Restricted
        let formatted = NumberPresentation::Restricted.format_number(Some(&phone));
        assert_eq!(formatted.number, "");
        assert_eq!(formatted.toa, TypeOfAddress::Unknown);

        // Not Available
        let formatted = NumberPresentation::NotAvailable.format_number(Some(&phone));
        assert_eq!(formatted.number, "");
        assert_eq!(formatted.toa, TypeOfAddress::Unknown);
    }

    #[test]
    fn test_adn_record_encode_from_number() {
        let record_len = UiccFileId::Msisdn.default_record_len().expect("file id is record based");
        let phone = PhoneNumber::new_for_test("+15555215554");
        let record = AdnRecord::encode_from_number(&phone, record_len);
        assert_eq!(record.len(), record_len);
        // Alpha identifier is padded with 0xFF
        assert_eq!(&record[..14], &[0xFF; 14]);
        // Length of BCD number is 7 (1 byte TON + 6 bytes dialed digits)
        assert_eq!(record[14], 7);
        // International TON/NPI
        assert_eq!(record[15], 0x91);
        // Dialing digits 15555215554 -> 51 55 25 51 55 F4
        assert_eq!(&record[16..22], &[0x51, 0x55, 0x25, 0x51, 0x55, 0xF4]);
        // Trailing dialing bytes padded with 0xFF
        assert_eq!(&record[22..26], &[0xFF; 4]);
        // Capability/Extension bytes
        assert_eq!(&record[26..28], &[0xFF, 0xFF]);

        // Empty digits returns standard unassigned 0xFF record
        let empty = PhoneNumber::new_for_test("");
        assert_eq!(AdnRecord::encode_from_number(&empty, 28), vec![0xFF; 28]);
    }

    #[test]
    fn test_adn_record_cts_mbdn_roundtrip() {
        let cts_hex =
            "74616741FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF06812143658709FFFFFFFFFFFFFF";
        let raw_bytes = hex::decode(cts_hex).unwrap();
        assert_eq!(raw_bytes.len(), 38);

        let decoded = AdnRecord::decode(&raw_bytes).expect("Should decode CTS MBDN record");
        assert_eq!(decoded.alpha_tag.as_deref(), Some("tagA"));
        assert_eq!(decoded.number.as_ref().map(|p| p.as_str()), Some("1234567890"));

        let reencoded = decoded.encode(38);
        assert_eq!(hex::encode_upper(&reencoded), cts_hex);
    }

    #[test]
    fn test_adn_record_msisdn_with_alpha_tag() {
        let record = AdnRecord::new(Some("MySIM"), Some(PhoneNumber::new_for_test("+15551234567")));
        let encoded = record.encode(28);
        assert_eq!(encoded.len(), 28);

        let decoded = AdnRecord::decode(&encoded).expect("Should decode MSISDN record");
        assert_eq!(decoded.alpha_tag.as_deref(), Some("MySIM"));
        assert_eq!(decoded.number.as_ref().map(|p| p.as_str()), Some("+15551234567"));
    }

    #[test]
    fn test_adn_record_service_code() {
        let record = AdnRecord::new(Some("Voicemail"), Some(PhoneNumber::new_for_test("*86")));
        let encoded = record.encode(38);
        let decoded = AdnRecord::decode(&encoded).expect("Should decode service code record");
        assert_eq!(decoded.alpha_tag.as_deref(), Some("Voicemail"));
        assert_eq!(decoded.number.as_ref().map(|p| p.as_str()), Some("*86"));
    }

    #[test]
    fn test_adn_record_uninitialized() {
        assert!(AdnRecord::decode(&[0xFF; 28]).is_none());
        assert!(AdnRecord::decode(&[0xFF; 38]).is_none());
        assert!(AdnRecord::decode(&[0xFF; 10]).is_none()); // malformed < 14
    }

    #[test]
    fn test_adn_record_long_number_truncation() {
        let long_number = PhoneNumber::new_for_test("123456789012345678901234");
        let record = AdnRecord::new(Some("Long"), Some(long_number));
        let encoded = record.encode(28);

        let decoded = AdnRecord::decode(&encoded).expect("Should decode truncated record");
        assert_eq!(decoded.alpha_tag.as_deref(), Some("Long"));
        assert_eq!(decoded.number.as_ref().map(|p| p.as_str()), Some("12345678901234567890"));
    }

    #[test]
    fn test_adn_record_length_boundaries() {
        // Less than 14 bytes returns None
        assert!(AdnRecord::decode(&[]).is_none());
        assert!(AdnRecord::decode(&[0xFF; 13]).is_none());
        assert!(AdnRecord::decode(&[0x06, 0x81, 0x21, 0x43, 0x65, 0x87, 0x09]).is_none());

        // encode() with len < 14 returns 0xFF buffer
        let rec = AdnRecord::new(Some("A"), Some(PhoneNumber::new_for_test("123")));
        assert_eq!(rec.encode(0), Vec::<u8>::new());
        assert_eq!(rec.encode(13), vec![0xFF; 13]);

        // Exactly 14 bytes (alpha_len == 0)
        let rec_14 = AdnRecord::new(None::<&str>, Some(PhoneNumber::new_for_test("+15551234567")));
        let encoded_14 = rec_14.encode(14);
        assert_eq!(encoded_14.len(), 14);
        let decoded_14 = AdnRecord::decode(&encoded_14).unwrap();
        assert_eq!(decoded_14.alpha_tag, None);
        assert_eq!(decoded_14.number.as_ref().map(|p| p.as_str()), Some("+15551234567"));

        // Exactly 14 bytes with alpha_tag truncates alpha to 0 bytes cleanly
        let rec_with_tag = AdnRecord::new(Some("Tag"), Some(PhoneNumber::new_for_test("123456")));
        let encoded_14_tag = rec_with_tag.encode(14);
        let decoded_14_tag = AdnRecord::decode(&encoded_14_tag).unwrap();
        assert_eq!(decoded_14_tag.alpha_tag, None);
        assert_eq!(decoded_14_tag.number.as_ref().map(|p| p.as_str()), Some("123456"));
    }

    #[test]
    fn test_adn_record_odd_vs_even_digit_padding() {
        // Odd digits: 7 digits ("1234567") -> 4 BCD bytes: 21 43 65 F7
        let rec_odd = AdnRecord::new(None::<&str>, Some(PhoneNumber::new_for_test("1234567")));
        let encoded_odd = rec_odd.encode(28);
        assert_eq!(encoded_odd[14], 5); // 1 TON + 4 BCD bytes
        assert_eq!(encoded_odd[15], 0x81);
        assert_eq!(&encoded_odd[16..20], &[0x21, 0x43, 0x65, 0xF7]);
        assert_eq!(&encoded_odd[20..26], &[0xFF; 6]); // Unused 6 bytes in 10-byte buffer
        let decoded_odd = AdnRecord::decode(&encoded_odd).unwrap();
        assert_eq!(decoded_odd.number.as_ref().map(|p| p.as_str()), Some("1234567"));

        // Even digits: 8 digits ("12345678") -> 4 BCD bytes: 21 43 65 87
        let rec_even = AdnRecord::new(None::<&str>, Some(PhoneNumber::new_for_test("12345678")));
        let encoded_even = rec_even.encode(28);
        assert_eq!(encoded_even[14], 5);
        assert_eq!(&encoded_even[16..20], &[0x21, 0x43, 0x65, 0x87]);
        assert_eq!(&encoded_even[20..26], &[0xFF; 6]);
        let decoded_even = AdnRecord::decode(&encoded_even).unwrap();
        assert_eq!(decoded_even.number.as_ref().map(|p| p.as_str()), Some("12345678"));

        // Single digit ("5") -> 1 BCD byte: F5, len 2
        let rec_single = AdnRecord::new(None::<&str>, Some(PhoneNumber::new_for_test("5")));
        let encoded_single = rec_single.encode(28);
        assert_eq!(encoded_single[14], 2);
        assert_eq!(encoded_single[16], 0xF5);
        let decoded_single = AdnRecord::decode(&encoded_single).unwrap();
        assert_eq!(decoded_single.number.as_ref().map(|p| p.as_str()), Some("5"));
    }

    #[test]
    fn test_adn_record_length_byte_edge_cases() {
        // len_byte == 0 returns number: None
        let mut raw = vec![0xFF; 28];
        raw[..4].copy_from_slice(b"Test");
        raw[14] = 0x00;
        let dec = AdnRecord::decode(&raw).unwrap();
        assert_eq!(dec.alpha_tag.as_deref(), Some("Test"));
        assert_eq!(dec.number, None);

        // len_byte == 1 (TON only, 0 BCD digits) returns number: None
        raw[14] = 0x01;
        raw[15] = 0x81;
        let dec = AdnRecord::decode(&raw).unwrap();
        assert_eq!(dec.number, None);

        // len_byte > 11 (malformed length byte) returns number: None without panic
        raw[14] = 12;
        assert_eq!(AdnRecord::decode(&raw).unwrap().number, None);
        raw[14] = 254;
        assert_eq!(AdnRecord::decode(&raw).unwrap().number, None);
    }

    #[test]
    fn test_adn_record_illegal_bcd_nibble_rejection() {
        // Record with illegal nibbles 0xC, 0xD, 0xE in BCD digits
        let mut raw = vec![0xFF; 28];
        raw[..4].copy_from_slice(b"Test");
        raw[14] = 3; // 1 TON + 2 BCD bytes
        raw[15] = 0x81;
        raw[16] = 0xC1; // '1' and 'a'
        raw[17] = 0x32; // '2' and '3'
        // Decoded string would be "1a23", which PhoneNumber::parse rejects
        let dec = AdnRecord::decode(&raw).unwrap();
        assert_eq!(dec.number, None);
    }

    #[test]
    fn test_adn_record_alpha_tag_edge_cases() {
        // Full alpha field with no trailing 0xFF (exactly 14 characters)
        let rec = AdnRecord::new(Some("12345678901234"), Some(PhoneNumber::new_for_test("999")));
        let enc = rec.encode(28);
        assert_eq!(&enc[..14], b"12345678901234");
        let dec = AdnRecord::decode(&enc).unwrap();
        assert_eq!(dec.alpha_tag.as_deref(), Some("12345678901234"));

        // Alpha field with spaces preserved
        let rec_spaces = AdnRecord::new(Some("John Doe "), Some(PhoneNumber::new_for_test("999")));
        let enc_spaces = rec_spaces.encode(28);
        let dec_spaces = AdnRecord::decode(&enc_spaces).unwrap();
        assert_eq!(dec_spaces.alpha_tag.as_deref(), Some("John Doe "));
    }

    #[test]
    fn test_adn_record_large_buffer() {
        // 50-byte record (alpha_len = 50 - 14 = 36 bytes)
        let rec = AdnRecord::new(
            Some("Alpha tag in 50-byte record"),
            Some(PhoneNumber::new_for_test("+15550001111")),
        );
        let enc = rec.encode(50);
        assert_eq!(enc.len(), 50);
        let dec = AdnRecord::decode(&enc).unwrap();
        assert_eq!(dec.alpha_tag.as_deref(), Some("Alpha tag in 50-byte record"));
        assert_eq!(dec.number.as_ref().map(|p| p.as_str()), Some("+15550001111"));
    }

    #[test]
    fn test_plmn_parse_and_accessors() {
        // Valid 5-digit PLMN (e.g. UK Vodafone 234-15)
        let plmn5 = Plmn::parse("23415").unwrap();
        assert_eq!(plmn5.mcc(), "234");
        assert_eq!(plmn5.mnc(), "15");
        assert_eq!(plmn5.mnc_length(), 2);
        assert_eq!(plmn5.as_str(), "23415");
        assert_eq!(format!("{plmn5}"), "23415");
        assert!(matches!(plmn5, Plmn::TwoDigitMnc(_)));

        // Valid 6-digit PLMN (e.g. US T-Mobile 310-260)
        let plmn6 = Plmn::parse("310260").unwrap();
        assert_eq!(plmn6.mcc(), "310");
        assert_eq!(plmn6.mnc(), "260");
        assert_eq!(plmn6.mnc_length(), 3);
        assert_eq!(plmn6.as_str(), "310260");
        assert!(matches!(plmn6, Plmn::ThreeDigitMnc(_)));

        // Construction from separate MCC/MNC
        let from_parts = Plmn::from_mcc_mnc("311", "740").unwrap();
        assert_eq!(from_parts.as_str(), "311740");
        assert_eq!(from_parts.mnc_length(), 3);

        let from_parts_2digit = Plmn::from_mcc_mnc("310", "26").unwrap();
        assert_eq!(from_parts_2digit.as_str(), "31026");
        assert_eq!(from_parts_2digit.mnc_length(), 2);

        // Construction with surrounding whitespace
        let from_trimmed = Plmn::from_mcc_mnc(" 310 \t", "\n260 ").unwrap();
        assert_eq!(from_trimmed.as_str(), "310260");

        // Derivation from IMSI
        let derived_default = Plmn::from_imsi("310260000000000", None).unwrap();
        assert_eq!(derived_default.as_str(), "310260");

        let derived_explicit_2digit = Plmn::from_imsi("310260000000000", Some(2)).unwrap();
        assert_eq!(derived_explicit_2digit.as_str(), "31026");

        let derived_short_imsi = Plmn::from_imsi("31026", None).unwrap();
        assert_eq!(derived_short_imsi.as_str(), "31026");

        // Invalid formats must return Err / None
        assert!(matches!(Plmn::parse("1234"), Err(PlmnError::InvalidLength(4))));
        assert!(matches!(Plmn::parse("1234567"), Err(PlmnError::InvalidLength(7))));
        assert!(matches!(Plmn::parse("3102A"), Err(PlmnError::InvalidDigits(_))));
        assert!(matches!(Plmn::parse(""), Err(PlmnError::InvalidLength(0))));
        assert_eq!(Plmn::from_mcc_mnc(" 12 ", "345"), Err(PlmnError::InvalidMcc("12".to_string())));
        assert_eq!(Plmn::from_mcc_mnc("12A", "345"), Err(PlmnError::InvalidMcc("12A".to_string())));
        assert_eq!(Plmn::from_mcc_mnc("123", " 4 "), Err(PlmnError::InvalidMnc("4".to_string())));
        assert_eq!(
            Plmn::from_mcc_mnc("123", "4567"),
            Err(PlmnError::InvalidMnc("4567".to_string()))
        );
        assert_eq!(Plmn::from_mcc_mnc("123", "4B"), Err(PlmnError::InvalidMnc("4B".to_string())));
        assert!(Plmn::from_imsi("1234", None).is_none());
        assert!(Plmn::from_imsi("", None).is_none());
    }

    #[test]
    fn test_plmn_serde() {
        let plmn = Plmn::parse("310260").unwrap();
        let json = serde_json::to_string(&plmn).unwrap();
        assert_eq!(json, "\"310260\"");
        let deserialized: Plmn = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, plmn);

        let invalid_json = "\"1234\"";
        assert!(serde_json::from_str::<Plmn>(invalid_json).is_err());
    }

    #[test]
    fn test_command_error_format_response() {
        let cme = CommandError::Cme(CmeError::SimPinRequired);
        assert_eq!(cme.format_response(CmeeMode::Disable), "ERROR\r\n");
        assert_eq!(cme.format_response(CmeeMode::Numeric), "+CME ERROR: 11\r\n");
        assert_eq!(cme.format_response(CmeeMode::Verbose), "+CME ERROR: SIM PIN required\r\n");

        let cms = CommandError::Cms(CmsError::SimNotInserted);
        assert_eq!(cms.format_response(CmeeMode::Disable), "ERROR\r\n");
        assert_eq!(cms.format_response(CmeeMode::Numeric), "+CMS ERROR: 310\r\n");
        assert_eq!(cms.format_response(CmeeMode::Verbose), "+CMS ERROR: SIM not inserted\r\n");

        let generic = CommandError::Generic;
        assert_eq!(generic.format_response(CmeeMode::Disable), "ERROR\r\n");
        assert_eq!(generic.format_response(CmeeMode::Numeric), "ERROR\r\n");
        assert_eq!(generic.format_response(CmeeMode::Verbose), "ERROR\r\n");
        assert_eq!(CommandError::default(), CommandError::Generic);

        let mut out = String::new();
        let res = ExecutionResult::cme_error(CmeError::SimPinRequired);
        assert!(res.format_error_into(&mut out, CmeeMode::Numeric));
        assert_eq!(out, "+CME ERROR: 11\r\n");

        let mut out_success = String::new();
        assert!(!ExecutionResult::ok().format_error_into(&mut out_success, CmeeMode::Numeric));
        assert!(out_success.is_empty());
    }

    #[test]
    fn test_sim_sms_message_mark_read() {
        let mut msg = SimSmsMessage::new(SmsMessageStatus::ReceivedUnread, vec![1, 2, 3]);
        assert_eq!(msg.status, SmsMessageStatus::ReceivedUnread);
        msg.mark_read();
        assert_eq!(msg.status, SmsMessageStatus::ReceivedRead);
        msg.mark_read();
        assert_eq!(msg.status, SmsMessageStatus::ReceivedRead);

        let mut sent_msg = SimSmsMessage::new(SmsMessageStatus::StoredSent, vec![4, 5]);
        sent_msg.mark_read();
        assert_eq!(sent_msg.status, SmsMessageStatus::StoredSent);
    }

    #[test]
    fn test_phone_number_as_str() {
        let phone = PhoneNumber::new_for_test("+1234567890");
        assert_eq!(phone.as_str(), "+1234567890");
        let opt_phone = Some(phone);
        assert_eq!(opt_phone.as_ref().map(PhoneNumber::as_str), Some("+1234567890"));
    }

    #[test]
    fn test_cms_error_codes_and_messages() {
        let expected = [
            (CmsError::InvalidPduParameter, 304, "invalid PDU mode parameter"),
            (CmsError::InvalidTextModeParameter, 305, "invalid text mode parameter"),
            (CmsError::SimNotInserted, 310, "SIM not inserted"),
            (CmsError::SimPinRequired, 311, "SIM PIN required"),
            (CmsError::InvalidMemoryIndex, 321, "invalid memory index"),
            (CmsError::MemoryFull, 322, "memory full"),
            (CmsError::NoNetworkService, 331, "no network service"),
        ];
        for (err, code, verbose) in expected {
            assert_eq!(err.code(), code);
            assert_eq!(err.verbose_str(), verbose);
            assert_eq!(format!("{err}"), verbose);
        }
    }

    #[test]
    fn test_phone_number_serde_and_parsing() {
        let (rem, phone) = PhoneNumber::parse(b"+16505550100").unwrap();
        assert_eq!(rem, b"");
        assert_eq!(phone.as_str(), "+16505550100");
        assert!(phone.is_international());

        let phone = PhoneNumber::try_from("+16505550100".to_string()).unwrap();
        assert_eq!(phone.as_str(), "+16505550100");
        assert!(phone.is_international());

        let json = serde_json::to_string(&phone).unwrap();
        assert_eq!(json, "\"+16505550100\"");
        let deserialized: PhoneNumber = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, phone);

        assert!(matches!(
            PhoneNumber::try_from("invalid#phone"),
            Err(ParsePhoneNumberError::InvalidCharacters(_))
        ));
        assert!(matches!(PhoneNumber::try_from(""), Err(ParsePhoneNumberError::Empty)));
        assert!(matches!(
            PhoneNumber::try_from("12345extra"),
            Err(ParsePhoneNumberError::TrailingCharacters(_))
        ));
        assert!(serde_json::from_str::<PhoneNumber>("\"invalid#phone\"").is_err());

        // Quoted phone number parsing validates full consumption of quoted content
        assert!(PhoneNumber::parse(b"\"12345extra\"").is_err());
        let (rem, phone) = PhoneNumber::parse(b"\"12345\",trailing").unwrap();
        assert_eq!(rem, b",trailing");
        assert_eq!(phone.as_str(), "12345");
    }

    #[test]
    fn test_access_technology_from_wire_standard() {
        let quirks = Quirks::default();
        let expected = [
            (0, Ok(AccessTechnology::Gsm)),
            (1, Ok(AccessTechnology::Gsm)),
            (2, Ok(AccessTechnology::Wcdma)),
            (3, Ok(AccessTechnology::Gsm)),
            (4, Ok(AccessTechnology::Wcdma)),
            (5, Ok(AccessTechnology::Wcdma)),
            (6, Ok(AccessTechnology::Wcdma)),
            (7, Ok(AccessTechnology::Lte)),
            (8, Ok(AccessTechnology::Gsm)),
            (9, Ok(AccessTechnology::Lte)),
            (10, Ok(AccessTechnology::Lte)),
            (11, Ok(AccessTechnology::Nr)),
            (12, Ok(AccessTechnology::Nr)),
            (13, Err(ExecutionResult::cme_error(CmeError::IncorrectParameters))),
            (99, Err(ExecutionResult::cme_error(CmeError::IncorrectParameters))),
        ];
        for (raw, want) in expected {
            assert_eq!(AccessTechnology::from_wire(raw, quirks), want, "<AcT>={raw}");
        }
    }

    #[test]
    fn test_access_technology_from_wire_goldfish_37() {
        let quirks = Quirks { goldfish_ril_37_or_earlier: true, ..Default::default() };
        let expected = [
            (0, Ok(AccessTechnology::Gsm)),
            (1, Ok(AccessTechnology::Gsm)),
            (2, Ok(AccessTechnology::Wcdma)),
            (3, Ok(AccessTechnology::Lte)),
            (4, Ok(AccessTechnology::Wcdma)),
            (5, Ok(AccessTechnology::Wcdma)),
            (6, Ok(AccessTechnology::Nr)),
            (7, Ok(AccessTechnology::Lte)),
            (8, Ok(AccessTechnology::Gsm)),
            (9, Ok(AccessTechnology::Lte)),
            (10, Ok(AccessTechnology::Lte)),
            (11, Ok(AccessTechnology::Nr)),
            (12, Ok(AccessTechnology::Nr)),
            (13, Err(ExecutionResult::cme_error(CmeError::IncorrectParameters))),
            (99, Err(ExecutionResult::cme_error(CmeError::IncorrectParameters))),
        ];
        for (raw, want) in expected {
            assert_eq!(AccessTechnology::from_wire(raw, quirks), want, "<AcT>={raw}");
        }
    }

    #[test]
    fn test_types_defaults() {
        assert_eq!(IcfFormat::default(), IcfFormat::Data8Stop1);
        assert_eq!(IcfParity::default(), IcfParity::Space);
        assert_eq!(FlowControlMode::default(), FlowControlMode::Hardware);
        assert_eq!(ServiceClass::default(), ServiceClass::VOICE_DATA_FAX);
        assert_eq!(TypeOfAddress::default(), TypeOfAddress::Unknown);

        let (_, sc) = ServiceClass::parse(b"3").unwrap();
        assert_eq!(sc.as_u8(), 3);
        assert!(sc.contains(ServiceClass::VOICE));
        assert!(sc.contains(ServiceClass::DATA));
        assert!(!sc.contains(ServiceClass::FAX));
    }

    #[test]
    fn test_ctec_technology_from_wire() {
        let expected = [
            (1, Ok(CtecTechnology::Gsm)),
            (2, Ok(CtecTechnology::Wcdma)),
            (4, Ok(CtecTechnology::Cdma)),
            (8, Ok(CtecTechnology::Evdo)),
            (16, Ok(CtecTechnology::Tdscdma)),
            (32, Ok(CtecTechnology::Lte)),
            (64, Ok(CtecTechnology::Nr)),
            (0, Err(ExecutionResult::cme_error(CmeError::IncorrectParameters))),
            (3, Err(ExecutionResult::cme_error(CmeError::IncorrectParameters))),
            (5, Err(ExecutionResult::cme_error(CmeError::IncorrectParameters))),
            (6, Err(ExecutionResult::cme_error(CmeError::IncorrectParameters))),
            (7, Err(ExecutionResult::cme_error(CmeError::IncorrectParameters))),
            (99, Err(ExecutionResult::cme_error(CmeError::IncorrectParameters))),
        ];
        for (raw, want) in expected {
            assert_eq!(CtecTechnology::from_wire(raw), want, "wire={raw}");
        }
    }

    #[test]
    fn test_ctec_preferred_mask_parsing() {
        assert_eq!(CtecPreferredMask::parse(b"0x04").unwrap().1, CtecPreferredMask(0x04));
        assert_eq!(CtecPreferredMask::parse(b"4").unwrap().1, CtecPreferredMask(0x04));
        assert_eq!(CtecPreferredMask::parse(b"\"201\"").unwrap().1, CtecPreferredMask(0x0201));
        assert_eq!(CtecPreferredMask::parse(b"\"7F\"").unwrap().1, CtecPreferredMask(0x7F));
        assert!(CtecPreferredMask::parse(b"0x80").is_err());
        assert!(CtecPreferredMask::parse(b"\"8000\"").is_err());
    }

    #[test]
    fn test_ctec_technology_all_variants_exhaustive() {
        assert_eq!(CtecTechnology::ALL.len(), 7);
        for tech in CtecTechnology::ALL {
            let mask = match *tech {
                CtecTechnology::Gsm => 0x01,
                CtecTechnology::Wcdma => 0x02,
                CtecTechnology::Cdma => 0x04,
                CtecTechnology::Evdo => 0x08,
                CtecTechnology::Tdscdma => 0x10,
                CtecTechnology::Lte => 0x20,
                CtecTechnology::Nr => 0x40,
            };
            assert_eq!(*tech as u8, mask);
        }
        assert_eq!(CtecTechnology::all_mask(), 0x7F);
    }

    #[test]
    fn test_hangup_reason_as_urc_str() {
        assert_eq!(HangupReason::Busy.as_urc_str(false), "BUSY\r\n");
        assert_eq!(HangupReason::NoAnswer.as_urc_str(false), "NO ANSWER\r\n");
        assert_eq!(HangupReason::Normal.as_urc_str(false), "NO CARRIER\r\n");

        assert_eq!(HangupReason::Busy.as_urc_str(true), "RING\r\n");
        assert_eq!(HangupReason::NoAnswer.as_urc_str(true), "RING\r\n");
        assert_eq!(HangupReason::Normal.as_urc_str(true), "RING\r\n");
    }
}
