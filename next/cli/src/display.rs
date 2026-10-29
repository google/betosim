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

use netsim_model::{
    AdvertiseData, AdvertiseSettings, BleBeacon, BluetoothMode, CallState, Chip, ChipVariant,
    Device, Interval, Position, Radio, RegistrationStatus as ModelRegistrationStatus, TxPower,
};
use netsim_proto::cell::RegistrationStatus;
use netsim_rest_api::{ListDeviceResponse, ListLinkResponse};

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

fn format_model_registration_status(status: ModelRegistrationStatus) -> &'static str {
    match status {
        ModelRegistrationStatus::NotRegistered => "unregistered",
        ModelRegistrationStatus::RegisteredHome => "home",
        ModelRegistrationStatus::Searching => "searching",
        ModelRegistrationStatus::Denied => "denied",
        ModelRegistrationStatus::Unknown => "unknown",
        ModelRegistrationStatus::Roaming => "roaming",
        ModelRegistrationStatus::RegisteredSmsOnlyHome => "sms-only-home",
        ModelRegistrationStatus::RegisteredSmsOnlyRoaming => "sms-only-roaming",
        ModelRegistrationStatus::Emergency => "emergency",
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

fn format_model_call_state(state: CallState) -> &'static str {
    match state {
        CallState::Unknown => "unknown",
        CallState::Active => "active",
        CallState::Holding => "holding",
        CallState::Dialing => "dialing",
        CallState::Alerting => "alerting",
        CallState::Incoming => "incoming",
        CallState::Waiting => "waiting",
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
pub(crate) struct Displayer<T> {
    pub(crate) value: T,
    pub(crate) verbose: bool,
    pub(crate) indent: usize,
}

impl<T> Displayer<T> {
    /// Returns a new displayer for values of the provided type.
    pub(crate) fn new(value: T, verbose: bool) -> Self {
        Displayer { value, verbose, indent: 0 }
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
    fn display(&self, verbose: bool) -> Displayer<&Self> {
        Displayer::new(self, verbose)
    }
}

impl<T: ?Sized> DisplayExt for T where for<'a> Displayer<&'a T>: fmt::Display {}

impl fmt::Display for Displayer<&ListDeviceResponse> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let indent = self.indent;

        let mut devices = self.value.devices.iter().peekable();
        while let Some(device) = devices.next() {
            write!(f, "{:indent$}{}", "", Displayer::new(device, self.verbose))?;
            if devices.peek().is_some() {
                // We print the newline here instead of in the Device displayer because we don't
                // want a newline before the very first device.
                writeln!(f)?;
            }
        }

        Ok(())
    }
}

impl fmt::Display for Displayer<&Device> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let indent = self.indent;
        let width = 9;

        write!(
            f,
            "{:indent$}{:width$} {}",
            "",
            self.value.name,
            Displayer::new(&self.value.pose.position, self.verbose)
        )?;

        for chip in self.value.chips.iter() {
            let mut chip_displayer = Displayer::new(chip, self.verbose);
            write!(f, "{:indent$}{}", "", chip_displayer.indent(self.indent))?;
        }

        Ok(())
    }
}

