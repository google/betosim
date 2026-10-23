// Copyright 2025 The Android Open Source Project
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

//! Capture Actor
//!
//! This crate provides an actor for managing packet captures. It handles
//! starting and stopping captures, writing to PCAP files, and managing
//! capture state for different chips.

mod bt_pcap;
mod capture_actor;
mod error;
mod ethernet_pcap;
mod lifecycle;
mod modem_pcap;
mod nci_pcap;
mod service;
mod uwb_pcap;
mod wifi_pcap;
mod writer;

use actor_framework::ResourceActor;
pub use capture_actor::CaptureActor;
pub use capture_api::CaptureCreate;
pub use error::CaptureError;
pub use writer::DataLinkType;

mod client;
pub use client::CaptureClient;

/// Creates a new Capture actor and its client.
pub fn new() -> (ResourceActor<CaptureActor>, CaptureClient) {
    // Buffer size of 32 is sufficient for capture control commands.
    // Packet data flows through a separate channel if needed, but here we handle
    // control.
    let (runner, client) = ResourceActor::new(32);
    (runner, CaptureClient::new(client))
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        sync::atomic::{AtomicUsize, Ordering},
        time::SystemTime,
    };

    use capture_api::Direction;

    use crate::{
        DataLinkType, bt_pcap::BluetoothH4Writer, modem_pcap::ModemPcapWriter,
        nci_pcap::NciPcapWriter,
    };

    static TEST_COUNTER: AtomicUsize = AtomicUsize::new(0);

    #[tokio::test]
    async fn test_pcap_writer() {
        let id = TEST_COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("netsim_test_pcap_writer_{}", id));
        fs::create_dir_all(&dir).unwrap();
        let filename = dir.join("test_pcap.pcap");
        let mut writer = BluetoothH4Writer::new(&filename).await.unwrap();
        let data = vec![0x01, 0x00, 0x00, 0x00]; // Fake H4 Command
        writer.write_packet(SystemTime::now(), Direction::Sent, &data).await.unwrap();

        let (records, bytes) = writer.get_stats();
        assert_eq!(records, 1);
        assert_eq!(bytes, 8);

        writer.flush().await.unwrap();
        drop(writer);

        let file_bytes = fs::read(&filename).unwrap();
        assert_eq!(&file_bytes[40..44], &[0, 0, 0, 1]);
        assert_eq!(&file_bytes[44..48], &data[..]);

        fs::remove_file(&filename).unwrap();

        let mut writer = BluetoothH4Writer::new(&filename).await.unwrap();
        writer.write_packet(SystemTime::now(), Direction::Received, &data).await.unwrap();
        writer.flush().await.unwrap();
        drop(writer);

        let file_bytes = fs::read(&filename).unwrap();
        assert_eq!(&file_bytes[40..44], &[0, 0, 0, 0]);
        assert_eq!(&file_bytes[44..48], &data[..]);

        fs::remove_file(filename).unwrap();
        fs::remove_dir(dir).unwrap();
    }

    #[tokio::test]
    async fn test_nci_pcap_writer() {
        let id = TEST_COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("netsim_test_nci_pcap_writer_{}", id));
        fs::create_dir_all(&dir).unwrap();
        let filename = dir.join("test_nci_pcap.pcap");
        let mut writer = NciPcapWriter::new(&filename).await.unwrap();
        let data = vec![0x20, 0x00, 0x01, 0x00]; // Fake NCI CORE_RESET_CMD
        writer.write_packet(SystemTime::now(), Direction::Sent, &data).await.unwrap();

        let (records, bytes) = writer.get_stats();
        assert_eq!(records, 1);
        assert_eq!(bytes, 4);

        writer.flush().await.unwrap();
        drop(writer);
        fs::remove_file(filename).unwrap();
        fs::remove_dir(dir).unwrap();
    }

    #[tokio::test]
    async fn test_modem_pcap_writer() {
        let id = TEST_COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("netsim_test_modem_pcap_writer_{}", id));
        fs::create_dir_all(&dir).unwrap();
        let filename = dir.join("test_modem_pcap.pcap");
        let mut writer = ModemPcapWriter::new(&filename).await.unwrap();
        let data = b"AT+CPIN?\r";
        writer.write_packet(SystemTime::now(), Direction::Received, data).await.unwrap();

        let (records, bytes) = writer.get_stats();
        assert_eq!(records, 1);
        assert_eq!(bytes, data.len() as u64 + 8); // 8 bytes 3GPP TS 27.010 framing + payload

        writer.flush().await.unwrap();
        drop(writer);

        // Read and parse the generated binary file
        let pcap_bytes = fs::read(&filename).unwrap();

        // 1. Verify PCAP Global Header (24 bytes)
        assert!(pcap_bytes.len() >= 24);
        let magic = u32::from_le_bytes(pcap_bytes[0..4].try_into().unwrap());
        let version_major = u16::from_le_bytes(pcap_bytes[4..6].try_into().unwrap());
        let version_minor = u16::from_le_bytes(pcap_bytes[6..8].try_into().unwrap());
        let network = u32::from_le_bytes(pcap_bytes[20..24].try_into().unwrap());

        assert_eq!(magic, 0xa1b2c3d4);
        assert_eq!(version_major, 2);
        assert_eq!(version_minor, 4);
        assert_eq!(network, DataLinkType::User0.as_u32());

        // 2. Verify PCAP Record Header (16 bytes starting at index 24)
        assert!(pcap_bytes.len() >= 24 + 16);
        let incl_len = u32::from_le_bytes(pcap_bytes[32..36].try_into().unwrap());
        let orig_len = u32::from_le_bytes(pcap_bytes[36..40].try_into().unwrap());

        assert_eq!(incl_len, 17); // 9 bytes payload + 8 bytes framing
        assert_eq!(orig_len, 17);

        // 3. Verify 3GPP TS 27.010 MUX Frame (17 bytes starting at index 40)
        assert_eq!(pcap_bytes.len(), 24 + 16 + 17);
        let frame = &pcap_bytes[40..];

        let expected_frame = [
            0x00, // Extended header size (0)
            0x00, // Direction: Application -> Module
            0xF9, // Start Flag
            0x07, // Address: DLCI 1, C/R=1, EA=1
            0xEF, // Control: UIH
            0x13, // Length: (9 << 1) | 1
            b'A', b'T', b'+', b'C', b'P', b'I', b'N', b'?', b'\r', // Payload
            0xC8,  // FCS Checksum (correctly calculated CRC-8)
            0xF9,  // Stop Flag
        ];

        assert_eq!(frame, &expected_frame);

        // Cleanup
        fs::remove_file(filename).unwrap();
        fs::remove_dir(dir).unwrap();
    }
}
