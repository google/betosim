// Copyright 2023 The Android Open Source Project
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

use netsim_proto::{
    cell::{Cell, RegistrationStatus},
    frontend::ListDeviceResponse,
    model::{
        self,
        chip::ble_beacon::{AdvertiseData, AdvertiseSettings, advertise_settings},
    },
};
use netsim_rest_api::ListLinkResponse;
use protobuf::MessageField;

const INDENT_WIDTH: usize = 2;

pub(crate) fn format_registration_status(status: RegistrationStatus) -> &'static str {
    match status {
        RegistrationStatus::NOT_REGISTERED => "unregistered",
        RegistrationStatus::REGISTERED_HOME => "home",
        RegistrationStatus::SEARCHING => "searching",
        RegistrationStatus::DENIED => "denied",
        RegistrationStatus::UNKNOWN => "unknown",
        RegistrationStatus::ROAMING => "roaming",
        RegistrationStatus::SMS_ONLY_HOME => "sms-only-home",
        RegistrationStatus::SMS_ONLY_ROAMING => "sms-only-roaming",
        RegistrationStatus::EMERGENCY => "emergency",
    }
}

pub(crate) fn format_call_state(state: netsim_proto::cell::call::State) -> &'static str {
    match state {
        netsim_proto::cell::call::State::UNKNOWN => "unknown",
        netsim_proto::cell::call::State::ACTIVE => "active",
        netsim_proto::cell::call::State::HOLDING => "holding",
        netsim_proto::cell::call::State::DIALING => "dialing",
        netsim_proto::cell::call::State::ALERTING => "alerting",
        netsim_proto::cell::call::State::INCOMING => "incoming",
        netsim_proto::cell::call::State::WAITING => "waiting",
    }
}

pub(crate) fn format_call_direction(
    direction: netsim_proto::cell::call::Direction,
) -> &'static str {
    match direction {
        netsim_proto::cell::call::Direction::DIR_UNKNOWN => "unknown",
        netsim_proto::cell::call::Direction::MOBILE_ORIGINATED => "outgoing",
        netsim_proto::cell::call::Direction::MOBILE_TERMINATED => "incoming",
    }
}

/// # Invariants
/// Displayed values **do not** end in a newline.
pub(crate) struct Displayer<'a, T> {
    pub(crate) value: T,
    pub(crate) verbose: bool,
    pub(crate) indent: usize,
    pub(crate) cells: Option<&'a [Cell]>,
}

impl<'a, T> Displayer<'a, T> {
    /// Returns a new displayer for values of the provided type.
    pub(crate) fn new(value: T, verbose: bool) -> Self {
        Displayer { value, verbose, indent: 0, cells: None }
    }

    pub(crate) fn with_cells(mut self, cells: &'a [Cell]) -> Self {
        self.cells = Some(cells);
        self
    }

    pub(crate) fn indent(&mut self, current_indent: usize) -> &Self {
        self.indent = current_indent + INDENT_WIDTH;
        self
    }
}

/// Adds `.display(verbose)` to types that have a `Displayer`, like
/// [`std::path::Path::display`]. Bounded so it never shadows other `display()`
/// methods (e.g. `PathBuf` still reaches `Path::display`).
pub(crate) trait DisplayExt {
    fn display(&self, verbose: bool) -> Displayer<'_, &Self> {
        Displayer::new(self, verbose)
    }
}

impl<T: ?Sized> DisplayExt for T where for<'a> Displayer<'a, &'a T>: fmt::Display {}

impl fmt::Display for Displayer<'_, &ListDeviceResponse> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let indent = self.indent;

        let mut devices = self.value.devices.iter().peekable();
        while let Some(device) = devices.next() {
            let mut device_displayer = Displayer::new(device, self.verbose);
            if let Some(cells) = self.cells {
                device_displayer = device_displayer.with_cells(cells);
            }
            write!(f, "{:indent$}{}", "", device_displayer)?;
            if devices.peek().is_some() {
                // We print the newline here instead of in the Device displayer because we don't
                // want a newline before the very first device.
                writeln!(f)?;
            }
        }

        Ok(())
    }
}

impl fmt::Display for Displayer<'_, &model::Device> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let indent = self.indent;
        let width = 9;

        write!(
            f,
            "{:indent$}{:width$} {}",
            "",
            self.value.name,
            Displayer::new(self.value.position.as_ref().unwrap_or_default(), self.verbose)
        )?;

        for chip in self.value.chips.iter() {
            let mut chip_displayer = Displayer::new(chip, self.verbose);
            if let Some(cells) = self.cells {
                chip_displayer = chip_displayer.with_cells(cells);
            }
            write!(f, "{:indent$}{}", "", chip_displayer.indent(self.indent))?;
        }

        Ok(())
    }
}