impl fmt::Display for Displayer<&Chip> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let indent = self.indent;
        let width = 9;

        match self.value.variant.as_ref() {
            Some(ChipVariant::Bluetooth(bt)) => match &bt.mode {
                BluetoothMode::Beacon(beacon_params) => {
                    let beacon_width = 16 + width;

                    writeln!(f)?;
                    write!(
                        f,
                        "{:indent$}{:beacon_width$} {}",
                        "",
                        format!("beacon-ble ({}):", self.value.name),
                        Displayer::new(&bt.low_energy, self.verbose),
                    )?;

                    if self.verbose {
                        writeln!(f)?;
                        write!(
                            f,
                            "{}",
                            Displayer::new(&beacon_params.ble_beacon, self.verbose)
                                .indent(self.indent)
                        )?;
                    }
                }
                _ => {
                    writeln!(f)?;
                    write!(
                        f,
                        "{:indent$}{:width$}{}",
                        "",
                        "ble: ",
                        Displayer::new(&bt.low_energy, self.verbose)
                    )?;

                    writeln!(f)?;
                    write!(
                        f,
                        "{:indent$}{:width$}{}",
                        "",
                        "classic: ",
                        Displayer::new(&bt.classic, self.verbose)
                    )?;
                }
            },
            Some(ChipVariant::Wifi(wifi)) => {
                writeln!(f)?;
                write!(
                    f,
                    "{:indent$}{:width$}{}",
                    "",
                    "wifi: ",
                    Displayer::new(&wifi.radio, self.verbose)
                )?;
            }
            Some(ChipVariant::Uwb(uwb)) => {
                writeln!(f)?;
                write!(
                    f,
                    "{:indent$}{:width$}{}",
                    "",
                    "uwb: ",
                    Displayer::new(&uwb.radio, self.verbose)
                )?;
            }
            Some(ChipVariant::Ethernet(eth)) => {
                writeln!(f)?;
                write!(
                    f,
                    "{:indent$}{:width$}{}",
                    "",
                    "ethernet: ",
                    Displayer::new(&eth.radio, self.verbose)
                )?;
            }
            Some(ChipVariant::Cell(cell)) => {
                writeln!(f)?;
                write!(f, "{:indent$}{:width$}", "", "cellular: ")?;
                if !cell.state.is_empty() {
                    write!(f, "state: {} | rssi: {}", cell.state, cell.rssi)?;
                    if self.verbose {
                        write!(f, " | ber: {} | sms_count: {}", cell.ber, cell.sms_count)?;
                    }
                    write!(
                        f,
                        " | voice: {} | data: {}",
                        format_model_registration_status(cell.voice_registration),
                        format_model_registration_status(cell.data_registration)
                    )?;
                    if !cell.active_calls.is_empty() {
                        write!(f, " | active_calls: ")?;
                        let mut calls = cell.active_calls.iter().peekable();
                        while let Some(call) = calls.next() {
                            write!(f, "{}({})", call.number, format_model_call_state(call.state))?;
                            if calls.peek().is_some() {
                                write!(f, ", ")?;
                            }
                        }
                    }
                } else {
                    write!(f, "{}", Displayer::new(&cell.radio, self.verbose))?;
                }
            }
            Some(ChipVariant::CellularData(cell_data)) => {
                writeln!(f)?;
                write!(
                    f,
                    "{:indent$}{:width$}{}",
                    "",
                    "cellular-data: ",
                    Displayer::new(&cell_data.radio, self.verbose)
                )?;
            }
            Some(ChipVariant::Nfc(nfc)) => {
                writeln!(f)?;
                write!(
                    f,
                    "{:indent$}{:width$}{}",
                    "",
                    "nfc: ",
                    Displayer::new(&nfc.radio, self.verbose)
                )?;
            }
            None => {
                if self.verbose {
                    writeln!(f)?;
                    write!(f, "{:indent$}Unknown chip (Kind: {:?})", "", self.value.kind)?
                }
            }
        }

        Ok(())
    }
}

impl fmt::Display for Displayer<&BleBeacon> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let indent = self.indent;
        let address_width = 16;
        write!(f, "{:indent$}address: {:address_width$}", "", self.value.address)?;

        if let Some(settings) =
            self.value.settings.as_ref().filter(|s| **s != AdvertiseSettings::default())
        {
            writeln!(f)?;
            write!(
                f,
                "{:indent$}advertise settings:{}",
                "",
                Displayer::new(settings, self.verbose).indent(self.indent)
            )?;
        }

        if let Some(adv_data) =
            self.value.adv_data.as_ref().filter(|data| **data != AdvertiseData::default())
        {
            writeln!(f)?;
            write!(
                f,
                "{:indent$}advertise packet data:{}",
                "",
                Displayer::new(adv_data, self.verbose).indent(self.indent)
            )?;
        }

        // TODO(jmes): Add scan response data.

        Ok(())
    }
}

impl fmt::Display for Displayer<&Position> {
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

impl fmt::Display for Displayer<&AdvertiseSettings> {
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

impl fmt::Display for Displayer<&AdvertiseData> {
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

impl fmt::Display for Displayer<&Interval> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let indent = self.indent;
        let width = 25;

        match self.value {
            Interval::Milliseconds(interval) => {
                write!(f, "{:indent$}{:width$}: {} ms", "", "interval", interval)
            }
            Interval::AdvertiseMode(mode) => {
                write!(f, "{:indent$}{:width$}: {:?}", "", "advertise mode", mode)
            }
        }
    }
}

impl fmt::Display for Displayer<&TxPower> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let indent = self.indent;
        let width = 25;

        match self.value {
            TxPower::Dbm(dbm) => {
                write!(f, "{:indent$}{:width$}: {} dBm", "", "tx power", dbm)
            }
            TxPower::TxPowerLevel(level) => {
                write!(f, "{:indent$}{:width$}: {:?}", "", "tx power level", level)
            }
        }
    }
}

impl fmt::Display for Displayer<&Radio> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let indent = self.indent;
        let count_width = 9;
        write!(
            f,
            "{:indent$}{}",
            "",
            Displayer::new(&self.value.state.unwrap_or(true), self.verbose),
        )?;

        write!(
            f,
            "| rx_count: {:count_width$} | tx_count: {:count_width$}",
            self.value.rx_count, self.value.tx_count
        )?;

        Ok(())
    }
}

