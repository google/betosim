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

//! 3GPP TS 23.041 Cell Broadcast Service (CBS) PDU serialization and parsing.
//!
//! A fixed-size GSM/UMTS/LTE Cell Broadcast page consists of an 88-octet message
//! block (`CBS_PDU_LEN`):
//! - Octets 0..=1: Serial Number (Geographical Scope [2b], Message Code [10b], Update Number [4b])
//! - Octets 2..=3: Message Identifier (e.g., `0x1112` for CMAS Presidential Alert)
//! - Octet 4: Data Coding Scheme (3GPP TS 23.038 §5)
//! - Octet 5: Page Parameter (bits 7..4 = current page 1..=15, bits 3..0 = total pages 1..=15)
//! - Octets 6..=87: 82 octets of message content (`CBS_CONTENT_LEN`)

use super::{EncodeError, ParseError, is_7bit_dcs};

/// Total length in octets of a single 3GPP TS 23.041 CBS PDU page.
pub const CBS_PDU_LEN: usize = 88;
/// Length in octets of the 3GPP TS 23.041 CBS header.
pub const CBS_HEADER_LEN: usize = 6;
/// Length in octets of the 3GPP TS 23.041 CBS page content payload.
pub const CBS_CONTENT_LEN: usize = 82;
/// Maximum number of GSM 7-bit septets in a single 82-octet CBS page (`(82 * 8) / 7 = 93`).
pub const CBS_MAX_GSM7_SEPTETS_PER_PAGE: usize = 93;
/// Maximum number of UCS-2 (UTF-16BE) code units in a single 82-octet CBS page (`82 / 2 = 41`).
pub const CBS_MAX_UCS2_CHARS_PER_PAGE: usize = 41;
/// Maximum number of pages in a concatenated CBS message (3GPP TS 23.041 §9.4.1.2.4).
pub const CBS_MAX_PAGES: usize = 15;

/// GSM 7-bit carriage return (`<CR>`, `0x0D`) used as CBS padding character per 3GPP TS 23.038.
const GSM7_CR: u8 = 0x0D;
/// GSM 7-bit escape character (`0x1B`) for the basic character set extension table.
const GSM7_ESC: u8 = 0x1B;

/// Represents a single 88-octet 3GPP TS 23.041 Cell Broadcast Service (CBS) PDU page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CbsPdu {
    serial_number: u16,
    message_id: u16,
    dcs: u8,
    page_param: u8,
    content: [u8; CBS_CONTENT_LEN],
}

impl CbsPdu {
    /// Constructs a new `CbsPdu` from raw header fields and an 82-octet content block.
    ///
    /// Returns [`EncodeError::InvalidFormat`] if `current_page` or `total_pages` is outside
    /// `1..=15` or if `current_page > total_pages`.
    pub fn new(
        serial_number: u16,
        message_id: u16,
        dcs: u8,
        current_page: u8,
        total_pages: u8,
        content: [u8; CBS_CONTENT_LEN],
    ) -> Result<Self, EncodeError> {
        let valid_range = 1..=CBS_MAX_PAGES as u8;
        if !valid_range.contains(&current_page)
            || !valid_range.contains(&total_pages)
            || current_page > total_pages
        {
            return Err(EncodeError::InvalidFormat);
        }
        let page_param = (current_page << 4) | (total_pages & 0x0F);
        Ok(Self { serial_number, message_id, dcs, page_param, content })
    }

    /// Encodes a single-page (`1/1`) CBS PDU from text according to `dcs`.
    ///
    /// Supports GSM 7-bit default alphabet (up to 93 septets), UCS-2 / UTF-16BE (up to 41
    /// 16-bit code units), and 8-bit data (up to 82 bytes). Returns [`EncodeError::InvalidFormat`]
    /// if the encoded text exceeds a single 82-octet page or uses unsupported characters for GSM 7-bit.
    pub fn encode_single_page(
        serial_number: u16,
        message_id: u16,
        dcs: u8,
        text: &str,
    ) -> Result<Self, EncodeError> {
        let mut pages = Self::encode_pages(serial_number, message_id, dcs, text)?;
        if pages.len() != 1 {
            return Err(EncodeError::InvalidFormat);
        }
        pages.pop().ok_or(EncodeError::InvalidFormat)
    }

