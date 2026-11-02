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

use std::{io, path::Path, time::SystemTime};

use async_trait::async_trait;
use bytes::Bytes;
use capture_api::Direction;
use tokio::sync::mpsc;

use crate::writer::{CaptureWriter, DataLinkType, PcapWriter};

/// PCAP writer for Bluetooth H4 packets.
pub(crate) struct BluetoothH4Writer {
    inner: PcapWriter,
}

impl BluetoothH4Writer {
    /// Creates a new Bluetooth H4 PCAP writer.
    #[allow(clippy::new_ret_no_self)]
    pub(crate) async fn new<P: AsRef<Path>>(path: P) -> io::Result<Box<dyn CaptureWriter>> {
        let inner = PcapWriter::new(path, DataLinkType::BluetoothH4).await?;
        Ok(Box::new(Self { inner }))
    }

    pub(crate) fn new_stream() -> (Box<dyn CaptureWriter>, mpsc::Receiver<Bytes>) {
        let (inner, rx) = PcapWriter::new_stream(DataLinkType::BluetoothH4);
        (Box::new(Self { inner }), rx)
    }
}

#[async_trait]
impl CaptureWriter for BluetoothH4Writer {
    async fn write_packet(
        &mut self,
        timestamp: SystemTime,
        direction: Direction,
        data: &[u8],
    ) -> io::Result<()> {
        let dir_bytes = match direction {
            Direction::Received => 0u32.to_be_bytes(),
            Direction::Sent => 1u32.to_be_bytes(),
        };
        let framed: Vec<u8> = dir_bytes.into_iter().chain(data.iter().copied()).collect();
        self.inner.write_packet(timestamp, direction, &framed).await
    }

    async fn flush(&mut self) -> io::Result<()> {
        self.inner.flush().await
    }

    fn get_stats(&self) -> (u64, u64) {
        self.inner.get_stats()
    }

    fn is_file(&self) -> bool {
        self.inner.is_file()
    }

    fn is_closed(&self) -> bool {
        self.inner.is_closed()
    }
}
