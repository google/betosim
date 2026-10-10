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

//! # Ethernet PCAP Writer
//!
//! This module provides a PCAP writer specifically for Ethernet packets.

use std::{io, path::Path};

use crate::writer::{CaptureWriter, DataLinkType, PcapWriter};

/// PCAP writer for Ethernet packets.
pub(crate) struct EthernetPcapWriter;

impl EthernetPcapWriter {
    /// Wraps [`PcapWriter`] with [`DataLinkType::Ethernet`] as the format.
    #[allow(clippy::new_ret_no_self)]
    pub(crate) async fn new<P: AsRef<Path>>(path: P) -> io::Result<Box<dyn CaptureWriter>> {
        Ok(Box::new(PcapWriter::new(path, DataLinkType::Ethernet).await?))
    }
}