    /// Encodes text into one or more 88-octet CBS PDU pages (up to 15 pages) according to `dcs`.
    pub fn encode_pages(
        serial_number: u16,
        message_id: u16,
        dcs: u8,
        text: &str,
    ) -> Result<Vec<Self>, EncodeError> {
        let content_pages = if is_ucs2_cbs_dcs(dcs) {
            encode_ucs2_pages(text)?
        } else if is_7bit_cbs_dcs(dcs) {
            encode_gsm7_pages(text)?
        } else {
            encode_8bit_pages(text.as_bytes())?
        };

        let total_pages =
            u8::try_from(content_pages.len()).map_err(|_err| EncodeError::InvalidFormat)?;
        let mut pdus = Vec::with_capacity(content_pages.len());
        for (idx, content) in content_pages.into_iter().enumerate() {
            let current_page = u8::try_from(idx + 1).map_err(|_err| EncodeError::InvalidFormat)?;
            pdus.push(Self::new(
                serial_number,
                message_id,
                dcs,
                current_page,
                total_pages,
                content,
            )?);
        }
        Ok(pdus)
    }

    /// Parses a hex-encoded 88-octet CBS PDU string.
    pub fn parse(hex_str: &str) -> Result<Self, ParseError> {
        let bytes = hex::decode(hex_str).map_err(|_err| ParseError::InvalidHex)?;
        Self::from_bytes(&bytes)
    }

    /// Parses an 88-octet CBS PDU byte slice.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, ParseError> {
        let &[s0, s1, m0, m1, dcs, page_param, ref rest @ ..] = bytes else {
            return Err(ParseError::InvalidFormat);
        };
        let content: [u8; CBS_CONTENT_LEN] =
            rest.try_into().map_err(|_err| ParseError::InvalidFormat)?;
        let serial_number = u16::from_be_bytes([s0, s1]);
        let message_id = u16::from_be_bytes([m0, m1]);