impl fmt::Display for Displayer<'_, &model::Chip> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let indent = self.indent;
        let width = 9;

        match self.value.chip.as_ref() {
            Some(model::chip::Chip::BleBeacon(ble_beacon)) => {
                let radio = ble_beacon.bt.low_energy.as_ref().unwrap_or_default();
                let beacon_width = 16 + width;

                writeln!(f)?;
                write!(
                    f,
                    "{:indent$}{:beacon_width$} {}",
                    "",
                    format!("beacon-ble ({}):", self.value.name),
                    Displayer::new(radio, self.verbose),
                )?;

                if self.verbose {
                    writeln!(f)?;
                    write!(f, "{}", Displayer::new(ble_beacon, self.verbose).indent(self.indent))?;
                }
            }
            Some(model::chip::Chip::Bt(bt)) => {
                if let Some(ble) = bt.low_energy.as_ref() {
                    writeln!(f)?;
                    write!(
                        f,
                        "{:indent$}{:width$}{}",
                        "",
                        "ble: ",
                        Displayer::new(ble, self.verbose)
                    )?;
                };

                if let Some(classic) = bt.classic.as_ref() {
                    writeln!(f)?;
                    write!(
                        f,
                        "{:indent$}{:width$}{}",
                        "",
                        "classic: ",
                        Displayer::new(classic, self.verbose)
                    )?;
                };
            }
            Some(model::chip::Chip::Wifi(wifi)) => {
                writeln!(f)?;
                write!(
                    f,
                    "{:indent$}{:width$}{}",
                    "",
                    "wifi: ",
                    Displayer::new(wifi, self.verbose)
                )?;
            }
            Some(model::chip::Chip::Uwb(uwb)) => {
                writeln!(f)?;
                write!(f, "{:indent$}{:width$}{}", "", "uwb: ", Displayer::new(uwb, self.verbose))?;
            }
            Some(model::chip::Chip::Ethernet(eth)) => {
                writeln!(f)?;
                write!(
                    f,
                    "{:indent$}{:width$}{}",
                    "",
                    "ethernet: ",
                    Displayer::new(eth, self.verbose)
                )?;
            }
            Some(model::chip::Chip::Cellular(cell)) => {
                writeln!(f)?;
                write!(f, "{:indent$}{:width$}", "", "cellular: ",)?;
                // Note: The Chip ID matches the Cell ID. This is guaranteed by the backend
                // (next/grpc-server/src/cell.rs setting cell.id = chip.id).
                let cell_detail =
                    self.cells.and_then(|cells| cells.iter().find(|c| c.id == self.value.id));
                if let Some(detail) = cell_detail {
                    write!(f, "state: {} | rssi: {}", detail.state, detail.rssi,)?;
                    if self.verbose {
                        write!(f, " | ber: {} | sms_count: {}", detail.ber, detail.sms_count,)?;
                    }
                    write!(
                        f,
                        " | voice: {} | data: {}",
                        format_registration_status(
                            detail.voice_registration.enum_value_or_default()
                        ),
                        format_registration_status(
                            detail.data_registration.enum_value_or_default()
                        )
                    )?;
                    if !detail.active_calls.is_empty() {
                        write!(f, " | active_calls: ")?;
                        let mut calls = detail.active_calls.iter().peekable();
                        while let Some(call) = calls.next() {
                            write!(
                                f,
                                "{}({})",
                                call.number,
                                format_call_state(call.state.enum_value_or_default())
                            )?;
                            if calls.peek().is_some() {
                                write!(f, ", ")?;
                            }
                        }
                    }
                } else {
                    write!(f, "{}", Displayer::new(cell, self.verbose))?;
                }
            }
            Some(model::chip::Chip::CellularData(cell_data)) => {
                writeln!(f)?;
                write!(
                    f,
                    "{:indent$}{:width$}{}",
                    "",
                    "cellular-data: ",
                    Displayer::new(cell_data, self.verbose)
                )?;
            }
            Some(model::chip::Chip::Nfc(nfc)) => {
                writeln!(f)?;
                write!(f, "{:indent$}{:width$}{}", "", "nfc: ", Displayer::new(nfc, self.verbose))?;
            }
            _ => {
                if self.verbose {
                    writeln!(f)?;
                    write!(
                        f,
                        "{:indent$}Unknown chip (Kind: {:?})",
                        "",
                        self.value.kind.enum_value_or_default()
                    )?
                }
            }
        }

        Ok(())
    }
}

