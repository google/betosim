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

use std::{fmt, iter::once};

use jiff::{Zoned, tz::TimeZone};
use nom::{
    Err as NomErr, IResult, bytes::complete::take, error::ErrorKind, number::complete::u8 as nom_u8,
};
use tracing::{debug, warn};

use crate::types::TypeOfAddress;

pub(crate) mod bcd;
pub(crate) mod cbs;

pub use cbs::{CBS_CONTENT_LEN, CBS_HEADER_LEN, CBS_PDU_LEN, CbsPdu};

#[derive(Debug, PartialEq, Eq)]
pub enum ParseError {
    InvalidHex,
    TooShort,
    InvalidFormat,
}

impl std::error::Error for ParseError {}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParseError::InvalidHex => write!(f, "Invalid hex string"),
            ParseError::TooShort => write!(f, "PDU is too short"),
            ParseError::InvalidFormat => write!(f, "Invalid PDU format"),
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum EncodeError {
    InvalidFormat,
}

impl std::error::Error for EncodeError {}

impl fmt::Display for EncodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EncodeError::InvalidFormat => write!(f, "Invalid PDU format for encoding"),
        }
    }
}

// 3GPP TS 23.040 PDU Type Mask and Header Constants
/// Bit 5 of SMS-SUBMIT PDU type: Status Report Request (SRR)
const SMS_SUBMIT_SRR_MASK: u8 = 0x20;
/// Bit 6 of SMS-SUBMIT PDU type: User Data Header Indicator (UDHI)
const SMS_SUBMIT_UDHI_MASK: u8 = 0x40;

/// Default SMS Center Address length (0x00 = empty / default SCA)
const PDU_SCA_DEFAULT: u8 = 0x00;
/// PDU Type for SMS-STATUS-REPORT (3GPP TS 23.040 § 9.2.2.3)
const SMS_STATUS_REPORT_PDU_TYPE: u8 = 0x02;
/// Status 0x00: Short message received by SME successfully (3GPP TS 23.040 §
/// 9.2.3.22)
const SMS_STATUS_SUCCESS: u8 = 0x00;

/// PDU Type for SMS-DELIVER without User Data Header
const SMS_DELIVER_PDU_TYPE: u8 = 0x24;
/// PDU Type for SMS-DELIVER with User Data Header (UDHI set)
const SMS_DELIVER_UDHI_PDU_TYPE: u8 = 0x64;

/// Represents a parsed SMS-SUBMIT (outgoing) PDU.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubmitPdu {
    pdu_type: u8,
    message_reference: u8,
    address: Vec<u8>, // Raw bytes of address (including length, type, and BCD digits)
    protocol_id: u8,
    data_code_scheme: u8,
    user_data: Vec<u8>, // Raw bytes of user data (including UDL and UD)
}

impl SubmitPdu {
    /// Parses a raw SMS-SUBMIT PDU byte slice into a `SubmitPdu` struct.
    pub fn parse(bytes: &[u8]) -> Result<Self, ParseError> {
        match parse_pdu_nom(bytes) {
            Ok((_, pdu)) => Ok(pdu),
            Err(NomErr::Error(e)) | Err(NomErr::Failure(e)) => {
                // Map nom's Eof/Complete errors to TooShort, others to InvalidFormat
                if e.code == ErrorKind::Eof || e.code == ErrorKind::Complete {
                    Err(ParseError::TooShort)
                } else {
                    Err(ParseError::InvalidFormat)
                }
            }
            Err(NomErr::Incomplete(_)) => Err(ParseError::TooShort),
        }
    }

    /// Parses a hex-encoded SMS-SUBMIT PDU string into a `SubmitPdu` struct.
    pub fn parse_str(pdu_hex: &str) -> Result<Self, ParseError> {
        let bytes = hex::decode(pdu_hex).map_err(|_err| ParseError::InvalidHex)?;
        Self::parse(&bytes)
    }

