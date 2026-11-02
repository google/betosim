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

use netsim_packets::{
    HwsimAttrSet, HwsimCmd, HwsimFrame, HwsimMsg, HwsimMsgHdr, Ieee80211, MacAddress, NlMsgHdr,
    TxRate, TxRateFlag,
};

use crate::{error::WifiError, medium::WifiResult};

const RX_RATE: u32 = 1;
const DEFAULT_SIGNAL_DBM: i32 = -50;
const SIGNAL: u32 = DEFAULT_SIGNAL_DBM as u32;
const NL_MSG_HDR_LEN: usize = 16;
const NLMSG_MIN_TYPE: u16 = 0x10;
const NL_AUTO_SEQ: u16 = 0;
const NL_AUTO_PORT: u32 = 0;

pub(crate) fn create_hwsim_msg_with_attrs(
    frame: &HwsimFrame,
    ieee80211: &Ieee80211,
    dest_hwsim_addr: &MacAddress,
) -> WifiResult<HwsimMsg> {
    let attrs = &frame.attrs;
    let frame_bytes = ieee80211.as_bytes();

    let hwsim_msg = construct_hwsim_msg(
        &dest_hwsim_addr.bytes,
        frame_bytes,
        None, // Legacy did not include transmitter for RX frames
        attrs.freq,
        Some(attrs.rx_rate_idx.unwrap_or(RX_RATE)),
        Some(attrs.signal.unwrap_or(SIGNAL)),
        attrs.flags,
        None, // Do not echo the TX cookie to the RX path
        attrs.tx_info.as_deref(),
        attrs.tx_info_flags.as_deref(),
        frame.hwsim_msg.nl_hdr.nlmsg_flags,
    )?;

    Ok(hwsim_msg)
}

pub(crate) fn parse_hwsim_frame(packet: &bytes::Bytes, client_id: u32) -> WifiResult<HwsimFrame> {
    let hwsim_msg =
        HwsimMsg::decode_full(packet).map_err(|e| WifiError::Internal(Box::from(e.to_string())))?;
    match hwsim_msg.hwsim_hdr.hwsim_cmd {
        HwsimCmd::Frame => {
            let frame = HwsimFrame::parse(&hwsim_msg)
                .map_err(|e| WifiError::Internal(Box::from(e.to_string())))?;
            if frame.transmitter.is_none()
                || frame.flags.is_none()
                || frame.cookie.is_none()
                || frame.tx_info.is_none()
            {
                return Err(WifiError::Internal(Box::from(format!(
                    "Missing Hwsim attributes for incoming packet for client: {client_id}"
                ))));
            }
            let _ = frame.transmitter.ok_or(WifiError::Internal(Box::from(format!(
                "Missing transmitter attribute in frame for client: {client_id}"
            ))))?;
            let _ = frame.flags.ok_or(WifiError::Internal(Box::from(format!(
                "Missing flags attribute in frame for client: {client_id}"
            ))))?;
            let _ = frame.cookie.ok_or(WifiError::Internal(Box::from(format!(
                "Missing cookie attribute in frame for client: {client_id}"
            ))))?;
            // Removed debug logging here to avoid log dependency in utils, or we can add
            // it. Caller can log if needed.
            Ok(frame)
        }
        _ => Err(WifiError::Internal(Box::from(format!(
            "Another command found {hwsim_msg:?} for client: {client_id}"
        )))),
    }
}

pub fn create_hwsim_msg_from_frame(
    ieee80211: &Ieee80211,
    dest_hwsim_addr: &MacAddress,
    freq: u32,
    transmitter: Option<&MacAddress>,
) -> WifiResult<HwsimMsg> {
    let frame_bytes = ieee80211.as_bytes();
    construct_hwsim_msg(
        &dest_hwsim_addr.bytes,
        frame_bytes,
        transmitter.map(|t| &t.bytes),
        Some(freq),
        Some(RX_RATE),
        Some(SIGNAL),
        Some(0),   // flags
        Some(0),   // cookie
        Some(&[]), // tx_info
        None,      // tx_info_flags
        NL_AUTO_SEQ,
    )
}

