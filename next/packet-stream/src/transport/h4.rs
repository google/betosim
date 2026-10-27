// Copyright 2023-2026 The Android Open Source Project
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
use tracing::warn;

/* H4 message type */
const H4_CMD_TYPE: u8 = 1;
const H4_ACL_TYPE: u8 = 2;
const H4_SCO_TYPE: u8 = 3;
const H4_EVT_TYPE: u8 = 4;
const H4_ISO_TYPE: u8 = 5;

/* HCI message preamble size */
const HCI_CMD_PREAMBLE_SIZE: usize = 3;
const HCI_ACL_PREAMBLE_SIZE: usize = 4;
const HCI_SCO_PREAMBLE_SIZE: usize = 3;
const HCI_EVT_PREAMBLE_SIZE: usize = 2;
const HCI_ISO_PREAMBLE_SIZE: usize = 4;

pub struct H4Codec;

impl Decoder for H4Codec {
    type Item = Bytes;
    type Error = std::io::Error;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        // 1. Peek at the Indicator byte
        let Some(&h4_type) = src.first() else {
            return Ok(None);
        };

        // 2. Check for valid H4 type
        let preamble_size = match h4_type {
            H4_CMD_TYPE => HCI_CMD_PREAMBLE_SIZE,
            H4_ACL_TYPE => HCI_ACL_PREAMBLE_SIZE,
            H4_SCO_TYPE => HCI_SCO_PREAMBLE_SIZE,
            H4_EVT_TYPE => HCI_EVT_PREAMBLE_SIZE,
            H4_ISO_TYPE => HCI_ISO_PREAMBLE_SIZE,
            _ => {
                let skip = self.recover(src);
                if skip > 0 {
                    return Ok(None);
                }
                0 // This branch is practically unreachable if recover returns
                // usize::MAX - 1
            }
        };

        if src.len() < 1 + preamble_size {
            return Ok(None); // Wait for more header data
        }

        // 3. Calculate payload length
        // Note: src has NOT been advanced, so indices are 0-based from start of packet
        let payload_length = match (h4_type, src.as_ref()) {
            (H4_CMD_TYPE, &[_, _, _, len, ..]) => len as usize,
            (H4_ACL_TYPE, &[_, _, _, l0, l1, ..]) => u16::from_le_bytes([l0, l1]) as usize,
            (H4_SCO_TYPE, &[_, _, _, len, ..]) => len as usize,
            (H4_EVT_TYPE, &[_, _, len, ..]) => len as usize,
            (H4_ISO_TYPE, &[_, _, _, l0, l1, ..]) => {
                (usize::from(l1 & 0x0f) << 8) | usize::from(l0)
            }
            _ => {
                warn!("Unexpected H4 type: {h4_type:#x}");
                return Ok(None);
            }
        };

        let total_len = 1 + preamble_size + payload_length;

        if src.len() < total_len {
            // Reserve space to avoid reallocations
            src.reserve(total_len - src.len());
            return Ok(None);
        }

        // 4. Extract the full packet including the H4 indicator byte
        let data = src.split_to(total_len);
        let payload = data.freeze(); // Zero-copy conversion to Bytes

        Ok(Some(payload))
    }
}

impl H4Codec {
    /// Skip all received bytes until the HCI Reset command is received.
    ///
    /// Cuttlefish sometimes sends invalid bytes in the virtio-console to
    /// rootcanal/netsim when the emulator is restarted in the middle of
    /// HCI exchanges. This function recovers from this situation.
    fn recover(&mut self, src: &mut BytesMut) -> usize {
        const RESET_COMMAND: [u8; 4] = [0x01, 0x03, 0x0c, 0x00];

        let h4_type = src.first().copied().unwrap_or(0);
        warn!("Invalid H4 type {h4_type}, entering recovery");

        if let Some(pos) =
            src.windows(RESET_COMMAND.len()).position(|window| window == RESET_COMMAND)
        {
            warn!("Recovered at index {}", pos);
            src.advance(pos);
            // Return a large value to force `decode` to return `Ok(None)`.
            // This restarts decoding with the now valid `src` buffer (starting with
            // RESET_COMMAND).
            usize::MAX - 1
        } else {
            // No full header found. Keep partial match at the end if any.
            let keep = RESET_COMMAND.len() - 1;
            if src.len() > keep {
                src.advance(src.len() - keep);
            }
            // Request more data.
            usize::MAX - 1
        }
    }
}
#[cfg(test)]
mod tests {
    use bytes::BufMut;

    use super::*;

    #[test]
    fn test_decode_command() {
        let mut codec = H4Codec;
        let mut buf = BytesMut::new();
        // Command: Type(1), Opcode(0x0102), Len(1), Payload(0x03)
        buf.put_u8(H4_CMD_TYPE);
        buf.put_u8(0x02);
        buf.put_u8(0x01);
        buf.put_u8(0x01);
        buf.put_u8(0x03);

        let packet = codec.decode(&mut buf).unwrap().unwrap();
        assert_eq!(packet.len(), 5);
        assert_eq!(packet.as_ref(), &[H4_CMD_TYPE, 0x02, 0x01, 0x01, 0x03]);
    }

    #[test]
    fn test_decode_partial() {
        let mut codec = H4Codec;
        let mut buf = BytesMut::new();
        buf.put_u8(H4_CMD_TYPE);
        buf.put_u8(0x02);

        let res = codec.decode(&mut buf).unwrap();
        assert!(res.is_none());
        assert_eq!(buf.len(), 2);
    }

    #[test]
    fn test_recovery() {
        let mut codec = H4Codec;
        let mut buf = BytesMut::new();
        // Invalid type followed by garbage then reset pattern
        buf.put_u8(0xFF);
        buf.put_slice(&[0xaa, 0xbb]);
        buf.put_slice(&[0x01, 0x03, 0x0c, 0x00]); // Reset command

        // First decode should trigger recovery and return None, consuming up to reset
        // pattern
        let res = codec.decode(&mut buf).unwrap();
        assert!(res.is_none());

        // Buffer should now start with Reset command
        assert_eq!(buf[0], 0x01);
        assert_eq!(buf.len(), 4);

        // Next decode should return the reset command
        let packet = codec.decode(&mut buf).unwrap().unwrap();
        assert_eq!(packet.as_ref(), &[H4_CMD_TYPE, 0x03, 0x0c, 0x00]);
    }
}
