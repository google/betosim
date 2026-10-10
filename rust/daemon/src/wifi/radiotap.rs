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

/// Produce PCAP Radiotap buffers from Hwsim Frames
///
/// This module produces the Radiotap buffers used by PCAP and PcapNG
/// for logging 802.11 frames.
///
/// See https://www.radiotap.org/
use crate::wifi::error::{WifiError, WifiResult};
use crate::wifi::frame::Frame;
use log::info;
use netsim_packets::mac80211_hwsim::{HwsimCmd, HwsimMsg};
use pdl_runtime::Packet;

#[repr(C, packed)]
struct RadiotapHeader {
    version: u8,
    pad: u8,
    len: u16,
    present: u32,
    channel: ChannelInfo,
    signal: u8,
}

#[repr(C)]
struct ChannelInfo {
    freq: u16,
    flags: u16,
}

#[allow(dead_code)]
#[derive(Debug)]
enum HwsimCmdEnum {
    Unspec,
    Register,
    Frame(Box<Frame>),
    TxInfoFrame,
    NewRadio,
    DelRadio,
    GetRadio,
    AddMacAddr,
    DelMacAddr,
}

fn parse_hwsim_cmd(packet: &[u8]) -> WifiResult<HwsimCmdEnum> {
    let hwsim_msg = HwsimMsg::decode_full(packet)?;
    match hwsim_msg.hwsim_hdr.hwsim_cmd {
        HwsimCmd::Frame => {
            let frame = Frame::parse(&hwsim_msg)?;
            Ok(HwsimCmdEnum::Frame(Box::new(frame)))
        }
        HwsimCmd::TxInfoFrame => Ok(HwsimCmdEnum::TxInfoFrame),
        _ => Err(WifiError::Other(format!(
            "Unknown HwsimMsg cmd={:?}",
            hwsim_msg.hwsim_hdr.hwsim_cmd
        ))),
    }
}

pub fn into_pcap(packet: &[u8]) -> Option<Vec<u8>> {
    match parse_hwsim_cmd(packet) {
        Ok(HwsimCmdEnum::Frame(frame)) => frame_into_pcap(*frame).ok(),
        Ok(_) => None,
        Err(e) => {
            info!("Failed to convert packet to pcap format. Err: {}. Packet: {:?}", e, &packet);
            None
        }
    }
}

pub fn frame_into_pcap(frame: Frame) -> WifiResult<Vec<u8>> {
    // Create an instance of the RadiotapHeader with fields for
    // Channel and Signal.  In the future add more fields from the
    // Frame.

    let radiotap_hdr: RadiotapHeader = RadiotapHeader {
        version: 0,
        pad: 0,
        len: (std::mem::size_of::<RadiotapHeader>() as u16),
        present: ((1 << 3) /* channel */ | (1 << 5)/* signal dBm */),
        channel: ChannelInfo { freq: frame.freq.unwrap_or(0) as u16, flags: 0 },
        signal: frame.signal.unwrap_or(0) as u8,
    };

    // Add the struct fields to the buffer manually in little-endian.
    let mut buffer = Vec::<u8>::new();
    buffer.push(radiotap_hdr.version);
    buffer.push(radiotap_hdr.pad);
    buffer.extend_from_slice(&radiotap_hdr.len.to_le_bytes());
    buffer.extend_from_slice(&radiotap_hdr.present.to_le_bytes());
    buffer.extend_from_slice(&radiotap_hdr.channel.freq.to_le_bytes());
    buffer.extend_from_slice(&radiotap_hdr.channel.flags.to_le_bytes());
    buffer.push(radiotap_hdr.signal);
    buffer.extend_from_slice(&frame.data);

    Ok(buffer)
}

#[test]
fn test_netlink_attr() {
    let packet: Vec<u8> = include!("test_packets/hwsim_cmd_frame.csv");
    assert!(parse_hwsim_cmd(&packet).is_ok());

    let tx_info_packet: Vec<u8> = include!("test_packets/hwsim_cmd_tx_info.csv");
    assert!(parse_hwsim_cmd(&tx_info_packet).is_ok());
}

#[test]
fn test_netlink_attr_response_packet() {
    // Response packet may not contain transmitter, flags, tx_info, or cookie fields.
    let response_packet: Vec<u8> =
        include!("test_packets/hwsim_cmd_frame_response_no_transmitter_flags_tx_info.csv");
    assert!(parse_hwsim_cmd(&response_packet).is_ok());

    let response_packet2: Vec<u8> = include!("test_packets/hwsim_cmd_frame_response_no_cookie.csv");
    assert!(parse_hwsim_cmd(&response_packet2).is_ok());
}
