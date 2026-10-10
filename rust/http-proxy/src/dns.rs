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
use std::fmt;
use std::io::{Cursor, Read, Seek, SeekFrom};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::str;
#[allow(unused_imports)]
use std::str::FromStr;

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
        Ok(buf[0])
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

/// '''
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
/// '''

#[derive(Debug)]
struct Message {
    #[allow(dead_code)]
    header: Header,
    #[allow(dead_code)]
    questions: Vec<Question>,
    answers: Vec<ResourceRecord>,
    // Other types not needed
    // Authority
    // Additional
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

        let mut questions = Vec::with_capacity(header.question_count);
        for _i in 0..header.question_count {
            let question = Question::split_once(cursor)?;
            questions.push(question);
        }
        let mut answers = Vec::with_capacity(header.answer_count);
        for _i in 0..header.answer_count {
            let answer = ResourceRecord::split_once(cursor)?;
            answers.push(answer);
        }
        Ok(Message { header, questions, answers })
    }
}

pub fn parse_answers(bytes: &[u8]) -> Result<Vec<(IpAddr, String)>> {
    let mut cursor = Cursor::new(bytes);
    let msg = Message::parse(&mut cursor)?;
    let mut responses = Vec::with_capacity(msg.answers.len());
    for answer in msg.answers {
        responses.push((answer.resource_data.into(), answer.name));
    }
    Ok(responses)
}

// END REGION MESSAGE

// REGION HEADER

/// Represents parsed header of the packet.
/// The header contains the following fields:
/// '''
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
/// '''
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
/// '''
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
/// '''

#[derive(Debug)]
struct Question {
    #[allow(dead_code)]
    name: String,
    #[allow(dead_code)]
    qtype: u16,
    #[allow(dead_code)]
    qclass: u16,
}

impl Question {
    fn split_once(cursor: &mut impl CursorExt) -> Result<Question> {
        let name = Name::to_string(cursor)?;
        let qtype = cursor.read_u16()?;
        let qclass = cursor.read_u16()?;
        Ok(Question { name, qtype, qclass })
    }
}

// END REGION QUESTION

// REGION RESOURCE RECORD

/// All RRs have the same top level format shown below:
///
/// '''
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
/// '''

// DNS resource record classes.
//
// The only one we care about is Internet
#[derive(Debug)]
enum ResourceClass {
    Internet = 1,
}

// Type fields in resource records.
//
// The only ones we care about are A and AAAA
#[derive(Debug)]
enum ResourceType {
    // IPv4 address.
    A = 1,
    // IPv6 address, see RFC 3596.
    Aaaa = 28,
}

#[derive(Debug)]
struct ResourceRecord {
    name: String,
    #[allow(dead_code)]
    resource_type: ResourceType,
    #[allow(dead_code)]
    resource_class: ResourceClass,
    #[allow(dead_code)]
    ttl: u32,
    resource_data: ResourceData,
}

impl ResourceRecord {
    fn split_once(cursor: &mut impl CursorExt) -> Result<ResourceRecord> {
        let name = Name::to_string(cursor)?;
        let rtype = cursor.read_u16()?;
        let resource_type = match rtype {
            x if x == ResourceType::A as u16 => ResourceType::A,
            x if x == ResourceType::Aaaa as u16 => ResourceType::Aaaa,
            _ => return Err(DnsError::InvalidResourceType),
        };
        let rclass = cursor.read_u16()?;
        let resource_class = match rclass {
            x if x == ResourceClass::Internet as u16 => ResourceClass::Internet,
            _ => return Err(DnsError::InvalidResourceClass),
        };
        let ttl = cursor.read_u32()?;
        let _ = cursor.read_u16()?;
        let resource_data = ResourceData::split_once(cursor, &resource_type)?;
        Ok(ResourceRecord { name, resource_type, resource_class, ttl, resource_data })
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

#[derive(Debug)]
pub enum DnsError {
    ResponseExpected,
    StandardQueryExpected,
    ResponseCodeExpected,
    AnswerExpected,
    PointerLoop,
    InvalidLength,
    Utf8Error(str::Utf8Error),
    InvalidResourceType,
    InvalidResourceClass,
    AddrParseError(std::net::AddrParseError),
    InvalidOpcode(u16),
    InvalidResponseCode(u16),
    ReservedBitsAreNonZero,
    IoError(std::io::Error),
}

impl std::error::Error for DnsError {}

impl fmt::Display for DnsError {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(fmt, "{self:?}")
    }
}

impl From<std::io::Error> for DnsError {
    fn from(err: std::io::Error) -> Self {
        DnsError::IoError(err)
    }
}
impl From<str::Utf8Error> for DnsError {
    fn from(err: str::Utf8Error) -> Self {
        DnsError::Utf8Error(err)
    }
}

impl From<std::net::AddrParseError> for DnsError {
    fn from(err: std::net::AddrParseError) -> Self {
        DnsError::AddrParseError(err)
    }
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
/// '''
///  +--+--+--+--+--+--+--+--+--+--+--+--+--+--+--+--+
///  | 1  1|                OFFSET                   |
///  +--+--+--+--+--+--+--+--+--+--+--+--+--+--+--+--+
/// '''

enum NamePart {
    Label(String),
    Pointer(u64),
    Root,
}

const PTR_MASK: u8 = 0b11000000;

impl NamePart {
    /// Domain name labels have a maximum length of 63 octets.
    const MAX: u8 = 63;

    #[allow(dead_code)]
    fn split_once(cursor: &mut impl CursorExt) -> Result<NamePart> {
        let size = cursor.read_u8()?;
        if size & PTR_MASK == PTR_MASK {
            let two = cursor.read_u8()?;
            let offset: u64 = u16::from_be_bytes([size & !PTR_MASK, two]).into();
            return Ok(NamePart::Pointer(offset));
        }
        if size == 0 {
            return Ok(NamePart::Root);
        }
        if size > Self::MAX {
            return Err(DnsError::InvalidLength);
        }
        let end = size as usize;
        let buffer_ref: &[u8] = cursor.get_ref();
        let start = cursor.position() as usize;
        let label = str::from_utf8(&buffer_ref[start..start + end])?.to_string();
        cursor.seek(SeekFrom::Current(end as i64))?;
        Ok(NamePart::Label(label))
    }
}

/// The Fully Qualitifed Domain Name from ANSWER and RR records

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
        if jumps > 2 {
            return Err(DnsError::PointerLoop);
        }
        let mut name = String::with_capacity(255);
        loop {
            match NamePart::split_once(cursor)? {
                NamePart::Root => return Ok(name),
                NamePart::Pointer(offset) => {
                    let mut pointer_cursor = cursor.clone();
                    pointer_cursor.set_position(offset);
                    let pointer_name = Name::to_string_guard(&mut pointer_cursor, jumps + 1)?;
                    name.push_str(&pointer_name);
                    return Ok(name);
                }
                NamePart::Label(label) => {
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
            *answers.get(0).unwrap(),
            (Ipv6Addr::from_str("2600:1406:5e00:293::3831")?.into(), "ibm.com".to_string())
        );
        assert_eq!(
            *answers.get(1).unwrap(),
            (Ipv6Addr::from_str("2600:1406:5e00:2aa::3831")?.into(), "ibm.com".to_string())
        );
        Ok(())
    }
}
