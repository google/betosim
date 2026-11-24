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

//! 3GPP TS 23.038 Character Sets (GSM 7-bit default alphabet & extension table) and septet packing.

/// GSM 7-bit carriage return (`<CR>`, `0x0D`) used as padding character per 3GPP TS 23.038.
pub(super) const GSM7_CR: u8 = 0x0D;

/// 3GPP TS 23.038 § 6.2.1.1: escape septet (`0x1B`) introducing an Extension Table character.
pub(super) const GSM7_ESC: u8 = 0x1B;

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
pub(super) fn gsm7_basic_to_char(code: u8) -> char {
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
pub(super) fn gsm7_ext_to_char(code: u8) -> Option<char> {
    GSM7_EXTENSION_MAP.iter().find(|(_, c)| *c == code).map(|&(ch, _)| ch)
}

/// Returns the number of septets required to encode `ch` in 3GPP TS 23.038 GSM 7-bit,
/// or `None` if `ch` is outside the GSM 7-bit alphabet.
fn gsm7_char_cost(ch: char) -> Option<usize> {
    if char_to_gsm7_basic(ch).is_some() {
        Some(1)
    } else if char_to_gsm7_ext(ch).is_some() {
        Some(2)
    } else {
        None
    }
}

/// 3GPP TS 23.038 § 4 Data Coding Scheme.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct DataCodingScheme(u8);

impl DataCodingScheme {
    pub const GSM_7BIT_DEFAULT: Self = Self(0x00);
    pub const UCS2: Self = Self(0x08);

    const CODING_GROUP_MASK: u8 = 0xC0;
    const GENERAL_CODING_GROUP: u8 = 0x00;
    const AUTO_DELETION_GROUP: u8 = 0x40;
    const RESERVED_CODING_GROUP: u8 = 0x80;
    const MWI_GROUP_MASK: u8 = 0xF0;
    const MWI_DISCARD_7BIT: u8 = 0xC0;
    const MWI_STORE_7BIT: u8 = 0xD0;
    const MWI_STORE_UCS2: u8 = 0xE0;
    const DATA_CODING_MESSAGE_CLASS: u8 = 0xF0;
    const ALPHABET_MASK: u8 = 0x0C;
    const ALPHABET_7BIT: u8 = 0x00;
    const MESSAGE_CLASS_8BIT_MASK: u8 = 0x04;

    pub const fn from_raw(dcs: u8) -> Self {
        Self(dcs)
    }

    pub const fn as_u8(self) -> u8 {
        self.0
    }

    pub const fn uses_gsm_7bit(self) -> bool {
        let dcs = self.0;
        match dcs & Self::CODING_GROUP_MASK {
            Self::GENERAL_CODING_GROUP | Self::AUTO_DELETION_GROUP => {
                (dcs & Self::ALPHABET_MASK) == Self::ALPHABET_7BIT
            }
            Self::RESERVED_CODING_GROUP => false,
            _ => match dcs & Self::MWI_GROUP_MASK {
                Self::MWI_DISCARD_7BIT | Self::MWI_STORE_7BIT => true,
                Self::MWI_STORE_UCS2 => false,
                Self::DATA_CODING_MESSAGE_CLASS => (dcs & Self::MESSAGE_CLASS_8BIT_MASK) == 0,
                _ => false,
            },
        }
    }
}

/// Target character alphabet for SMS text encoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Alphabet {
    Gsm7,
    Ucs2,
}

