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

use std::marker::Unpin;
use std::mem::size_of;
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use zerocopy::{FromBytes, IntoBytes};
use zerocopy_derive::*;

type Result<A> = std::result::Result<A, std::io::Error>;

/// Represents the global header of a pcap capture file.
///
/// This struct defines the global header that appears at the beginning of a
/// pcap capture file. It contains metadata about the capture, such as the
/// file format version, the data link type, and the maximum snapshot length.
///
/// # File Header format
/// ```text
///                         1                   2                   3
///     0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1
///    +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
///  0 |                          Magic Number                         |
///    +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
///  4 |          Major Version        |         Minor Version         |
///    +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
///  8 |                           Reserved1                           |
///    +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
/// 12 |                           Reserved2                           |
///    +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
/// 16 |                            SnapLen                            |
///    +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
/// 20 | FCS |f|0 0 0 0 0 0 0 0 0 0 0 0|         LinkType              |
///    +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
/// ```
///
/// * `magic`: A magic number that identifies the file format.
/// * `version_major`: The major version number of the file format.
/// * `version_minor`: The minor version number of the file format.
/// * `thiszone`: The time zone offset of the capture.
/// * `sigfigs`: The accuracy of the timestamps.
/// * `snaplen`: The maximum number of bytes captured from each packet.
/// * `linktype`: The data link type of the network interface used to capture the packets.
#[derive(IntoBytes, FromBytes, Immutable, Debug, PartialEq, Eq)]
#[repr(C)]
/// Represents the global header of a pcap capture file.
pub struct FileHeader {
    /// Magic number identifying the file format.
    pub magic: u32,
    /// Major version of the pcap format.
    pub version_major: u16,
    /// Minor version of the pcap format.
    pub version_minor: u16,
    /// Time zone offset.
    pub thiszone: i32,
    /// Timestamp accuracy.
    pub sigfigs: u32,
    /// Maximum packet length in bytes.
    pub snaplen: u32,
    /// Data link type of packets.
    pub linktype: u32,
}

impl FileHeader {
    const MAGIC: u32 = 0xa1b2c3d4;
    const VERSION_MAJOR: u16 = 2u16;
    const VERSION_MINOR: u16 = 4u16;
    const RESERVED_1: i32 = 0;
    const RESERVED_2: u32 = 0;
    const SNAP_LEN: u32 = u16::MAX as u32;
}

impl Default for FileHeader {
    fn default() -> Self {
        FileHeader {
            magic: FileHeader::MAGIC,
            version_major: FileHeader::VERSION_MAJOR,
            version_minor: FileHeader::VERSION_MINOR,
            thiszone: FileHeader::RESERVED_1,
            sigfigs: FileHeader::RESERVED_2,
            snaplen: FileHeader::SNAP_LEN,
            linktype: LinkType::Null as u32,
        }
    }
}

/// Represents the link layer header type of a pcap capture.
///
/// This enum defines the different link layer types that can be used in a
/// pcap capture file. These values specify the format of the link-layer
/// header that precedes the network layer (e.g., IP) header in each packet.
///
/// For a complete list of supported link types and their descriptions,
/// refer to the tcpdump documentation:
/// https://www.tcpdump.org/linktypes.html
#[repr(u32)]
#[derive(Debug, PartialEq, Eq, Copy, Clone)]
pub enum LinkType {
    /// Null link type (BSD loopback)
    Null = 0,
    /// Ethernet
    Ethernet = 1,
    /// Radiotap link-layer information followed by an 802.11
    /// header. Radiotap is used with mac80211_hwsim networking.
    Ieee80211RadioTap = 127,
    /// Bluetooth HCI UART transport layer
    BluetoothHciH4WithPhdr = 201,
    /// Ultra-wideband controller interface protocol
    FiraUci = 299,
}

impl From<u32> for LinkType {
    fn from(val: u32) -> Self {
        match val {
            0 => LinkType::Null,
            1 => LinkType::Ethernet,
            127 => LinkType::Ieee80211RadioTap,
            201 => LinkType::BluetoothHciH4WithPhdr,
            299 => LinkType::FiraUci,
            _ => panic!("Invalid link type: {}", val),
        }
    }
}

