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

//! Defines structures for DNS packets.

use zerocopy::{
    FromBytes, Immutable, IntoBytes, KnownLayout, U16, Unaligned, byteorder::NetworkEndian,
};

/// Represents the DNS header.
#[derive(FromBytes, KnownLayout, Unaligned, Immutable, IntoBytes, Debug)]
#[repr(C)]
pub struct DnsHeader {
    pub transaction_id: U16<NetworkEndian>,
    pub flags: U16<NetworkEndian>,
    pub num_questions: U16<NetworkEndian>,
    pub num_answers: U16<NetworkEndian>,
    pub num_authority: U16<NetworkEndian>,
    pub num_additional: U16<NetworkEndian>,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
#[repr(u16)]
pub enum Opcode {
    StandardQuery = 0,
    InverseQuery = 1,
    ServerStatusRequest = 2,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
#[repr(u16)]
pub enum ResponseCode {
    NoError = 0,
    FormatError = 1,
    ServerFailure = 2,
    NameError = 3,
    NotImplemented = 4,
    Refused = 5,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DnsFlags(u16);

impl DnsFlags {
    pub const RESPONSE: u16 = 0x8000;
    pub const OPCODE_MASK: u16 = 0x7800;
    pub const RECURSION_DESIRED: u16 = 0x0100;
    pub const RESPONSE_CODE_MASK: u16 = 0x000F;
}

#[derive(Debug)]
pub struct Question {
    pub name: String,
    pub qtype: u16,
    pub qclass: u16,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
#[repr(u16)]
pub enum ResourceType {
    A = 1,
    Aaaa = 28,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
#[repr(u16)]
pub enum ResourceClass {
    Internet = 1,
}