/// Maximum TP-User-Data payload size in octets per 3GPP TS 23.040 § 9.2.3.24.
const MAX_TP_UD_OCTETS: usize = 140;
/// Total octets in an 8-bit reference concatenated SMS UDH (`[UDHL=0x05, IEI=0x00, IEDL=0x03, ref, total, seq]`).
pub(super) const CONCAT_UDH_OCTETS: usize = 6;
/// Number of 7-bit septets occupied by the 6-octet (48-bit) concatenation UDH (`ceil(48 / 7) = 7`, leaving 1 fill bit).
pub(super) const CONCAT_UDH_SEPTETS: usize = (CONCAT_UDH_OCTETS * 8).div_ceil(7);
/// Maximum GSM 7-bit septets in an unsegmented SMS (`(140 * 8) / 7 = 160`).
const GSM7_MAX_UNSEGMENTED_SEPTETS: usize = (MAX_TP_UD_OCTETS * 8) / 7;
/// Maximum UTF-16 code units in an unsegmented UCS-2 SMS (`140 / 2 = 70`).
const UCS2_MAX_UNSEGMENTED_CHARS: usize = MAX_TP_UD_OCTETS / 2;
/// Maximum GSM 7-bit septets per concatenated SMS segment (`160 - 7 = 153`).
const GSM7_MAX_CONCAT_SEPTETS: usize = GSM7_MAX_UNSEGMENTED_SEPTETS - CONCAT_UDH_SEPTETS;
/// Maximum UTF-16 code units per concatenated UCS-2 SMS segment (`(140 - 6) / 2 = 67`).
const UCS2_MAX_CONCAT_CHARS: usize = (MAX_TP_UD_OCTETS - CONCAT_UDH_OCTETS) / 2;

impl Alphabet {
    /// Classifies `text` into the most compact supported alphabet.
    ///
    /// Returns `Alphabet::Gsm7` if every character is in the 3GPP TS 23.038 GSM 7-bit
    /// default alphabet or extension table, else `Alphabet::Ucs2`.
    pub fn classify(text: &str) -> Self {
        if text.chars().all(|ch| gsm7_char_cost(ch).is_some()) { Self::Gsm7 } else { Self::Ucs2 }
    }

    pub const fn dcs(self) -> DataCodingScheme {
        match self {
            Self::Gsm7 => DataCodingScheme::GSM_7BIT_DEFAULT,
            Self::Ucs2 => DataCodingScheme::UCS2,
        }
    }

    pub const fn max_unsegmented_capacity(self) -> usize {
        match self {
            Self::Gsm7 => GSM7_MAX_UNSEGMENTED_SEPTETS,
            Self::Ucs2 => UCS2_MAX_UNSEGMENTED_CHARS,
        }
    }

    pub(super) const fn max_concat_segment_capacity(self) -> usize {
        match self {
            Self::Gsm7 => GSM7_MAX_CONCAT_SEPTETS,
            Self::Ucs2 => UCS2_MAX_CONCAT_CHARS,
        }
    }

    pub(super) fn char_cost(self, ch: char) -> usize {
        match self {
            Self::Gsm7 => gsm7_char_cost(ch).unwrap_or(1),
            Self::Ucs2 => ch.len_utf16(),
        }
    }

    pub fn measure(self, text: &str) -> usize {
        text.chars().map(|ch| self.char_cost(ch)).sum()
    }
}

/// Encodes a UTF-8 string into UCS-2 (UTF-16BE) bytes.
pub(super) fn encode_ucs2_bytes(text: &str) -> Vec<u8> {
    text.encode_utf16().flat_map(u16::to_be_bytes).collect()
}

/// Encodes a UTF-8 string into GSM 7-bit default alphabet septets (including extension table).
/// Returns `None` if `text` contains any character outside the 3GPP TS 23.038 alphabet.
pub(super) fn encode_gsm7_septets(text: &str) -> Option<Vec<u8>> {
    let mut septets = Vec::with_capacity(text.len());
    for ch in text.chars() {
        if let Some(code) = char_to_gsm7_basic(ch) {
            septets.push(code);
        } else if let Some(ext_code) = char_to_gsm7_ext(ch) {
            septets.push(GSM7_ESC);
            septets.push(ext_code);
        } else {
            return None;
        }
    }
    Some(septets)
}