impl From<LinkType> for u32 {
    fn from(val: LinkType) -> Self {
        val as u32
    }
}

/// Represents the header prepended to each packet in a pcap capture file.
///
/// This struct defines the header that precedes each packet in a pcap
/// capture file. It provides information about the timestamp and length
/// of the captured packet.
///
/// # Fields
/// ```text
///                        1                   2                   3
///    0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1
///    +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
///  0 |                      Timestamp (Seconds)                      |
///    +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
///  4 |            Timestamp (Microseconds or nanoseconds)            |
///    +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
///  8 |                    Captured Packet Length                     |
///    +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
/// 12 |                    Original Packet Length                     |
///    +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
/// 16 /                                                               /
///    /                          Packet Data                          /
///    /                        variable length                        /
///    /                                                               /
///    +---------------------------------------------------------------+
/// ```
///
/// * `tv_sec`:  The seconds component of the timestamp.
/// * `tv_usec`: The microseconds component of the timestamp.
/// * `caplen`: The number of bytes of packet data actually captured and saved in the file.
/// * `len`: The original length of the packet on the network.
//
#[derive(IntoBytes, FromBytes, Immutable, Debug, PartialEq, Eq)]
#[repr(C)]
/// Represents the header prepended to each packet in a pcap capture file.
pub struct PacketHeader {
    /// Timestamp of the captured packet (seconds).
    pub tv_sec: u32,
    /// Timestamp of the captured packet (microseconds).
    pub tv_usec: u32,
    /// Number of bytes captured from the packet.
    pub caplen: u32,
    /// Original length of the packet on the network.
    pub len: u32,
}

/// Reads a pcap file header from the given reader.
///
/// # Arguments
///
/// * `reader` - A reader to read the header from.
///
/// # Returns
///
/// * `Ok(FileHeader)` - If the header was successfully read.
/// * `Err(std::io::Error)` - If an error occurred while reading or parsing the header.
pub async fn read_file_header(mut reader: impl AsyncRead + Unpin) -> Result<FileHeader> {
    let mut header_bytes = [0u8; size_of::<FileHeader>()];
    reader.read_exact(&mut header_bytes).await?;
    let header = FileHeader::read_from_bytes(&header_bytes[..]).map_err(|_| {
        std::io::Error::new(std::io::ErrorKind::InvalidData, "Failed to parse pcap file header")
    })?;
    if header.magic != FileHeader::MAGIC {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("Invalid magic in pcap file 0x{:x}", header.magic),
        ));
    }
    Ok(header)
}

/// Reads a pcap record from the given reader.
/// A record consists of a packet header (`PacketHeader`) and the packet data itself.
///
/// # Arguments
///
/// * `reader` - A reader to read the record from.
///
/// # Returns
///
/// * `Ok((PacketHeader, Vec<u8>))` - If the record was successfully read.
/// * `Err(std::io::Error)` - If an error occurred while reading or parsing the record.
pub async fn read_record(mut reader: impl AsyncRead + Unpin) -> Result<(PacketHeader, Vec<u8>)> {
    let mut pkt_hdr_bytes = [0u8; std::mem::size_of::<PacketHeader>()];
    reader.read_exact(&mut pkt_hdr_bytes).await?;
    let pkt_hdr = PacketHeader::read_from_bytes(&pkt_hdr_bytes[..]).map_err(|_| {
        std::io::Error::new(std::io::ErrorKind::InvalidData, "Failed to parse pcap record header")
    })?;
    let mut packet_data = vec![0u8; pkt_hdr.caplen as usize];
    reader.read_exact(&mut packet_data).await?;
    Ok((pkt_hdr, packet_data))
}

