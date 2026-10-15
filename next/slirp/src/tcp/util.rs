// Copyright 2023-2025 The Android Open Source Project
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

use netsim_packets::TcpHeader;

pub(super) fn parse_tcp_options(
    tcp_header: &TcpHeader,
    packet: &[u8],
) -> (Option<u16>, Option<u8>, Vec<(u32, u32)>) {
    let mut mss = None;
    let mut window_scale = None;
    let mut sack_blocks = Vec::new();
    let data_offset = tcp_header.header_length();
    let tcp_header_len = std::mem::size_of::<TcpHeader>();
    if data_offset <= tcp_header_len {
        return (mss, window_scale, sack_blocks);
    }

    let Some(mut options) = packet.get(tcp_header_len..data_offset) else {
        return (mss, window_scale, sack_blocks);
    };
    while !options.is_empty() {
        match *options {
            // End of options
            [0, ..] => break,
            // NOP
            [1, ref rest @ ..] => options = rest,
            // MSS
            [2, 4, m0, m1, ref rest @ ..] => {
                mss = Some(u16::from_be_bytes([m0, m1]));
                options = rest;
            }
            [2, ..] => break,
            // Window Scale
            [3, 3, shift, ref rest @ ..] => {
                window_scale = Some(shift);
                options = rest;
            }
            [3, ..] => break,
            // SACK
            [5, len, ref rest @ ..] => {
                let len = len as usize;
                if len < 2 || !(len - 2).is_multiple_of(8) {
                    break;
                }
                let data_len = len - 2;
                let Some((data, next_options)) = rest.split_at_checked(data_len) else {
                    break;
                };
                let (chunks, _) = data.as_chunks::<8>();
                for chunk in chunks {
                    let &[l0, l1, l2, l3, r0, r1, r2, r3] = chunk;
                    sack_blocks.push((
                        u32::from_be_bytes([l0, l1, l2, l3]),
                        u32::from_be_bytes([r0, r1, r2, r3]),
                    ));
                }
                options = next_options;
            }
            [_, len, ref rest @ ..] => {
                let len = len as usize;
                if len < 2 {
                    break;
                }
                let Some(next_options) = rest.get(len - 2..) else {
                    break;
                };
                options = next_options;
            }
            _ => break,
        }
    }
    (mss, window_scale, sack_blocks)
}
