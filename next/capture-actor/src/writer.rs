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

//! # Capture Writers
//!
//! This module provides traits and implementations for writing packet captures
//! to files. Currently supports PCAP format with Bluetooth H4 encapsulation.

use std::{io, path::Path, time::SystemTime};

use async_trait::async_trait;
use bytes::{BufMut, Bytes, BytesMut};
use capture_api::Direction;
use netsim_packets::{PcapHeader, PcapRecordHeader};
use tokio::{
    fs::File,
    io::{AsyncWriteExt, BufWriter},
    sync::mpsc,
};
use tracing::trace;
use zerocopy::{I32, IntoBytes, U16, U32};

/// Trait for writing packet captures.
///
/// Implementations of this trait handle the actual writing of packets to a
/// specific format.
#[async_trait]
pub(crate) trait CaptureWriter: Send + Sync {
    /// Writes a packet to the capture.
    ///
    /// # Arguments
    ///
    /// * `timestamp` - The time the packet was captured.
    /// * `direction` - The direction of the packet (Sent/Received).
    /// * `data` - The packet data.
    async fn write_packet(
        &mut self,
        timestamp: SystemTime,
        direction: Direction,
        data: &[u8],
    ) -> io::Result<()>;

    async fn flush(&mut self) -> io::Result<()>;

    /// Returns statistics about the capture.
    ///
    /// Returns a tuple of (number of records, total bytes written).
    fn get_stats(&self) -> (u64, u64); // (records, bytes)

    /// Returns whether this writer persists to the capture file on disk.
    fn is_file(&self) -> bool {
        true
    }

    /// Returns whether the underlying destination has closed (e.g. a
    /// disconnected WebSocket stream subscriber).
    fn is_closed(&self) -> bool {
        false
    }
}

/// PCAP Global Header magic number (microsecond timestamps, standard byte order).
const PCAP_MAGIC_NUMBER: u32 = 0xa1b2c3d4;

/// PCAP major version (version 2.4).
const PCAP_VERSION_MAJOR: u16 = 2;

/// PCAP minor version (version 2.4).
const PCAP_VERSION_MINOR: u16 = 4;

/// Default maximum captured packet snapshot length (65,535 bytes).
const DEFAULT_SNAPLEN: u32 = u16::MAX as u32;

/// Maximum number of records buffered per stream subscriber. A subscriber that
/// falls further behind loses records rather than stalling the capture actor.
const STREAM_SUBSCRIBER_CAPACITY: usize = 256;

enum PcapSink {
    File(BufWriter<File>),
    Stream(mpsc::Sender<Bytes>),
}

/// Generic PCAP writer.
///
/// Handles writing the PCAP global header and record headers to either a file
/// or a live subscriber stream.
pub(crate) struct PcapWriter {
    sink: PcapSink,
    records_written: u64,
    bytes_written: u64,
}

impl PcapWriter {
    /// Creates a new file-backed PCAP writer.
    pub(crate) async fn new<P: AsRef<Path>>(path: P, network: DataLinkType) -> io::Result<Self> {
        let file = File::create(path).await?;
        let mut writer = BufWriter::new(file);
        writer.write_all(Self::header(network).as_bytes()).await?;
        Ok(Self { sink: PcapSink::File(writer), records_written: 0, bytes_written: 0 })
    }

    /// Creates a new stream-backed PCAP writer and returns its receiver with
    /// the PCAP global header queued first.
    pub(crate) fn new_stream(network: DataLinkType) -> (Self, mpsc::Receiver<Bytes>) {
        let (tx, rx) = mpsc::channel(STREAM_SUBSCRIBER_CAPACITY);
        let _ = tx.try_send(Bytes::copy_from_slice(Self::header(network).as_bytes()));
        (Self { sink: PcapSink::Stream(tx), records_written: 0, bytes_written: 0 }, rx)
    }

    fn header(network: DataLinkType) -> PcapHeader {
        PcapHeader {
            magic_number: U32::new(PCAP_MAGIC_NUMBER),
            version_major: U16::new(PCAP_VERSION_MAJOR),
            version_minor: U16::new(PCAP_VERSION_MINOR),
            thiszone: I32::new(0),
            sigfigs: U32::new(0),
            snaplen: U32::new(DEFAULT_SNAPLEN),
            network: U32::new(network.as_u32()),
        }
    }
}

