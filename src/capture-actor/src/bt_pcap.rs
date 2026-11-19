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

//! # Bluetooth H4 PCAP Writer
//!
//! This module provides a PCAP writer specifically for Bluetooth H4 packets.

use std::{io, path::Path};

use bytes::Bytes;
use capture_api::Direction;
use tokio::sync::mpsc;

use crate::writer::{CaptureWriter, DataLinkType, PcapWriter};

const H4_DIRECTION_RECEIVED: u32 = 0;
const H4_DIRECTION_SENT: u32 = 1;

fn encode_h4_packet(direction: Direction, data: &[u8]) -> Option<Vec<u8>> {
    let dir_header = match direction {
        Direction::Received => H4_DIRECTION_RECEIVED,
        Direction::Sent => H4_DIRECTION_SENT,
    };
    Some([&dir_header.to_be_bytes(), data].concat())
}

/// PCAP writer for Bluetooth H4 packets.
pub(crate) struct BluetoothH4Writer;

impl BluetoothH4Writer {
    /// Creates a new Bluetooth H4 PCAP writer.
    #[allow(clippy::new_ret_no_self)]
    pub(crate) async fn new<P: AsRef<Path>>(path: P) -> io::Result<Box<dyn CaptureWriter>> {
        let inner = PcapWriter::new(path, DataLinkType::BluetoothH4).await?;
        Ok(Box::new(inner.with_encoder(encode_h4_packet)))
    }

    pub(crate) fn new_stream() -> (Box<dyn CaptureWriter>, mpsc::Receiver<Bytes>) {
        let (inner, rx) = PcapWriter::new_stream(DataLinkType::BluetoothH4);
        (Box::new(inner.with_encoder(encode_h4_packet)), rx)
    }
}

#[cfg(test)]
mod tests {
    use std::time::SystemTime;

    use netsim_packets::{PcapHeader, PcapRecordHeader};
    use zerocopy::FromBytes;

    use super::*;

    const TEST_H4_EVT_PAYLOAD: &[u8] = &[0x04, 0x0E, 0x04];
    const TEST_H4_CMD_PAYLOAD: &[u8] = &[0x01, 0x03, 0x0C, 0x00];
    const TEST_H4_ACL_PAYLOAD: &[u8] = &[0x02, 0xAA];

    #[test]
    fn encode_h4_packet_prepends_zero_header_for_received() {
        let expected = [&0u32.to_be_bytes()[..], TEST_H4_EVT_PAYLOAD].concat();
        assert_eq!(encode_h4_packet(Direction::Received, TEST_H4_EVT_PAYLOAD), Some(expected));
    }

    #[test]
    fn encode_h4_packet_prepends_one_header_for_sent() {
        let expected = [&1u32.to_be_bytes()[..], TEST_H4_CMD_PAYLOAD].concat();
        assert_eq!(encode_h4_packet(Direction::Sent, TEST_H4_CMD_PAYLOAD), Some(expected));
    }

    #[tokio::test]
    async fn bluetooth_h4_stream_writer_emits_dlt_and_framed_record() {
        let (mut writer, mut rx) = BluetoothH4Writer::new_stream();
        let header = rx.recv().await.unwrap();
        let pcap_header = PcapHeader::read_from_bytes(&header).unwrap();
        assert_eq!(pcap_header.network.get(), DataLinkType::BluetoothH4.as_u32());

        writer.write_packet(SystemTime::now(), Direction::Sent, TEST_H4_ACL_PAYLOAD).await.unwrap();
        let expected_payload = [&1u32.to_be_bytes()[..], TEST_H4_ACL_PAYLOAD].concat();
        let expected_len = u32::try_from(expected_payload.len()).unwrap();
        assert_eq!(writer.get_stats(), (1, u64::from(expected_len)));

        let record = rx.recv().await.unwrap();
        let (record_header, record_payload) = PcapRecordHeader::read_from_prefix(&record).unwrap();
        assert_eq!(record_header.incl_len.get(), expected_len);
        assert_eq!(record_header.orig_len.get(), expected_len);
        assert_eq!(record_payload, expected_payload.as_slice());
    }
}
