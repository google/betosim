// Copyright 2023 The Android Open Source Project
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

use crate::wifi::error::WifiError;
use crate::wifi::hwsim_attr_set::HwsimAttrSet;
use netsim_packets::ieee80211::{Ieee80211, MacAddress};
use netsim_packets::mac80211_hwsim::{HwsimCmd, HwsimMsg, TxRate};
use pdl_runtime::Packet;

/// Parser for the hwsim Frame command (HWSIM_CMD_FRAME).
///
/// The Frame command is sent by the kernel's mac80211_hwsim subsystem
/// and contains the IEEE 802.11 frame along with hwsim attributes.
///
/// This module parses the required and optional hwsim attributes and
/// returns errors if any required attributes are missing.

// The Frame struct contains parsed attributes along with the raw and
// parsed 802.11 frame in `data` and `ieee80211.`
#[derive(Debug)]
pub struct Frame {
    pub transmitter: Option<MacAddress>,
    pub flags: Option<u32>,
    pub tx_info: Option<Vec<TxRate>>,
    pub cookie: Option<u64>,
    pub signal: Option<u32>,
    pub freq: Option<u32>,
    pub data: Vec<u8>,
    pub ieee80211: Ieee80211,
    pub hwsim_msg: HwsimMsg,
    pub attrs: HwsimAttrSet,
}

impl Frame {
    // Builds and validates the Frame from the attributes in the
    // packet. Called when a hwsim packet with HwsimCmd::Frame is
    // found.
    pub fn parse(msg: &HwsimMsg) -> Result<Frame, WifiError> {
        // Only expected to be called with HwsimCmd::Frame
        if msg.hwsim_hdr.hwsim_cmd != HwsimCmd::Frame {
            panic!("Invalid hwsim_cmd");
        }
        let attrs = HwsimAttrSet::parse(&msg.attributes)?;
        let frame =
            attrs.frame.clone().ok_or(WifiError::Frame("Missing frame attribute".to_string()))?;
        let ieee80211 = Ieee80211::decode_full(&frame)?;
        // Required attributes are unwrapped and return an error if
        // they are not present.
        Ok(Frame {
            transmitter: attrs.transmitter,
            flags: attrs.flags,
            tx_info: attrs.tx_info.clone(),
            cookie: attrs.cookie,
            signal: attrs.signal,
            freq: attrs.freq,
            data: frame,
            ieee80211,
            hwsim_msg: msg.clone(),
            attrs,
        })
    }
}