        Ok(Self { serial_number, message_id, dcs, page_param, content })
    }

    /// Serializes this CBS PDU into an 88-octet array.
    pub fn to_bytes(&self) -> [u8; CBS_PDU_LEN] {
        let mut out = [0u8; CBS_PDU_LEN];
        let [s0, s1] = self.serial_number.to_be_bytes();
        let [m0, m1] = self.message_id.to_be_bytes();
        for (dst, src) in out
            .iter_mut()
            .zip([s0, s1, m0, m1, self.dcs, self.page_param].into_iter().chain(self.content))
        {
            *dst = src;
        }
        out
    }

    /// Serializes this CBS PDU into an uppercase 176-character hex string.
    pub fn to_hex(&self) -> String {
        hex::encode_upper(self.to_bytes())
    }

    /// Formats this CBS PDU as an unsolicited `+CBM: <length>\r\n<hex_pdu>\r\n` response
    /// (3GPP TS 27.005 §3.4.1).
    pub fn to_urc(&self) -> String {
        format!("+CBM: {CBS_PDU_LEN}\r\n{}\r\n", self.to_hex())
    }

    /// Returns the 16-bit Serial Number (GS, Message Code, Update Number).
    pub fn serial_number(&self) -> u16 {
        self.serial_number
    }

    /// Returns the 2-bit Geographical Scope (`(serial_number >> 14) & 0x03`).
    pub fn geographical_scope(&self) -> u8 {
        ((self.serial_number >> 14) & 0x03) as u8
    }

    /// Returns the 10-bit Message Code (`(serial_number >> 4) & 0x03FF`).
    pub fn message_code(&self) -> u16 {
        (self.serial_number >> 4) & 0x03FF
    }

    /// Returns the 4-bit Update Number (`serial_number & 0x0F`).
    pub fn update_number(&self) -> u8 {
        (self.serial_number & 0x0F) as u8
    }

    /// Returns the 16-bit Message Identifier (service category / channel ID).
    pub fn message_id(&self) -> u16 {
        self.message_id
    }

    /// Returns the Data Coding Scheme octet.
    pub fn dcs(&self) -> u8 {
        self.dcs
    }

    /// Returns the raw Page Parameter octet.
    pub fn page_param(&self) -> u8 {
        self.page_param
    }

    /// Returns the 1-based current page number (bits 7..4 of `page_param`).
    pub fn current_page(&self) -> u8 {
        (self.page_param >> 4) & 0x0F
    }

    /// Returns the total number of pages (bits 3..0 of `page_param`).
    pub fn total_pages(&self) -> u8 {
        self.page_param & 0x0F
    }

    /// Returns the 82-octet content payload slice.
    pub fn content(&self) -> &[u8; CBS_CONTENT_LEN] {
        &self.content
    }

    /// Decodes the 82-octet page payload into a UTF-8 `String` according to `dcs`,
    /// stripping trailing `<CR>` (`0x0D`) padding.
    pub fn decode_content(&self) -> Result<String, ParseError> {
        if is_ucs2_cbs_dcs(self.dcs) {
            decode_ucs2_page(&self.content)
        } else if is_7bit_cbs_dcs(self.dcs) {
            Ok(decode_gsm7_page(&self.content))
        } else {
            let trimmed_end = self
                .content
                .iter()
                .rposition(|&b| b != GSM7_CR && b != 0x00)
                .map_or(0, |idx| idx + 1);
            let slice = self.content.get(..trimmed_end).unwrap_or(&[]);
            String::from_utf8(slice.to_vec()).map_err(|_err| ParseError::InvalidFormat)
        }
    }
}

/// Determines whether a CBS Data Coding Scheme uses GSM 7-bit packing per 3GPP TS 23.038 §5.
fn is_7bit_cbs_dcs(dcs: u8) -> bool {
    let coding_group = (dcs & 0xF0) >> 4;
    match coding_group {
        // Group 0000, 0010, 0011: Language using the GSM 7-bit default alphabet
        0x0 | 0x2 | 0x3 => true,
        // Group 0001: 0x10 = GSM 7-bit with language in prefix; 0x11 = UCS-2 with language in prefix
        0x1 => dcs == 0x10,
        // Group 01xx: General Data Coding indication — bits 3..2 == 00 indicates GSM 7-bit alphabet
        0x4..=0x7 => (dcs & 0x0C) == 0x00,
        _ => is_7bit_dcs(dcs),
    }
}

/// Determines whether a CBS Data Coding Scheme uses UCS-2 (16-bit) encoding per 3GPP TS 23.038 §5.
fn is_ucs2_cbs_dcs(dcs: u8) -> bool {
    let coding_group = (dcs & 0xF0) >> 4;
    match coding_group {
        0x1 => dcs == 0x11,
        0x4..=0x7 => (dcs & 0x0C) == 0x08,
        _ => false,
    }
}

/// Encodes a UTF-8 string into GSM 7-bit default alphabet septets (including extension table).
fn encode_gsm7_septets(text: &str) -> Result<Vec<u8>, EncodeError> {
    let mut septets = Vec::with_capacity(text.len());
    for ch in text.chars() {
        if let Some(code) = char_to_gsm7_basic(ch) {
            septets.push(code);
        } else if let Some(ext_code) = char_to_gsm7_ext(ch) {
            septets.push(GSM7_ESC);
            septets.push(ext_code);
        } else {
            return Err(EncodeError::InvalidFormat);
        }
    }
    Ok(septets)
}

