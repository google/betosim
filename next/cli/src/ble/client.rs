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

use std::{
    io::{self, Write},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use netsim_packets::{create_bredr_bb_packet, create_le_ll_packet};

use super::{
    args::{BleCommand, BleScan, BleSniff, LinkTypeOption},
    display::{H4_EVENT_PACKET_TYPE, parse_scan_packet, write_json_event, write_sniff_packet},
};
use crate::{capture_stream::stream_frames, error::Result};

/// `LINKTYPE_BLUETOOTH_HCI_H4_WITH_PHDR` (201) for `netsim ble scan --pcap`.
const LINKTYPE_BLUETOOTH_HCI_H4_WITH_PHDR: u32 = 201;
/// In libpcap DLT 201 (`pcap/bluetooth.h`), `direction = 1` (`0x00000001` in
/// network byte order) marks incoming Controller-to-Host frames (`0` is
/// outgoing Host-to-Controller; `bt_pcap.rs` maps simulator `Direction::Sent`
/// to `1u32.to_be_bytes()`).
const H4_PHDR_RX: [u8; 4] = [0, 0, 0, 1];
const H4_PHDR_RX_WITH_EVENT_TYPE: [u8; 5] = [0, 0, 0, 1, H4_EVENT_PACKET_TYPE];

pub(crate) fn execute(cmd: &BleCommand, http_server: &str, verbose: bool) -> Result<()> {
    match cmd {
        BleCommand::Scan(args) => {
            execute_scan(args, http_server, verbose, io::stdout()).map(|_| ())
        }
        BleCommand::Sniff(args) => {
            eprintln!("Starting BLE sniff at ({}, {}, {})...", args.x, args.y, args.z);
            let out: Box<dyn Write + Send> = match args.pcap.as_deref() {
                Some("-") | None => Box::new(io::stdout()),
                Some(path) => {
                    eprintln!("Writing PCAP output to {path}");
                    Box::new(std::fs::File::create(path)?)
                }
            };
            eprintln!("Press Ctrl-C to stop.");
            execute_sniff(args, http_server, out).map(|_| ())
        }
    }
}

fn execute_scan<W: Write + Send + 'static>(
    args: &BleScan,
    http_server: &str,
    verbose: bool,
    mut out: W,
) -> Result<W> {
    let pcap = args.pcap;
    if pcap {
        write_pcap_header(&mut out, LINKTYPE_BLUETOOTH_HCI_H4_WITH_PHDR)?;
    }
    stream_frames(http_server, &args.query_path(), out, move |w, packet| {
        let ts = now();
        if pcap {
            let prefix: &[u8] = if packet.first() == Some(&H4_EVENT_PACKET_TYPE) {
                &H4_PHDR_RX
            } else {
                &H4_PHDR_RX_WITH_EVENT_TYPE
            };
            return write_pcap_record(w, ts, prefix, packet);
        }
        for event in parse_scan_packet(packet, ts, verbose) {
            write_json_event(w, &event, verbose)?;
        }
        Ok(())
    })
}

fn execute_sniff<W: Write + Send + 'static>(
    args: &BleSniff,
    http_server: &str,
    mut out: W,
) -> Result<W> {
    let is_pcap = args.pcap.is_some();
    let link_type = args.link_type;
    if is_pcap {
        write_pcap_header(&mut out, link_type.link_type_id())?;
    }
    stream_frames(http_server, &args.query_path(), out, move |w, packet| {
        let ts = now();
        if is_pcap {
            let formatted = match link_type {
                LinkTypeOption::Bredr => create_bredr_bb_packet(packet),
                LinkTypeOption::Le => create_le_ll_packet(packet),
            };
            if let Some(pcap_bytes) = formatted {
                write_pcap_record(w, ts, &[], &pcap_bytes)?;
            }
            Ok(())
        } else {
            write_sniff_packet(w, packet, ts)
        }
    })
}

fn now() -> Duration {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default()
}

fn write_pcap_header(w: &mut impl Write, link_type: u32) -> io::Result<()> {
    w.write_all(&0xa1b2c3d4u32.to_ne_bytes())?;
    w.write_all(&2u16.to_ne_bytes())?;
    w.write_all(&4u16.to_ne_bytes())?;
    w.write_all(&0u32.to_ne_bytes())?;
    w.write_all(&0u32.to_ne_bytes())?;
    w.write_all(&65535u32.to_ne_bytes())?;
    w.write_all(&link_type.to_ne_bytes())?;
    w.flush()
}

