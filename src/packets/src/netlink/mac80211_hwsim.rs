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

use num_derive::{FromPrimitive, ToPrimitive};
use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout, Ref, Unaligned};

use crate::netlink::{NlAttrHdr, NlMsgHdr};

#[derive(FromPrimitive, ToPrimitive, PartialEq, Eq, Debug, Copy, Clone)]
#[repr(u8)]
pub enum HwsimCmd {
    Unspec = 0,
    Register = 1,
    Frame = 2,
    TxInfoFrame = 3,
    NewRadio = 4,
    DelRadio = 5,
    GetRadio = 6,
    AddMacAddr = 7,
    DelMacAddr = 8,
    StartPmsr = 9,
    AbortPmsr = 10,
    ReportPmsr = 11,
}

#[derive(FromPrimitive, ToPrimitive, PartialEq, Eq, Debug, Copy, Clone)]
#[repr(u16)]
pub enum HwsimAttrEnum {
    Unspec = 0,
    AddrReceiver = 1,
    AddrTransmitter = 2,
    Frame = 3,
    Flags = 4,
    RxRate = 5,
    Signal = 6,
    TxInfo = 7,
    Cookie = 8,
    Channels = 9,
    RadioId = 10,
    RegHintAlpha2 = 11,
    RegCustomReg = 12,
    RegStrictReg = 13,
    SupportP2PDevice = 14,
    UseChanctx = 15,
    DestroyRadioOnClose = 16,
    RadioName = 17,
    NoVif = 18,
    Freq = 19,
    Pad = 20,
    TxInfoFlags = 21,
    PermAddr = 22,
    IftypeSupport = 23,
    CipherSupport = 24,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[repr(C, packed)]
pub struct HwsimMsgHdr {
    pub hwsim_cmd: HwsimCmd,
    pub hwsim_version: u8,
    pub reserved: u16,
}

impl HwsimMsgHdr {
    pub const SIZE: usize = std::mem::size_of::<Self>();

    pub fn encoded_len(&self) -> u32 {
        4 // 1 + 1 + 2
    }

    pub fn parse(bytes: &[u8]) -> Option<Self> {
        let &[cmd, hwsim_version, ref res @ ..] = bytes.first_chunk::<4>()?;
        let hwsim_cmd = num_traits::FromPrimitive::from_u8(cmd)?;
        let reserved = u16::from_le_bytes(*res);
        Some(Self { hwsim_cmd, hwsim_version, reserved })
    }

    pub fn as_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(4);
        bytes.push(num_traits::ToPrimitive::to_u8(&self.hwsim_cmd).unwrap());
        bytes.push(self.hwsim_version);
        bytes.extend_from_slice(&self.reserved.to_le_bytes());
        bytes
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HwsimMsg {
    pub nl_hdr: NlMsgHdr,
    pub hwsim_hdr: HwsimMsgHdr,
    pub attributes: Vec<u8>,
}

impl HwsimMsg {
    pub fn decode_full(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() < 20 {
            return Err("Packet too short for HwsimMsg".into());
        }
        let (nl_hdr, rest) = Ref::<&[u8], NlMsgHdr>::from_prefix(bytes)
            .map_err(|err| format!("Failed to read NlMsgHdr: {}", err))?;
        let hwsim_hdr = HwsimMsgHdr::parse(rest).ok_or("Failed to read HwsimMsgHdr")?;
        let attributes = rest.get(4..).ok_or("Failed to read attributes")?.to_vec();
        Ok(Self { nl_hdr: *nl_hdr, hwsim_hdr, attributes })
    }

    pub fn encode_to_vec(&self) -> Result<Vec<u8>, String> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(self.nl_hdr.as_bytes());
        bytes.extend_from_slice(&self.hwsim_hdr.as_bytes());
        bytes.extend_from_slice(&self.attributes);
        Ok(bytes)
    }
}

// Attribute structs

#[derive(
    FromBytes, IntoBytes, Unaligned, Immutable, KnownLayout, Debug, Copy, Clone, PartialEq, Eq,
)]
#[repr(C, packed)]
pub struct HwsimAttrAddrReceiver {
    pub header: NlAttrHdr,
    pub address: [u8; 6],
}

