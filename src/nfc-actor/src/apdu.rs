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

//! ISO 7816-4 Command-APDU framing parser.

pub(crate) const SW_WRONG_LENGTH: [u8; 2] = [0x67, 0x00];

const APDU_HEADER_LEN: usize = 4;
const APDU_EXTENDED_LENGTH_MARKER: u8 = 0x00;
const APDU_EXTENDED_LC_LEN: usize = 2;
const APDU_SHORT_LE_WILDCARD: usize = 256;
const APDU_EXTENDED_LE_WILDCARD: usize = 65536;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ParsedApdu<'a> {
    pub(crate) cla: u8,
    pub(crate) ins: u8,
    pub(crate) p1: u8,
    pub(crate) p2: u8,
    pub(crate) data: &'a [u8],
    pub(crate) le: Option<usize>,
}

pub(crate) fn parse_apdu(apdu: &[u8]) -> Result<ParsedApdu<'_>, [u8; 2]> {
    let Some((header, rest)) = apdu.split_first_chunk::<APDU_HEADER_LEN>() else {
        return Err(SW_WRONG_LENGTH);
    };
    let &[cla, ins, p1, p2] = header;

    if rest.is_empty() {
        return Ok(ParsedApdu { cla, ins, p1, p2, data: &[], le: None });
    }

    let Some((&first_byte, tail)) = rest.split_first() else {
        return Err(SW_WRONG_LENGTH);
    };

    if tail.is_empty() {
        let le = if first_byte == 0 { APDU_SHORT_LE_WILDCARD } else { usize::from(first_byte) };
        return Ok(ParsedApdu { cla, ins, p1, p2, data: &[], le: Some(le) });
    }

    if first_byte == APDU_EXTENDED_LENGTH_MARKER {
        if let &[hi, lo] = tail {
            let raw = usize::from(u16::from_be_bytes([hi, lo]));
            let le = if raw == 0 { APDU_EXTENDED_LE_WILDCARD } else { raw };
            return Ok(ParsedApdu { cla, ins, p1, p2, data: &[], le: Some(le) });
        }
        let Some((lc_bytes, data_and_le)) = tail.split_first_chunk::<APDU_EXTENDED_LC_LEN>() else {
            return Err(SW_WRONG_LENGTH);
        };
        let lc = usize::from(u16::from_be_bytes(*lc_bytes));
        if lc == 0 {
            return Err(SW_WRONG_LENGTH);
        }
        let Some((data, le_slice)) = data_and_le.split_at_checked(lc) else {
            return Err(SW_WRONG_LENGTH);
        };
        if le_slice.is_empty() {
            return Ok(ParsedApdu { cla, ins, p1, p2, data, le: None });
        }
        if let &[hi, lo] = le_slice {
            let raw = usize::from(u16::from_be_bytes([hi, lo]));
            let le = if raw == 0 { APDU_EXTENDED_LE_WILDCARD } else { raw };
            return Ok(ParsedApdu { cla, ins, p1, p2, data, le: Some(le) });
        }
        Err(SW_WRONG_LENGTH)
    } else {
        let lc = usize::from(first_byte);
        let Some((data, le_slice)) = tail.split_at_checked(lc) else {
            return Err(SW_WRONG_LENGTH);
        };
        if le_slice.is_empty() {
            return Ok(ParsedApdu { cla, ins, p1, p2, data, le: None });
        }
        if let &[le_byte] = le_slice {
            let le = if le_byte == 0 { APDU_SHORT_LE_WILDCARD } else { usize::from(le_byte) };
            return Ok(ParsedApdu { cla, ins, p1, p2, data, le: Some(le) });
        }
        Err(SW_WRONG_LENGTH)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_apdu_case1_has_no_data_and_no_le() {
        let raw = [0x00, 0xA4, 0x00, 0x00];
        let apdu = parse_apdu(&raw).expect("valid case 1");
        assert_eq!(apdu.cla, 0x00);
        assert_eq!(apdu.ins, 0xA4);
        assert_eq!(apdu.p1, 0x00);
        assert_eq!(apdu.p2, 0x00);
        assert!(apdu.data.is_empty());
        assert_eq!(apdu.le, None);
    }

    #[test]
    fn parse_apdu_case2s_parses_short_expected_length() {
        let raw_le15 = [0x00, 0xB0, 0x00, 0x00, 0x0F];
        let apdu_le15 = parse_apdu(&raw_le15).expect("valid case 2s");
        assert_eq!(apdu_le15.le, Some(15));
        assert!(apdu_le15.data.is_empty());

        let raw_le0 = [0x00, 0xB0, 0x00, 0x00, 0x00];
        let apdu_le0 = parse_apdu(&raw_le0).expect("valid case 2s le 0");
        assert_eq!(apdu_le0.le, Some(256));
        assert!(apdu_le0.data.is_empty());
    }

    #[test]
    fn parse_apdu_case2e_parses_extended_expected_length() {
        let raw_ext = [0x00, 0xB0, 0x00, 0x00, 0x00, 0x01, 0x00];
        let apdu_ext = parse_apdu(&raw_ext).expect("valid case 2e");
        assert_eq!(apdu_ext.le, Some(256));
        assert!(apdu_ext.data.is_empty());

        let raw_ext0 = [0x00, 0xB0, 0x00, 0x00, 0x00, 0x00, 0x00];
        let apdu_ext0 = parse_apdu(&raw_ext0).expect("valid case 2e le 0");
        assert_eq!(apdu_ext0.le, Some(65536));
        assert!(apdu_ext0.data.is_empty());
    }

    #[test]
    fn parse_apdu_case3s_and_case3e_parse_payload_without_le() {
        let raw_3s = [0x00, 0xA4, 0x00, 0x00, 0x02, 0xE1, 0x04];
        let apdu_3s = parse_apdu(&raw_3s).expect("valid case 3s");
        assert_eq!(apdu_3s.data, &[0xE1, 0x04]);
        assert_eq!(apdu_3s.le, None);

        let raw_3e = [0x00, 0xD6, 0x00, 0x00, 0x00, 0x00, 0x05, 0x11, 0x22, 0x33, 0x44, 0x55];
        let apdu_3e = parse_apdu(&raw_3e).expect("valid case 3e");
        assert_eq!(apdu_3e.data, &[0x11, 0x22, 0x33, 0x44, 0x55]);
        assert_eq!(apdu_3e.le, None);
    }

    #[test]
    fn parse_apdu_case4s_and_case4e_parse_both_data_and_le() {
        let raw_4s = [0x00, 0xA4, 0x04, 0x00, 0x02, 0x11, 0x22, 0x00];
        let apdu_4s = parse_apdu(&raw_4s).expect("valid case 4s");
        assert_eq!(apdu_4s.data, &[0x11, 0x22]);
        assert_eq!(apdu_4s.le, Some(256));

        let raw_4e = [0x00, 0xA4, 0x04, 0x00, 0x00, 0x00, 0x02, 0xAA, 0xBB, 0x01, 0x00];
        let apdu_4e = parse_apdu(&raw_4e).expect("valid case 4e");
        assert_eq!(apdu_4e.data, &[0xAA, 0xBB]);
        assert_eq!(apdu_4e.le, Some(256));
    }

    #[test]
    fn parse_apdu_rejects_malformed_framing_with_wrong_length() {
        assert_eq!(parse_apdu(&[0x00, 0xA4, 0x00]), Err(SW_WRONG_LENGTH));
        assert_eq!(parse_apdu(&[0x00, 0xA4, 0x00, 0x00, 0x05, 0x01]), Err(SW_WRONG_LENGTH));
        assert_eq!(
            parse_apdu(&[0x00, 0xA4, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01]),
            Err(SW_WRONG_LENGTH)
        );
    }

    #[test]
    fn rejects_apdu_with_trailing_bytes_after_le() {
        let raw_4s_trailing =
            [0x00, 0xA4, 0x04, 0x00, 0x07, 0xD2, 0x76, 0x00, 0x00, 0x85, 0x01, 0x01, 0x00, 0xFF];
        assert_eq!(parse_apdu(&raw_4s_trailing), Err(SW_WRONG_LENGTH));

        let raw_4e_trailing =
            [0x00, 0xA4, 0x04, 0x00, 0x00, 0x00, 0x02, 0xAA, 0xBB, 0x01, 0x00, 0xFF];
        assert_eq!(parse_apdu(&raw_4e_trailing), Err(SW_WRONG_LENGTH));
    }

    #[test]
    fn rejects_extended_lc_larger_than_remaining_data() {
        let raw = [0x00, 0xD6, 0x00, 0x00, 0x00, 0x00, 0x05, 0xAA, 0xBB];
        assert_eq!(parse_apdu(&raw), Err(SW_WRONG_LENGTH));
    }
}
