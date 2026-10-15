// Copyright 2024 The Android Open Source Project
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

use capture::pcap;
use std::io::Cursor;
use tokio::io::{AsyncSeekExt, BufReader};

fn timestamp(hdr: pcap::PacketHeader) -> f64 {
    hdr.tv_sec as f64 + (hdr.tv_usec as f64 / 1_000_000.0)
}

// Read a file with a known number of records.
//
// Test magic numbers, record len, and timestamp fields
#[tokio::test]
async fn read_file() -> Result<(), std::io::Error> {
    const DATA: &[u8] = include_bytes!("../data/dns.cap");
    const RECORDS: i32 = 38;
    let mut reader = BufReader::new(Cursor::new(DATA));
    let header = pcap::read_file_header(&mut reader).await?;
    assert_eq!(header.linktype, pcap::LinkType::Ethernet.into());
    assert_eq!(header.snaplen, u16::MAX as u32);
    let mut records = 0;
    loop {
        match pcap::read_record(&mut reader).await {
            Ok((hdr, _record)) => {
                records += 1;
                if records == 1 {
                    assert_eq!(1112172466.496046000f64, timestamp(hdr));
                } else if records == 38 {
                    assert_eq!(1112172745.375359000f64, timestamp(hdr));
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                assert_eq!(records, RECORDS);
                assert_eq!(DATA.len() as u64, reader.stream_position().await?);
                break;
            }
            Err(e) => {
                return Err(e);
            }
        }
    }

    Ok(())
}

#[tokio::test]
async fn write_file() -> Result<(), std::io::Error> {
    let mut writer = tokio::io::BufWriter::new(Vec::new());
    let link_type = pcap::LinkType::Ethernet;
    pcap::write_file_header(link_type, &mut writer).await?;
    let packet = [1, 2, 3, 4];
    let timestamp = std::time::Duration::new(1, 0);
    pcap::write_record(timestamp, &mut writer, &packet).await?;
    let written_data = writer.into_inner();
    let mut reader = BufReader::new(Cursor::new(written_data));
    let _ = pcap::read_file_header(&mut reader).await?;
    let (_, record_data) = pcap::read_record(&mut reader).await?;
    assert_eq!(record_data, packet);
    Ok(())
}
