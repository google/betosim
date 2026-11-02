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
                } else {
                    // Include \x1a or \x1b in the yielded frame for parser evaluation.
                    let data = src.split_to(pos + 1);
                    return Ok(Some(data.freeze()));
                }
            } else {
                if src.len() >= 4096 {
                    let data = src.split_to(4096);
                    return Ok(Some(data.freeze()));
                }
                return Ok(None);
            }
        }
    }
}