#[allow(clippy::too_many_arguments)]
fn construct_hwsim_msg(
    receiver: &[u8; 6],
    frame: &[u8],
    transmitter: Option<&[u8; 6]>,
    freq: Option<u32>,
    rx_rate: Option<u32>,
    signal: Option<u32>,
    flags: Option<u32>,
    cookie: Option<u64>,
    tx_info: Option<&[TxRate]>,
    tx_info_flags: Option<&[TxRateFlag]>,
    nlmsg_flags: u16,
) -> WifiResult<HwsimMsg> {
    let mut builder = HwsimAttrSet::builder();
    builder.receiver(receiver);
    builder.frame(frame);
    if let Some(tx) = transmitter {
        builder.transmitter(tx);
    }
    if let Some(r) = rx_rate {
        builder.rx_rate(r);
    }
    if let Some(s) = signal {
        builder.signal(s);
    }
    if let Some(f) = freq {
        builder.freq(f);
    }
    if let Some(f) = flags {
        builder.flags(f);
    }
    if let Some(c) = cookie {
        builder.cookie(c);
    }
    if let Some(t) = tx_info {
        builder.tx_info(t);
    }
    if let Some(t) = tx_info_flags {
        builder.tx_info_flags(t);
    }

    let attributes =
        builder.build().map_err(|e| WifiError::Internal(Box::from(e.to_string())))?.attributes;
    let hwsim_hdr = HwsimMsgHdr { hwsim_cmd: HwsimCmd::Frame, hwsim_version: 0, reserved: 0 };
    let nlmsg_len = (NL_MSG_HDR_LEN + hwsim_hdr.encoded_len() as usize + attributes.len()) as u32;
    let nl_hdr = NlMsgHdr {
        nlmsg_len,
        nlmsg_type: NLMSG_MIN_TYPE,
        nlmsg_flags,
        nlmsg_seq: NL_AUTO_PORT,
        nlmsg_pid: 0,
    };
    Ok(HwsimMsg { nl_hdr, hwsim_hdr, attributes })
}

pub(crate) fn build_tx_info(hwsim_msg: &HwsimMsg, acked: bool) -> WifiResult<HwsimMsg> {
    let attrs = HwsimAttrSet::parse(&hwsim_msg.attributes)
        .map_err(|e| WifiError::Internal(Box::from(e.to_string())))?;

    let hwsim_hdr = &hwsim_msg.hwsim_hdr;
    let nl_hdr = &hwsim_msg.nl_hdr;
    let mut new_attr_builder = HwsimAttrSet::builder();
    const HWSIM_TX_STAT_ACK: u32 = 1 << 2;

    let raw_flags =
        attrs.flags.ok_or(WifiError::Internal(Box::from("Missing flags in HwsimAttrSet")))?;
    let tx_flags =
        if acked { raw_flags | HWSIM_TX_STAT_ACK } else { raw_flags & !HWSIM_TX_STAT_ACK };

    new_attr_builder
        .transmitter(
            &attrs
                .transmitter
                .ok_or(WifiError::Internal(Box::from("Missing transmitter in HwsimAttrSet")))?
                .into(),
        )
        .flags(tx_flags)
        .cookie(
            attrs.cookie.ok_or(WifiError::Internal(Box::from("Missing cookie in HwsimAttrSet")))?,
        )
        .signal(attrs.signal.unwrap_or(SIGNAL))
        .tx_info(
            attrs
                .tx_info
                .ok_or(WifiError::Internal(Box::from("Missing tx_info in HwsimAttrSet")))?
                .as_slice(),
        );

    let new_attr =
        new_attr_builder.build().map_err(|e| WifiError::Internal(Box::from(e.to_string())))?;
    let nlmsg_len = (NlMsgHdr::SIZE + HwsimMsgHdr::SIZE + new_attr.attributes.len()) as u32;
    let new_hwsim_msg = HwsimMsg {
        attributes: new_attr.attributes,
        hwsim_hdr: HwsimMsgHdr {
            hwsim_cmd: HwsimCmd::TxInfoFrame,
            hwsim_version: 0,
            reserved: hwsim_hdr.reserved,
        },
        nl_hdr: NlMsgHdr {
            nlmsg_len,
            nlmsg_type: NLMSG_MIN_TYPE,
            nlmsg_flags: nl_hdr.nlmsg_flags,
            nlmsg_seq: 0,
            nlmsg_pid: 0,
        },
    };
    Ok(new_hwsim_msg)
}
