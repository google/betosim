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

//! A minimal pcap parser.
//!
//! Note that the support for pcap format is very limited.
//! It only supports ethernet packets.

use crate::pcapng::{EnhancedPacketBlock, SectionHeaderBlock};
use std::io::{self, Read};
use zerocopy::{FromBytes, IntoBytes, byteorder::little_endian};

pub const PCAP_MAGIC_NUMBER: u32 = 0xa1b2c3d4;
pub const PCAPNG_MAGIC_NUMBER: u32 = 0x1A2B3C4D;
pub const PCAPNG_MAGIC_NUMBER_SWAPPED: u32 = 0x4D3C2B1A;
pub const PCAPNG_SECTION_HEADER_BLOCK_TYPE: u32 = 0x0A0D0D0A;
pub const PCAPNG_ENHANCED_PACKET_BLOCK_TYPE: u32 = 0x00000006;

pub enum PcapReader<R: Read> {
    Pcap(LegacyPcapReader<R>),
    Pcapng(PcapngReader<R>),
}

impl<R: Read + 'static> PcapReader<R> {
    pub fn new(mut reader: R) -> anyhow::Result<PcapReader<Box<dyn Read>>> {
        let mut magic_buf = [0; 4];
        reader.read_exact(&mut magic_buf)?;
        let magic_number = u32::from_le_bytes(magic_buf);
        let remaining_reader = io::Cursor::new(magic_buf).chain(reader);
        let boxed_reader: Box<dyn Read> = Box::new(remaining_reader);

        if magic_number == PCAP_MAGIC_NUMBER {
            Ok(PcapReader::Pcap(LegacyPcapReader::new(boxed_reader)?))
        } else if magic_number == PCAPNG_SECTION_HEADER_BLOCK_TYPE {
            Ok(PcapReader::Pcapng(PcapngReader::new(boxed_reader)?))
        } else {
            anyhow::bail!("Invalid pcap magic number: {:#x}", magic_number);
        }
    }

    pub fn next_record(&mut self) -> anyhow::Result<Option<(PcapRecordHeader, Vec<u8>)>> {
        match self {
            PcapReader::Pcap(reader) => reader.next_record(),
            PcapReader::Pcapng(reader) => reader.next_record(),
        }
    }
}

#[repr(C)]
#[derive(IntoBytes, FromBytes, Copy, Clone, Debug)]
pub struct PcapHeader {
    pub magic_number: little_endian::U32,
    pub version_major: little_endian::U16,
    pub version_minor: little_endian::U16,
    pub thiszone: little_endian::I32,
    pub sigfigs: little_endian::U32,
    pub snaplen: little_endian::U32,
    pub network: little_endian::U32,
}

#[repr(C)]
#[derive(IntoBytes, FromBytes, Copy, Clone, Debug, PartialEq, Eq)]
pub struct PcapRecordHeader {
    pub ts_sec: little_endian::U32,
    pub ts_usec: little_endian::U32,
    pub incl_len: little_endian::U32,
    pub orig_len: little_endian::U32,
}

pub struct LegacyPcapReader<R: Read> {
    reader: R,
    pub header: PcapHeader,
}

impl<R: Read> LegacyPcapReader<R> {
    pub fn new(mut reader: R) -> anyhow::Result<Self> {
        let mut header_buf = [0; std::mem::size_of::<PcapHeader>()];
        reader.read_exact(&mut header_buf)?;
        let header = PcapHeader::read_from_bytes(&header_buf[..]).unwrap();
        // The magic number check is already done in PcapReader::new
        Ok(Self { reader, header })
    }

    pub fn next_record(&mut self) -> anyhow::Result<Option<(PcapRecordHeader, Vec<u8>)>> {
        let mut header_buf = [0; std::mem::size_of::<PcapRecordHeader>()];
        if self.reader.read_exact(&mut header_buf).is_err() {
            return Ok(None);
        }
        let header = PcapRecordHeader::read_from_bytes(&header_buf[..]).unwrap();
        let mut data = vec![0; header.incl_len.get() as usize];
        self.reader.read_exact(&mut data)?;
        Ok(Some((header, data)))
    }
}

pub struct PcapngReader<R: Read> {
    reader: R,
    swap_bytes: bool,
}