fn write_pcap_record(
    w: &mut impl Write,
    ts: Duration,
    prefix: &[u8],
    payload: &[u8],
) -> io::Result<()> {
    let len = (prefix.len() + payload.len()) as u32;
    w.write_all(&(ts.as_secs() as u32).to_ne_bytes())?;
    w.write_all(&ts.subsec_micros().to_ne_bytes())?;
    w.write_all(&len.to_ne_bytes())?;
    w.write_all(&len.to_ne_bytes())?;
    w.write_all(prefix)?;
    w.write_all(payload)?;
    w.flush()
}

#[cfg(test)]
mod tests {
    use std::thread;

    use tungstenite::Message;

    use super::*;
    use crate::capture_stream::tests::{accept_upgrade, drain_until_closed, listen};

    const SAMPLE_ADV_REPORT: [u8; 18] = [
        0x04, 0x3e, 15, 0x02, 0x01, 0x00, 0x00, 0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff, 0x03, 0x02,
        0x09, b'A', 0xce,
    ];
    const SAMPLE_LE_PDU: [u8; 19] =
        [0x0b, 1, 2, 3, 4, 5, 6, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x02, 0x01, 0x06];

    fn serve_one(
        expected_path: &'static str,
        payload: &'static [u8],
    ) -> (String, thread::JoinHandle<()>) {
        let (listener, addr) = listen();
        let handle = thread::spawn(move || {
            let mut ws = accept_upgrade(&listener, expected_path);
            ws.send(Message::binary(payload.to_vec())).unwrap();
            ws.close(None).unwrap();
            drain_until_closed(&mut ws);
        });
        (addr, handle)
    }

    #[test]
    fn scan_streams_json_and_pcap() {
        let (addr, server) = serve_one("/v1/ble/scan?x=1&y=2&z=3&active=true", &SAMPLE_ADV_REPORT);
        let args = BleScan { x: 1.0, y: 2.0, z: 3.0, active: true, pcap: false };
        let out = execute_scan(&args, &addr, true, Vec::new()).unwrap();
        server.join().unwrap();
        let line = String::from_utf8(out).unwrap();
        assert!(line.contains("FF:EE:DD:CC:BB:AA (Public)"), "{line}");
        assert!(line.contains("\"rssi\":-50"), "{line}");

        let (addr, server) =
            serve_one("/v1/ble/scan?x=0&y=0&z=0&active=false", &SAMPLE_ADV_REPORT[1..]);
        let args = BleScan { x: 0.0, y: 0.0, z: 0.0, active: false, pcap: true };
        let pcap = execute_scan(&args, &addr, false, Vec::new()).unwrap();
        server.join().unwrap();
        assert_eq!(pcap.len(), 24 + 16 + 4 + SAMPLE_ADV_REPORT.len());
        assert_eq!(u32::from_ne_bytes(pcap[20..24].try_into().unwrap()), 201);
        assert!(pcap.ends_with(&SAMPLE_ADV_REPORT));
    }

    #[test]
    fn sniff_streams_json_and_pcap() {
        let (addr, server) = serve_one("/v1/ble/sniff?x=4&y=5&z=6", &SAMPLE_LE_PDU);
        let args = BleSniff { x: 4.0, y: 5.0, z: 6.0, pcap: None, link_type: LinkTypeOption::Le };
        let out = execute_sniff(&args, &addr, Vec::new()).unwrap();
        server.join().unwrap();
        let line = String::from_utf8(out).unwrap();
        assert!(line.contains("LE_LEGACY_ADVERTISING_PDU"), "{line}");

        let (addr, server) = serve_one("/v1/ble/sniff?x=0&y=0&z=0", &SAMPLE_LE_PDU);
        let args = BleSniff {
            x: 0.0,
            y: 0.0,
            z: 0.0,
            pcap: Some("-".to_string()),
            link_type: LinkTypeOption::Le,
        };
        let pcap = execute_sniff(&args, &addr, Vec::new()).unwrap();
        server.join().unwrap();
        let expected_ll = create_le_ll_packet(&SAMPLE_LE_PDU).unwrap();
        assert_eq!(pcap.len(), 24 + 16 + expected_ll.len());
        assert_eq!(u32::from_ne_bytes(pcap[20..24].try_into().unwrap()), 251);
        assert!(pcap.ends_with(&expected_ll));
    }
}