    /// Extracts and decodes the phone number from the address field.
    ///
    /// # Behavior
    /// To maintain compatibility with C++ simulator expectations, if the raw
    /// address field is exactly 9 bytes (representing a 13-14 digit number
    /// including headers), it assumes a 2-digit country code (e.g., "86"
    /// for China) is prefixed and strips it by skipping the first BCD byte.
    pub fn phone_number(&self) -> Option<String> {
        let start = if self.address.len() == 9 { 3 } else { 2 };
        let bcd_bytes = self.address.get(start..)?;
        Some(bcd::bcd_to_string(bcd_bytes))
    }

    /// Checks if a status report is requested for this SMS-SUBMIT PDU.
    pub fn status_report_requested(&self) -> bool {
        (self.pdu_type & SMS_SUBMIT_SRR_MASK) != 0
    }

    pub fn status_report(&self, message_reference: u8, scts_bcd: [u8; 7]) -> StatusReport {
        StatusReport { message_reference, recipient_address: self.address.clone(), scts_bcd }
    }

    /// Converts this SMS-SUBMIT PDU (outgoing) to an SMS-DELIVER PDU
    /// (incoming). Returns the hex-encoded PDU string.
    pub fn to_deliver_pdu_hex(
        &self,
        sender: Option<&str>,
        scts_bcd: [u8; 7],
    ) -> Result<String, EncodeError> {
        let rx_pdu_type = if (self.pdu_type & SMS_SUBMIT_UDHI_MASK) != 0 {
            SMS_DELIVER_UDHI_PDU_TYPE
        } else {
            SMS_DELIVER_PDU_TYPE
        };

        let oa_bytes = match sender.filter(|s| !s.is_empty()) {
            Some(sender_num) => std::borrow::Cow::Owned(encode_address(sender_num)),
            None => std::borrow::Cow::Borrowed(self.address.as_slice()),
        };

        let bytes: Vec<u8> = once(PDU_SCA_DEFAULT) // 1. SCA
            .chain(once(rx_pdu_type)) // 2. PDU-Type
            .chain(oa_bytes.iter().copied()) // 3. Address (OA)
            .chain(once(self.protocol_id)) // 4. Protocol ID
            .chain(once(self.data_code_scheme)) // 5. DCS
            .chain(scts_bcd) // 6. SCTS
            .chain(self.user_data.iter().copied()) // 7. User Data
            .collect();

        Ok(hex::encode_upper(bytes))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusReport {
    pub message_reference: u8,
    pub recipient_address: Vec<u8>,
    pub scts_bcd: [u8; 7],
}

impl StatusReport {
    pub fn to_pdu_hex(&self, discharge_time: &Zoned) -> String {
        let dt_bcd = encode_timestamp_bcd(discharge_time);
        let bytes: Vec<u8> = once(PDU_SCA_DEFAULT) // 1. SCA
            .chain(once(SMS_STATUS_REPORT_PDU_TYPE)) // 2. PDU-Type (SMS-STATUS-REPORT)
            .chain(once(self.message_reference)) // 3. MR (assigned by modem)
            .chain(self.recipient_address.iter().copied()) // 4. RA (Recipient Address)
            .chain(self.scts_bcd) // 5. SCTS
            .chain(dt_bcd) // 6. DT (Discharge Time)
            .chain(once(SMS_STATUS_SUCCESS)) // 7. Status (0 = success)
            .collect();
        hex::encode_upper(bytes)
    }
}

/// Core parsing logic using `nom` combinators.
fn parse_pdu_nom(input: &[u8]) -> IResult<&[u8], SubmitPdu> {
    // 1. SMSC Address (SCA)
    let (input, sca_len) = nom_u8(input)?;
    let (input, _) = take(sca_len as usize)(input)?;

    // 2. PDU-Type
    let (input, pdu_type) = nom_u8(input)?;

    // 3. Message Reference (MR)
    let (input, message_reference) = nom_u8(input)?;

    // 4. Destination/Originator Address (DA/OA)
    let (input, address_len_digits) = nom_u8(input)?;
    let address_len_bytes =
        if address_len_digits == 0 { 0 } else { 1 + (address_len_digits as usize).div_ceil(2) };
    let (input, address_bytes) = take(address_len_bytes)(input)?;

    // The address vector must prepend the length digit to match the expected format
    // for downstream processing.
    let mut address = Vec::with_capacity(1 + address_bytes.len());
    address.push(address_len_digits);
    address.extend_from_slice(address_bytes);

    // 5. Protocol ID (PID)
    let (input, protocol_id) = nom_u8(input)?;

    // 6. Data Coding Scheme (DCS)
    let (input, data_code_scheme) = nom_u8(input)?;

    // Skip Validity Period (VP) if present in SMS-SUBMIT
    let mut input = input;
    let mti = pdu_type & 0x03;
    if mti == 0x01 {
        let vpf = (pdu_type >> 3) & 0x03;
        match vpf {
            0x01 => {
                let (i, _) = take(7_usize)(input)?;
                input = i;
            } // Enhanced format (7 bytes)
            0x02 => {
                let (i, _) = take(1_usize)(input)?;
                input = i;
            } // Relative format (1 byte)
            0x03 => {
                let (i, _) = take(7_usize)(input)?;
                input = i;
            } // Absolute format (7 bytes)
            _ => {}
        }
    }

    // 7. User Data Length (UDL) and User Data (UD)
    let (input, ud_length) = nom_u8(input)?;
    let expected_ud_bytes = match data_code_scheme {
        dcs if is_7bit_dcs(dcs) => (ud_length as usize * 7).div_ceil(8),
        _ => ud_length as usize,
    };

    // Consume exactly the expected user data bytes.
    // If there are not enough bytes, nom will naturally return an Eof/Complete
    // error which SubmitPdu::parse maps to ParseError::TooShort.
    let (input, ud_bytes) = take(expected_ud_bytes)(input)?;

    // We expect no trailing garbage bytes after the user data (strict parsing)
    if !input.is_empty() {
        return Err(NomErr::Error(nom::error::Error::new(input, ErrorKind::Verify)));
    }

    // The user data vector must prepend the UDL (User Data Length) byte for
    // downstream processing.
    let mut user_data = Vec::with_capacity(1 + ud_bytes.len());
    user_data.push(ud_length);
    user_data.extend_from_slice(ud_bytes);

    Ok((
        &[],
        SubmitPdu {
            pdu_type,
            message_reference,
            address,
            protocol_id,
            data_code_scheme,
            user_data,
        },
    ))
}

/// Identifies if the Data Coding Scheme (DCS) represents GSM 7-bit packed data
/// according to 3GPP TS 23.038.
fn is_7bit_dcs(dcs: u8) -> bool {
    match dcs & 0xC0 {
        0x00 => (dcs & 0x0C) == 0x00, // General Data Coding: bits 3-2 == 00 (7-bit)
        0x40 | 0x80 => false,         // Reserved / 8-bit / UCS2
        0xC0 => {
            if (dcs & 0x30) == 0x30 {
                (dcs & 0x04) == 0x00 // Data coding/message class: bit 2 == 0 (7-bit)
            } else {
                (dcs & 0x10) == 0x00 // MWI: 1100xxxx and 1110xxxx are 7-bit
            }
        }
        _ => false,
    }
}

pub(crate) fn encode_timestamp_bcd(timestamp: &Zoned) -> [u8; 7] {
    let utc = timestamp.with_time_zone(TimeZone::UTC);
    [
        bcd::to_bcd_byte((utc.year().rem_euclid(100)) as u8),
        bcd::to_bcd_byte(utc.month() as u8),
        bcd::to_bcd_byte(utc.day() as u8),
        bcd::to_bcd_byte(utc.hour() as u8),
        bcd::to_bcd_byte(utc.minute() as u8),
        bcd::to_bcd_byte(utc.second() as u8),
        0, // Timezone: UTC (+00) -> BCD 00
    ]
}

pub(crate) fn get_current_timestamp_bcd() -> [u8; 7] {
    encode_timestamp_bcd(&Zoned::now())
}

/// Encodes a phone number into the raw bytes format expected for PDU address
/// fields (OA/DA).
pub fn encode_address(number: &str) -> Vec<u8> {
    let clean_number = number.strip_prefix('+').unwrap_or(number);
    let address_len_digits = clean_number.len() as u8;
    let address_type = TypeOfAddress::from_number(number).as_u8();
    let bcd_digits = bcd::string_to_bcd(clean_number);

    once(address_len_digits).chain(once(address_type)).chain(bcd_digits).collect()
}

/// Creates an SMS-DELIVER PDU from sender number and plain text, encoded in
/// UCS-2. Returns the hex-encoded PDU string.
pub fn create_deliver_pdu_ucs2(sender: &str, text: &str) -> String {
    let rx_pdu_type = 0x24; // SMS-DELIVER, SRI=1, MMS=1
    let scts = get_current_timestamp_bcd();

    // 1. OA Address
    let address_bytes = encode_address(sender);

    // 2. User Data (UCS-2)
    let utf16_chars: Vec<u16> = text.encode_utf16().collect();
    let mut ud_bytes = Vec::with_capacity(1 + utf16_chars.len() * 2);
    ud_bytes.push((utf16_chars.len() * 2) as u8); // UDL (length of user data in bytes for UCS2)
    for &ch in &utf16_chars {
        ud_bytes.push((ch >> 8) as u8);
        ud_bytes.push((ch & 0xFF) as u8);
    }

    let bytes: Vec<u8> = std::iter::once(0x00) // 1. SCA (default)
        .chain(std::iter::once(rx_pdu_type)) // 2. PDU-Type
        .chain(address_bytes.iter().copied()) // 3. OA
        .chain(std::iter::once(0x00)) // 4. PID (SmsDefault)
        .chain(std::iter::once(0x08)) // 5. DCS (UCS-2)
        .chain(scts) // 6. SCTS
        .chain(ud_bytes.iter().copied()) // 7. User Data (UDL + UD)
        .collect();

    hex::encode_upper(bytes)
}

pub(crate) fn try_decode_hex(pdu: &[u8]) -> Option<Vec<u8>> {
    std::str::from_utf8(pdu).ok().and_then(|s| hex::decode(s.trim()).ok())
}

/// Decodes a PDU from a hexadecimal string representation into raw bytes.
///
/// In 3GPP TS 27.005 PDU mode, PDUs are entered as ASCII hexadecimal strings.
/// For compatibility with binary test inputs, non-UTF8 bytes are accepted
/// directly. Returns `CmsError::InvalidPduParameter` if hex decoding fails or
/// if the payload is empty.
pub fn decode_hex_pdu(pdu: &[u8]) -> Result<Vec<u8>, crate::types::CmsError> {
    match std::str::from_utf8(pdu) {
        Ok(s) => match hex::decode(s.trim()) {
            Ok(bytes) if !bytes.is_empty() => Ok(bytes),
            _ => Err(crate::types::CmsError::InvalidPduParameter),
        },
        Err(_) => Ok(pdu.to_vec()),
    }
}

pub struct ProcessedSms {
    pub to: Option<String>,
    pub pdu: Vec<u8>,
    pub status_report: Option<StatusReport>,
}

/// Processes an outgoing SMS PDU (which might be hex-encoded or raw bytes).
/// If it is a valid SMS-SUBMIT PDU, it extracts the destination and converts it
/// to SMS-DELIVER. Otherwise, it returns the original bytes as-is.
pub fn process_outgoing_sms(
    pdu: &[u8],
    sender: Option<&str>,
    message_reference: u8,
    scts: &Zoned,
) -> ProcessedSms {
    let decoded = try_decode_hex(pdu);
    let bytes_to_parse = decoded.as_deref().unwrap_or(pdu);

    match SubmitPdu::parse(bytes_to_parse) {
        Ok(parsed_pdu) => {
            let to = parsed_pdu.phone_number();
            let scts_bcd = encode_timestamp_bcd(scts);
            let status_report = if parsed_pdu.status_report_requested() {
                Some(parsed_pdu.status_report(message_reference, scts_bcd))
            } else {
                None
            };
            match parsed_pdu.to_deliver_pdu_hex(sender, scts_bcd) {
                Ok(rx_pdu_hex) => {
                    return ProcessedSms { to, pdu: rx_pdu_hex.into_bytes(), status_report };
                }
                Err(e) => {
                    warn!("Failed to encode DELIVER PDU: {:?}", e);
                }
            }
        }
        Err(e) => {
            if decoded.is_some() {
                warn!("Failed to parse PDU: {:?}", e);
            } else {
                debug!("PDU is not valid hex, forwarding as raw");
            }
        }
    }

    ProcessedSms { to: None, pdu: pdu.to_vec(), status_report: None }
}

/// Calculates the TPDU length from a PDU (which might be hex-encoded or raw
/// bytes). Optimizes by only decoding the first byte of the hex string if
/// possible.
pub fn calculate_tpdu_len(pdu: &[u8]) -> usize {
    if let Ok(s) = std::str::from_utf8(pdu) {
        let s = s.trim();

        if s.len() % 2 == 0 && s.bytes().all(|b| b.is_ascii_hexdigit()) {
            // Decode only the SCA length byte to avoid allocating a full decoded vector.
            if let Some(prefix) = s.get(..2)
                && let Ok(b0) = u8::from_str_radix(prefix, 16)
            {
                let sca_len = b0 as usize;
                let raw_len = s.len() / 2;
                if raw_len > 1 + sca_len {
                    return raw_len - 1 - sca_len;
                }
                return 0;
            }
        }
    }
    pdu.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_valid_pdu_true() {
        let pdu_hex = "0001000D91688118109844F0000017AFD7903AB55A9BBA69D639D4ADCBF99E3DCCAE9701";
        let parsed = SubmitPdu::parse_str(pdu_hex);
        assert!(parsed.is_ok());
        let pdu = parsed.unwrap();
        assert_eq!(pdu.pdu_type, 0x01);
        assert_eq!(pdu.message_reference, 0x00);
        assert_eq!(pdu.protocol_id, 0x00);
        assert_eq!(pdu.data_code_scheme, 0x00);

        let pdu_unicode = "000100048145540008024F60";
        let parsed_unicode = SubmitPdu::parse_str(pdu_unicode);
        assert!(parsed_unicode.is_ok());
        let pdu_u = parsed_unicode.unwrap();
        assert_eq!(pdu_u.pdu_type, 0x01);
        assert_eq!(pdu_u.data_code_scheme, 0x08);

        // PDU with VPF=10 (Relative Validity Period, 1 byte)
        let pdu_vpf_relative = "0011000B915155255155F40000AA01F0";
        let parsed_vpf = SubmitPdu::parse_str(pdu_vpf_relative);
        assert!(parsed_vpf.is_ok());
        let pdu_v = parsed_vpf.unwrap();
        assert_eq!(pdu_v.pdu_type, 0x11);
        assert_eq!(pdu_v.phone_number(), Some("15555215554".to_string()));
    }

    #[test]
    fn test_is_valid_pdu_false() {
        // Invalid hex characters
        assert_eq!(SubmitPdu::parse_str("000100gD"), Err(ParseError::InvalidHex));

        // Too short
        assert_eq!(SubmitPdu::parse_str("000100"), Err(ParseError::TooShort));

        // Declared address length is too large for the PDU
        let pdu_too_large_addr =
            "000100fD91688118109844F0000017AFD7903AB55A9BBA69D639D4ADCBF99E3DCCAE9701";
        assert_eq!(SubmitPdu::parse_str(pdu_too_large_addr), Err(ParseError::TooShort));

        // User data length mismatch (extra bytes at the end)
        let pdu_extra_bytes =
            "0001000D91688118109844F0000017AFD7903AB55A9BBA69D639D4ADCBF99E3DCCAE970100";
        assert_eq!(SubmitPdu::parse_str(pdu_extra_bytes), Err(ParseError::InvalidFormat));
    }

    #[test]
    fn test_get_phone_number_from_address() {
        let pdu_hex = "0001000D91688118109844F0000017AFD7903AB55A9BBA69D639D4ADCBF99E3DCCAE9701";
        let pdu = SubmitPdu::parse_str(pdu_hex).unwrap();
        assert_eq!(pdu.phone_number(), Some("18810189440".to_string()));
    }

    #[test]
    fn test_to_deliver_pdu_hex_fallback() {
        let pdu_hex = "0001000D91688118109844F0000017AFD7903AB55A9BBA69D639D4ADCBF99E3DCCAE9701";
        let pdu = SubmitPdu::parse_str(pdu_hex).unwrap();
        let rx_pdu_hex =
            pdu.to_deliver_pdu_hex(None, [0x26, 0x06, 0x15, 0x19, 0x30, 0x45, 0x00]).unwrap();

        let rx_bytes = hex::decode(rx_pdu_hex).unwrap();

        assert_eq!(rx_bytes[0], 0x00); // SCA length
        assert_eq!(rx_bytes[1], 0x24); // PDU-Type (DELIVER, SRI=1, MMS=1)

        let addr_len = pdu.address.len();
        assert_eq!(rx_bytes[2..2 + addr_len], pdu.address[..]);

        let mut pos = 2 + addr_len;
        assert_eq!(rx_bytes[pos], pdu.protocol_id);
        pos += 1;
        assert_eq!(rx_bytes[pos], pdu.data_code_scheme);
        pos += 1;

        assert_eq!(&rx_bytes[pos..pos + 7], &[0x26, 0x06, 0x15, 0x19, 0x30, 0x45, 0x00]);
        pos += 7;

        assert_eq!(rx_bytes[pos..], pdu.user_data[..]);
    }

    #[test]
    fn test_to_deliver_pdu_hex_with_sender() {
        let pdu_hex = "0001000D91688118109844F0000017AFD7903AB55A9BBA69D639D4ADCBF99E3DCCAE9701";
        let pdu = SubmitPdu::parse_str(pdu_hex).unwrap();
        let rx_pdu_hex = pdu
            .to_deliver_pdu_hex(Some("+12345"), [0x26, 0x06, 0x15, 0x19, 0x30, 0x45, 0x00])
            .unwrap();
        let rx_bytes = hex::decode(rx_pdu_hex).unwrap();

        assert_eq!(rx_bytes[0], 0x00); // SCA length
        assert_eq!(rx_bytes[1], 0x24); // PDU-Type (DELIVER, SRI=1, MMS=1)

        // Expected OA BCD for "+12345": length=5, type=0x91, digits=21 43 F5
        let expected_oa = [5, 0x91, 0x21, 0x43, 0xF5];
        assert_eq!(rx_bytes[2..7], expected_oa[..]);
    }

    #[test]
    fn test_is_7bit_dcs() {
        // General Data Coding
        assert!(is_7bit_dcs(0x00));
        assert!(!is_7bit_dcs(0x04));
        assert!(!is_7bit_dcs(0x08)); // UCS2
        assert!(!is_7bit_dcs(0x04 | 0x08)); // UCS2 with class
        assert!(!is_7bit_dcs(0x0C)); // Reserved
        assert!(is_7bit_dcs(0x10)); // Flash SMS (General Data Coding, Class 0, 7-bit)

        // Message Waiting Info (Discard)
        assert!(is_7bit_dcs(0xC0)); // 1100 0000 (7-bit Voicemail)
        assert!(!is_7bit_dcs(0xD0)); // 1101 0000 (UCS2 Voicemail)

        // Message Waiting Info (Store)
        assert!(is_7bit_dcs(0xE0)); // 1110 0000 (7-bit Voicemail)

        // Data Coding / Message Class
        assert!(is_7bit_dcs(0xF0)); // 1111 0000 (7-bit Class 0)
        assert!(!is_7bit_dcs(0xF4)); // 1111 0100 (8-bit Class 0)
    }

    #[test]
    fn test_calculate_tpdu_len() {
        // Happy path: Valid hex PDU (SCA len = 0)
        // "0001..." -> raw_len = 36, sca_len = 0. TPDU len = 36 - 1 - 0 = 35.
        let pdu_hex = "0001000D91688118109844F0000017AFD7903AB55A9BBA69D639D4ADCBF99E3DCCAE9701";
        assert_eq!(super::calculate_tpdu_len(pdu_hex.as_bytes()), 35);

        // Happy path: Valid hex PDU with non-zero SCA (SCA len = 7 bytes)
        // Total len: 43 bytes. Expected TPDU len: 43 - 1 - 7 = 35.
        let pdu_with_sca = "07916881100000F001000D91688118109844F0000017AFD7903AB55A9BBA69D639D4ADCBF99E3DCCAE9701";
        assert_eq!(super::calculate_tpdu_len(pdu_with_sca.as_bytes()), 35);

        // Edge Case: Non-hex input (should fallback to raw byte length)
        assert_eq!(super::calculate_tpdu_len(b"not a hex string"), 16);

        // Edge Case: Odd length hex (should fallback to raw byte length)
        assert_eq!(super::calculate_tpdu_len(b"000"), 3);

        // Edge Case: Invalid hex characters (should fallback to raw byte length)
        assert_eq!(super::calculate_tpdu_len(b"000g"), 4);

        // Edge Case: Extremely short input (raw_len <= 1 + sca_len)
        assert_eq!(super::calculate_tpdu_len(b"00"), 0);
        assert_eq!(super::calculate_tpdu_len(b"051122"), 0);
    }

    #[test]
    fn test_parse_pdu_vpf_enhanced() {
        // VPF = 01 (Enhanced, 7 bytes VP). PDU Type = 0x09
        let pdu_hex = "0009000D91688118109844F000001122334455667717AFD7903AB55A9BBA69D639D4ADCBF99E3DCCAE9701";
        let pdu = SubmitPdu::parse_str(pdu_hex).unwrap();
        assert_eq!(pdu.phone_number().unwrap(), "18810189440");
        assert_eq!(pdu.user_data[0], 23); // UDL
    }

    #[test]
    fn test_parse_pdu_vpf_absolute() {
        // VPF = 11 (Absolute, 7 bytes VP). PDU Type = 0x19
        let pdu_hex = "0019000D91688118109844F000001122334455667717AFD7903AB55A9BBA69D639D4ADCBF99E3DCCAE9701";
        let pdu = SubmitPdu::parse_str(pdu_hex).unwrap();
        assert_eq!(pdu.phone_number().unwrap(), "18810189440");
        assert_eq!(pdu.user_data[0], 23); // UDL
    }

    #[test]
    fn test_process_outgoing_sms_happy_path() {
        // Valid SMS-SUBMIT PDU targeting "8618810189440"
        let pdu_hex = "0001000D91688118109844F0000017AFD7903AB55A9BBA69D639D4ADCBF99E3DCCAE9701";
        let scts = jiff::Timestamp::from_second(1_781_551_845)
            .expect("valid timestamp")
            .to_zoned(TimeZone::UTC);
        let processed = super::process_outgoing_sms(pdu_hex.as_bytes(), Some("+12345"), 0, &scts);
        assert_eq!(processed.to, Some("18810189440".to_string()));
        // The generated PDU should be SMS-DELIVER (starts with "0024...")
        assert!(processed.pdu.starts_with(b"0024"));

        // The PDU should contain the encoded sender "+12345" (05912143F5)
        let pdu_str = std::str::from_utf8(&processed.pdu).unwrap();
        // DELIVER PDU: SCA(00) + PDU_TYPE(24 or 64) + OA(05912143F5)
        assert!(pdu_str.starts_with("002405912143F5") || pdu_str.starts_with("006405912143F5"));
    }

    #[test]
    fn test_process_outgoing_sms_fallback_invalid_pdu() {
        let raw_text = b"hello";
        let scts = jiff::Timestamp::from_second(1_781_551_845)
            .expect("valid timestamp")
            .to_zoned(TimeZone::UTC);
        let processed = super::process_outgoing_sms(raw_text, None, 0, &scts);
        assert_eq!(processed.to, None);
        assert_eq!(processed.pdu, raw_text.to_vec());
    }

    #[test]
    fn test_parse_pdu_empty_address() {
        // PDU with 0-length Destination Address (omits TOA and Value)
        // SCA: 00, PDU-Type: 01, MR: 00, DA Len: 00, PID: 00, DCS: 00, UDL: 00
        let pdu_hex = "00010000000000";
        let pdu = SubmitPdu::parse_str(pdu_hex).unwrap();
        assert_eq!(pdu.address, vec![0]);
        assert_eq!(pdu.phone_number(), None);
    }
}
