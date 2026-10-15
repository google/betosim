// Copyright 2024 The Android Open Source Project
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

//! LLC

#![allow(clippy::all)]
#![allow(missing_docs)]
#![allow(unused)]

include!(concat!(env!("OUT_DIR"), "/llc_packets.rs"));

impl LlcSnapHeader {
    // Length of LLC/SNAP headers on data frames
    pub const LEN: usize = 8;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_llc_snap_header_len() {
        let payload = vec![
            LlcSap::Snap as u8,
            LlcSap::Snap as u8,
            LlcCtrl::UiCmd as u8,
            // OUI
            0x00,
            0x00,
            0x00,
            // EtherType
            0x08,
            0x00,
        ];

        let hdr = LlcSnapHeader::decode_full(&payload).unwrap();
        assert_eq!(hdr.encoded_len(), LlcSnapHeader::LEN);
    }

    #[test]
    fn test_llc_snap_header_valid() {
        let payload = vec![
            LlcSap::Snap as u8,
            LlcSap::Snap as u8,
            LlcCtrl::UiCmd as u8,
            // OUI
            0x00,
            0x00,
            0x00,
            // EtherType
            0x08,
            0x00,
        ];
        let hdr = LlcSnapHeader::decode_full(&payload).unwrap();

        assert_eq!(hdr.dsap, LlcSap::Snap);
        assert_eq!(hdr.ssap, LlcSap::Snap);
        assert_eq!(hdr.ctrl, LlcCtrl::UiCmd);
        assert_eq!(hdr.ethertype, EtherType::IPv4);
    }

    #[test]
    fn test_llc_snap_header_invalid_llc() {
        #[rustfmt::skip]
        let payload = vec![
            // LLC
            0 as u8, 0 as u8, 0 as u8,
            // OUI
            0x00, 0x00, 0x00,
            // EtherType
            0x00, 0x00,
        ];
        let hdr_result = LlcSnapHeader::decode_full(&payload);
        assert!(hdr_result.is_err());
    }

    #[test]
    fn test_llc_snap_header_invalid_ethertype() {
        let payload = vec![
            LlcSap::Snap as u8,
            LlcSap::Snap as u8,
            LlcCtrl::UiCmd as u8,
            // OUI
            0x00,
            0x00,
            0x00,
            // EtherType
            0x00,
            0x00,
        ];
        let hdr_result = LlcSnapHeader::decode_full(&payload);
        assert!(hdr_result.is_err());
    }
}