/// Packs 7-bit septets into 8-bit octets per 3GPP TS 23.040 § 9.1.2.1.
///
/// `fill_bits` (which must be in `0..=6` per 3GPP TS 23.040) leaves that many
/// zero bits at the bottom of the first octet so that a payload following a
/// User Data Header starts on a septet boundary (3GPP TS 23.040 § 9.2.3.24).
///
/// Unused bits in the final octet are zero-filled per SMS standard.
pub(super) fn pack_septets_with_fill(septets: &[u8], fill_bits: u8) -> Vec<u8> {
    if septets.is_empty() {
        return Vec::new();
    }
    let fill_shift = fill_bits % 7;
    let mut packed = Vec::with_capacity((septets.len() * 7 + fill_shift as usize).div_ceil(8));
    let mut current_byte = 0u8;
    let mut shift = fill_shift;

    for &septet in septets {
        current_byte |= (septet & 0x7F) << shift;
        shift += 7;
        if shift >= 8 {
            packed.push(current_byte);
            shift -= 8;
            current_byte = (septet & 0x7F) >> (7 - shift);
        }
    }
    if shift > 0 {
        packed.push(current_byte);
    }
    packed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encode_ucs2_bytes() {
        let bytes = encode_ucs2_bytes("AB");
        assert_eq!(bytes, vec![0x00, b'A', 0x00, b'B']);
    }

    #[test]
    fn test_encode_gsm7_septets_basic() {
        let septets = encode_gsm7_septets("ABC 123").expect("valid GSM-7 basic text");
        assert_eq!(septets, vec![b'A', b'B', b'C', b' ', b'1', b'2', b'3']);

        let specials = encode_gsm7_septets("@£$¥").expect("valid GSM-7 specials");
        assert_eq!(specials, vec![0x00, 0x01, 0x02, 0x03]);
    }

    #[test]
    fn test_encode_gsm7_septets_extension_and_unsupported() {
        let ext_text = "€{}[]\\^~|";
        let septets = encode_gsm7_septets(ext_text).expect("valid GSM-7 extension text");
        let expected = vec![
            GSM7_ESC, 0x65, // €
            GSM7_ESC, 0x28, // {
            GSM7_ESC, 0x29, // }
            GSM7_ESC, 0x3C, // [
            GSM7_ESC, 0x3E, // ]
            GSM7_ESC, 0x2F, // \
            GSM7_ESC, 0x14, // ^
            GSM7_ESC, 0x3D, // ~
            GSM7_ESC, 0x40, // |
        ];
        assert_eq!(septets, expected);

        assert_eq!(encode_gsm7_septets("Hello 😀"), None);
        assert_eq!(encode_gsm7_septets("你好"), None);
        assert_eq!(encode_gsm7_septets("Привет"), None);
    }

    #[test]
    fn test_pack_septets_with_fill_zero() {
        // "hello" -> septets [0x68, 0x65, 0x6C, 0x6C, 0x6F]
        let septets = [0x68, 0x65, 0x6C, 0x6C, 0x6F];
        let packed = pack_septets_with_fill(&septets, 0);
        assert_eq!(packed, vec![0xE8, 0x32, 0x9B, 0xFD, 0x06]);

        assert!(pack_septets_with_fill(&[], 0).is_empty());

        // 7-septet boundary: 7 * 7 = 49 bits -> 7 octets (last octet has 1 bit + 7 pad bits)
        let packed_7 = pack_septets_with_fill(&[0x41; 7], 0);
        assert_eq!(packed_7.len(), 7);
        assert_eq!(packed_7.get(6).copied().unwrap_or(0xFF) & 0xFE, 0x00, "pad bits must be zero");

        // 8-septet boundary: 8 * 7 = 56 bits -> exactly 7 octets
        let packed_8 = pack_septets_with_fill(&[0x41; 8], 0);
        assert_eq!(packed_8.len(), 7);
    }

    #[test]
    fn test_pack_septets_with_fill_one() {
        // One fill bit shifts the septet up by one: 0x41 << 1 == 0x82.
        assert_eq!(pack_septets_with_fill(&[0x41], 1), vec![0x82]);
        assert!(pack_septets_with_fill(&[], 1).is_empty());

        let packed = pack_septets_with_fill(&[0x68, 0x65], 1);
        assert_eq!(packed, vec![0xD0, 0x65]);
    }

    #[test]
    fn test_gsm7_decode_tables() {
        assert_eq!(gsm7_basic_to_char(0x00), '@');
        assert_eq!(gsm7_basic_to_char(b'A'), 'A');
        assert_eq!(gsm7_ext_to_char(0x65), Some('€'));
        assert_eq!(gsm7_ext_to_char(0x28), Some('{'));
        assert_eq!(gsm7_ext_to_char(0x7F), None);
    }
}