/// Packs up to 93 GSM 7-bit septets into an 82-octet CBS page, padding unused septets with `0x0D`.
fn pack_gsm7_cbs_page(septets: &[u8]) -> Result<[u8; CBS_CONTENT_LEN], EncodeError> {
    if septets.len() > CBS_MAX_GSM7_SEPTETS_PER_PAGE {
        return Err(EncodeError::InvalidFormat);
    }

    let mut padded = [GSM7_CR; CBS_MAX_GSM7_SEPTETS_PER_PAGE];
    for (dst, &src) in padded.iter_mut().zip(septets.iter()) {
        if src > 0x7F {
            return Err(EncodeError::InvalidFormat);
        }
        *dst = src;
    }

    let mut out = [0u8; CBS_CONTENT_LEN];
    for (i, &septet) in padded.iter().enumerate() {
        let bit_offset = i * 7;
        let byte_idx = bit_offset / 8;
        let bit_shift = bit_offset % 8;

        if let Some(b) = out.get_mut(byte_idx) {
            *b |= (septet & 0x7F) << bit_shift;
        }
        if bit_shift > 1
            && let Some(b_next) = out.get_mut(byte_idx + 1)
        {
            *b_next |= (septet & 0x7F) >> (8 - bit_shift);
        }
    }
    Ok(out)
}

/// Splits a text string into up to 15 GSM 7-bit packed 82-octet CBS pages, avoiding splitting
/// a 2-septet `0x1B` extension sequence across a page boundary.
fn encode_gsm7_pages(text: &str) -> Result<Vec<[u8; CBS_CONTENT_LEN]>, EncodeError> {
    let septets = encode_gsm7_septets(text)?;
    if septets.is_empty() {
        return Ok(vec![pack_gsm7_cbs_page(&[])?]);
    }

    let mut pages = Vec::new();
    let mut start = 0;
    while start < septets.len() {
        let mut end = (start + CBS_MAX_GSM7_SEPTETS_PER_PAGE).min(septets.len());
        // Do not leave a trailing GSM7_ESC (0x1B) at the end of a page if its extension char is on the next page
        if end < septets.len() && septets.get(end.saturating_sub(1)) == Some(&GSM7_ESC) {
            end -= 1;
        }
        let slice = septets.get(start..end).ok_or(EncodeError::InvalidFormat)?;
        pages.push(pack_gsm7_cbs_page(slice)?);
        if pages.len() > CBS_MAX_PAGES {
            return Err(EncodeError::InvalidFormat);
        }
        start = end;
    }
    Ok(pages)
}

/// Unpacks an 82-octet GSM 7-bit CBS page into a UTF-8 `String`, stripping trailing `<CR>` septets.
fn decode_gsm7_page(content: &[u8; CBS_CONTENT_LEN]) -> String {
    let mut septets = [0u8; CBS_MAX_GSM7_SEPTETS_PER_PAGE];
    for (i, slot) in septets.iter_mut().enumerate() {
        let bit_offset = i * 7;
        let byte_idx = bit_offset / 8;
        let bit_shift = bit_offset % 8;

        let low = content.get(byte_idx).copied().unwrap_or(0) >> bit_shift;
        let high = if bit_shift > 1 {
            content.get(byte_idx + 1).copied().unwrap_or(0) << (8 - bit_shift)
        } else {
            0
        };
        *slot = (low | high) & 0x7F;
    }

    let trimmed_len = septets.iter().rposition(|&s| s != GSM7_CR).map_or(0, |idx| idx + 1);
    let trimmed = septets.get(..trimmed_len).unwrap_or(&[]);

    let mut out = String::with_capacity(trimmed.len());
    let mut iter = trimmed.iter().copied();
    while let Some(code) = iter.next() {
        if code == GSM7_ESC
            && let Some(ext) = iter.next()
        {
            out.push(gsm7_ext_to_char(ext).unwrap_or_else(|| gsm7_basic_to_char(ext)));
        } else {
            out.push(gsm7_basic_to_char(code));
        }
    }
    out
}

