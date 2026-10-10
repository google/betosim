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
use std::time::Instant;
use tokio;
use tokio::io::BufReader;
use tokio::runtime::Runtime;

async fn dns_benchmark() {
    const DATA: &[u8] = include_bytes!("../../capture/data/dns.cap");

    let mut reader = BufReader::new(Cursor::new(DATA));
    let header = pcap::read_file_header(&mut reader).await.unwrap();
    assert_eq!(header.linktype, pcap::LinkType::Ethernet.into());
    let mut dns_manager = http_proxy::DnsManager::new();
    loop {
        match pcap::read_record(&mut reader).await {
            Ok((_hdr, record)) => {
                dns_manager.add_from_ethernet_slice(&record);
            }
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                break;
            }
            Err(e) => {
                println!("Error: {:?}", e);
                assert!(false);
            }
        }
    }
}

fn main() {
    let iterations = 50_000;
    let rt = Runtime::new().unwrap();
    let handle = rt.handle();
    for _ in 0..5 {
        let time_start = Instant::now();
        for _ in 0..iterations {
            handle.block_on(dns_benchmark());
        }
        let elapsed_time = time_start.elapsed();
        println!(
            "** Time per iteration {}us",
            (elapsed_time.as_micros() as f64) / (iterations as f64)
        );
    }
}
