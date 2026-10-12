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

/// This module parses DNS response records and extracts fully
/// qualified domain names (FQDNs) along with their corresponding
/// IP Addresses (IpAddr).
///
/// **Note:** This is not a general-purpose DNS response parser. It is
/// designed to handle specific record types and response formats.
use std::convert::TryFrom;
use std::{
    io::{Cursor, Read, Seek, SeekFrom},
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    str,
};

// REGION CURSOR

/// Extension trait providing convenient methods for reading primitive
/// data types used by DNS messages from a `Cursor<&[u8]>`.
trait CursorExt: Read + Seek + Clone {
    fn read_u8(&mut self) -> std::io::Result<u8>;
    fn read_u16(&mut self) -> std::io::Result<u16>;
    fn read_u32(&mut self) -> std::io::Result<u32>;
    fn read_ipv4addr(&mut self) -> std::io::Result<Ipv4Addr>;
    fn read_ipv6addr(&mut self) -> std::io::Result<Ipv6Addr>;
    fn get_ref(&self) -> &[u8];
    fn position(&self) -> u64;
    fn set_position(&mut self, pos: u64);
}

impl CursorExt for Cursor<&[u8]> {
    fn read_u8(&mut self) -> std::io::Result<u8> {
        let mut buf = [0; 1];
        self.read_exact(&mut buf)?;
        let [byte] = buf;
        Ok(byte)
    }

    fn read_u16(&mut self) -> std::io::Result<u16> {
        let mut buf = [0; 2];
        self.read_exact(&mut buf)?;
        Ok(u16::from_be_bytes(buf))
    }

    fn read_u32(&mut self) -> std::io::Result<u32> {
        let mut buf = [0; 4];
        self.read_exact(&mut buf)?;
        Ok(u32::from_be_bytes(buf))
    }

    fn read_ipv4addr(&mut self) -> std::io::Result<Ipv4Addr> {
        let mut buf = [0; 4];
        self.read_exact(&mut buf)?;
        Ok(Ipv4Addr::from(buf))
    }

    fn read_ipv6addr(&mut self) -> std::io::Result<Ipv6Addr> {
        let mut buf = [0; 16];
        self.read_exact(&mut buf)?;
        Ok(Ipv6Addr::from(buf))
    }

    fn get_ref(&self) -> &[u8] {
        self.get_ref() // Call the original get_ref method
    }
    fn position(&self) -> u64 {
        self.position()
    }
    fn set_position(&mut self, pos: u64) {
        self.set_position(pos)
    }
}

// END REGION CURSOR

// REGION MESSAGE

/// ```text
///  +---------------------+
///  |        Header       |
///  +---------------------+
///  |       Question      | the question for the name server
///  +---------------------+
///  |        Answer       | RRs answering the question
///  +---------------------+
///  |      Authority      | RRs pointing toward an authority
///  +---------------------+
///  |      Additional     | RRs holding additional information
///  +---------------------+
/// ```
#[derive(Debug, PartialEq, Eq)]
struct Message {
    answers: Vec<(IpAddr, String)>,
}

impl Message {
    fn parse(cursor: &mut impl CursorExt) -> Result<Message> {
        let header = Header::parse(cursor)?;

        // Reject DNS messages that are not responses
        if !header.response {
            return Err(DnsError::ResponseExpected);
        }
        if header.opcode != Opcode::StandardQuery {
            return Err(DnsError::StandardQueryExpected);
        }
        if header.response_code != ResponseCode::NoError {
            return Err(DnsError::ResponseCodeExpected);
        }

        if header.answer_count == 0 {
            return Err(DnsError::AnswerExpected);
        }

        for _i in 0..header.question_count {
            let _question = Question::split_once(cursor)?;
        }
        let mut answers = Vec::with_capacity(header.answer_count);
        for _i in 0..header.answer_count {
            let answer = ResourceRecord::split_once(cursor)?;
            answers.push((answer.resource_data.into(), answer.name));
        }
        Ok(Message { answers })
    }
}

pub(crate) fn parse_answers(bytes: &[u8]) -> Result<Vec<(IpAddr, String)>> {
    let mut cursor = Cursor::new(bytes);
    Message::parse(&mut cursor).map(|msg| msg.answers)
}

// END REGION MESSAGE

// REGION HEADER