impl fmt::Display for Displayer<'_, &model::chip::BleBeacon> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let indent = self.indent;
        let address_width = 16;
        write!(f, "{:indent$}address: {:address_width$}", "", self.value.address)?;

        if self.value.settings.is_some()
            && self.value.settings != MessageField::some(AdvertiseSettings::default())
        {
            writeln!(f)?;
            write!(
                f,
                "{:indent$}advertise settings:{}",
                "",
                Displayer::new(self.value.settings.as_ref().unwrap_or_default(), self.verbose)
                    .indent(self.indent)
            )?;
        }

        if self.value.adv_data.is_some()
            && self.value.adv_data != MessageField::some(AdvertiseData::default())
        {
            writeln!(f)?;
            write!(
                f,
                "{:indent$}advertise packet data:{}",
                "",
                Displayer::new(self.value.adv_data.as_ref().unwrap_or_default(), self.verbose)
                    .indent(self.indent)
            )?;
        }

        // TODO(jmes): Add scan response data.

        Ok(())
    }
}

impl fmt::Display for Displayer<'_, &model::Position> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let indent = self.indent;
        let precision = 2;

        if self.verbose
            || (self.value.x != f32::default()
                || self.value.y != f32::default()
                || self.value.z != f32::default())
        {
            write!(
                f,
                "{:indent$} position: {:.precision$}, {:.precision$}, {:.precision$}",
                "", self.value.x, self.value.y, self.value.z,
            )?;
        }

        Ok(())
    }
}

impl fmt::Display for Displayer<'_, &model::chip::ble_beacon::AdvertiseSettings> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let indent = self.indent;
        let width = 25;

        if let Some(tx_power) = self.value.tx_power.as_ref() {
            writeln!(f)?;
            write!(f, "{:indent$}{}", "", Displayer::new(tx_power, self.verbose))?;
        }

        if let Some(interval) = self.value.interval.as_ref() {
            writeln!(f)?;
            write!(f, "{:indent$}{}", "", Displayer::new(interval, self.verbose))?;
        }

        if self.value.scannable {
            writeln!(f)?;
            write!(f, "{:indent$}{:width$}: {}", "", "scannable", self.value.scannable)?;
        }

        if self.value.timeout != u64::default() {
            writeln!(f)?;
            write!(f, "{:indent$}{:width$}: {} ms", "", "timeout", self.value.timeout)?;
        }

        Ok(())
    }
}

impl fmt::Display for Displayer<'_, &model::chip::ble_beacon::AdvertiseData> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let indent = self.indent;
        let width = 25;

        if self.value.include_device_name != bool::default() {
            writeln!(f)?;
            write!(
                f,
                "{:indent$}{:width$}: {}",
                "", "include device name", self.value.include_device_name
            )?;
        }

        if self.value.include_tx_power_level != bool::default() {
            writeln!(f)?;
            write!(
                f,
                "{:indent$}{:width$}: {}",
                "", "include tx power level", self.value.include_tx_power_level
            )?;
        }

        if self.value.manufacturer_data != Vec::<u8>::default() {
            writeln!(f)?;
            write!(
                f,
                "{:indent$}{:width$}: {}",
                "",
                "manufacturer data bytes",
                self.value.manufacturer_data.len()
            )?;
        }

        Ok(())
    }
}

impl fmt::Display for Displayer<'_, &advertise_settings::Interval> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let indent = self.indent;
        let width = 25;

        match self.value {
            advertise_settings::Interval::Milliseconds(interval) => {
                write!(f, "{:indent$}{:width$}: {} ms", "", "interval", interval)
            }
            advertise_settings::Interval::AdvertiseMode(mode) => {
                write!(f, "{:indent$}{:width$}: {:?}", "", "advertise mode", mode)
            }
            _ => Err(fmt::Error),
        }
    }
}

impl fmt::Display for Displayer<'_, &advertise_settings::Tx_power> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let indent = self.indent;
        let width = 25;

        match self.value {
            advertise_settings::Tx_power::Dbm(dbm) => {
                write!(f, "{:indent$}{:width$}: {} dBm", "", "tx power", dbm)
            }
            advertise_settings::Tx_power::TxPowerLevel(level) => {
                write!(f, "{:indent$}{:width$}: {:?}", "", "tx power level", level)
            }
            _ => Err(fmt::Error),
        }
    }
}

impl fmt::Display for Displayer<'_, &model::chip::Radio> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let indent = self.indent;
        let count_width = 9;
        write!(f, "{:indent$}{}", "", Displayer::new(&self.value.state, self.verbose),)?;

        write!(
            f,
            "| rx_count: {:count_width$} | tx_count: {:count_width$}",
            self.value.rx_count, self.value.tx_count
        )?;

        Ok(())
    }
}

