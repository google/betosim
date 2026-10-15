// Copyright 2025 The Android Open Source Project
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

//! Netlink message stream iterator.

use zerocopy::Ref;

use crate::netlink::frame::NlMsgHdr;

/// Alignment for Netlink messages.
const NLMSG_ALIGNTO: usize = 4;

/// Aligns a length to the Netlink message alignment boundary.
fn nlmsg_align(len: usize) -> usize {
    (len + NLMSG_ALIGNTO - 1) & !(NLMSG_ALIGNTO - 1)
}

/// Iterator over Netlink messages in a byte stream.
pub struct NetlinkStream<'a> {
    buffer: &'a [u8],
}

impl<'a> NetlinkStream<'a> {
    /// Creates a new `NetlinkStream` from a byte buffer.
    pub fn new(buffer: &'a [u8]) -> Self {
        Self { buffer }
    }
}

impl<'a> Iterator for NetlinkStream<'a> {
    type Item = Result<(NlMsgHdr, &'a [u8]), String>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.buffer.is_empty() {
            return None;
        }

        if self.buffer.len() < std::mem::size_of::<NlMsgHdr>() {
            return Some(Err("Buffer too short for NlMsgHdr".to_string()));
        }

        let (hdr, _) = match Ref::<&[u8], NlMsgHdr>::from_prefix(self.buffer) {
            Ok(res) => res,
            Err(_) => return Some(Err("Failed to read NlMsgHdr".to_string())),
        };

        let msg_len = hdr.nlmsg_len as usize;

        if msg_len < std::mem::size_of::<NlMsgHdr>() {
            return Some(Err(format!("Invalid nlmsg_len: {msg_len}")));
        }

        let Some(payload) = self.buffer.get(std::mem::size_of::<NlMsgHdr>()..msg_len) else {
            return Some(Err(format!(
                "Incomplete message: expected {msg_len} bytes, have {}",
                self.buffer.len()
            )));
        };

        // Advance buffer
        let aligned_len = nlmsg_align(msg_len);
        self.buffer = self.buffer.get(aligned_len..).unwrap_or(&[]);

        Some(Ok((*hdr, payload)))
    }
}

#[cfg(test)]
mod tests {
    use zerocopy::IntoBytes;

    use super::*;

    #[test]
    fn test_netlink_stream_single() {
        let mut buffer = Vec::new();
        let hdr = NlMsgHdr {
            nlmsg_len: 20, // 16 header + 4 payload
            nlmsg_type: 1,
            nlmsg_flags: 0,
            nlmsg_seq: 100,
            nlmsg_pid: 200,
        };
        buffer.extend_from_slice(hdr.as_bytes());
        buffer.extend_from_slice(&[0xAA, 0xBB, 0xCC, 0xDD]); // 4 bytes payload

        let mut stream = NetlinkStream::new(&buffer);
        let (parsed_hdr, payload) = stream.next().unwrap().unwrap();
        let len = parsed_hdr.nlmsg_len;
        assert_eq!(len, 20);
        assert_eq!(payload, &[0xAA, 0xBB, 0xCC, 0xDD]);
        assert!(stream.next().is_none());
    }

    #[test]
    fn test_netlink_stream_multiple_aligned() {
        let mut buffer = Vec::new();

        // Msg 1: 20 bytes (aligned)
        let hdr1 =
            NlMsgHdr { nlmsg_len: 20, nlmsg_type: 1, nlmsg_flags: 0, nlmsg_seq: 1, nlmsg_pid: 0 };
        buffer.extend_from_slice(hdr1.as_bytes());
        buffer.extend_from_slice(&[1, 2, 3, 4]);

        // Msg 2: 20 bytes (aligned)
        let hdr2 =
            NlMsgHdr { nlmsg_len: 20, nlmsg_type: 2, nlmsg_flags: 0, nlmsg_seq: 2, nlmsg_pid: 0 };
        buffer.extend_from_slice(hdr2.as_bytes());
        buffer.extend_from_slice(&[5, 6, 7, 8]);

        let mut stream = NetlinkStream::new(&buffer);

        let (h1, p1) = stream.next().unwrap().unwrap();
        let seq1 = h1.nlmsg_seq;
        assert_eq!(seq1, 1);
        assert_eq!(p1, &[1, 2, 3, 4]);

        let (h2, p2) = stream.next().unwrap().unwrap();
        let seq2 = h2.nlmsg_seq;
        assert_eq!(seq2, 2);
        assert_eq!(p2, &[5, 6, 7, 8]);

        assert!(stream.next().is_none());
    }

    #[test]
    fn test_netlink_stream_padding() {
        let mut buffer = Vec::new();

        // Msg 1: 17 bytes (needs 3 bytes padding)
        // 16 header + 1 payload
        let hdr1 =
            NlMsgHdr { nlmsg_len: 17, nlmsg_type: 1, nlmsg_flags: 0, nlmsg_seq: 1, nlmsg_pid: 0 };
        buffer.extend_from_slice(hdr1.as_bytes());
        buffer.push(0xAA);
        // Padding
        buffer.extend_from_slice(&[0, 0, 0]);

        // Msg 2: 16 bytes (header only, aligned)
        let hdr2 =
            NlMsgHdr { nlmsg_len: 16, nlmsg_type: 2, nlmsg_flags: 0, nlmsg_seq: 2, nlmsg_pid: 0 };
        buffer.extend_from_slice(hdr2.as_bytes());

        let mut stream = NetlinkStream::new(&buffer);

        let (h1, p1) = stream.next().unwrap().unwrap();
        let len1 = h1.nlmsg_len;
        assert_eq!(len1, 17);
        assert_eq!(p1, &[0xAA]);

        let (h2, p2) = stream.next().unwrap().unwrap();
        let len2 = h2.nlmsg_len;
        assert_eq!(len2, 16);
        assert_eq!(p2, &[] as &[u8]);

        assert!(stream.next().is_none());
    }
}
