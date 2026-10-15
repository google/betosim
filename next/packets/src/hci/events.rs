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

use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout, Unaligned};

use crate::hci::{
    commands::OpCode,
    types::{Address, LeAdvertisingEventType, OwnAddressType},
};

/// HCI Event Codes for Event packets.
#[derive(
    Debug, Copy, Clone, PartialEq, Eq, FromBytes, IntoBytes, Unaligned, Immutable, KnownLayout,
)]
#[repr(transparent)]
pub struct EventCode(pub u8);

impl EventCode {
    pub const COMMAND_COMPLETE: Self = Self(0x0E);
    pub const COMMAND_STATUS: Self = Self(0x0F);
    pub const LE_META_EVENT: Self = Self(0x3E);
}

/// HCI LE Subevent Codes for LE Meta Events.
#[derive(
    Debug, Copy, Clone, PartialEq, Eq, FromBytes, IntoBytes, Unaligned, Immutable, KnownLayout,
)]
#[repr(transparent)]
pub struct SubeventCode(pub u8);

impl SubeventCode {
    pub const LE_ADVERTISING_REPORT: Self = Self(0x02);
}

/// HCI Event Packet Header.
#[repr(C)]
#[derive(FromBytes, IntoBytes, Unaligned, Immutable, KnownLayout, Debug)]
pub struct HciEventHeader {
    pub event_code: EventCode,
    pub parameter_total_length: u8,
}

/// HCI LE Meta Event Header (follows HciEventHeader if event_code is
/// LE_META_EVENT).
#[repr(C)]
#[derive(FromBytes, IntoBytes, Unaligned, Immutable, KnownLayout, Debug)]
pub struct HciLeMetaEventHeader {
    pub subevent_code: SubeventCode,
}

/// Command Complete Event Header.
#[repr(C)]
#[derive(
    FromBytes, IntoBytes, Unaligned, Immutable, KnownLayout, Debug, PartialEq, Eq, Clone, Copy,
)]
pub struct CommandCompleteHeader {
    pub num_hci_command_packets: u8,
    pub op_code: OpCode,
}

/// Command Status Event.
#[repr(C)]
#[derive(
    FromBytes, IntoBytes, Unaligned, Immutable, KnownLayout, Debug, PartialEq, Eq, Clone, Copy,
)]
pub struct CommandStatus {
    pub status: u8,
    pub num_hci_command_packets: u8,
    pub op_code: OpCode,
}

/// Dispatched HCI Event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HciEvent<'a> {
    CommandComplete { header: CommandCompleteHeader, return_parameters: &'a [u8] },
    CommandStatus(CommandStatus),
    LeMetaEvent(LeMetaEvent<'a>),
    Unknown { event_code: EventCode, payload: &'a [u8] },
}

/// Dispatched LE Meta Event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LeMetaEvent<'a> {
    LeAdvertisingReport(Vec<LeAdvertisingReport<'a>>),
    Unknown { subevent_code: SubeventCode, payload: &'a [u8] },
}

/// Top-level parser for HCI events.
pub fn parse_hci_event(data: &[u8]) -> Option<HciEvent<'_>> {
    let (header, payload) = HciEventHeader::read_from_prefix(data).ok()?;
    match header.event_code {
        EventCode::COMMAND_COMPLETE => {
            let (cc_header, return_parameters) =
                CommandCompleteHeader::read_from_prefix(payload).ok()?;
            Some(HciEvent::CommandComplete { header: cc_header, return_parameters })
        }
        EventCode::COMMAND_STATUS => {
            let (status, _) = CommandStatus::read_from_prefix(payload).ok()?;
            Some(HciEvent::CommandStatus(status))
        }
        EventCode::LE_META_EVENT => {
            let (le_header, le_payload) = HciLeMetaEventHeader::read_from_prefix(payload).ok()?;
            let le_event = match le_header.subevent_code {
                SubeventCode::LE_ADVERTISING_REPORT => {
                    let reports = parse_le_advertising_report(le_payload)?;
                    LeMetaEvent::LeAdvertisingReport(reports)
                }
                _ => LeMetaEvent::Unknown {
                    subevent_code: le_header.subevent_code,
                    payload: le_payload,
                },
            };
            Some(HciEvent::LeMetaEvent(le_event))
        }
        _ => Some(HciEvent::Unknown { event_code: header.event_code, payload }),
    }
}

/// Number of reports for LE Advertising Report event.
#[repr(C)]
#[derive(
    FromBytes, IntoBytes, Unaligned, Immutable, KnownLayout, Debug, PartialEq, Eq, Clone, Copy,
)]
pub struct NumReports {
    pub num_reports: u8,
}