/// Writes the header of a pcap file to the output writer.
///
/// This function writes the global header of a pcap file to the provided
/// asynchronous writer. It returns the size of the header written.
///
/// # Arguments
///
/// * `link_type` - The link type of the network interface used to capture the packets.
/// * `output` - The asynchronous writer to write the header to.
///
/// # Returns
///
/// A `Result` containing the size of the header in bytes on success,
/// or a `std::io::Error` on failure.
pub async fn write_file_header(
    link_type: LinkType,
    mut output: impl AsyncWrite + Unpin,
) -> Result<usize> {
    // https://tools.ietf.org/id/draft-gharris-opsawg-pcap-00.html#name-file-header
    let header = FileHeader { linktype: link_type as u32, ..Default::default() };
    output.write_all(header.as_bytes()).await?;
    Ok(size_of::<FileHeader>())
}

/// Appends a single packet record to the output writer.
///
/// This function writes a packet record to the provided asynchronous writer,
/// including the packet header and the packet data itself. It returns the
/// total number of bytes written to the writer.
///
/// # Arguments
///
/// * `timestamp` - The timestamp of the packet.
/// * `output` - The asynchronous writer to write the record to.
/// * `packet` - The packet data as a byte slice.
///
/// # Returns
///
/// A `Result` containing the total number of bytes written on success,
/// or a `std::io::Error` on failure.
pub async fn write_record(
    timestamp: Duration,
    mut output: impl AsyncWrite + Unpin,
    packet: &[u8],
) -> Result<usize> {
    // https://tools.ietf.org/id/draft-gharris-opsawg-pcap-00.html#name-packet-record
    let pkt_len = packet.len();
    let pkt_hdr_len = size_of::<PacketHeader>();
    let header = PacketHeader {
        tv_sec: timestamp.as_secs() as u32,
        tv_usec: timestamp.subsec_micros(),
        caplen: pkt_len as u32,
        len: pkt_len as u32,
    };
    output.write_all(header.as_bytes()).await?;
    output.write_all(packet).await?;
    output.flush().await?;
    Ok(pkt_hdr_len + pkt_len)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::BufReader;

    #[tokio::test]
    async fn test_read_file_header_success() {
        let file_header = FileHeader {
            magic: FileHeader::MAGIC,
            version_major: FileHeader::VERSION_MAJOR,
            version_minor: FileHeader::VERSION_MINOR,
            thiszone: FileHeader::RESERVED_1,
            sigfigs: FileHeader::RESERVED_2,
            snaplen: FileHeader::SNAP_LEN,
            linktype: LinkType::Null as u32,
        };
        let mut reader = BufReader::new(file_header.as_bytes());
        let result = read_file_header(&mut reader).await;
        assert!(result.is_ok());
        let read_header = result.unwrap();
        assert_eq!(read_header, file_header);
    }

    #[tokio::test]
    async fn test_read_file_header_invalid_magic() {
        let file_header = FileHeader {
            magic: 0, // Invalid magic number
            version_major: FileHeader::VERSION_MAJOR,
            version_minor: FileHeader::VERSION_MINOR,
            thiszone: FileHeader::RESERVED_1,
            sigfigs: FileHeader::RESERVED_2,
            snaplen: FileHeader::SNAP_LEN,
            linktype: LinkType::Null as u32,
        };
        let mut reader = BufReader::new(file_header.as_bytes());
        let result = read_file_header(&mut reader).await;
        assert!(result.is_err());
        let error = result.err().unwrap();
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
    }

    #[tokio::test]
    async fn test_read_record_success() {
        let packet_header = PacketHeader { tv_sec: 1, tv_usec: 2, caplen: 4, len: 4 };
        let packet_data = vec![1, 2, 3, 4];
        let mut record_data = packet_header.as_bytes().to_vec();
        record_data.extend_from_slice(&packet_data);

        let mut reader = BufReader::new(&record_data[..]);
        let result = read_record(&mut reader).await;
        assert!(result.is_ok());
        let (read_header, read_data) = result.unwrap();
        assert_eq!(read_header, packet_header);
        assert_eq!(read_data, packet_data);
    }

    #[tokio::test]
    #[should_panic(expected = "Invalid link type: 999")]
    async fn test_link_type_from_invalid() {
        let _ = LinkType::from(999);
    }
}
