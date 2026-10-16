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
use bytes::Bytes;
use capture_api::Direction;
use tokio::sync::mpsc;

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

    pub(crate) fn new_stream() -> (Box<dyn CaptureWriter>, mpsc::Receiver<Bytes>) {
        let (inner, rx) = PcapWriter::new_stream(DataLinkType::User0);
        (Box::new(Self { inner }), rx)
    }

    /// Encapsulates an AT command or response into a standard 3GPP TS 27.010
    /// MUX frame.
    fn frame_gsm0710(direction: Direction, payload: &[u8]) -> Vec<u8> {
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
        // [Ext Header Size (0x00)][Direction (0x00 App->Module (Received) / 0x01
        // Module->App (Sent))][0xF9][Addr][Ctrl][Len...][Payload][FCS][0xF9]
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

    fn is_file(&self) -> bool {
        self.inner.is_file()
    }

    fn is_closed(&self) -> bool {
        self.inner.is_closed()
    }
}

#[cfg(test)]
mod tests {
    use netsim_packets::{PcapHeader, PcapRecordHeader};

    use super::*;

    const PCAP_GLOBAL_HEADER_SIZE: usize = size_of::<PcapHeader>();
    const PCAP_RECORD_HEADER_SIZE: usize = size_of::<PcapRecordHeader>();

    fn check_frame<'a>(frame: &'a [u8], direction: Direction, payload: &[u8]) -> &'a [u8] {
        let dir_byte = if matches!(direction, Direction::Received) { 0x00 } else { 0x01 };
        let len_field_size = if payload.len() <= 127 { 1 } else { 2 };
        assert_eq!(frame.len(), 5 + len_field_size + payload.len() + 2);
        assert_eq!(&frame[0..5], &[0x00, dir_byte, GSM0710_FLAG, 0x07, 0xEF]);
        let len_bytes = &frame[5..5 + len_field_size];
        assert_eq!(&frame[5 + len_field_size..5 + len_field_size + payload.len()], payload);
        let expected_fcs = compute_fcs([0x07, 0xEF].into_iter().chain(len_bytes.iter().copied()));
        assert_eq!(frame[frame.len() - 2], expected_fcs);
        assert_eq!(frame[frame.len() - 1], GSM0710_FLAG);
        len_bytes
    }

    #[test]
    fn test_compute_fcs() {
        let input = [0x07, 0xEF, 0x13];
        let fcs = compute_fcs(input);
        assert_eq!(fcs, 0xC8);
    }

    #[test]
    fn test_frame_gsm0710_empty_payload() {
        let frame_rx = ModemPcapWriter::frame_gsm0710(Direction::Received, &[]);
        assert_eq!(check_frame(&frame_rx, Direction::Received, &[]), &[0x01]);

        let frame_tx = ModemPcapWriter::frame_gsm0710(Direction::Sent, &[]);
        assert_eq!(check_frame(&frame_tx, Direction::Sent, &[]), &[0x01]);
    }

    #[test]
    fn test_frame_gsm0710_short_payload() {
        let payload = b"AT+CSQ\r";
        let frame = ModemPcapWriter::frame_gsm0710(Direction::Sent, payload);
        assert_eq!(check_frame(&frame, Direction::Sent, payload), &[0x0F]);
    }

    #[test]
    fn test_frame_gsm0710_length_boundary_127_bytes() {
        let payload = vec![0x41; 127];
        let frame = ModemPcapWriter::frame_gsm0710(Direction::Received, &payload);
        assert_eq!(check_frame(&frame, Direction::Received, &payload), &[0xFF]);
    }

    #[test]
    fn test_frame_gsm0710_long_payload_two_byte_length() {
        let payload = vec![0x42; 128];
        let frame = ModemPcapWriter::frame_gsm0710(Direction::Received, &payload);
        assert_eq!(check_frame(&frame, Direction::Received, &payload), &[0x00, 0x01]);
    }

    #[test]
    fn test_frame_gsm0710_large_multibyte_payload() {
        let payload = vec![0x43; 300];
        let frame = ModemPcapWriter::frame_gsm0710(Direction::Sent, &payload);
        assert_eq!(check_frame(&frame, Direction::Sent, &payload), &[0x58, 0x02]);
    }

    #[tokio::test]
    async fn test_modem_pcap_writer_stream_and_write_packet() {
        let (mut writer, mut rx) = ModemPcapWriter::new_stream();
        assert!(!writer.is_file());
        assert!(!writer.is_closed());
        assert_eq!(writer.get_stats(), (0, 0));

        let payload = b"AT\r";
        let expected_frame = ModemPcapWriter::frame_gsm0710(Direction::Sent, payload);
        assert_eq!(expected_frame.len(), 11);

        writer.write_packet(SystemTime::now(), Direction::Sent, payload).await.unwrap();
        assert_eq!(writer.get_stats(), (1, 11));

        let header_bytes = rx.recv().await.unwrap();
        assert_eq!(header_bytes.len(), PCAP_GLOBAL_HEADER_SIZE);

        let record_bytes = rx.recv().await.unwrap();
        assert_eq!(record_bytes.len(), PCAP_RECORD_HEADER_SIZE + 11);
        assert_eq!(&record_bytes[PCAP_RECORD_HEADER_SIZE..], &expected_frame[..]);

        assert!(writer.flush().await.is_ok());

        drop(rx);
        assert!(writer.is_closed());
    }
}
