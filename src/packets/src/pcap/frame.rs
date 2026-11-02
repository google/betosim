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

use std::io::{self, Read};

use zerocopy::{
    FromBytes, Immutable, IntoBytes, KnownLayout, U16, U32, Unaligned, byteorder::LittleEndian,
};

use crate::pcap::ng::{EnhancedPacketBlock, InterfaceDescriptionBlock, SectionHeaderBlock};

pub const PCAP_MAGIC_NUMBER: u32 = 0xa1b2c3d4;
#[cfg(test)]
pub const PCAPNG_MAGIC_NUMBER: u32 = 0x1A2B3C4D;
pub const PCAPNG_MAGIC_NUMBER_SWAPPED: u32 = 0x4D3C2B1A;
pub const PCAPNG_SECTION_HEADER_BLOCK_TYPE: u32 = 0x0A0D0D0A;
pub const PCAPNG_ENHANCED_PACKET_BLOCK_TYPE: u32 = 0x00000006;
pub const PCAPNG_INTERFACE_DESCRIPTION_BLOCK_TYPE: u32 = 0x00000001;

pub const LINKTYPE_ETHERNET: u32 = 1;
pub const LINKTYPE_IEEE802_11: u32 = 105;
pub const LINKTYPE_RADIOTAP: u32 = 127;
pub const LINKTYPE_NETLINK: u32 = 253; // Netlink capture (Linux cooked socket?) No, 253 is Netlink.

pub enum PcapReader<R: Read> {
    Pcap(LegacyPcapReader<R>),
    Pcapng(PcapngReader<R>),
}

impl<R: Read + 'static> PcapReader<R> {
    pub fn new(mut reader: R) -> io::Result<PcapReader<Box<dyn Read>>> {
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
            Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("Invalid pcap magic number: {:#x}", magic_number),
            ))
        }
    }

    pub fn next_record(&mut self) -> io::Result<Option<(PcapRecordHeader, Vec<u8>)>> {
        match self {
            PcapReader::Pcap(reader) => reader.next_record(),
            PcapReader::Pcapng(reader) => reader.next_record(),
        }
    }

    pub fn link_type(&self) -> Option<u32> {
        match self {
            PcapReader::Pcap(reader) => Some(reader.header.network.get()),
            PcapReader::Pcapng(reader) => reader.link_type,
        }
    }
}

#[derive(FromBytes, IntoBytes, Unaligned, KnownLayout, Immutable, Debug, Clone, Copy)]
#[repr(C)]
pub struct PcapHeader {
    pub magic_number: U32<LittleEndian>,
    pub version_major: U16<LittleEndian>,
    pub version_minor: U16<LittleEndian>,
    pub thiszone: zerocopy::I32<LittleEndian>,
    pub sigfigs: U32<LittleEndian>,
    pub snaplen: U32<LittleEndian>,
    pub network: U32<LittleEndian>,
}

#[derive(
    FromBytes, IntoBytes, Unaligned, KnownLayout, Immutable, Debug, Clone, Copy, PartialEq, Eq,
)]
#[repr(C)]
pub struct PcapRecordHeader {
    pub ts_sec: U32<LittleEndian>,
    pub ts_usec: U32<LittleEndian>,
    pub incl_len: U32<LittleEndian>,
    pub orig_len: U32<LittleEndian>,
}

pub struct LegacyPcapReader<R: Read> {
    reader: R,
    pub header: PcapHeader,
}

impl<R: Read> LegacyPcapReader<R> {
    pub fn new(mut reader: R) -> io::Result<Self> {
        let mut header_buf = [0; std::mem::size_of::<PcapHeader>()];
        reader.read_exact(&mut header_buf)?;
        let header = PcapHeader::read_from_bytes(header_buf.as_slice())
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
        // The magic number check is already done in PcapReader::new
        Ok(Self { reader, header })
    }

    pub fn next_record(&mut self) -> io::Result<Option<(PcapRecordHeader, Vec<u8>)>> {
        let mut header_buf = [0; std::mem::size_of::<PcapRecordHeader>()];
        if self.reader.read_exact(&mut header_buf).is_err() {
            return Ok(None);
        }
        let header = PcapRecordHeader::read_from_bytes(header_buf.as_slice())
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
        let mut data = vec![0; header.incl_len.get() as usize];
        self.reader.read_exact(&mut data)?;
        Ok(Some((header, data)))
    }
}

pub struct PcapngReader<R: Read> {
    reader: R,
    swap_bytes: bool,
    pub link_type: Option<u32>,
}

impl<R: Read> PcapngReader<R> {
    pub fn new(mut reader: R) -> io::Result<Self> {
        let mut shb_buf = [0; std::mem::size_of::<SectionHeaderBlock>()];
        reader.read_exact(&mut shb_buf)?;
        let shb = SectionHeaderBlock::read_from_bytes(shb_buf.as_slice())
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
        let swap_bytes = shb.byte_order_magic.get() == PCAPNG_MAGIC_NUMBER_SWAPPED;
        let block_total_length = if swap_bytes {
            shb.block_total_length.get().swap_bytes()
        } else {
            shb.block_total_length.get()
        };
        let options_len = block_total_length as usize - std::mem::size_of::<SectionHeaderBlock>();
        io::copy(&mut reader.by_ref().take(options_len as u64), &mut io::sink())?;
        Ok(Self { reader, swap_bytes, link_type: None })
    }