/// Encodes text into up to 15 UCS-2 (UTF-16BE) 82-octet CBS pages padded with `0x000D`.
fn encode_ucs2_pages(text: &str) -> Result<Vec<[u8; CBS_CONTENT_LEN]>, EncodeError> {
    let code_units: Vec<u16> = text.encode_utf16().collect();
    if code_units.is_empty() {
        let mut page = [0u8; CBS_CONTENT_LEN];
        for chunk in page.as_chunks_mut::<2>().0 {
            *chunk = u16::from(GSM7_CR).to_be_bytes();
        }
        return Ok(vec![page]);
    }

    let mut pages = Vec::new();
    let mut start = 0;
    while start < code_units.len() {
        let mut end = (start + CBS_MAX_UCS2_CHARS_PER_PAGE).min(code_units.len());
        // Do not split a UTF-16 surrogate pair across a page boundary
        if end < code_units.len()
            && code_units
                .get(end.saturating_sub(1))
                .is_some_and(|&u| (0xD800..=0xDBFF).contains(&u))
        {
            end -= 1;
        }
        let chunk = code_units.get(start..end).ok_or(EncodeError::InvalidFormat)?;
        let mut page = [0u8; CBS_CONTENT_LEN];
        for (i, dst) in page.as_chunks_mut::<2>().0.iter_mut().enumerate() {
            let unit = chunk.get(i).copied().unwrap_or(u16::from(GSM7_CR));
            *dst = unit.to_be_bytes();
        }
        pages.push(page);
        if pages.len() > CBS_MAX_PAGES {
            return Err(EncodeError::InvalidFormat);
        }
        start = end;
    }
    Ok(pages)
}

/// Decodes an 82-octet UCS-2 (UTF-16BE) CBS page, stripping trailing `0x000D` or `0x0000` padding.
fn decode_ucs2_page(content: &[u8; CBS_CONTENT_LEN]) -> Result<String, ParseError> {
    let mut units = Vec::with_capacity(CBS_MAX_UCS2_CHARS_PER_PAGE);
    for &pair in content.as_chunks::<2>().0 {
        units.push(u16::from_be_bytes(pair));
    }
    while let Some(&last) = units.last() {
        if last == u16::from(GSM7_CR) || last == 0x0000 {
            units.pop();
        } else {
            break;
        }
    }
    String::from_utf16(&units).map_err(|_err| ParseError::InvalidFormat)
}

/// Encodes raw bytes into up to 15 8-bit 82-octet CBS pages padded with `0x0D`.
fn encode_8bit_pages(data: &[u8]) -> Result<Vec<[u8; CBS_CONTENT_LEN]>, EncodeError> {
    if data.is_empty() {
        return Ok(vec![[GSM7_CR; CBS_CONTENT_LEN]]);
    }
    let chunks: Vec<&[u8]> = data.chunks(CBS_CONTENT_LEN).collect();
    if chunks.len() > CBS_MAX_PAGES {
        return Err(EncodeError::InvalidFormat);
    }

    let mut pages = Vec::with_capacity(chunks.len());
    for chunk in chunks {
        let mut page = [GSM7_CR; CBS_CONTENT_LEN];
        if let Some(dst) = page.get_mut(..chunk.len()) {
            dst.copy_from_slice(chunk);
        }
        pages.push(page);
    }
    Ok(pages)
}