#[derive(
    FromBytes, IntoBytes, Unaligned, Immutable, KnownLayout, Debug, Copy, Clone, PartialEq, Eq,
)]
#[repr(C, packed)]
pub struct HwsimAttrAddrTransmitter {
    pub header: NlAttrHdr,
    pub address: [u8; 6],
}

#[derive(
    FromBytes, IntoBytes, Unaligned, Immutable, KnownLayout, Debug, Copy, Clone, PartialEq, Eq,
)]
#[repr(C, packed)]
pub struct HwsimAttrFlags {
    pub header: NlAttrHdr,
    pub flags: u32,
}

#[derive(
    FromBytes, IntoBytes, Unaligned, Immutable, KnownLayout, Debug, Copy, Clone, PartialEq, Eq,
)]
#[repr(C, packed)]
pub struct HwsimAttrRxRate {
    pub header: NlAttrHdr,
    pub rx_rate_idx: u32,
}

#[derive(
    FromBytes, IntoBytes, Unaligned, Immutable, KnownLayout, Debug, Copy, Clone, PartialEq, Eq,
)]
#[repr(C, packed)]
pub struct HwsimAttrSignal {
    pub header: NlAttrHdr,
    pub signal: u32,
}

#[derive(
    FromBytes, IntoBytes, Unaligned, Immutable, KnownLayout, Debug, Copy, Clone, PartialEq, Eq,
)]
#[repr(C, packed)]
pub struct HwsimAttrFreq {
    pub header: NlAttrHdr,
    pub freq: u32,
}

#[derive(
    FromBytes, IntoBytes, Unaligned, Immutable, KnownLayout, Debug, Copy, Clone, PartialEq, Eq,
)]
#[repr(C, packed)]
pub struct HwsimAttrCookie {
    pub header: NlAttrHdr,
    pub cookie: u64,
}

#[derive(
    FromBytes, IntoBytes, Unaligned, Immutable, KnownLayout, Debug, Copy, Clone, PartialEq, Eq,
)]
#[repr(C, packed)]
pub struct TxRate {
    pub idx: u8,
    pub count: u8,
}

#[derive(
    FromBytes, IntoBytes, Unaligned, Immutable, KnownLayout, Debug, Copy, Clone, PartialEq, Eq,
)]
#[repr(C, packed)]
pub struct TxRateFlag {
    pub idx: u8,
    pub flags: u16,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hwsim_msg_layout() {
        assert_eq!(std::mem::size_of::<HwsimMsgHdr>(), 4);
        // HwsimMsg contains a Vec, so its size is not just the sum of its
        // fields' wire size.
    }

    #[test]
    fn test_hwsim_msg_hdr_encode_decode() {
        let hdr = HwsimMsgHdr { hwsim_cmd: HwsimCmd::Register, hwsim_version: 1, reserved: 0xABCD };
        let bytes = hdr.as_bytes();
        assert_eq!(bytes.len(), 4);
        assert_eq!(bytes[0], 1); // Register
        assert_eq!(bytes[1], 1); // Version
        assert_eq!(bytes[2], 0xCD); // Reserved LE
        assert_eq!(bytes[3], 0xAB); // Reserved LE

        let parsed = HwsimMsgHdr::parse(&bytes).unwrap();
        assert_eq!(parsed, hdr);
    }

    #[test]
    fn test_hwsim_msg_encode_decode() {
        let nl_hdr = NlMsgHdr {
            nlmsg_len: 24,  // 16 (NL) + 4 (Hwsim) + 4 (Attr)
            nlmsg_type: 20, // Generic
            nlmsg_flags: 0,
            nlmsg_seq: 1,
            nlmsg_pid: 100,
        };
        let hwsim_hdr = HwsimMsgHdr { hwsim_cmd: HwsimCmd::Frame, hwsim_version: 1, reserved: 0 };
        let attributes = vec![0x04, 0x00, 0x01, 0x00]; // Dummy attribute (len=4, type=1)

        let msg = HwsimMsg { nl_hdr, hwsim_hdr, attributes: attributes.clone() };

        let bytes = msg.encode_to_vec().unwrap();
        assert_eq!(bytes.len(), 16 + 4 + 4);

        let decoded = HwsimMsg::decode_full(&bytes).unwrap();
        assert_eq!(decoded, msg);
    }
}
