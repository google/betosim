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