const GSM7_BASIC_SPECIALS: &[(char, u8)] = &[
    ('@', 0x00),
    ('£', 0x01),
    ('$', 0x02),
    ('¥', 0x03),
    ('è', 0x04),
    ('é', 0x05),
    ('ù', 0x06),
    ('ì', 0x07),
    ('ò', 0x08),
    ('Ç', 0x09),
    ('\n', 0x0A),
    ('Ø', 0x0B),
    ('ø', 0x0C),
    ('\r', 0x0D),
    ('Å', 0x0E),
    ('å', 0x0F),
    ('Δ', 0x10),
    ('_', 0x11),
    ('Φ', 0x12),
    ('Γ', 0x13),
    ('Λ', 0x14),
    ('Ω', 0x15),
    ('Π', 0x16),
    ('Ψ', 0x17),
    ('Σ', 0x18),
    ('Θ', 0x19),
    ('Ξ', 0x1A),
    ('Æ', 0x1C),
    ('æ', 0x1D),
    ('ß', 0x1E),
    ('É', 0x1F),
    ('¤', 0x24),
    ('¡', 0x40),
    ('Ä', 0x5B),
    ('Ö', 0x5C),
    ('Ñ', 0x5D),
    ('Ü', 0x5E),
    ('§', 0x5F),
    ('¿', 0x60),
    ('ä', 0x7B),
    ('ö', 0x7C),
    ('ñ', 0x7D),
    ('ü', 0x7E),
    ('à', 0x7F),
];

const GSM7_EXTENSION_MAP: &[(char, u8)] = &[
    ('\x0C', 0x0A),
    ('^', 0x14),
    ('{', 0x28),
    ('}', 0x29),
    ('\\', 0x2F),
    ('[', 0x3C),
    ('~', 0x3D),
    (']', 0x3E),
    ('|', 0x40),
    ('€', 0x65),
];

/// Maps a Unicode character to its 3GPP TS 23.038 GSM 7-bit default alphabet code point.
fn char_to_gsm7_basic(ch: char) -> Option<u8> {
    if let Some(&(_, code)) = GSM7_BASIC_SPECIALS.iter().find(|(c, _)| *c == ch) {
        return Some(code);
    }
    if matches!(ch, ' '..='#' | '%'..='?' | 'A'..='Z' | 'a'..='z') { Some(ch as u8) } else { None }
}

/// Maps a 3GPP TS 23.038 GSM 7-bit default alphabet code point to a Unicode character.
fn gsm7_basic_to_char(code: u8) -> char {
    if let Some(&(ch, _)) = GSM7_BASIC_SPECIALS.iter().find(|(_, c)| *c == code) {
        return ch;
    }
    match code {
        GSM7_ESC => ' ',
        0x20..=0x7A => code as char,
        _ => '?',
    }
}

/// Maps a Unicode character to its GSM 7-bit extension table code point (preceded by `0x1B`).
fn char_to_gsm7_ext(ch: char) -> Option<u8> {
    GSM7_EXTENSION_MAP.iter().find(|(c, _)| *c == ch).map(|&(_, code)| code)
}

/// Maps a GSM 7-bit extension table code point to a Unicode character.
fn gsm7_ext_to_char(code: u8) -> Option<char> {
    GSM7_EXTENSION_MAP.iter().find(|(_, c)| *c == code).map(|&(ch, _)| ch)
}

#[cfg(test)]
mod tests {
    use super::*;

    // 3GPP TS 23.041 / Android GsmCellBroadcastHandlerTest vector:
    // Serial Number = 0x1123 (GS=0, Message Code=0x112, Update=3)
    // Message ID = 0x1112 (4370, CMAS Presidential Level Alert)
    // DCS = 0x01 (GSM 7-bit default alphabet, English)
    // Page Parameter = 0x11 (Page 1 of 1)
    // Content = "Hello" (5 septets: C8 32 9B FD 06) padded with 88 <CR> (0x0D) septets to 93 septets.
    const TEST_CBS_HELLO_HEX: &str = concat!(
        "112311120111",
        "C8329BFD6E341A8D46A3D168341A8D46A3D168341A8D46A3D168341A8D46A3D1",
        "68341A8D46A3D168341A8D46A3D168341A8D46A3D168341A8D46A3D168341A8D",
        "46A3D168341A8D46A3D168341A8D46A3D100"
    );