impl<R: Read> PcapngReader<R> {
    pub fn new(mut reader: R) -> anyhow::Result<Self> {
        let mut shb_buf = [0; std::mem::size_of::<SectionHeaderBlock>()];
        reader.read_exact(&mut shb_buf)?;
        let shb = SectionHeaderBlock::read_from_bytes(&shb_buf[..]).unwrap();
        let swap_bytes = shb.byte_order_magic.get() == PCAPNG_MAGIC_NUMBER_SWAPPED;
        let block_total_length = if swap_bytes {
            shb.block_total_length.get().swap_bytes()
        } else {
            shb.block_total_length.get()
        };
        let options_len = block_total_length as usize - std::mem::size_of::<SectionHeaderBlock>();
        io::copy(&mut reader.by_ref().take(options_len as u64), &mut io::sink())?;
        Ok(Self { reader, swap_bytes })
    }

    pub fn next_record(&mut self) -> anyhow::Result<Option<(PcapRecordHeader, Vec<u8>)>> {
        loop {
            let mut epb_buf = [0; std::mem::size_of::<EnhancedPacketBlock>()];
            if self.reader.read_exact(&mut epb_buf).is_err() {
                return Ok(None);
            }
            let epb = EnhancedPacketBlock::read_from_bytes(&epb_buf[..]).unwrap();
            let block_type = if self.swap_bytes {
                epb.block_type.get().swap_bytes()
            } else {
                epb.block_type.get()
            };

            if block_type == PCAPNG_ENHANCED_PACKET_BLOCK_TYPE {
                let block_total_length = if self.swap_bytes {
                    epb.block_total_length.get().swap_bytes()
                } else {
                    epb.block_total_length.get()
                };
                let captured_len = if self.swap_bytes {
                    epb.captured_len.get().swap_bytes()
                } else {
                    epb.captured_len.get()
                };
                let packet_len = if self.swap_bytes {
                    epb.packet_len.get().swap_bytes()
                } else {
                    epb.packet_len.get()
                };
                let mut data = vec![0; captured_len as usize];
                self.reader.read_exact(&mut data)?;
                let remaining_len = block_total_length as usize
                    - std::mem::size_of::<EnhancedPacketBlock>()
                    - captured_len as usize;
                if remaining_len > 0 {
                    io::copy(
                        &mut self.reader.by_ref().take(remaining_len as u64),
                        &mut io::sink(),
                    )?;
                }
                let record_header = PcapRecordHeader {
                    ts_sec: little_endian::U32::new(if self.swap_bytes {
                        epb.timestamp_high.get().swap_bytes()
                    } else {
                        epb.timestamp_high.get()
                    }),
                    ts_usec: little_endian::U32::new(if self.swap_bytes {
                        epb.timestamp_low.get().swap_bytes()
                    } else {
                        epb.timestamp_low.get()
                    }),
                    incl_len: little_endian::U32::new(captured_len),
                    orig_len: little_endian::U32::new(packet_len),
                };
                return Ok(Some((record_header, data)));
            } else {
                let block_total_length = if self.swap_bytes {
                    epb.block_total_length.get().swap_bytes()
                } else {
                    epb.block_total_length.get()
                };
                let remaining_len =
                    block_total_length as usize - std::mem::size_of::<EnhancedPacketBlock>();
                io::copy(&mut self.reader.by_ref().take(remaining_len as u64), &mut io::sink())?;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zerocopy::I32;
    use zerocopy::byteorder::{LittleEndian, U16, U32};

    fn create_legacy_pcap_data() -> Vec<u8> {
        let mut data = Vec::new();
        let pcap_header = PcapHeader {
            magic_number: U32::new(PCAP_MAGIC_NUMBER),
            version_major: U16::new(2),
            version_minor: U16::new(4),
            thiszone: I32::new(0),
            sigfigs: U32::new(0),
            snaplen: U32::new(65535),
            network: U32::new(1), // LINKTYPE_ETHERNET
        };
        data.extend_from_slice(pcap_header.as_bytes());

        let record_header = PcapRecordHeader {
            ts_sec: U32::new(100),
            ts_usec: U32::new(200),
            incl_len: U32::new(4),
            orig_len: U32::new(4),
        };
        data.extend_from_slice(record_header.as_bytes());
        data.extend_from_slice(&[1, 2, 3, 4]); // Packet data

        data
    }

    fn create_pcapng_data(swapped: bool) -> Vec<u8> {
        let mut data = Vec::new();
        let magic = if swapped { PCAPNG_MAGIC_NUMBER_SWAPPED } else { PCAPNG_MAGIC_NUMBER };
        let shb = SectionHeaderBlock {
            block_type: U32::new(PCAPNG_SECTION_HEADER_BLOCK_TYPE),
            block_total_length: U32::new(28),
            byte_order_magic: U32::new(magic),
            major_version: U16::new(1),
            minor_version: U16::new(0),
            section_length: zerocopy::U64::new(u64::MAX),
        };
        data.extend_from_slice(shb.as_bytes());

        let epb = EnhancedPacketBlock {
            block_type: U32::new(PCAPNG_ENHANCED_PACKET_BLOCK_TYPE),
            block_total_length: U32::new(32 + 4), // 32 for block, 4 for data
            interface_id: U32::new(0),
            timestamp_high: U32::new(100),
            timestamp_low: U32::new(200),
            captured_len: U32::new(4),
            packet_len: U32::new(4),
        };
        data.extend_from_slice(epb.as_bytes());
        data.extend_from_slice(&[1, 2, 3, 4]); // Packet data

        data
    }

    #[test]
    fn test_pcap_reader_new_legacy() {
        let pcap_data = create_legacy_pcap_data();
        let reader = PcapReader::new(io::Cursor::new(pcap_data)).unwrap();
        assert!(matches!(reader, PcapReader::Pcap(_)));
    }

    #[test]
    fn test_pcap_reader_new_pcapng() {
        let pcapng_data = create_pcapng_data(false);
        let reader = PcapReader::new(io::Cursor::new(pcapng_data)).unwrap();
        assert!(matches!(reader, PcapReader::Pcapng(_)));
    }

    #[test]
    fn test_pcap_reader_new_invalid() {
        let invalid_data = vec![0, 1, 2, 3, 4, 5, 6, 7];
        let result = PcapReader::new(io::Cursor::new(invalid_data));
        assert!(result.is_err());
    }

    #[test]
    fn test_legacy_pcap_reader_next_record() {
        let pcap_data = create_legacy_pcap_data();
        let mut reader = PcapReader::new(io::Cursor::new(pcap_data)).unwrap();
        let (record_header, data) = reader.next_record().unwrap().unwrap();

        assert_eq!(record_header.ts_sec.get(), 100);
        assert_eq!(record_header.ts_usec.get(), 200);
        assert_eq!(record_header.incl_len.get(), 4);
        assert_eq!(record_header.orig_len.get(), 4);
        assert_eq!(data, &[1, 2, 3, 4]);

        // No more records
        assert!(reader.next_record().unwrap().is_none());
    }

    #[test]
    fn test_pcapng_reader_next_record() {
        let pcapng_data = create_pcapng_data(false);
        let mut reader = PcapReader::new(io::Cursor::new(pcapng_data)).unwrap();
        let (record_header, data) = reader.next_record().unwrap().unwrap();

        assert_eq!(record_header.ts_sec.get(), 100);
        assert_eq!(record_header.ts_usec.get(), 200);
        assert_eq!(record_header.incl_len.get(), 4);
        assert_eq!(record_header.orig_len.get(), 4);
        assert_eq!(data, &[1, 2, 3, 4]);

        // No more records
        assert!(reader.next_record().unwrap().is_none());
    }

    #[test]
    fn test_pcapng_reader_next_record_swapped() {
        let pcapng_data = create_pcapng_data(true);
        // Manually swap bytes for the test data
        let mut swapped_data = Vec::new();
        let shb = SectionHeaderBlock {
            block_type: U32::new(PCAPNG_SECTION_HEADER_BLOCK_TYPE.swap_bytes()),
            block_total_length: U32::new(28u32.swap_bytes()),
            byte_order_magic: U32::new(PCAPNG_MAGIC_NUMBER_SWAPPED.swap_bytes()),
            major_version: U16::new(1u16.swap_bytes()),
            minor_version: U16::new(0u16.swap_bytes()),
            section_length: zerocopy::U64::new(u64::MAX.swap_bytes()),
        };
        swapped_data.extend_from_slice(shb.as_bytes());

        let epb = EnhancedPacketBlock {
            block_type: U32::new(PCAPNG_ENHANCED_PACKET_BLOCK_TYPE.swap_bytes()),
            block_total_length: U32::new((32 + 4u32).swap_bytes()),
            interface_id: U32::new(0u32.swap_bytes()),
            timestamp_high: U32::new(100u32.swap_bytes()),
            timestamp_low: U32::new(200u32.swap_bytes()),
            captured_len: U32::new(4u32.swap_bytes()),
            packet_len: U32::new(4u32.swap_bytes()),
        };
        swapped_data.extend_from_slice(epb.as_bytes());
        swapped_data.extend_from_slice(&[1, 2, 3, 4]);

        let mut reader = PcapReader::new(io::Cursor::new(swapped_data)).unwrap();
        let (record_header, data) = reader.next_record().unwrap().unwrap();

        assert_eq!(record_header.ts_sec.get(), 100);
        assert_eq!(record_header.ts_usec.get(), 200);
        assert_eq!(record_header.incl_len.get(), 4);
        assert_eq!(record_header.orig_len.get(), 4);
        assert_eq!(data, &[1, 2, 3, 4]);
    }
}