impl fmt::Display for Displayer<'_, &Option<bool>> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let indent = self.indent;
        let width = 9;

        write!(
            f,
            "{:indent$}{:width$}",
            "",
            match self.value {
                Some(true) => "up",
                Some(false) => "down",
                None => "unknown",
            }
        )
    }
}

// Helper struct to display link's chip IDs, showing "ALL" for ID 0.
pub(crate) struct LinkChipIdDisplay(pub u32);

impl fmt::Display for LinkChipIdDisplay {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.0 == 0 { write!(f, "ALL") } else { write!(f, "{}", self.0) }
    }
}

impl fmt::Display for Displayer<'_, &ListLinkResponse> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let indent = self.indent;
        let chip_width = 10;
        let rssi_width = 6;
        let phykind_width = 12;

        if self.value.links.is_empty() {
            write!(f, "{:indent$}No links with properties are currently set.", "")?;
        } else {
            // Print a header for the table
            if self.verbose {
                let id_width = 4;
                write!(
                    f,
                    "{:indent$}{:id_width$} | {:chip_width$} | {:chip_width$} | {:phykind_width$} | {:rssi_width$}",
                    "", "ID", "Sender", "Receiver", "Type", "RSSI"
                )?;
                writeln!(f)?;
                write!(
                    f,
                    "{:indent$}{:-<id_width$}-+-{:-<chip_width$}-+-{:-<chip_width$}-+-{:-<phykind_width$}-+-{:-<rssi_width$}",
                    "", "", "", "", "", ""
                )?;
                // Iterate through the links and print each one as a row
                for link in self.value.links.iter() {
                    writeln!(f)?;
                    write!(
                        f,
                        "{:indent$}{:<id_width$} | {:<chip_width$} | {:<chip_width$} | {:<phykind_width$} | {:<rssi_width$}",
                        "",
                        link.id.0,
                        format!("{}", LinkChipIdDisplay(link.sender.0)),
                        format!("{}", LinkChipIdDisplay(link.receiver.0)),
                        format!("{:?}", link.kind),
                        link.rssi,
                    )?;
                }
            } else {
                write!(
                    f,
                    "{:indent$}{:chip_width$} | {:chip_width$} | {:phykind_width$} | {:rssi_width$}",
                    "", "Sender", "Receiver", "Type", "RSSI"
                )?;
                writeln!(f)?;
                write!(
                    f,
                    "{:indent$}{:-<chip_width$}-+-{:-<chip_width$}-+-{:-<phykind_width$}-+-{:-<rssi_width$}",
                    "", "", "", "", ""
                )?;
                // Iterate through the links and print each one as a row
                for link in self.value.links.iter() {
                    writeln!(f)?;
                    write!(
                        f,
                        "{:indent$}{:<chip_width$} | {:<chip_width$} | {:<phykind_width$} | {:<rssi_width$}",
                        "",
                        format!("{}", LinkChipIdDisplay(link.sender.0)),
                        format!("{}", LinkChipIdDisplay(link.receiver.0)),
                        format!("{:?}", link.kind),
                        link.rssi,
                    )?;
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use netsim_proto::{
        cell::{Call, Cell, RegistrationStatus, call::State as CallState},
        model::{
            Chip as ChipMsg,
            chip::{Chip as ChipProto, Radio},
        },
    };
    use protobuf::EnumOrUnknown;

    use super::*;

    #[test]
    fn test_display_cellular_chip_no_detail() {
        let mut chip = ChipMsg::new();
        chip.id = 1;
        chip.name = "cellular-chip".to_string();
        let mut radio = Radio::new();
        radio.state = Some(true).into();
        radio.rx_count = 10;
        radio.tx_count = 20;
        chip.chip = Some(ChipProto::Cellular(radio));

        // We need to set indent to 2 to match how it is printed in Device
        let mut displayer = Displayer::new(&chip, false);
        displayer.indent = 2;
        let output = format!("{}", displayer);

        // Expect: \n  cellular: up       | rx_count:        10 | tx_count:        20
        let expected = "\n  cellular: up       | rx_count:        10 | tx_count:        20";
        assert_eq!(output, expected);
    }

    #[test]
    fn test_display_cellular_chip_with_detail() {
        let mut chip = ChipMsg::new();
        chip.id = 42;
        chip.name = "cellular-chip".to_string();
        let mut radio = Radio::new();
        radio.state = Some(true).into();
        chip.chip = Some(ChipProto::Cellular(radio));

        let mut cell = Cell::new();
        cell.id = 42;
        cell.state = "idle".to_string();
        cell.rssi = 15;
        cell.voice_registration = EnumOrUnknown::new(RegistrationStatus::REGISTERED_HOME);
        cell.data_registration = EnumOrUnknown::new(RegistrationStatus::ROAMING);

        let cells = vec![cell];
        let mut displayer = Displayer::new(&chip, false).with_cells(&cells);
        displayer.indent = 2;
        let output = format!("{}", displayer);

        let expected = "\n  cellular: state: idle | rssi: 15 | voice: home | data: roaming";
        assert_eq!(output, expected);
    }

    #[test]
    fn test_display_cellular_chip_with_detail_verbose_and_calls() {
        let mut chip = ChipMsg::new();
        chip.id = 42;
        chip.name = "cellular-chip".to_string();
        let mut radio = Radio::new();
        radio.state = Some(true).into();
        chip.chip = Some(ChipProto::Cellular(radio));

        let mut cell = Cell::new();
        cell.id = 42;
        cell.state = "active".to_string();
        cell.rssi = 15;
        cell.ber = 2;
        cell.sms_count = 5;
        cell.voice_registration = EnumOrUnknown::new(RegistrationStatus::REGISTERED_HOME);
        cell.data_registration = EnumOrUnknown::new(RegistrationStatus::ROAMING);

        let mut call = Call::new();
        call.number = "12345".to_string();
        call.state = EnumOrUnknown::new(CallState::ACTIVE);
        cell.active_calls.push(call);

        let cells = vec![cell];
        let mut displayer = Displayer::new(&chip, true).with_cells(&cells); // verbose = true
        displayer.indent = 2;
        let output = format!("{}", displayer);

        let expected = "\n  cellular: state: active | rssi: 15 | ber: 2 | sms_count: 5 | voice: home | data: roaming | active_calls: 12345(active)";
        assert_eq!(output, expected);
    }

    #[test]
    fn test_display_nfc_chip() {
        let mut chip = ChipMsg::new();
        chip.id = 1;
        chip.name = "nfc-chip".to_string();
        let mut radio = Radio::new();
        radio.state = Some(true).into();
        radio.rx_count = 10;
        radio.tx_count = 20;
        chip.chip = Some(ChipProto::Nfc(radio));

        let mut displayer = Displayer::new(&chip, false);
        displayer.indent = 2;
        let output = format!("{}", displayer);

        let expected = "\n  nfc:     up       | rx_count:        10 | tx_count:        20";
        assert_eq!(output, expected);
    }

    #[test]
    fn test_display_ethernet_chip() {
        let mut chip = ChipMsg::new();
        chip.id = 2;
        chip.name = "eth-chip".to_string();
        let mut radio = Radio::new();
        radio.state = Some(true).into();
        radio.rx_count = 5;
        radio.tx_count = 15;
        chip.chip = Some(ChipProto::Ethernet(radio));

        let mut displayer = Displayer::new(&chip, false);
        displayer.indent = 2;
        let output = format!("{}", displayer);

        let expected = "\n  ethernet: up       | rx_count:         5 | tx_count:        15";
        assert_eq!(output, expected);
    }

    #[test]
    fn test_display_cellular_data_chip() {
        let mut chip = ChipMsg::new();
        chip.id = 3;
        chip.name = "cell-data-chip".to_string();
        let mut radio = Radio::new();
        radio.state = Some(true).into();
        radio.rx_count = 100;
        radio.tx_count = 200;
        chip.chip = Some(ChipProto::CellularData(radio));

        let mut displayer = Displayer::new(&chip, false);
        displayer.indent = 2;
        let output = format!("{}", displayer);

        let expected = "\n  cellular-data: up       | rx_count:       100 | tx_count:       200";
        assert_eq!(output, expected);
    }

    #[test]
    fn test_display_rest_list_link_response() {
        let resp = ListLinkResponse {
            links: vec![netsim_model::Link {
                id: netsim_model::LinkId(1),
                sender: netsim_model::ChipId(1000),
                receiver: netsim_model::ChipId(1001),
                kind: netsim_model::ChipKind::BLUETOOTH,
                rssi: -65,
            }],
        };

        let output = format!("{}", Displayer::new(&resp, false));
        let expected = concat!(
            "Sender     | Receiver   | Type         | RSSI  \n",
            "-----------+------------+--------------+-------\n",
            "1000       | 1001       | BLUETOOTH    | -65   "
        );
        assert_eq!(output, expected);
    }
}
