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

//! # Cellular Modem 3GPP TS 27.010 PCAP Writer
//!
//! Provides PCAP writing for cellular modem AT command traffic formatted as
//! standard 3GPP TS 27.010 (GSM 07.10) multiplexer frames under DLT_USER0
//! (147).
//!
//! Wireshark and `tshark` natively dissect these frames using the built-in
//! `mux27010` dissector.
//!
//! ## Inspecting Captures with `tshark`
//!
//! To inspect the generated PCAP in the terminal using `tshark`:
//!
//! ```bash
//! # Summary view
//! tshark -o 'uat:user_dlts:"User 0 (DLT=147)","mux27010","0","","0",""' -r <pcap_file>.pcap
//!
//! # Verbose protocol tree (showing DLCI, direction, FCS, and AT command payload)
//! tshark -o 'uat:user_dlts:"User 0 (DLT=147)","mux27010","0","","0",""' -r <pcap_file>.pcap -V
//!
//! # Extract specific fields (frame number, direction, and AT command string)
//! tshark -o 'uat:user_dlts:"User 0 (DLT=147)","mux27010","0","","0",""' -r <pcap_file>.pcap \
//!     -T fields -e frame.number -e mux27010.direction -e mux27010.information_str
//! ```
//!
//! ## Inspecting Captures in Wireshark GUI
//!
//! 1. Open Wireshark and go to **Edit -> Preferences -> Protocols ->
//!    DLT_USER**.
//! 2. In the Encapsulations Table, add `User 0 (DLT=147)` and set the dissector
//!    to `mux27010`.
//! 3. Alternatively, right-click any packet in the capture -> **Decode As...**
//!    -> select `MUX27010`.

use std::{io, path::Path, time::SystemTime};

use async_trait::async_trait;
use capture_api::Direction;

use crate::writer::{CaptureWriter, DataLinkType, PcapWriter};

/// GSM 07.10 Basic Frame opening/closing delimiter.
const GSM0710_FLAG: u8 = 0xF9;
/// GSM 07.10 UIH (Unnumbered Information with Header check) control byte.
const GSM0710_CTRL_UIH: u8 = 0xEF;
/// Default DLCI for primary AT command channel.
const GSM0710_DEFAULT_DLCI: u8 = 1;

/// Computes 3GPP TS 27.010 FCS checksum dynamically (polynomial 0xE0).
/// Accepts any iterator over bytes to avoid requiring a slice allocation.
fn compute_fcs<I: IntoIterator<Item = u8>>(data: I) -> u8 {
    let mut fcs = 0xFF;
    for b in data {
        fcs ^= b;
        for _ in 0..8 {
            if (fcs & 1) != 0 {
                fcs = (fcs >> 1) ^ 0xE0;
            } else {
                fcs >>= 1;
            }
        }
    }
    !fcs
}

/// PCAP writer for Cellular Modem AT command traffic.
pub(crate) struct ModemPcapWriter {
    inner: PcapWriter,
}

impl ModemPcapWriter {
    /// Creates a new Modem AT PCAP writer using User0 / DLT_USER0 (147).
    #[allow(clippy::new_ret_no_self)]
    pub(crate) async fn new<P: AsRef<Path>>(path: P) -> io::Result<Box<dyn CaptureWriter>> {
        let inner = PcapWriter::new(path, DataLinkType::User0).await?;
        Ok(Box::new(Self { inner }))
    }

    /// Encapsulates an AT command or response into a standard 3GPP TS 27.010
    /// MUX frame.
    pub(crate) fn frame_gsm0710(direction: Direction, payload: &[u8]) -> Vec<u8> {
        // In 3GPP TS 27.010 Section 5.2.1.2, C/R bit interpretation depends on role:
        // - Initiator (App) Command: C/R = 1
        // - Responder (Modem) Response: C/R = 1
        // Setting C/R = 1 in both directions ensures Wireshark correctly identifies
        // AT commands and AT responses.
        let cr = 1;
        let addr = (GSM0710_DEFAULT_DLCI << 2) | (cr << 1) | 1;
        let ctrl = GSM0710_CTRL_UIH;
        let len = payload.len();

        // Standard Rust pattern to reference a local temporary array slice safely
        let len_arr;
        let len_bytes: &[u8] = if len < 128 {
            len_arr = [((len as u8) << 1) | 1, 0];
            &len_arr[..1]
        } else {
            len_arr = [((len & 0x7F) as u8) << 1, (len >> 7) as u8];
            &len_arr[..2]
        };

        // Compute FCS over Address, Control, and Length fields using chained iterators
        let fcs = compute_fcs([addr, ctrl].into_iter().chain(len_bytes.iter().copied()));

        // Frame structure:
        // [Ext Header Size (0x00)][Direction (0x00 Tx / 0x01
        // Rx)][0xF9][Addr][Ctrl][Len...][Payload][FCS][0xF9]
        let dir_byte = if matches!(direction, Direction::Received) { 0x00 } else { 0x01 };
        let header = [0x00, dir_byte, GSM0710_FLAG, addr, ctrl];
        let footer = [fcs, GSM0710_FLAG];

        header
            .into_iter()
            .chain(len_bytes.iter().copied())
            .chain(payload.iter().copied())
            .chain(footer)
            .collect()
    }
}

#[async_trait]
impl CaptureWriter for ModemPcapWriter {
    async fn write_packet(
        &mut self,
        timestamp: SystemTime,
        direction: Direction,
        data: &[u8],
    ) -> io::Result<()> {
        let frame = Self::frame_gsm0710(direction, data);
        self.inner.write_packet(timestamp, direction, &frame).await
    }

    async fn flush(&mut self) -> io::Result<()> {
        self.inner.flush().await
    }

    fn get_stats(&self) -> (u64, u64) {
        self.inner.get_stats()
    }
}
