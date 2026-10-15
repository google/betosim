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

use std::io::Write;

use futures_util::StreamExt;
use grpcio::CallOption;
use netsim_packets::{create_bredr_bb_packet, create_le_ll_packet};
use netsim_proto::{ble_service::ScanRequest, ble_service_grpc::BleServiceClient, model::Position};

use super::{
    args::{BleCommand, BleSniff},
    display::{parse_scan_response, print_json_event, print_sniff_response},
};
use crate::error::Result;

pub(crate) fn execute(cmd: &BleCommand, client: &BleServiceClient, verbose: bool) -> Result<()> {
    match cmd {
        BleCommand::Scan(args) => {
            let mut req = ScanRequest::new();
            let mut pos = Position::new();
            pos.x = args.x;
            pos.y = args.y;
            pos.z = args.z;
            req.position = protobuf::MessageField::some(pos);
            req.active = args.active;

            let mut stream = client.scan_opt(&req, CallOption::default())?;

            if args.pcap {
                write_pcap_global_header();
            }

            futures::executor::block_on(async {
                while let Some(result) = stream.next().await {
                    match result {
                        Ok(res) => {
                            if args.pcap {
                                write_pcap_packet(&res.packet);
                                continue;
                            }
                            let events = parse_scan_response(&res, verbose);
                            for event in events {
                                print_json_event(&event, verbose);
                            }
                        }
                        Err(e) => {
                            eprintln!("Scan stream disconnected or encountered an error: {}", e);
                            break;
                        }
                    }
                }
            });
        }
        BleCommand::Sniff(args) => execute_sniff(args, client, verbose)?,
    }
    Ok(())
}

fn execute_sniff(args: &BleSniff, client: &BleServiceClient, verbose: bool) -> Result<()> {
    let mut req = netsim_proto::ble_service::SniffRequest::new();
    let mut pos = Position::new();
    pos.x = args.x;
    pos.y = args.y;
    pos.z = args.z;
    req.position = protobuf::MessageField::some(pos);

    let mut stream = client.sniff_opt(&req, CallOption::default())?;

    eprintln!("Starting BLE sniff at ({}, {}, {})...", args.x, args.y, args.z);

    let mut pcap_file: Option<Box<dyn Write>> = if let Some(path) = &args.pcap {
        let mut file: Box<dyn Write> = if path == "-" {
            Box::new(std::io::stdout())
        } else {
            eprintln!("Writing PCAP output to {}", path);
            Box::new(std::fs::File::create(path)?)
        };
        // Write PCAP Global Header
        // magic_number (4) + version_major (2) + version_minor (2)
        // + thiszone (4) + sigfigs (4) + snaplen (4) + network/linktype (4)
        file.write_all(&0xa1b2c3d4u32.to_ne_bytes())?;
        file.write_all(&2u16.to_ne_bytes())?; // major
        file.write_all(&4u16.to_ne_bytes())?; // minor
        file.write_all(&0u32.to_ne_bytes())?; // timezone
        file.write_all(&0u32.to_ne_bytes())?; // sigfigs
        file.write_all(&65535u32.to_ne_bytes())?; // snaplen
        // 255 = LINKTYPE_BLUETOOTH_BREDR_BB, 251 = LINKTYPE_BLUETOOTH_LE_LL
        let link_type_val = match args.link_type {
            crate::ble::args::LinkTypeOption::Bredr => 255u32,
            crate::ble::args::LinkTypeOption::Le => 251u32,
        };
        file.write_all(&link_type_val.to_ne_bytes())?;
        file.flush()?;
        Some(file)
    } else {
        None
    };

    eprintln!("Press Ctrl-C to stop.");

    futures::executor::block_on(async {
        while let Some(result) = stream.next().await {
            match result {
                Ok(res) => {
                    let packet_data = &res.packet;
                    if let Some(file) = &mut pcap_file {
                        let formatted_opt = match args.link_type {
                            crate::ble::args::LinkTypeOption::Bredr => {
                                create_bredr_bb_packet(packet_data)
                            }
                            crate::ble::args::LinkTypeOption::Le => {
                                create_le_ll_packet(packet_data)
                            }
                        };

                        if let Some(pcap_bytes) = formatted_opt {
                            let duration = std::time::Duration::from_micros(res.timestamp as u64);
                            let ts_sec = duration.as_secs() as u32;
                            let ts_usec = duration.subsec_micros();
                            let incl_len = pcap_bytes.len() as u32;
                            let orig_len = pcap_bytes.len() as u32;

                            if let Err(e) = file
                                .write_all(&ts_sec.to_ne_bytes())
                                .and_then(|_| file.write_all(&ts_usec.to_ne_bytes()))
                                .and_then(|_| file.write_all(&incl_len.to_ne_bytes()))
                                .and_then(|_| file.write_all(&orig_len.to_ne_bytes()))
                                .and_then(|_| file.write_all(&pcap_bytes))
                                .and_then(|_| file.flush())
                            {
                                eprintln!("PCAP output write failed: {}", e);
                                break;
                            }
                        }
                    } else {
                        print_sniff_response(&res, verbose);
                    }
                }
                Err(e) => {
                    eprintln!("Sniff stream disconnected or encountered an error: {}", e);
                    break;
                }
            }
        }
    });

    Ok(())
}

fn write_pcap_global_header() {
    let mut file = std::io::stdout();
    let _ = file.write_all(&0xa1b2c3d4u32.to_ne_bytes());
    let _ = file.write_all(&2u16.to_ne_bytes()); // major
    let _ = file.write_all(&4u16.to_ne_bytes()); // minor
    let _ = file.write_all(&0u32.to_ne_bytes()); // timezone
    let _ = file.write_all(&0u32.to_ne_bytes()); // sigfigs
    let _ = file.write_all(&65535u32.to_ne_bytes()); // snaplen
    let _ = file.write_all(&201u32.to_ne_bytes()); // LINKTYPE_BLUETOOTH_HCI_H4_WITH_PH
    let _ = file.flush();
}

fn write_pcap_packet(packet: &[u8]) {
    let mut file = std::io::stdout();
    let now =
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    let ts_sec = now.as_secs() as u32;
    let ts_usec = now.subsec_micros();

    let dir = [0u8, 0, 0, 1]; // Received by host

    let mut h4_packet = Vec::new();
    if packet.first() != Some(&0x04) {
        h4_packet.push(0x04);
    }
    h4_packet.extend_from_slice(packet);

    let incl_len = (dir.len() + h4_packet.len()) as u32;
    let orig_len = incl_len;

    let _ = file.write_all(&ts_sec.to_ne_bytes());
    let _ = file.write_all(&ts_usec.to_ne_bytes());
    let _ = file.write_all(&incl_len.to_ne_bytes());
    let _ = file.write_all(&orig_len.to_ne_bytes());
    let _ = file.write_all(&dir);
    let _ = file.write_all(&h4_packet);
    let _ = file.flush();
}