/// Represents parsed header of the packet.
/// The header contains the following fields:
/// ```text
///                                  1  1  1  1  1  1
///    0  1  2  3  4  5  6  7  8  9  0  1  2  3  4  5
///  +--+--+--+--+--+--+--+--+--+--+--+--+--+--+--+--+
///  |                      ID                       |
///  +--+--+--+--+--+--+--+--+--+--+--+--+--+--+--+--+
///  |QR|   Opcode  |AA|TC|RD|RA|   Z    |   RCODE   |
///  +--+--+--+--+--+--+--+--+--+--+--+--+--+--+--+--+
///  |                    QDCOUNT                    |
///  +--+--+--+--+--+--+--+--+--+--+--+--+--+--+--+--+
///  |                    ANCOUNT                    |
///  +--+--+--+--+--+--+--+--+--+--+--+--+--+--+--+--+
///  |                    NSCOUNT                    |
///  +--+--+--+--+--+--+--+--+--+--+--+--+--+--+--+--+
///  |                    ARCOUNT                    |
///  +--+--+--+--+--+--+--+--+--+--+--+--+--+--+--+--+
/// ```
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
struct Header {
    /// A 16 bit identifier assigned by the program that
    /// generates any kind of query.  This identifier is copied
    /// the corresponding reply and can be used by the requester
    /// to match up replies to outstanding queries.
    id: u16,
    /// A one bit field that specifies whether this message is a
    /// query (0), or a response (1).
    response: bool,
    /// A four bit field that specifies kind of query in this
    /// message.  This value is set by the originator of a query
    /// and copied into the response.
    opcode: Opcode,
    response_code: ResponseCode,
    question_count: usize,
    answer_count: usize,
    nameserver_count: usize,
    additional_count: usize,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum Opcode {
    /// Normal query
    StandardQuery = 0,
    /// Inverse query (query a name by IP)
    InverseQuery = 1,
    /// Server status request
    ServerStatusRequest = 2,
}

/// The RCODE value according to RFC 1035
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum ResponseCode {
    NoError,
    FormatError,
    ServerFailure,
    NameError,
    NotImplemented,
    Refused,
}

impl TryFrom<u16> for ResponseCode {
    type Error = DnsError;

    fn try_from(value: u16) -> Result<Self> {
        match value {
            0 => Ok(ResponseCode::NoError),
            1 => Ok(ResponseCode::FormatError),
            2 => Ok(ResponseCode::ServerFailure),
            3 => Ok(ResponseCode::NameError),
            4 => Ok(ResponseCode::NotImplemented),
            5 => Ok(ResponseCode::Refused),
            _ => Err(DnsError::InvalidResponseCode(value)),
        }
    }
}

impl TryFrom<u16> for Opcode {
    type Error = DnsError;

