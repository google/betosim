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

//! Defines structures for Radiotap headers.
//!
//! Radiotap is a standard for 802.11 frame injection and reception.
//! See <https://www.radiotap.org/>

use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout, Unaligned};

/// Radiotap header.
#[repr(C, packed)]
#[derive(Debug, Copy, Clone, FromBytes, IntoBytes, Immutable, KnownLayout, Unaligned)]
pub struct RadiotapHeader {
    pub version: u8,
    pub pad: u8,
    pub len: u16,
    pub present: u32,
    pub channel: ChannelInfo,
    pub signal: u8,
}

/// Channel information in Radiotap header.
#[repr(C, packed)]
#[derive(Debug, Copy, Clone, FromBytes, IntoBytes, Immutable, KnownLayout, Unaligned)]
pub struct ChannelInfo {
    pub freq: u16,
    pub flags: u16,
}

use crate::netlink::hwsim_frame::HwsimFrame;

/// Creates a Radiotap packet from a HwsimFrame.
pub fn create_radiotap_packet(frame: &HwsimFrame) -> Vec<u8> {
    let radiotap_hdr = RadiotapHeader {
        version: 0,
        pad: 0,
        len: (std::mem::size_of::<RadiotapHeader>() as u16),
        present: ((1 << 3) /* channel */ | (1 << 5)/* signal dBm */),
        channel: ChannelInfo { freq: frame.freq.unwrap_or(0) as u16, flags: 0 },
        signal: frame.signal.unwrap_or(0) as u8,
    };

    let mut buffer = Vec::new();
    buffer.extend_from_slice(radiotap_hdr.as_bytes());
    buffer.extend_from_slice(&frame.data);
    buffer
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        ethernet::MacAddr,
        ieee80211::Ieee80211,
        netlink::{
            TxRate,
            hwsim_attr_set::HwsimAttrSet,
            mac80211_hwsim::{HwsimCmd, HwsimMsg, HwsimMsgHdr},
        },
    };

    #[test]
    fn test_create_radiotap_packet() {
        // Create a dummy HwsimFrame
        let data = vec![0x00; 10]; // Minimum valid length for Ieee80211::decode
        let frame = HwsimFrame {
            transmitter: Some(MacAddr::new([0x00, 0x11, 0x22, 0x33, 0x44, 0x55])),
            flags: Some(0),
            tx_info: Some(vec![TxRate { idx: 0, count: 1 }]),
            cookie: Some(12345),
            signal: Some(-50i32 as u32), // -50 dBm
            freq: Some(2412),
            data: data.clone(),
            // Dummy Ieee80211
            ieee80211: Ieee80211::decode(&data).unwrap(),
            // Dummy HwsimMsg
            hwsim_msg: HwsimMsg {
                nl_hdr: crate::netlink::NlMsgHdr {
                    nlmsg_len: 0,
                    nlmsg_type: 0,
                    nlmsg_flags: 0,
                    nlmsg_seq: 0,
                    nlmsg_pid: 0,
                },
                hwsim_hdr: HwsimMsgHdr {
                    hwsim_cmd: HwsimCmd::Frame,
                    hwsim_version: 1,
                    reserved: 0,
                },
                attributes: vec![],
            },
            // Dummy HwsimAttrSet
            attrs: HwsimAttrSet {
                transmitter: None,
                receiver: None,
                frame: None,
                flags: None,
                rx_rate_idx: None,
                signal: None,
                cookie: None,
                freq: None,
                tx_info: None,
                tx_info_flags: None,
                attributes: vec![],
            },
        };

        let radiotap_packet = create_radiotap_packet(&frame);

        // Verify header
        let (hdr, payload) =
            zerocopy::Ref::<&[u8], RadiotapHeader>::from_prefix(&radiotap_packet).unwrap();
        assert_eq!(hdr.version, 0);
        let len = hdr.len;
        assert_eq!(len, std::mem::size_of::<RadiotapHeader>() as u16);
        let present = hdr.present;
        assert_eq!(present, (1 << 3) | (1 << 5)); // Channel | Signal
        let freq = hdr.channel.freq;
        assert_eq!(freq, 2412);
        assert_eq!(hdr.signal, -50i32 as u8);

        // Verify payload
        assert_eq!(payload, &data[..]);
    }
}
