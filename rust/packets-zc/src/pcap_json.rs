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

use crate::pcap::{PcapHeader, PcapRecordHeader};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct PcapHeaderJson {
    magic_number: String,
    version_major: u16,
    version_minor: u16,
    thiszone: i32,
    sigfigs: u32,
    snaplen: u32,
    network: u32,
}

impl From<PcapHeader> for PcapHeaderJson {
    fn from(header: PcapHeader) -> Self {
        Self {
            magic_number: format!("{:#x}", header.magic_number.get()),
            version_major: header.version_major.get(),
            version_minor: header.version_minor.get(),
            thiszone: header.thiszone.get(),
            sigfigs: header.sigfigs.get(),
            snaplen: header.snaplen.get(),
            network: header.network.get(),
        }
    }
}

#[derive(Serialize, Deserialize)]
pub struct PcapRecordHeaderJson {
    ts_sec: u32,
    ts_usec: u32,
    incl_len: u32,
    orig_len: u32,
}

impl From<PcapRecordHeader> for PcapRecordHeaderJson {
    fn from(header: PcapRecordHeader) -> Self {
        Self {
            ts_sec: header.ts_sec.get(),
            ts_usec: header.ts_usec.get(),
            incl_len: header.incl_len.get(),
            orig_len: header.orig_len.get(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zerocopy::byteorder::LittleEndian;
    use zerocopy::{U16, U32};

    #[test]
    fn test_pcap_header_from() {
        let header = PcapHeader {
            magic_number: U32::<LittleEndian>::new(0xa1b2c3d4),
            version_major: U16::<LittleEndian>::new(2),
            version_minor: U16::<LittleEndian>::new(4),
            thiszone: zerocopy::I32::<LittleEndian>::new(0),
            sigfigs: U32::<LittleEndian>::new(0),
            snaplen: U32::<LittleEndian>::new(65535),
            network: U32::<LittleEndian>::new(1),
        };
        let json: PcapHeaderJson = header.into();
        assert_eq!(json.magic_number, "0xa1b2c3d4");
        assert_eq!(json.version_major, 2);
        assert_eq!(json.version_minor, 4);
        assert_eq!(json.snaplen, 65535);
        assert_eq!(json.network, 1);
    }

    #[test]
    fn test_pcap_record_header_from() {
        let header = PcapRecordHeader {
            ts_sec: U32::<LittleEndian>::new(1234567890),
            ts_usec: U32::<LittleEndian>::new(123456),
            incl_len: U32::<LittleEndian>::new(100),
            orig_len: U32::<LittleEndian>::new(120),
        };
        let json: PcapRecordHeaderJson = header.into();
        assert_eq!(json.ts_sec, 1234567890);
        assert_eq!(json.ts_usec, 123456);
        assert_eq!(json.incl_len, 100);
        assert_eq!(json.orig_len, 120);
    }
}
