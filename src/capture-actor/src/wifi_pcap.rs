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

//! # Wi-Fi PCAP Writer
//!
//! This module provides a PCAP writer specifically for Wi-Fi packets.
//! It implements the Radiotap header encapsulation for IEEE 802.11 payloads.

use std::{io::Result, path::Path};

use bytes::Bytes;
use capture_api::Direction;
use netsim_packets::{HwsimCmd, HwsimFrame, HwsimMsg, create_radiotap_packet};
use tokio::sync::mpsc;
use tracing::debug;

use crate::writer::{CaptureWriter, DataLinkType, PcapWriter};

fn encode_wifi_packet(_direction: Direction, data: &[u8]) -> Option<Vec<u8>> {
    let hwsim_msg = HwsimMsg::decode_full(data)
        .inspect_err(|error| debug!(?error, "WifiPcapWriter: Failed to decode HwsimMsg"))
        .ok()?;
    if hwsim_msg.hwsim_hdr.hwsim_cmd != HwsimCmd::Frame {
        return None;
    }
    let frame = HwsimFrame::parse(&hwsim_msg)
        .inspect_err(|error| {
            debug!(?error, "WifiPcapWriter: Failed to parse HwsimFrame from HwsimMsg");
        })
        .ok()?;
    Some(create_radiotap_packet(&frame))
}

/// PCAP writer for Wi-Fi packets.
///
/// Wraps IEEE 802.11 packets with a Radiotap header.
pub(crate) struct WifiPcapWriter;

impl WifiPcapWriter {
    /// Creates a new Wi-Fi PCAP writer.
    #[allow(clippy::new_ret_no_self)]
    pub(crate) async fn new<P: AsRef<Path>>(path: P) -> Result<Box<dyn CaptureWriter>> {
        let inner = PcapWriter::new(path, DataLinkType::Ieee80211Radio).await?;
        Ok(Box::new(inner.with_encoder(encode_wifi_packet)))
    }

    pub(crate) fn new_stream() -> (Box<dyn CaptureWriter>, mpsc::Receiver<Bytes>) {
        let (inner, rx) = PcapWriter::new_stream(DataLinkType::Ieee80211Radio);
        (Box::new(inner.with_encoder(encode_wifi_packet)), rx)
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, time::SystemTime};

    use netsim_packets::{
        DataSubType, FrameType, HwsimAttrSet, HwsimCmd, HwsimMsg, HwsimMsgHdr, Ieee80211ToAp,
        MacAddress, NlMsgHdr, PcapHeader, PcapRecordHeader,
    };
    use zerocopy::FromBytes;

    use super::*;
    use crate::writer::TempPcapFile;

    #[tokio::test]
    async fn test_wifi_pcap_writer() {
        let temp_file = TempPcapFile::new("test_wifi_netsim");
        let mut writer = WifiPcapWriter::new(temp_file.path()).await.unwrap();
        // Since `[1,2,3]` is an invalid HwsimMsg, it will fail to decode and write 0
        // packets.
        writer.write_packet(SystemTime::now(), Direction::Sent, &[1, 2, 3]).await.unwrap();

        let (records, bytes) = writer.get_stats();
        assert_eq!(records, 0);
        assert_eq!(bytes, 0);
        writer.flush().await.unwrap();
        assert_eq!(fs::read(temp_file.path()).unwrap().len(), size_of::<PcapHeader>());
    }

    #[tokio::test]
    async fn test_wifi_pcap_writer_filter_tx_info() {
        let temp_file = TempPcapFile::new("test_wifi_filter_tx_info");
        let mut writer = WifiPcapWriter::new(temp_file.path()).await.unwrap();

        let data = encode_test_hwsim_msg(HwsimCmd::TxInfoFrame, Vec::new());
        writer.write_packet(SystemTime::now(), Direction::Sent, &data).await.unwrap();

        let (records, bytes) = writer.get_stats();
        assert_eq!(records, 0);
        assert_eq!(bytes, 0);
        writer.flush().await.unwrap();
        assert_eq!(fs::read(temp_file.path()).unwrap().len(), size_of::<PcapHeader>());
    }

    const TEST_FREQ_MHZ: u16 = 2412;
    const TEST_SIGNAL_DBM: i8 = -50;
    const TEST_HWSIM_NLMSG_TYPE: u16 = 20;
    const TEST_HWSIM_VERSION: u8 = 1;