    pub fn next_record(&mut self) -> io::Result<Option<(PcapRecordHeader, Vec<u8>)>> {
        loop {
            // We need to read the Block Type (4 bytes) and Block Total Length (4 bytes)
            // first to know what kind of block it is and how big it is.
            // But EnhancedPacketBlock and InterfaceDescriptionBlock start with these same
            // fields. So we can peek or read a common header.
            // However, zerocopy structs are fixed size.
            // Let's read enough for the smallest block header we care about?
            // Or just read 8 bytes first?

            let mut block_header_buf = [0; 8];
            if self.reader.read_exact(&mut block_header_buf).is_err() {
                return Ok(None);
            }

            let [b0, b1, b2, b3, b4, b5, b6, b7] = block_header_buf;
            let block_type_raw = u32::from_le_bytes([b0, b1, b2, b3]);
            let block_len_raw = u32::from_le_bytes([b4, b5, b6, b7]);

            let block_type =
                if self.swap_bytes { block_type_raw.swap_bytes() } else { block_type_raw };
            let block_total_length =
                if self.swap_bytes { block_len_raw.swap_bytes() } else { block_len_raw };

            if block_type == PCAPNG_ENHANCED_PACKET_BLOCK_TYPE {
                // It's an EPB. We already read 8 bytes.
                // We need to read the rest of EPB struct minus 8 bytes.
                let epb_size = std::mem::size_of::<EnhancedPacketBlock>();
                let mut rest_of_epb = vec![0; epb_size - 8];
                self.reader.read_exact(&mut rest_of_epb)?;

                // Reconstruct full buffer to parse (or just parse fields manually, but using
                // zerocopy is safer)
                let full_epb_buf = [&block_header_buf[..], &rest_of_epb].concat();

                let epb = EnhancedPacketBlock::read_from_bytes(full_epb_buf.as_slice())
                    .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;

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

                let remaining_len = block_total_length as usize - epb_size - captured_len as usize;

                if remaining_len > 0 {
                    io::copy(
                        &mut self.reader.by_ref().take(remaining_len as u64),
                        &mut io::sink(),
                    )?;
                }

                let record_header = PcapRecordHeader {
                    ts_sec: U32::new(if self.swap_bytes {
                        epb.timestamp_high.get().swap_bytes()
                    } else {
                        epb.timestamp_high.get()
                    }),
                    ts_usec: U32::new(if self.swap_bytes {
                        epb.timestamp_low.get().swap_bytes()
                    } else {
                        epb.timestamp_low.get()
                    }),
                    incl_len: U32::new(captured_len),
                    orig_len: U32::new(packet_len),
                };
                return Ok(Some((record_header, data)));
            } else if block_type == PCAPNG_INTERFACE_DESCRIPTION_BLOCK_TYPE {
                // It's an IDB. Read it to get LinkType.
                let idb_size = std::mem::size_of::<InterfaceDescriptionBlock>();
                let mut rest_of_idb = vec![0; idb_size - 8];
                self.reader.read_exact(&mut rest_of_idb)?;

                let full_idb_buf = [&block_header_buf[..], &rest_of_idb].concat();

                let idb = InterfaceDescriptionBlock::read_from_bytes(full_idb_buf.as_slice())
                    .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
                let link_type = if self.swap_bytes {
                    idb.link_type.get().swap_bytes()
                } else {
                    idb.link_type.get()
                };

                self.link_type = Some(link_type as u32);

                let remaining_len = block_total_length as usize - idb_size;
                io::copy(&mut self.reader.by_ref().take(remaining_len as u64), &mut io::sink())?;
            } else {
                // Skip other blocks
                let remaining_len = block_total_length as usize - 8;
                io::copy(&mut self.reader.by_ref().take(remaining_len as u64), &mut io::sink())?;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use zerocopy::{I32, U16, U32};

    use super::*;

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

        let val_u32 = |v: u32| U32::new(if swapped { v.swap_bytes() } else { v });
        let val_u16 = |v: u16| U16::new(if swapped { v.swap_bytes() } else { v });
        let val_u64 = |v: u64| zerocopy::U64::new(if swapped { v.swap_bytes() } else { v });

        let shb = SectionHeaderBlock {
            block_type: val_u32(PCAPNG_SECTION_HEADER_BLOCK_TYPE),
            block_total_length: val_u32(28),
            byte_order_magic: U32::new(magic), // Magic is already swapped if needed
            major_version: val_u16(1),
            minor_version: val_u16(0),
            section_length: val_u64(u64::MAX),
        };
        data.extend_from_slice(shb.as_bytes());
        data.extend_from_slice(shb.block_total_length.as_bytes());

        let epb = EnhancedPacketBlock {
            block_type: val_u32(PCAPNG_ENHANCED_PACKET_BLOCK_TYPE),
            block_total_length: val_u32(32 + 4), // 32 for block, 4 for data
            interface_id: val_u32(0),
            timestamp_high: val_u32(100),
            timestamp_low: val_u32(200),
            captured_len: val_u32(4),
            packet_len: val_u32(4),
        };
        data.extend_from_slice(epb.as_bytes());
        data.extend_from_slice(&[1, 2, 3, 4]); // Packet data
        data.extend_from_slice(epb.block_total_length.as_bytes());

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
        let swapped_data = create_pcapng_data(true);
        let mut reader = PcapReader::new(io::Cursor::new(swapped_data)).unwrap();
        let (record_header, data) = reader.next_record().unwrap().unwrap();

        assert_eq!(record_header.ts_sec.get(), 100);
        assert_eq!(record_header.ts_usec.get(), 200);
        assert_eq!(record_header.incl_len.get(), 4);
        assert_eq!(record_header.orig_len.get(), 4);
        assert_eq!(data, &[1, 2, 3, 4]);
    }
}