    #[test]
    fn test_encode_presidential_alert_gsm7_vector() {
        let pdu = CbsPdu::encode_single_page(0x1123, 0x1112, 0x01, "Hello").unwrap();
        assert_eq!(pdu.to_bytes().len(), CBS_PDU_LEN);
        assert_eq!(pdu.to_hex(), TEST_CBS_HELLO_HEX);
        assert_eq!(pdu.serial_number(), 0x1123);
        assert_eq!(pdu.geographical_scope(), 0);
        assert_eq!(pdu.message_code(), 0x0112);
        assert_eq!(pdu.update_number(), 0x03);
        assert_eq!(pdu.message_id(), 0x1112);
        assert_eq!(pdu.dcs(), 0x01);
        assert_eq!(pdu.page_param(), 0x11);
        assert_eq!(pdu.current_page(), 1);
        assert_eq!(pdu.total_pages(), 1);
        assert_eq!(pdu.decode_content().unwrap(), "Hello");
        assert_eq!(pdu.to_urc(), format!("+CBM: 88\r\n{TEST_CBS_HELLO_HEX}\r\n"));
    }

    #[test]
    fn test_parse_cbs_pdu_and_roundtrip() {
        let parsed = CbsPdu::parse(TEST_CBS_HELLO_HEX).unwrap();
        assert_eq!(parsed.serial_number(), 0x1123);
        assert_eq!(parsed.message_id(), 0x1112);
        assert_eq!(parsed.dcs(), 0x01);
        assert_eq!(parsed.current_page(), 1);
        assert_eq!(parsed.total_pages(), 1);
        assert_eq!(parsed.decode_content().unwrap(), "Hello");
        assert_eq!(parsed.to_hex(), TEST_CBS_HELLO_HEX);
    }

    #[test]
    fn test_gsm7_extension_and_max_93_septets() {
        let text_with_ext = "Alert [CMAS] €100 {test}";
        let pdu = CbsPdu::encode_single_page(0x4010, 0x1113, 0x00, text_with_ext).unwrap();
        assert_eq!(pdu.geographical_scope(), 1);
        assert_eq!(pdu.message_code(), 0x0001);
        assert_eq!(pdu.update_number(), 0x00);
        assert_eq!(pdu.decode_content().unwrap(), text_with_ext);

        // 3GPP TS 23.038 §6.2.1.1: escape (0x1B) followed by a septet not in the extension
        // table (e.g. 0x41 'A') falls back to the basic alphabet character ('A').
        let fallback_content = pack_gsm7_cbs_page(&[GSM7_ESC, 0x41]).unwrap();
        let fallback_pdu = CbsPdu::new(0x0001, 0x1112, 0x01, 1, 1, fallback_content).unwrap();
        assert_eq!(fallback_pdu.decode_content().unwrap(), "A");

        // Exactly 93 GSM 7-bit characters fits in 1 page
        let exact_93: String = (0..93).map(|i| (b'A' + (i % 26)) as char).collect();
        let pdu_93 = CbsPdu::encode_single_page(0x0001, 0x1112, 0x01, &exact_93).unwrap();
        assert_eq!(pdu_93.decode_content().unwrap(), exact_93);

        // 94 characters requires 2 pages, so encode_single_page rejects it
        let over_93: String = (0..94).map(|i| (b'A' + (i % 26)) as char).collect();
        assert_eq!(
            CbsPdu::encode_single_page(0x0001, 0x1112, 0x01, &over_93),
            Err(EncodeError::InvalidFormat)
        );
    }

    #[test]
    fn test_multi_page_cbs_encoding() {
        let long_text: String = (0..200).map(|i| (b'a' + (i % 26) as u8) as char).collect();
        let pages = CbsPdu::encode_pages(0x2105, 0x1112, 0x01, &long_text).unwrap();
        assert_eq!(pages.len(), 3);

        let mut reassembled = String::new();
        for (i, page) in pages.iter().enumerate() {
            assert_eq!(page.current_page(), (i + 1) as u8);
            assert_eq!(page.total_pages(), 3);
            assert_eq!(page.page_param(), (((i + 1) as u8) << 4) | 3);
            reassembled.push_str(&page.decode_content().unwrap());
        }
        assert_eq!(reassembled, long_text);
    }

