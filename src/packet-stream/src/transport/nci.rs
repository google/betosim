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

use bytes::{Bytes, BytesMut};
use tokio_util::codec::Decoder;

pub struct NciCodec;

impl Decoder for NciCodec {
    type Item = Bytes;
    type Error = std::io::Error;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        const NCI_HEADER_SIZE: usize = 3;

        // NCI packets always have the payload length at index 2.
        let &[_, _, len, ..] = src.as_ref() else {
            return Ok(None);
        };
        let payload_len = len as usize;
        // Max payload length is 255, so max total_len is 258.
        let total_len = NCI_HEADER_SIZE + payload_len;

        if src.len() < total_len {
            src.reserve(total_len - src.len());
            return Ok(None);
        }

        let data = src.split_to(total_len);
        Ok(Some(data.freeze()))
    }
}

#[cfg(test)]
mod tests {
    use bytes::BufMut;

    use super::*;

    const NCI_HEADER_SIZE: usize = 3;
    const NCI_CMD_HDR: [u8; 2] = [0x20, 0x01];
    const NCI_RSP_HDR: [u8; 2] = [0x40, 0x01];

    #[test]
    fn test_nci_decode_empty_buffer_returns_none() {
        let mut codec = NciCodec;
        let mut empty = BytesMut::new();
        assert!(codec.decode(&mut empty).unwrap().is_none());
    }

    #[test]
    fn test_nci_decode_partial_header_returns_none() {
        let mut codec = NciCodec;
        let mut partial_hdr = BytesMut::from(&NCI_CMD_HDR[..]);
        assert!(codec.decode(&mut partial_hdr).unwrap().is_none());
        assert_eq!(partial_hdr.len(), NCI_CMD_HDR.len());
    }

    #[test]
    fn test_nci_decode_partial_payload_waits_for_full_packet() {
        let mut codec = NciCodec;
        let mut buf = BytesMut::from(&[NCI_CMD_HDR[0], NCI_CMD_HDR[1], 0x04, 0xaa, 0xbb][..]);

        assert!(codec.decode(&mut buf).unwrap().is_none());
        assert_eq!(buf.len(), NCI_HEADER_SIZE + 2);

        buf.put_slice(&[0xcc, 0xdd]);
        let packet = codec.decode(&mut buf).unwrap().unwrap();
        assert_eq!(
            packet.as_ref(),
            &[NCI_CMD_HDR[0], NCI_CMD_HDR[1], 0x04, 0xaa, 0xbb, 0xcc, 0xdd]
        );
        assert!(buf.is_empty());
    }

    #[test]
    fn test_nci_decode_zero_length_payload() {
        let mut codec = NciCodec;
        let mut zero_len = BytesMut::from(&[NCI_CMD_HDR[0], NCI_CMD_HDR[1], 0x00][..]);
        let zero_pkt = codec.decode(&mut zero_len).unwrap().unwrap();
        assert_eq!(zero_pkt.as_ref(), &[NCI_CMD_HDR[0], NCI_CMD_HDR[1], 0x00]);
        assert!(zero_len.is_empty());
    }

    #[test]
    fn test_nci_decode_max_255_byte_payload() {
        let mut codec = NciCodec;
        let mut max_len = BytesMut::from(&[NCI_CMD_HDR[0], NCI_CMD_HDR[1], u8::MAX][..]);
        let payload = vec![0x5a; usize::from(u8::MAX)];
        max_len.put_slice(&payload);
        let max_pkt = codec.decode(&mut max_len).unwrap().unwrap();
        assert_eq!(max_pkt.len(), NCI_HEADER_SIZE + usize::from(u8::MAX));
        assert_eq!(&max_pkt[NCI_HEADER_SIZE..], payload.as_slice());
        assert!(max_len.is_empty());
    }

    #[test]
    fn test_nci_decode_back_to_back_packets() {
        let mut codec = NciCodec;
        let mut concat = BytesMut::new();
        concat.put_slice(&[NCI_CMD_HDR[0], NCI_CMD_HDR[1], 0x01, 0x11]);
        concat.put_slice(&[NCI_RSP_HDR[0], NCI_RSP_HDR[1], 0x02, 0x22, 0x33]);

        let first = codec.decode(&mut concat).unwrap().unwrap();
        assert_eq!(first.as_ref(), &[NCI_CMD_HDR[0], NCI_CMD_HDR[1], 0x01, 0x11]);

        let second = codec.decode(&mut concat).unwrap().unwrap();
        assert_eq!(second.as_ref(), &[NCI_RSP_HDR[0], NCI_RSP_HDR[1], 0x02, 0x22, 0x33]);
        assert!(codec.decode(&mut concat).unwrap().is_none());
    }
}
