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

use std::fmt;

use netsim_proto::cell::{Cell, ListCellsResponse};

use crate::display::{
    Displayer, format_call_direction, format_call_state, format_registration_status,
};

impl fmt::Display for Displayer<'_, &ListCellsResponse> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let indent = self.indent;
        if self.value.cells.is_empty() {
            write!(f, "{:indent$}No cellular devices are currently simulated.", "")?;
        } else {
            let id_width = 4;
            let state_width = 10;
            let ringing_width = 8;
            let sms_width = 10;
            let rssi_width = 6;
            let ber_width = 5;
            let reg_width = 12;

            write!(
                f,
                "{:indent$}{:<id_width$} | {:<state_width$} | {:<ringing_width$} | {:<sms_width$} | {:<rssi_width$} | {:<ber_width$} | {:<reg_width$} | {:<reg_width$}",
                "", "ID", "State", "Ringing", "SMS Count", "RSSI", "BER", "Voice Reg", "Data Reg"
            )?;
            writeln!(f)?;
            write!(
                f,
                "{:indent$}{:-<id_width$} | {:-<state_width$} | {:-<ringing_width$} | {:-<sms_width$} | {:-<rssi_width$} | {:-<ber_width$} | {:-<reg_width$} | {:-<reg_width$}",
                "", "", "", "", "", "", "", "", ""
            )?;

            for cell in &self.value.cells {
                writeln!(f)?;
                write!(
                    f,
                    "{:indent$}{:<id_width$} | {:<state_width$} | {:<ringing_width$} | {:<sms_width$} | {:<rssi_width$} | {:<ber_width$} | {:<reg_width$} | {:<reg_width$}",
                    "",
                    cell.id,
                    cell.state,
                    cell.ringing,
                    cell.sms_count,
                    cell.rssi,
                    cell.ber,
                    format_registration_status(cell.voice_registration.enum_value_or_default()),
                    format_registration_status(cell.data_registration.enum_value_or_default())
                )?;
            }
        }
        Ok(())
    }
}

impl fmt::Display for Displayer<'_, &Cell> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let indent = self.indent;
        writeln!(f, "{:indent$}Cell ID: {}", "", self.value.id)?;
        writeln!(f, "{:indent$}State: {} (ringing: {})", "", self.value.state, self.value.ringing)?;
        writeln!(
            f,
            "{:indent$}Signal Strength: RSSI: {}, BER: {}",
            "", self.value.rssi, self.value.ber
        )?;
        writeln!(f, "{:indent$}SMS Count: {}", "", self.value.sms_count)?;
        writeln!(
            f,
            "{:indent$}Voice Registration: {}",
            "",
            format_registration_status(self.value.voice_registration.enum_value_or_default())
        )?;
        writeln!(
            f,
            "{:indent$}Data Registration: {}",
            "",
            format_registration_status(self.value.data_registration.enum_value_or_default())
        )?;
        if self.value.active_calls.is_empty() {
            write!(f, "{:indent$}Active Calls: None", "")?;
        } else {
            writeln!(f, "{:indent$}Active Calls:", "")?;
            let call_indent = indent + 2;
            let mut calls = self.value.active_calls.iter().peekable();
            while let Some(call) = calls.next() {
                write!(
                    f,
                    "{:call_indent$}- Number: {}, State: {}, Direction: {}",
                    "",
                    call.number,
                    format_call_state(call.state.enum_value_or_default()),
                    format_call_direction(call.direction.enum_value_or_default())
                )?;
                if calls.peek().is_some() {
                    writeln!(f)?;
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use netsim_proto::cell::{
        Call, RegistrationStatus,
        call::{Direction as CallDirection, State as CallState},
    };
    use protobuf::EnumOrUnknown;

    use super::*;

    #[test]
    fn test_display_list_cells_response_empty() {
        let response = ListCellsResponse::new();
        let displayer = Displayer::new(&response, false);
        let output = format!("{}", displayer);
        assert_eq!(output, "No cellular devices are currently simulated.");
    }

    #[test]
    fn test_display_list_cells_response_non_empty() {
        let mut response = ListCellsResponse::new();
        let mut cell1 = Cell::new();
        cell1.id = 1;
        cell1.state = "active".to_string();
        cell1.ringing = false;
        cell1.sms_count = 5;
        cell1.rssi = 15;
        cell1.ber = 1;
        cell1.voice_registration = EnumOrUnknown::new(RegistrationStatus::REGISTERED_HOME);
        cell1.data_registration = EnumOrUnknown::new(RegistrationStatus::ROAMING);
        response.cells.push(cell1);

        let displayer = Displayer::new(&response, false);
        let output = format!("{}", displayer);

        // Check that header is present
        assert!(output.contains("ID"));
        assert!(output.contains("State"));
        assert!(output.contains("Voice Reg"));
        // Check that cell info is present
        assert!(output.contains("active"));
        assert!(output.contains("roaming"));
    }

    #[test]
    fn test_display_cell_default() {
        let cell = Cell::new();
        let displayer = Displayer::new(&cell, false);
        let output = format!("{}", displayer);

        let expected = "\
Cell ID: 0
State:  (ringing: false)
Signal Strength: RSSI: 0, BER: 0
SMS Count: 0
Voice Registration: unregistered
Data Registration: unregistered
Active Calls: None";
        assert_eq!(output, expected);
    }

    #[test]
    fn test_display_cell_with_calls() {
        let mut cell = Cell::new();
        cell.id = 42;
        cell.state = "active".to_string();
        cell.ringing = true;
        cell.rssi = 20;
        cell.ber = 2;
        cell.sms_count = 10;
        cell.voice_registration = EnumOrUnknown::new(RegistrationStatus::REGISTERED_HOME);
        cell.data_registration = EnumOrUnknown::new(RegistrationStatus::NOT_REGISTERED);

        let mut call1 = Call::new();
        call1.number = "12345".to_string();
        call1.state = EnumOrUnknown::new(CallState::ACTIVE);
        call1.direction = EnumOrUnknown::new(CallDirection::MOBILE_ORIGINATED);
        cell.active_calls.push(call1);

        let mut call2 = Call::new();
        call2.number = "67890".to_string();
        call2.state = EnumOrUnknown::new(CallState::HOLDING);
        call2.direction = EnumOrUnknown::new(CallDirection::MOBILE_TERMINATED);
        cell.active_calls.push(call2);

        let displayer = Displayer::new(&cell, false);
        let output = format!("{}", displayer);

        let expected = "\
Cell ID: 42
State: active (ringing: true)
Signal Strength: RSSI: 20, BER: 2
SMS Count: 10
Voice Registration: home
Data Registration: unregistered
Active Calls:
  - Number: 12345, State: active, Direction: outgoing
  - Number: 67890, State: holding, Direction: incoming";
        assert_eq!(output, expected);
    }
}
