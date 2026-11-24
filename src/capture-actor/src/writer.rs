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

use std::{error::Error, io, path::Path, time::SystemTime};

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

/// Optional packet encoder function applied before writing to PCAP.
/// Returning `None` skips writing without incrementing stats or erroring.
pub(crate) type PacketEncoderFn = fn(Direction, &[u8]) -> Option<Vec<u8>>;

enum PcapSink {
    File(BufWriter<File>),
    Stream(mpsc::Sender<Bytes>),
}

/// Generic PCAP writer.
///
/// Handles writing the PCAP global header and record headers to either a file
/// or a live subscriber stream.
#[must_use]
pub(crate) struct PcapWriter {
    sink: PcapSink,
    records_written: u64,
    bytes_written: u64,
    encode_fn: Option<PacketEncoderFn>,
}

impl PcapWriter {
    fn new_internal(sink: PcapSink) -> Self {
        Self { sink, records_written: 0, bytes_written: 0, encode_fn: None }
    }

    /// Creates a new file-backed PCAP writer.
    pub(crate) async fn new<P: AsRef<Path>>(path: P, network: DataLinkType) -> io::Result<Self> {
        let file = File::create(path).await?;
        let mut writer = BufWriter::new(file);
        writer.write_all(Self::header(network).as_bytes()).await?;
        writer.flush().await?;
        Ok(Self::new_internal(PcapSink::File(writer)))
    }

    /// Creates a new stream-backed PCAP writer and returns its receiver with
    /// the PCAP global header queued first.
    pub(crate) fn new_stream(network: DataLinkType) -> (Self, mpsc::Receiver<Bytes>) {
        let (tx, rx) = mpsc::channel(STREAM_SUBSCRIBER_CAPACITY);
        let _ = tx.try_send(Bytes::copy_from_slice(Self::header(network).as_bytes()));
        (Self::new_internal(PcapSink::Stream(tx)), rx)
    }

    /// Sets the packet encoder applied before writing each record.
    /// If the encoder returns `None`, writing is skipped without erroring or
    /// incrementing stats.
    pub(crate) fn with_encoder(mut self, encode_fn: PacketEncoderFn) -> Self {
        self.encode_fn = Some(encode_fn);
        self
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

fn invalid_input<E: Into<Box<dyn Error + Send + Sync>>>(err: E) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, err)
}

#[async_trait]
impl CaptureWriter for PcapWriter {
    async fn write_packet(
        &mut self,
        timestamp: SystemTime,
        direction: Direction,
        data: &[u8],
    ) -> io::Result<()> {
        let encoded;
        let payload = if let Some(encode) = self.encode_fn {
            let Some(buf) = encode(direction, data) else {
                return Ok(());
            };
            encoded = buf;
            encoded.as_slice()
        } else {
            data
        };

        let duration = timestamp.duration_since(SystemTime::UNIX_EPOCH).map_err(invalid_input)?;
        let ts_sec = u32::try_from(duration.as_secs()).map_err(invalid_input)?;
        let ts_usec = duration.subsec_micros();

        let payload_len = u32::try_from(payload.len()).map_err(invalid_input)?;

        let header = PcapRecordHeader {
            ts_sec: U32::new(ts_sec),
            ts_usec: U32::new(ts_usec),
            incl_len: U32::new(payload_len),
            orig_len: U32::new(payload_len),
        };

        match &mut self.sink {
            PcapSink::File(writer) => {
                writer.write_all(header.as_bytes()).await?;
                writer.write_all(payload).await?;
            }
            PcapSink::Stream(tx) => {
                let mut record = BytesMut::with_capacity(header.as_bytes().len() + payload.len());
                record.put_slice(header.as_bytes());
                record.put_slice(payload);
                if let Err(mpsc::error::TrySendError::Full(_)) = tx.try_send(record.freeze()) {
                    trace!("Capture stream subscriber is behind; dropping a record");
                }
            }
        }

        self.records_written += 1;
        self.bytes_written += u64::from(payload_len);

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
    BluetoothH4 = 201,
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
pub(crate) struct TempPcapFile(std::path::PathBuf);

#[cfg(test)]
impl TempPcapFile {
    pub(crate) fn new(prefix: &str) -> Self {
        static NEXT_TEMP_ID: std::sync::atomic::AtomicUsize =
            std::sync::atomic::AtomicUsize::new(0);
        let id = NEXT_TEMP_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("{prefix}_{}_{id}.pcap", std::process::id()));
        let _ = std::fs::remove_file(&path);
        Self(path)
    }

    pub(crate) fn path(&self) -> &Path {
        &self.0
    }
}

#[cfg(test)]
impl Drop for TempPcapFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use zerocopy::FromBytes;