impl fmt::Display for Displayer<&bool> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let indent = self.indent;
        let width = 9;

        write!(f, "{:indent$}{:width$}", "", if *self.value { "up" } else { "down" })
    }
}

// Helper struct to display link's chip IDs, showing "ALL" for ID 0.
pub(crate) struct LinkChipIdDisplay(pub u32);

impl fmt::Display for LinkChipIdDisplay {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.0 == 0 { write!(f, "ALL") } else { write!(f, "{}", self.0) }
    }
}

impl fmt::Display for Displayer<&ListLinkResponse> {
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
    use netsim_model::{
        Call, CallDirection, CallState, Cell, CellularData, Chip, ChipVariant, Ethernet, Nfc,
        Radio, RegistrationStatus,
    };

    use super::*;

    #[test]
    fn test_display_cellular_chip_no_detail() {
        let chip = Chip {
            id: 1,
            name: "cellular-chip".to_string(),
            variant: Some(ChipVariant::Cell(Cell {
                radio: Radio {
                    state: Some(true),
                    rx_count: 10,
                    tx_count: 20,
                    ..Default::default()
                },
                state: String::new(),
                ..Default::default()
            })),
            ..Default::default()
        };

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
        let chip = Chip {
            id: 42,
            name: "cellular-chip".to_string(),
            variant: Some(ChipVariant::Cell(Cell {
                radio: Radio { state: Some(true), ..Default::default() },
                state: "idle".to_string(),
                rssi: 15,
                voice_registration: RegistrationStatus::RegisteredHome,
                data_registration: RegistrationStatus::Roaming,
                ..Default::default()
            })),
            ..Default::default()
        };

        let mut displayer = Displayer::new(&chip, false);
        displayer.indent = 2;
        let output = format!("{}", displayer);

        let expected = "\n  cellular: state: idle | rssi: 15 | voice: home | data: roaming";
        assert_eq!(output, expected);
    }

    #[test]
    fn test_display_cellular_chip_with_detail_verbose_and_calls() {
        let chip = Chip {
            id: 42,
            name: "cellular-chip".to_string(),
            variant: Some(ChipVariant::Cell(Cell {
                radio: Radio { state: Some(true), ..Default::default() },
                state: "active".to_string(),
                rssi: 15,
                ber: 2,
                sms_count: 5,
                voice_registration: RegistrationStatus::RegisteredHome,
                data_registration: RegistrationStatus::Roaming,
                active_calls: vec![Call {
                    number: "12345".to_string(),
                    state: CallState::Active,
                    direction: CallDirection::MobileOriginated,
                }],
                ..Default::default()
            })),
            ..Default::default()
        };

        let mut displayer = Displayer::new(&chip, true); // verbose = true
        displayer.indent = 2;
        let output = format!("{}", displayer);

        let expected = "\n  cellular: state: active | rssi: 15 | ber: 2 | sms_count: 5 | voice: home | data: roaming | active_calls: 12345(active)";
        assert_eq!(output, expected);
    }

    #[test]
    fn test_display_nfc_chip() {
        let chip = Chip {
            id: 1,
            name: "nfc-chip".to_string(),
            variant: Some(ChipVariant::Nfc(Nfc {
                radio: Radio {
                    state: Some(true),
                    rx_count: 10,
                    tx_count: 20,
                    ..Default::default()
                },
            })),
            ..Default::default()
        };

        let mut displayer = Displayer::new(&chip, false);
        displayer.indent = 2;
        let output = format!("{}", displayer);

        let expected = "\n  nfc:     up       | rx_count:        10 | tx_count:        20";
        assert_eq!(output, expected);
    }

    #[test]
    fn test_display_ethernet_chip() {
        let chip = Chip {
            id: 2,
            name: "eth-chip".to_string(),
            variant: Some(ChipVariant::Ethernet(Ethernet {
                radio: Radio { state: Some(true), rx_count: 5, tx_count: 15, ..Default::default() },
            })),
            ..Default::default()
        };

        let mut displayer = Displayer::new(&chip, false);
        displayer.indent = 2;
        let output = format!("{}", displayer);

        let expected = "\n  ethernet: up       | rx_count:         5 | tx_count:        15";
        assert_eq!(output, expected);
    }

    #[test]
    fn test_display_cellular_data_chip() {
        let chip = Chip {
            id: 3,
            name: "cell-data-chip".to_string(),
            variant: Some(ChipVariant::CellularData(CellularData {
                radio: Radio {
                    state: Some(true),
                    rx_count: 100,
                    tx_count: 200,
                    ..Default::default()
                },
            })),
            ..Default::default()
        };

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
