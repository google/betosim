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

use std::{io, path::Path};

use bytes::Bytes;
use tokio::sync::mpsc;

use crate::writer::{CaptureWriter, DataLinkType, PcapWriter};

/// PCAP writer for NFC NCI packets.
pub(crate) struct NciPcapWriter;

impl NciPcapWriter {
    /// Wraps [`PcapWriter`] with [`DataLinkType::User0`] as the format.
    #[allow(clippy::new_ret_no_self)]
    pub(crate) async fn new<P: AsRef<Path>>(path: P) -> io::Result<Box<dyn CaptureWriter>> {
        Ok(Box::new(PcapWriter::new(path, DataLinkType::User0).await?))
    }

    pub(crate) fn new_stream() -> (Box<dyn CaptureWriter>, mpsc::Receiver<Bytes>) {
        let (pcap, rx) = PcapWriter::new_stream(DataLinkType::User0);
        (Box::new(pcap), rx)
    }
}
