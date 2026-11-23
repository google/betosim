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

use bytes::{Buf, Bytes, BytesMut};
use tokio_util::codec::Decoder;

pub struct ModemCodec;

impl Decoder for ModemCodec {
    type Item = Bytes;
    type Error = std::io::Error;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        loop {
            if src.is_empty() {
                return Ok(None);
            }

            // AT commands are terminated by \r or \n.
            // Interactive prompts (like SMS input) terminate on Ctrl+Z (\x1a) or ESC
            // (\x1b).
            if let Some((pos, &delimiter)) = src
                .iter()
                .enumerate()
                .find(|&(_, &b)| b == b'\n' || b == b'\r' || b == b'\x1a' || b == b'\x1b')
            {
                if delimiter == b'\n' || delimiter == b'\r' {
                    let data = src.split_to(pos);
                    src.advance(1); // Consume the delimiter
                    if delimiter == b'\r' && matches!(src.as_ref(), &[b'\n', ..]) {
                        src.advance(1); // Consume trailing \n
                    }
                    if data.is_empty() {
                        continue;
                    }
                    return Ok(Some(data.freeze()));
                }
                // Include \x1a or \x1b in the yielded frame for parser evaluation.
                let data = src.split_to(pos + 1);
                return Ok(Some(data.freeze()));
            }
            if src.len() >= 4096 {
                let data = src.split_to(4096);
                return Ok(Some(data.freeze()));
            }
            return Ok(None);
        }
    }
}

#[cfg(test)]
mod tests {
    use bytes::BufMut;

    use super::*;

    const MAX_UNTERMINATED_FRAME_LEN: usize = 4096;

    #[test]
    fn test_modem_decode_line_delimiters_and_blank_lines() {
        let mut codec = ModemCodec;
        let mut buf = BytesMut::from(&b"\r\n\r\nAT\r\nOK\n\nREADY\r"[..]);

        assert_eq!(codec.decode(&mut buf).unwrap().unwrap().as_ref(), b"AT");
        assert_eq!(codec.decode(&mut buf).unwrap().unwrap().as_ref(), b"OK");
        assert_eq!(codec.decode(&mut buf).unwrap().unwrap().as_ref(), b"READY");
        assert!(codec.decode(&mut buf).unwrap().is_none());
        assert!(buf.is_empty());
    }

    #[test]
    fn test_modem_decode_partial_line_waits_for_delimiter() {
        let mut codec = ModemCodec;
        let mut buf = BytesMut::from(&b"AT+CGMI"[..]);

        assert!(codec.decode(&mut buf).unwrap().is_none());
        assert_eq!(buf.len(), b"AT+CGMI".len());

        buf.put_slice(b"\r");
        let frame = codec.decode(&mut buf).unwrap().unwrap();
        assert_eq!(frame.as_ref(), b"AT+CGMI");
        assert!(buf.is_empty());
    }

    #[test]
    fn test_modem_decode_ctrl_z_and_esc_preserve_terminator() {
        let mut codec = ModemCodec;
        let mut buf = BytesMut::from(&b"PDU\x1aABORT\x1b"[..]);

        let pdu = codec.decode(&mut buf).unwrap().unwrap();
        assert_eq!(pdu.as_ref(), b"PDU\x1a");

        let abort = codec.decode(&mut buf).unwrap().unwrap();
        assert_eq!(abort.as_ref(), b"ABORT\x1b");
        assert!(codec.decode(&mut buf).unwrap().is_none());
    }

    #[test]
    fn test_modem_decode_overflow_flushes_4096_bytes() {
        let mut codec = ModemCodec;
        let mut buf = BytesMut::from(&vec![b'A'; MAX_UNTERMINATED_FRAME_LEN + 3][..]);

        let chunk = codec.decode(&mut buf).unwrap().unwrap();
        assert_eq!(chunk.len(), MAX_UNTERMINATED_FRAME_LEN);
        assert_eq!(buf.as_ref(), b"AAA");
        assert!(codec.decode(&mut buf).unwrap().is_none());

        buf.put_slice(b"\r");
        let tail = codec.decode(&mut buf).unwrap().unwrap();
        assert_eq!(tail.as_ref(), b"AAA");
        assert!(buf.is_empty());
    }
}