    #[test]
    fn test_ucs2_cbs_encoding_and_etws_vector() {
        // Android GsmSmsCbMessageTest etwsMessageNormal vector padded to 88 octets
        // (Serial=0x0000, MID=0x1100, DCS=0x11, Page=0x01)
        let etws_hex = concat!(
            "000011001101",
            "0D0A5BAE57CE770C531790E85C716CBF3044573065B930675730",
            "9707767A751F30025F37304463FA308C306B5099304830664E0B30553044FF086C178C615E81FF09",
            "00000000000000000000000000000000"
        );
        let etws_pdu = CbsPdu::parse(etws_hex).unwrap();
        assert_eq!(etws_pdu.serial_number(), 0x0000);
        assert_eq!(etws_pdu.message_id(), 0x1100);
        assert_eq!(etws_pdu.dcs(), 0x11);
        let decoded = etws_pdu.decode_content().unwrap();
        assert!(decoded.contains("宮城県北部山沿い地方で地震発生"));

        // Round-trip UCS-2 encoding with DCS 0x48 (General Data Coding UCS-2)
        let ucs2_text = "緊急地震速報 Emergency Alert";
        let ucs2_pdu = CbsPdu::encode_single_page(0x3001, 0x1100, 0x48, ucs2_text).unwrap();
        assert_eq!(ucs2_pdu.decode_content().unwrap(), ucs2_text);

        // Round-trip GSM 7-bit encoding with DCS 0x40 (General Data Coding GSM 7-bit)
        let gdc_7bit_pdu = CbsPdu::encode_single_page(0x1123, 0x1112, 0x40, "Hello").unwrap();
        assert_eq!(gdc_7bit_pdu.decode_content().unwrap(), "Hello");
    }

    #[test]
    fn test_invalid_page_params_and_parse_errors() {
        let content = [GSM7_CR; CBS_CONTENT_LEN];
        assert_eq!(CbsPdu::new(0, 0x1112, 0x01, 0, 1, content), Err(EncodeError::InvalidFormat));
        assert_eq!(CbsPdu::new(0, 0x1112, 0x01, 1, 0, content), Err(EncodeError::InvalidFormat));
        assert_eq!(CbsPdu::new(0, 0x1112, 0x01, 2, 1, content), Err(EncodeError::InvalidFormat));
        assert_eq!(CbsPdu::new(0, 0x1112, 0x01, 1, 16, content), Err(EncodeError::InvalidFormat));

        assert_eq!(CbsPdu::parse("ZZZZ"), Err(ParseError::InvalidHex));
        assert_eq!(CbsPdu::parse("112311120111"), Err(ParseError::InvalidFormat));
        let too_long = format!("{TEST_CBS_HELLO_HEX}00");
        assert_eq!(CbsPdu::parse(&too_long), Err(ParseError::InvalidFormat));
    }

    #[test]
    fn test_ucs2_cbs_surrogate_pair_page_boundary() {
        // 40 BMP chars + 1 emoji (2 UTF-16 code units).
        // 40 chars fit on page 1; the emoji would straddle the 41-code-unit boundary.
        // It must not split: page 1 should contain 40 chars, page 2 contains the emoji.
        let mut text = "A".repeat(40);
        text.push('🚨'); // U+1F6A8: [0xD83D, 0xDEA8]
        let pages = CbsPdu::encode_pages(0x1000, 0x1100, 0x48, &text).unwrap();
        assert_eq!(pages.len(), 2);
        assert_eq!(pages.first().unwrap().decode_content().unwrap(), "A".repeat(40));
        assert_eq!(pages.get(1).unwrap().decode_content().unwrap(), "🚨");
    }
}