    fn try_from(value: u16) -> Result<Self> {
        match value {
            0 => Ok(Opcode::StandardQuery),
            1 => Ok(Opcode::InverseQuery),
            2 => Ok(Opcode::ServerStatusRequest),
            _ => Err(DnsError::InvalidOpcode(value)),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Flag(u16);

impl Flag {
    const RESPONSE: u16 = 0x8000;
    const OPCODE_MASK: u16 = 0x7800;
    const RESERVED_MASK: u16 = 0x0004;
    const RESPONSE_CODE_MASK: u16 = 0x000F;

    fn new(value: u16) -> Self {
        Self(value)
    }

    fn is_set(&self, mask: u16) -> bool {
        (self.0 & mask) == mask
    }

    fn get(&self, mask: u16) -> u16 {
        (self.0 & mask) >> mask.trailing_zeros()
    }
}

impl Header {
    /// Parse the header into a header structure
    fn parse(cursor: &mut impl CursorExt) -> Result<Header> {
        let id = cursor.read_u16()?;
        let f = cursor.read_u16()?;
        let question_count = cursor.read_u16()? as usize;
        let answer_count = cursor.read_u16()? as usize;
        let nameserver_count = cursor.read_u16()? as usize;
        let additional_count = cursor.read_u16()? as usize;
        let flags = Flag::new(f);
        if flags.get(Flag::RESERVED_MASK) != 0 {
            return Err(DnsError::ReservedBitsAreNonZero);
        }
        let header = Header {
            id,
            response: flags.is_set(Flag::RESPONSE),
            opcode: Opcode::try_from(flags.get(Flag::OPCODE_MASK))?,
            response_code: ResponseCode::try_from(flags.get(Flag::RESPONSE_CODE_MASK))?,
            question_count,
            answer_count,
            nameserver_count,
            additional_count,
        };
        Ok(header)
    }
}

// END REGION HEADER

// REGION QUESTION

/// 4.1.2. Question section format
///
/// The question section is used to carry the "question" in most queries,
/// i.e., the parameters that define what is being asked.  The section
/// contains QDCOUNT (usually 1) entries, each of the following format:
/// ```text
///                               1  1  1  1  1  1
/// 0  1  2  3  4  5  6  7  8  9  0  1  2  3  4  5
/// +--+--+--+--+--+--+--+--+--+--+--+--+--+--+--+--+
/// |                                               |
/// /                     QNAME                     /
/// /                                               /
/// +--+--+--+--+--+--+--+--+--+--+--+--+--+--+--+--+
/// |                     QTYPE                     |
/// +--+--+--+--+--+--+--+--+--+--+--+--+--+--+--+--+
/// |                     QCLASS                    |
/// +--+--+--+--+--+--+--+--+--+--+--+--+--+--+--+--+
/// ```
#[derive(Debug)]
struct Question {}

impl Question {
    fn split_once(cursor: &mut impl CursorExt) -> Result<Question> {
        let _name = Name::to_string(cursor)?;
        let _qtype = cursor.read_u16()?;
        let _qclass = cursor.read_u16()?;
        Ok(Question {})
    }
}

// END REGION QUESTION

// REGION RESOURCE RECORD

// DNS resource record classes.
//
// The only one we care about is Internet
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
#[repr(u16)]
enum ResourceClass {
    Internet = 1,
}

impl TryFrom<u16> for ResourceClass {
    type Error = DnsError;

    fn try_from(value: u16) -> Result<Self> {
        match value {
            1 => Ok(ResourceClass::Internet),
            _ => Err(DnsError::InvalidResourceClass),
        }
    }
}

// Type fields in resource records.
//
// The only ones we care about are A and AAAA
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
#[repr(u16)]
enum ResourceType {
    // IPv4 address.
    A = 1,
    // IPv6 address, see RFC 3596.
    Aaaa = 28,
}

impl TryFrom<u16> for ResourceType {
    type Error = DnsError;

    fn try_from(value: u16) -> Result<Self> {
        match value {
            1 => Ok(ResourceType::A),
            28 => Ok(ResourceType::Aaaa),
            _ => Err(DnsError::InvalidResourceType),
        }
    }
}

/// All RRs have the same top level format shown below:
///
/// ```text
///                                1  1  1  1  1  1
///  0  1  2  3  4  5  6  7  8  9  0  1  2  3  4  5
///  +--+--+--+--+--+--+--+--+--+--+--+--+--+--+--+--+
///  |                                               |
///  /                                               /
///  /                      NAME                     /
///  |                                               |
///  +--+--+--+--+--+--+--+--+--+--+--+--+--+--+--+--+
///  |                      TYPE                     |
///  +--+--+--+--+--+--+--+--+--+--+--+--+--+--+--+--+
///  |                     CLASS                     |
///  +--+--+--+--+--+--+--+--+--+--+--+--+--+--+--+--+
///  |                      TTL                      |
///  |                                               |
///  +--+--+--+--+--+--+--+--+--+--+--+--+--+--+--+--+
///  |                   RDLENGTH                    |
///  +--+--+--+--+--+--+--+--+--+--+--+--+--+--+--+--|
///  /                     RDATA                     /
///  /                                               /
///  +--+--+--+--+--+--+--+--+--+--+--+--+--+--+--+--+
/// ```
#[derive(Debug)]
struct ResourceRecord {
    name: String,
    resource_data: ResourceData,
}

impl ResourceRecord {
    fn split_once(cursor: &mut impl CursorExt) -> Result<ResourceRecord> {
        let name = Name::to_string(cursor)?;
        let rtype = cursor.read_u16()?;
        let resource_type = ResourceType::try_from(rtype)?;
        let rclass = cursor.read_u16()?;
        let _resource_class = ResourceClass::try_from(rclass)?;
        let _ttl = cursor.read_u32()?;
        let rdlength = u64::from(cursor.read_u16()?);
        let pos_before = cursor.position();
        let resource_data = ResourceData::split_once(cursor, &resource_type)?;
        let bytes_consumed = cursor.position() - pos_before;
        if bytes_consumed > rdlength {
            return Err(DnsError::InvalidLength);
        }
        if bytes_consumed < rdlength {
            cursor.seek(SeekFrom::Current(
                i64::try_from(rdlength - bytes_consumed).unwrap_or(i64::MAX),
            ))?;
        }
        Ok(ResourceRecord { name, resource_data })
    }
}

// Only interested in IpAddr resource data
#[derive(Debug, PartialEq)]
struct ResourceData(IpAddr);

impl From<ResourceData> for IpAddr {
    fn from(resource_data: ResourceData) -> Self {
        resource_data.0
    }
}

impl ResourceData {
    fn split_once(
        cursor: &mut impl CursorExt,
        resource_type: &ResourceType,
    ) -> Result<ResourceData> {
        match resource_type {
            ResourceType::A => Ok(ResourceData(cursor.read_ipv4addr()?.into())),
            ResourceType::Aaaa => Ok(ResourceData(cursor.read_ipv6addr()?.into())),
        }
    }
}

// END REGION RESOURCE RECORD

// REGION LABEL

type Result<T> = core::result::Result<T, DnsError>;

#[derive(Debug, thiserror::Error)]
pub(crate) enum DnsError {
    #[error("Response expected")]
    ResponseExpected,
    #[error("Standard query expected")]
    StandardQueryExpected,
    #[error("Response code expected")]
    ResponseCodeExpected,
    #[error("Answer expected")]
    AnswerExpected,
    #[error("Pointer loop detected")]
    PointerLoop,
    #[error("Invalid length")]
    InvalidLength,
    #[error("UTF-8 error: {0}")]
    Utf8Error(#[from] std::str::Utf8Error),
    #[error("Invalid resource type")]
    InvalidResourceType,
    #[error("Invalid resource class")]
    InvalidResourceClass,
    #[error("Address parse error: {0}")]
    AddrParseError(#[from] std::net::AddrParseError),
    #[error("Invalid opcode: {0}")]
    InvalidOpcode(u16),
    #[error("Invalid response code: {0}")]
    InvalidResponseCode(u16),
    #[error("Reserved bits are non-zero")]
    ReservedBitsAreNonZero,
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
}

// REGION NAME

/// RFC 1035 4.1.4. Message compression
///
/// In order to reduce the size of messages, the domain system
/// utilizes a compression scheme which eliminates the repetition of
/// domain names in a message.  In this scheme, an entire domain name
/// or a list of labels at the end of a domain name is replaced with a
/// pointer to a prior occurrence of the same name.
///
/// The pointer takes the form of a two octet sequence:
///
/// ```text
///  +--+--+--+--+--+--+--+--+--+--+--+--+--+--+--+--+
///  | 1  1|                OFFSET                   |
///  +--+--+--+--+--+--+--+--+--+--+--+--+--+--+--+--+
/// ```
enum NamePart {
    Label(String),
    Pointer(u64),
    Root,
}

const PTR_MASK: u8 = 0b1100_0000;
const MAX_LABEL_LENGTH: u8 = 63;
const MAX_DOMAIN_NAME_LENGTH: usize = 255;
const MAX_POINTER_JUMPS: usize = 2;

impl NamePart {
    fn split_once(cursor: &mut impl CursorExt) -> Result<NamePart> {
        let size = cursor.read_u8()?;
        if size & PTR_MASK == PTR_MASK {
            let second_byte = cursor.read_u8()?;
            let offset: u64 = u16::from_be_bytes([size & !PTR_MASK, second_byte]).into();
            return Ok(NamePart::Pointer(offset));
        }
        if size == 0 {
            return Ok(NamePart::Root);
        }
        if size > MAX_LABEL_LENGTH {
            return Err(DnsError::InvalidLength);
        }
        let end = size as usize;
        let buffer_ref: &[u8] = cursor.get_ref();
        let start = cursor.position() as usize;
        let label_bytes = buffer_ref.get(start..start + end).ok_or(DnsError::InvalidLength)?;
        let label = str::from_utf8(label_bytes)?.to_string();
        cursor.seek(SeekFrom::Current(i64::from(size)))?;
        Ok(NamePart::Label(label))
    }
}

/// The Fully Qualified Domain Name from ANSWER and RR records
struct Name();

impl Name {
    // Convert a variable length QNAME or NAME to a String.
    //
    // The cursor is updated to the end of the first sequence of
    // labels, and not the position after a Pointer. This allows the
    // cursor to be used for reading the remainder of the Question or
    // ResourceRecord.
    //
    // Limit the number of Pointers in malificient messages to avoid
    // looping.
    //
    fn to_string(cursor: &mut impl CursorExt) -> Result<String> {
        Self::to_string_guard(cursor, 0)
    }

    fn to_string_guard(cursor: &mut impl CursorExt, jumps: usize) -> Result<String> {
        if jumps > MAX_POINTER_JUMPS {
            return Err(DnsError::PointerLoop);
        }
        let mut name = String::with_capacity(MAX_DOMAIN_NAME_LENGTH);
        loop {
            match NamePart::split_once(cursor)? {
                NamePart::Root => return Ok(name),
                NamePart::Pointer(offset) => {
                    let mut pointer_cursor = cursor.clone();
                    pointer_cursor.set_position(offset);
                    let pointer_name = Name::to_string_guard(&mut pointer_cursor, jumps + 1)?;
                    let has_separator = !name.is_empty() && !pointer_name.is_empty();
                    let needed =
                        if has_separator { pointer_name.len() + 1 } else { pointer_name.len() };
                    if name.len() + needed > MAX_DOMAIN_NAME_LENGTH {
                        return Err(DnsError::InvalidLength);
                    }
                    if has_separator {
                        name.push('.');
                    }
                    name.push_str(&pointer_name);
                    return Ok(name);
                }
                NamePart::Label(label) => {
                    let needed = if name.is_empty() { label.len() } else { label.len() + 1 };
                    if name.len() + needed > MAX_DOMAIN_NAME_LENGTH {
                        return Err(DnsError::InvalidLength);
                    }
                    if !name.is_empty() {
                        name.push('.');
                    }
                    name.push_str(&label);
                }
            };
        }
    }
}

// END REGION NAME

#[cfg(test)]
mod test_message {
    use std::str::FromStr;

    use super::*;

    #[test]
    fn test_dns_responses() -> Result<()> {
        let bytes: [u8; 81] = [
            0xc2, 0x87, 0x81, 0x80, 0x0, 0x1, 0x0, 0x2, 0x0, 0x0, 0x0, 0x0, 0x3, 0x69, 0x62, 0x6d,
            0x3, 0x63, 0x6f, 0x6d, 0x0, 0x0, 0x1c, 0x0, 0x1, 0xc0, 0xc, 0x0, 0x1c, 0x0, 0x1, 0x0,
            0x0, 0x0, 0x8, 0x0, 0x10, 0x26, 0x0, 0x14, 0x6, 0x5e, 0x0, 0x2, 0x93, 0x0, 0x0, 0x0,
            0x0, 0x0, 0x0, 0x38, 0x31, 0xc0, 0xc, 0x0, 0x1c, 0x0, 0x1, 0x0, 0x0, 0x0, 0x8, 0x0,
            0x10, 0x26, 0x0, 0x14, 0x6, 0x5e, 0x0, 0x2, 0xaa, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x38,
            0x31,
        ];
        let bytes: &[u8] = &bytes;
        let answers = parse_answers(bytes)?;
        assert_eq!(
            *answers.first().unwrap(),
            (Ipv6Addr::from_str("2600:1406:5e00:293::3831")?.into(), "ibm.com".to_string())
        );
        assert_eq!(
            *answers.get(1).unwrap(),
            (Ipv6Addr::from_str("2600:1406:5e00:2aa::3831")?.into(), "ibm.com".to_string())
        );
        Ok(())
    }

    #[test]
    fn test_resource_type_try_from() {
        assert_eq!(ResourceType::try_from(1).unwrap(), ResourceType::A);
        assert_eq!(ResourceType::try_from(28).unwrap(), ResourceType::Aaaa);
        assert!(ResourceType::try_from(99).is_err());
    }

    #[test]
    fn test_resource_class_try_from() {
        assert_eq!(ResourceClass::try_from(1).unwrap(), ResourceClass::Internet);
        assert!(ResourceClass::try_from(99).is_err());
    }

    #[test]
    fn test_name_pointer_with_prefix_label() -> Result<()> {
        // Wire bytes containing:
        // offset 0..13: \x07example\x03com\x00 ("example.com")
        // offset 13..19: \x03sub\xc0\x00 ("sub" followed by pointer to offset 0)
        let bytes: [u8; 19] = [
            0x07, b'e', b'x', b'a', b'm', b'p', b'l', b'e', 0x03, b'c', b'o', b'm', 0x00, 0x03,
            b's', b'u', b'b', 0xc0, 0x00,
        ];
        let mut cursor = Cursor::new(&bytes[..]);
        cursor.set_position(13);
        let name = Name::to_string(&mut cursor)?;
        assert_eq!(name, "sub.example.com");
        Ok(())
    }

    #[test]
    fn test_name_pointer_to_root() -> Result<()> {
        // Wire bytes containing:
        // offset 0: 0x00 (root)
        // offset 1..6: 0x03, "sub", 0xc0, 0x00 ("sub" followed by pointer to root at offset 0)
        let bytes = [0x00, 0x03, b's', b'u', b'b', 0xc0, 0x00];
        let mut cursor = Cursor::new(&bytes[..]);
        cursor.set_position(1);
        let name = Name::to_string(&mut cursor)?;
        assert_eq!(name, "sub");
        Ok(())
    }
}
