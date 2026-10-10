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

//! BCD (Binary Coded Decimal) helper functions for SMS PDU encoding/decoding.

/// Decodes semi-octet (BCD) representation to a string.
/// Swaps every pair of nibbles. Stops on padding 'F' (0x0F).
/// Supports GSM special characters (*, #, a, b, c) mapped to 10..14.
pub fn bcd_to_string(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        let low = b & 0x0F;
        let high = b >> 4;

        match low {
            0..=9 => s.push((b'0' + low) as char),
            10 => s.push('*'),
            11 => s.push('#'),
            12 => s.push('a'),
            13 => s.push('b'),
            14 => s.push('c'),
            _ => break,
        }

        match high {
            0..=9 => s.push((b'0' + high) as char),
            10 => s.push('*'),
            11 => s.push('#'),
            12 => s.push('a'),
            13 => s.push('b'),
            14 => s.push('c'),
            _ => break,
        }
    }
    s
}

/// Encodes a string of digits and special characters (*, #, a, b, c)
/// to semi-octet (BCD) representation.
/// Swaps every pair of digits. Pads with 'F' (0x0F) if odd length.
pub fn string_to_bcd(s: &str) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(s.len().div_ceil(2));
    let mut bytes_iter = s.bytes();

    let char_to_bcd = |b: u8| -> u8 {
        match b {
            b'0'..=b'9' => b - b'0',
            b'*' => 10,
            b'#' => 11,
            b'a' => 12,
            b'b' => 13,
            b'c' => 14,
            _ => 0,
        }
    };

    while let Some(b1) = bytes_iter.next() {
        let low = char_to_bcd(b1);
        let high = match bytes_iter.next() {
            Some(b2) => char_to_bcd(b2),
            None => 0x0F, // Pad
        };
        bytes.push((high << 4) | low);
    }
    bytes
}

/// Converts a byte to its BCD representation (swapped nibbles).
/// Assumes value is less than 100.
pub fn to_bcd_byte(val: u8) -> u8 {
    debug_assert!(val < 100, "Value must be less than 100 for BCD encoding");
    let val = val % 100; // Safety net for release mode (prevent bit-clobbering)
    let hi = val / 10;
    let lo = val % 10;
    (lo << 4) | hi
}

/// Encodes an IMSI string to its BCD representation according to 3GPP TS
/// 31.102. Returns None if IMSI is empty or contains non-digits.
pub fn encode_imsi(imsi: &str) -> Option<Vec<u8>> {
    if imsi.is_empty() || !imsi.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }

    let is_odd = !imsi.len().is_multiple_of(2);
    let odd_even_indicator = if is_odd { 1 } else { 0 };
    // Identity type for IMSI is 1 (001 binary)
    let identity_type = 1;
    let flags = (odd_even_indicator << 3) | identity_type;

    let first_digit = imsi.chars().next()?.to_digit(10).unwrap_or(0) as u8;
    let byte_2 = (first_digit << 4) | flags;

    let remaining_digits = imsi.get(1..)?;
    let swapped_bytes = string_to_bcd(remaining_digits);

    let len_byte = (1 + swapped_bytes.len()) as u8;
    let mut result = Vec::with_capacity(1 + len_byte as usize);
    result.push(len_byte);
    result.push(byte_2);
    result.extend(swapped_bytes);
    Some(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bcd_to_string() {
        let bytes = [0x12, 0x34, 0x56, 0x78];
        assert_eq!(bcd_to_string(&bytes), "21436587");

        let bytes_padded = [0x12, 0x34, 0xF5];
        assert_eq!(bcd_to_string(&bytes_padded), "21435");

        // Edge Case: Single digit BCD
        assert_eq!(bcd_to_string(&[0xF5]), "5");
    }

    #[test]
    fn test_string_to_bcd() {
        assert_eq!(string_to_bcd("21436587"), vec![0x12, 0x34, 0x56, 0x78]);
        assert_eq!(string_to_bcd("21435"), vec![0x12, 0x34, 0xF5]);

        // Edge Case: Single digit BCD
        assert_eq!(string_to_bcd("5"), vec![0xF5]);
    }

    #[test]
    fn test_bcd_special_characters() {
        let s = "*123#abc";
        let bcd = string_to_bcd(s);
        // * -> 10 (A), 1 -> 1. Byte 1: (1 << 4) | 10 = 0x1A
        // 2 -> 2, 3 -> 3. Byte 2: (3 << 4) | 2 = 0x32
        // # -> 11 (B), a -> 12 (C). Byte 3: (12 << 4) | 11 = 0xCB
        // b -> 13 (D), c -> 14 (E). Byte 4: (14 << 4) | 13 = 0xED
        assert_eq!(bcd, vec![0x1A, 0x32, 0xCB, 0xED]);
        assert_eq!(bcd_to_string(&bcd), s);
    }

    #[test]
    fn test_to_bcd_byte() {
        assert_eq!(to_bcd_byte(0), 0x00);
        assert_eq!(to_bcd_byte(45), 0x54);
        assert_eq!(to_bcd_byte(99), 0x99);
    }

    #[test]
    fn test_encode_imsi() {
        // Odd length IMSI (15 digits): "311740123456789"
        assert_eq!(
            encode_imsi("311740123456789"),
            Some(hex::decode("083911471032547698").unwrap())
        );

        // Even length IMSI (14 digits): "31174012345678"
        assert_eq!(encode_imsi("31174012345678"), Some(hex::decode("0831114710325476F8").unwrap()));

        // Invalid inputs
        assert_eq!(encode_imsi(""), None);
        assert_eq!(encode_imsi("123a45"), None);

        // New Edge Cases:
        // 1. Single digit IMSI
        assert_eq!(encode_imsi("1"), Some(hex::decode("0119").unwrap()));
        // 2. Trailing invalid character
        assert_eq!(encode_imsi("12345678901234a"), None);
        // 3. Small even length IMSI (2 digits)
        assert_eq!(encode_imsi("12"), Some(hex::decode("0211F2").unwrap()));
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "Value must be less than 100")]
    fn test_to_bcd_byte_panic_in_debug() {
        super::to_bcd_byte(100);
    }

    #[test]
    #[cfg(not(debug_assertions))]
    fn test_to_bcd_byte_release_safety() {
        // Verifies the modulo safety net (105 % 100 = 5 -> BCD 05 -> swapped 0x50)
        assert_eq!(to_bcd_byte(105), 0x50);
    }
}