    fn encode_test_hwsim_msg(hwsim_cmd: HwsimCmd, attributes: Vec<u8>) -> Vec<u8> {
        let nlmsg_len =
            u32::try_from(NlMsgHdr::SIZE + HwsimMsgHdr::SIZE + attributes.len()).unwrap();
        let nl_hdr = NlMsgHdr {
            nlmsg_len,
            nlmsg_type: TEST_HWSIM_NLMSG_TYPE,
            nlmsg_flags: 0,
            nlmsg_seq: 1,
            nlmsg_pid: 100,
        };
        let hwsim_hdr = HwsimMsgHdr { hwsim_cmd, hwsim_version: TEST_HWSIM_VERSION, reserved: 0 };
        HwsimMsg { nl_hdr, hwsim_hdr, attributes }.encode_to_vec().unwrap()
    }

    fn create_test_ieee80211_frame() -> Vec<u8> {
        Ieee80211ToAp {
            version: 0,
            ftype: FrameType::Data,
            stype: u8::from(DataSubType::Data),
            destination: MacAddress::new([0x21, 0x22, 0x23, 0x24, 0x25, 0x26]),
            source: MacAddress::new([0x11, 0x12, 0x13, 0x14, 0x15, 0x16]),
            bssid: MacAddress::new([0x01, 0x02, 0x03, 0x04, 0x05, 0x06]),
            duration_id: 0,
            seq_ctrl: 0,
            qos_ctrl: None,
            protected: 0,
            order: 0,
            more_frags: 0,
            retry: 0,
            pm: 0,
            more_data: 0,
            payload: vec![0xDE, 0xAD, 0xBE, 0xEF],
        }
        .encode_to_bytes()
        .unwrap()
    }

    fn encode_test_hwsim_frame_msg(frame_bytes: &[u8]) -> Vec<u8> {
        let mut builder = HwsimAttrSet::builder();
        builder
            .frame(frame_bytes)
            .freq(u32::from(TEST_FREQ_MHZ))
            .signal(i32::from(TEST_SIGNAL_DBM).cast_unsigned());
        encode_test_hwsim_msg(HwsimCmd::Frame, builder.build().unwrap().attributes)
    }

    #[tokio::test]
    async fn wifi_stream_writer_skips_malformed_hwsim_frame() {
        let (mut writer, mut rx) = WifiPcapWriter::new_stream();
        let _header = rx.recv().await.unwrap();

        let raw_bytes = encode_test_hwsim_msg(HwsimCmd::Frame, Vec::new());
        writer.write_packet(SystemTime::now(), Direction::Sent, &raw_bytes).await.unwrap();
        assert_eq!(writer.get_stats(), (0, 0));
        assert_eq!(rx.try_recv(), Err(mpsc::error::TryRecvError::Empty));
    }

    #[tokio::test]
    async fn wifi_stream_writer_encapsulates_valid_hwsim_frame_in_radiotap() {
        let (mut writer, mut rx) = WifiPcapWriter::new_stream();
        let header = rx.recv().await.unwrap();
        let pcap_header = PcapHeader::read_from_bytes(&header).unwrap();
        assert_eq!(pcap_header.network.get(), DataLinkType::Ieee80211Radio.as_u32());

        let frame_bytes = create_test_ieee80211_frame();
        let raw_bytes = encode_test_hwsim_frame_msg(&frame_bytes);

        writer.write_packet(SystemTime::now(), Direction::Sent, &raw_bytes).await.unwrap();

        let hwsim_msg = HwsimMsg::decode_full(&raw_bytes).unwrap();
        let hwsim_frame = HwsimFrame::parse(&hwsim_msg).unwrap();
        let expected_radiotap = create_radiotap_packet(&hwsim_frame);
        let expected_len = u32::try_from(expected_radiotap.len()).unwrap();

        let record = rx.recv().await.unwrap();
        let (record_header, record_payload) = PcapRecordHeader::read_from_prefix(&record).unwrap();
        assert_eq!(record_header.incl_len.get(), expected_len);
        assert_eq!(record_header.orig_len.get(), expected_len);
        assert_eq!(record_payload, expected_radiotap.as_slice());
        assert_eq!(writer.get_stats(), (1, u64::from(expected_len)));
    }
}
