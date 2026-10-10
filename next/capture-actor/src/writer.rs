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
use capture_api::Direction;
use netsim_packets::{PcapHeader, PcapRecordHeader};
use tokio::{
    fs::File,
    io::{AsyncWriteExt, BufWriter},
};
use zerocopy::{I32, IntoBytes, U16, U32};

/// Trait for writing packet captures.
///
/// Implementations of this trait handle the actual writing of packets to a
/// specific format.
#[async_trait]
pub trait CaptureWriter: Send + Sync {
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
}

/// PCAP Global Header magic number (microsecond timestamps, standard byte order).
const PCAP_MAGIC_NUMBER: u32 = 0xa1b2c3d4;

/// PCAP major version (version 2.4).
const PCAP_VERSION_MAJOR: u16 = 2;

/// PCAP minor version (version 2.4).
const PCAP_VERSION_MINOR: u16 = 4;

/// Default maximum captured packet snapshot length (65,535 bytes).
const DEFAULT_SNAPLEN: u32 = u16::MAX as u32;

/// Generic PCAP writer.
///
/// Handles writing the PCAP global header and record headers.
pub(crate) struct PcapWriter {
    writer: BufWriter<File>,
    records_written: u64,
    bytes_written: u64,
}

impl PcapWriter {
    /// Creates a new PCAP writer.
    ///
    /// # Arguments
    ///
    /// * `path` - The path to the output file.
    /// * `network` - The Link-Layer Header Type (DLT).
    pub(crate) async fn new<P: AsRef<Path>>(path: P, network: DataLinkType) -> io::Result<Self> {
        let file = File::create(path).await?;
        let mut writer = BufWriter::new(file);

        let header = PcapHeader {
            magic_number: U32::new(PCAP_MAGIC_NUMBER),
            version_major: U16::new(PCAP_VERSION_MAJOR),
            version_minor: U16::new(PCAP_VERSION_MINOR),
            thiszone: I32::new(0),
            sigfigs: U32::new(0),
            snaplen: U32::new(DEFAULT_SNAPLEN),
            network: U32::new(network.as_u32()),
        };

        writer.write_all(header.as_bytes()).await?;

        Ok(Self { writer, records_written: 0, bytes_written: 0 })
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
        let ts_usec = duration.subsec_micros() as u32;

        let header = PcapRecordHeader {
            ts_sec: U32::new(ts_sec),
            ts_usec: U32::new(ts_usec),
            incl_len: U32::new(data.len() as u32),
            orig_len: U32::new(data.len() as u32),
        };

        self.writer.write_all(header.as_bytes()).await?;
        self.writer.write_all(data).await?;

        self.records_written += 1;
        self.bytes_written += data.len() as u64;

        Ok(())
    }

    async fn flush(&mut self) -> io::Result<()> {
        self.writer.flush().await
    }

    fn get_stats(&self) -> (u64, u64) {
        (self.records_written, self.bytes_written)
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
}