    use super::*;

    const TEST_PACKET: &[u8] = &[1, 2, 3];

    #[test]
    fn test_data_link_type_values() {
        assert_eq!(DataLinkType::Ethernet.as_u32(), 1);
        assert_eq!(DataLinkType::Ieee80211Radio.as_u32(), 127);
        assert_eq!(DataLinkType::User0.as_u32(), 147);
        assert_eq!(DataLinkType::BluetoothH4.as_u32(), 201);
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

    #[tokio::test]
    async fn file_writer_flushes_global_header_on_creation() {
        let temp_file = TempPcapFile::new("netsim_pcap_writer_test");
        let _writer = PcapWriter::new(temp_file.path(), DataLinkType::BluetoothH4).await.unwrap();
        let bytes = std::fs::read(temp_file.path()).unwrap();
        let pcap_header = PcapHeader::read_from_bytes(&bytes).unwrap();
        assert_eq!(pcap_header.magic_number.get(), PCAP_MAGIC_NUMBER);
        assert_eq!(pcap_header.network.get(), DataLinkType::BluetoothH4.as_u32());
    }

    #[tokio::test]
    async fn stream_writer_rejects_pre_epoch_timestamp_when_not_skipped() {
        let (mut writer, _rx) = PcapWriter::new_stream(DataLinkType::Ethernet);
        let pre_epoch = SystemTime::UNIX_EPOCH - Duration::from_secs(1);
        let err = writer.write_packet(pre_epoch, Direction::Sent, TEST_PACKET).await.unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
        assert_eq!(writer.get_stats(), (0, 0));
    }

    #[tokio::test]
    async fn stream_writer_rejects_timestamp_exceeding_u32_max_seconds() {
        let (mut writer, _rx) = PcapWriter::new_stream(DataLinkType::Ethernet);
        let time = SystemTime::UNIX_EPOCH + Duration::from_secs(u64::from(u32::MAX) + 1);
        let err = writer.write_packet(time, Direction::Sent, TEST_PACKET).await.unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
        assert_eq!(writer.get_stats(), (0, 0));
    }

    #[tokio::test]
    async fn stream_writer_with_encoder_skips_none_before_timestamp_validation() {
        let (writer, mut rx) = PcapWriter::new_stream(DataLinkType::Ethernet);
        let mut writer = writer.with_encoder(|_, _| None);
        let pre_epoch = SystemTime::UNIX_EPOCH - Duration::from_secs(1);
        writer.write_packet(pre_epoch, Direction::Sent, TEST_PACKET).await.unwrap();
        assert_eq!(writer.get_stats(), (0, 0));
        let header = rx.recv().await.unwrap();
        assert_eq!(header.len(), size_of::<PcapHeader>());
        assert_eq!(rx.try_recv(), Err(mpsc::error::TryRecvError::Empty));
    }

    #[tokio::test]
    async fn stream_writer_with_encoder_writes_encoded_payload_and_updates_stats() {
        let (writer, mut rx) = PcapWriter::new_stream(DataLinkType::Ethernet);
        let mut writer = writer.with_encoder(|dir, data| {
            assert_eq!(dir, Direction::Sent);
            Some([&[0xBB], data].concat())
        });
        let payload = &[0x11, 0x22, 0x33];
        writer.write_packet(SystemTime::now(), Direction::Sent, payload).await.unwrap();
        let expected = [&[0xBB][..], payload].concat();
        let expected_len = u32::try_from(expected.len()).unwrap();
        assert_eq!(writer.get_stats(), (1, u64::from(expected_len)));
        let _header = rx.recv().await.unwrap();
        let record = rx.recv().await.unwrap();
        let (record_header, record_payload) = PcapRecordHeader::read_from_prefix(&record).unwrap();
        assert_eq!(record_header.incl_len.get(), expected_len);
        assert_eq!(record_header.orig_len.get(), expected_len);
        assert_eq!(record_payload, expected.as_slice());
    }

    #[tokio::test]
    async fn new_file_writer_rejects_missing_parent_directory() {
        let temp_file = TempPcapFile::new("netsim_missing_parent_test");
        let missing_path = temp_file.path().join("missing_parent/capture.pcap");
        let res = PcapWriter::new(&missing_path, DataLinkType::User0).await;
        assert!(matches!(res, Err(err) if err.kind() == io::ErrorKind::NotFound));
    }
}
