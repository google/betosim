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

use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout, Unaligned};

/// Netlink Message Header
#[derive(
    FromBytes, IntoBytes, Unaligned, Immutable, KnownLayout, Debug, Copy, Clone, PartialEq, Eq,
)]
#[repr(C, packed)]
pub struct NlMsgHdr {
    /// Length of message including header
    pub nlmsg_len: u32,
    /// Message type identifier
    pub nlmsg_type: u16,
    /// Flags (NLM_F_)
    pub nlmsg_flags: u16,
    /// Sequence number
    pub nlmsg_seq: u32,
    /// Sending process port ID
    pub nlmsg_pid: u32,
}

impl NlMsgHdr {
    pub const SIZE: usize = std::mem::size_of::<Self>();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_nl_msg_hdr_layout() {
        assert_eq!(std::mem::size_of::<NlMsgHdr>(), 16);
    }
}