#[async_trait]
impl CaptureWriter for PcapWriter {
    async fn write_packet(
        &mut self,
        timestamp: SystemTime,
        _direction: Direction,
        data: &[u8],
    ) -> io::Result<()> {
        let duration = timestamp
            .duration_since(SystemTime::UNIX_EPOCH)
            .map_err(|err| io::Error::new(io::ErrorKind::InvalidInput, err))?;
        let ts_sec = duration.as_secs() as u32;
        let ts_usec = duration.subsec_micros();

        let header = PcapRecordHeader {
            ts_sec: U32::new(ts_sec),
            ts_usec: U32::new(ts_usec),
            incl_len: U32::new(data.len() as u32),
            orig_len: U32::new(data.len() as u32),
        };

        match &mut self.sink {
            PcapSink::File(writer) => {
                writer.write_all(header.as_bytes()).await?;
                writer.write_all(data).await?;
            }
            PcapSink::Stream(tx) => {
                let mut record = BytesMut::with_capacity(header.as_bytes().len() + data.len());
                record.put_slice(header.as_bytes());
                record.put_slice(data);
                if let Err(mpsc::error::TrySendError::Full(_)) = tx.try_send(record.freeze()) {
                    trace!("Capture stream subscriber is behind; dropping a record");
                }
            }
        }

        self.records_written += 1;
        self.bytes_written += data.len() as u64;

        Ok(())
    }

    async fn flush(&mut self) -> io::Result<()> {
        match &mut self.sink {
            PcapSink::File(writer) => writer.flush().await,
            PcapSink::Stream(_) => Ok(()),
        }
    }

    fn get_stats(&self) -> (u64, u64) {
        (self.records_written, self.bytes_written)
    }

    fn is_file(&self) -> bool {
        matches!(self.sink, PcapSink::File(_))
    }

    fn is_closed(&self) -> bool {
        matches!(&self.sink, PcapSink::Stream(tx) if tx.is_closed())
    }
}

/// Link-Layer Header Type (DLT / Data Link Type) identifying the encapsulation protocol.
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataLinkType {
    /// IEEE 802.3 Ethernet.
    Ethernet = 1,
    /// IEEE 802.11 plus Radiotap radio header.
    Ieee80211Radio = 127,
    /// User defined 0 (DLT_USER0, often used for custom protocols like AT commands and NCI).
    User0 = 147,
    /// Bluetooth HCI H4 with direction flag.
    BluetoothH4 = 187,
    /// FiRa Ultra-Wideband Command Interface (UCI).
    FiraUci = 299,
}

impl DataLinkType {
    /// Returns the raw u32 numeric identifier for this Data Link Type.
    pub const fn as_u32(self) -> u32 {
        self as u32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_data_link_type_values() {
        assert_eq!(DataLinkType::Ethernet.as_u32(), 1);
        assert_eq!(DataLinkType::Ieee80211Radio.as_u32(), 127);
        assert_eq!(DataLinkType::User0.as_u32(), 147);
        assert_eq!(DataLinkType::BluetoothH4.as_u32(), 187);
        assert_eq!(DataLinkType::FiraUci.as_u32(), 299);
    }

    #[test]
    fn test_pcap_header_constants() {
        assert_eq!(PCAP_MAGIC_NUMBER, 0xa1b2c3d4);
        assert_eq!(PCAP_VERSION_MAJOR, 2);
        assert_eq!(PCAP_VERSION_MINOR, 4);
        assert_eq!(DEFAULT_SNAPLEN, 65535);
    }

    #[tokio::test]
    async fn stream_writer_sends_header_then_records_and_detects_close() {
        let (mut writer, mut rx) = PcapWriter::new_stream(DataLinkType::Ethernet);
        assert!(!writer.is_file());
        assert!(!writer.is_closed());

        let header = rx.recv().await.unwrap();
        assert_eq!(header.len(), size_of::<PcapHeader>());
        assert_eq!(&header[20..24], &DataLinkType::Ethernet.as_u32().to_le_bytes());

        writer.write_packet(SystemTime::now(), Direction::Sent, &[1, 2, 3]).await.unwrap();
        let record = rx.recv().await.unwrap();
        assert_eq!(record.len(), size_of::<PcapRecordHeader>() + 3);
        assert_eq!(&record[size_of::<PcapRecordHeader>()..], &[1, 2, 3]);

        drop(rx);
        assert!(writer.is_closed());
    }

    #[tokio::test]
    async fn stream_writer_drops_records_for_full_subscriber() {
        let (mut writer, mut rx) = PcapWriter::new_stream(DataLinkType::Ethernet);
        for _ in 0..STREAM_SUBSCRIBER_CAPACITY + 10 {
            writer.write_packet(SystemTime::now(), Direction::Sent, b"x").await.unwrap();
        }
        assert!(!writer.is_closed());
        let mut received = 0;
        while rx.try_recv().is_ok() {
            received += 1;
        }
        assert_eq!(received, STREAM_SUBSCRIBER_CAPACITY);
    }
}