/// LE Advertising Report structural components. Note that the event data length
/// is variable. The overall event contains `NumReports` followed by
/// `num_reports` instances of the reports parsed successively.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LeAdvertisingReport<'a> {
    pub event_type: LeAdvertisingEventType,
    pub address_type: OwnAddressType,
    pub address: Address,
    pub data_length: u8,
    pub data: &'a [u8],
    pub rssi: i8,
}

/// Fixed-size prefix of an LE Advertising Report for zerocopy parsing.
#[derive(FromBytes, KnownLayout, Unaligned)]
#[repr(C)]
struct LeAdvertisingReportPrefix {
    event_type: LeAdvertisingEventType,
    address_type: OwnAddressType,
    address: Address,
    data_length: u8,
}

/// Parse the multiple variable-length reports from an LE Advertising Report
/// event payload
pub fn parse_le_advertising_report(mut payload: &[u8]) -> Option<Vec<LeAdvertisingReport<'_>>> {
    let (num_reports_header, rest) = NumReports::read_from_prefix(payload).ok()?;
    let num_reports = num_reports_header.num_reports as usize;
    payload = rest;

    let mut reports = Vec::with_capacity(num_reports);

    for _ in 0..num_reports {
        let (prefix, rest) = LeAdvertisingReportPrefix::read_from_prefix(payload).ok()?;
        let data_len = prefix.data_length as usize;

        let (data, after_data) = rest.split_at_checked(data_len)?;
        let (&rssi_u8, next_rest) = after_data.split_first()?;
        let rssi = rssi_u8 as i8;

        reports.push(LeAdvertisingReport {
            event_type: prefix.event_type,
            address_type: prefix.address_type,
            address: prefix.address,
            data_length: prefix.data_length,
            data,
            rssi,
        });

        payload = next_rest;
    }

    Some(reports)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hci::commands::OpCode;

    #[test]
    fn test_parse_command_complete() {
        let data = [
            0x0E, // Event Code: Command Complete
            0x04, // Parameter Total Length
            0x01, // Num HCI Command Packets
            0x03, 0x0C, // OpCode: Reset (0x0C03)
            0x00, // Return Parameter: Status Success
        ];
        let event = parse_hci_event(&data).unwrap();
        if let HciEvent::CommandComplete { header, return_parameters } = event {
            assert_eq!(header.num_hci_command_packets, 1);
            assert_eq!(header.op_code, OpCode::RESET);
            assert_eq!(return_parameters, &[0x00]);
        } else {
            panic!("Expected CommandComplete event");
        }
    }

    #[test]
    fn test_parse_le_advertising_report_event() {
        let data = [
            0x3E, // Event Code: LE Meta Event
            12,   // Parameter Total Length
            0x02, // Subevent Code: LE Advertising Report
            0x01, // Num Reports
            0x00, // Event Type
            0x00, // Address Type
            0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF, // Address
            0x01, // Data Length
            0x11, // Data
            0xCE, // RSSI (-50)
        ];
        let event = parse_hci_event(&data).unwrap();
        if let HciEvent::LeMetaEvent(LeMetaEvent::LeAdvertisingReport(reports)) = event {
            assert_eq!(reports.len(), 1);
            let report = &reports[0];
            assert_eq!(report.address.bytes, [0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF]);
            assert_eq!(report.event_type, LeAdvertisingEventType::ADV_IND);
            assert_eq!(report.address_type, OwnAddressType::PUBLIC_DEVICE_ADDRESS);
            assert_eq!(report.data, &[0x11]);
            assert_eq!(report.rssi, -50);
        } else {
            panic!("Expected LE Advertising Report event");
        }
    }
}
