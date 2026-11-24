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

use std::{borrow::Cow, cmp::max, collections::HashMap, fmt};

use netsim_model::{
    AdvertiseData, AdvertiseSettings, BleBeacon, BluetoothMode, CallState, Chip, ChipId,
    ChipVariant, Device, Interval, Position, Radio, RegistrationStatus as ModelRegistrationStatus,
    TxPower,
};
use netsim_proto::cell::RegistrationStatus;
use netsim_rest_api::{ListDeviceResponse, ListLinkResponse};

const INDENT_WIDTH: usize = 2;
const CHIP_LABEL_WIDTH: usize = 9;
const BEACON_FIELD_WIDTH: usize = 25;
const WILDCARD_CHIP_ID: ChipId = ChipId(0);

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

pub(crate) fn format_link_chip_id(id: ChipId) -> Cow<'static, str> {
    if id == WILDCARD_CHIP_ID { Cow::Borrowed("ALL") } else { Cow::Owned(id.to_string()) }
}

pub(crate) fn format_optional_link_chip_id(id: Option<ChipId>) -> Cow<'static, str> {
    format_link_chip_id(id.unwrap_or(WILDCARD_CHIP_ID))
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

        for (idx, device) in self.value.devices.iter().enumerate() {
            if idx > 0 {
                writeln!(f)?;
            }
            write!(f, "{:indent$}{}", "", Displayer::new(device, self.verbose))?;
        }

        Ok(())
    }
}

fn device_label(device: &Device) -> Cow<'_, str> {
    match device.adb_serial() {
        Some(serial) if serial != device.name => Cow::Owned(format!("{} ({serial})", device.name)),
        _ => Cow::Borrowed(&device.name),
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
            device_label(self.value),
            Displayer::new(&self.value.pose.position, self.verbose)
        )?;

        for chip in &self.value.chips {
            let mut chip_displayer = Displayer::new(chip, self.verbose);
            write!(f, "{:indent$}{}", "", chip_displayer.indent(self.indent))?;
        }

        Ok(())
    }
}

impl Displayer<&Chip> {
    fn write_radio_line(
        &self,
        f: &mut fmt::Formatter<'_>,
        label: &str,
        radio: &Radio,
    ) -> fmt::Result {
        let indent = self.indent;
        writeln!(f)?;
        write!(f, "{:indent$}{label:CHIP_LABEL_WIDTH$}{}", "", Displayer::new(radio, self.verbose))
    }
}

impl fmt::Display for Displayer<&Chip> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let indent = self.indent;

        match self.value.variant.as_ref() {
            Some(ChipVariant::Bluetooth(bt)) => {
                if let BluetoothMode::Beacon(beacon_params) = &bt.mode {
                    let beacon_width = 16 + CHIP_LABEL_WIDTH;

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
                } else {
                    self.write_radio_line(f, "ble: ", &bt.low_energy)?;
                    self.write_radio_line(f, "classic: ", &bt.classic)?;
                }
            }
            Some(ChipVariant::Wifi(wifi)) => {
                self.write_radio_line(f, "wifi: ", &wifi.radio)?;
            }
            Some(ChipVariant::Uwb(uwb)) => {
                self.write_radio_line(f, "uwb: ", &uwb.radio)?;
            }
            Some(ChipVariant::Ethernet(eth)) => {
                self.write_radio_line(f, "ethernet: ", &eth.radio)?;
            }
            Some(ChipVariant::Cell(cell)) => {
                writeln!(f)?;
                write!(f, "{:indent$}{:CHIP_LABEL_WIDTH$}", "", "cellular: ")?;
                if cell.state.is_empty() {
                    write!(f, "{}", Displayer::new(&cell.radio, self.verbose))?;
                } else {
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
                        for (idx, call) in cell.active_calls.iter().enumerate() {
                            if idx > 0 {
                                write!(f, ", ")?;
                            }
                            write!(f, "{}({})", call.number, format_model_call_state(call.state))?;
                        }
                    }
                }
            }
            Some(ChipVariant::CellularData(cell_data)) => {
                self.write_radio_line(f, "cellular-data: ", &cell_data.radio)?;
            }
            Some(ChipVariant::Nfc(nfc)) => {
                self.write_radio_line(f, "nfc: ", &nfc.radio)?;
            }
            None => {
                if self.verbose {
                    writeln!(f)?;
                    write!(f, "{:indent$}Unknown chip (Kind: {:?})", "", self.value.kind)?;
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

        if self.verbose || *self.value != Position::default() {
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
            write!(
                f,
                "{:indent$}{:BEACON_FIELD_WIDTH$}: {}",
                "", "scannable", self.value.scannable
            )?;
        }

        if self.value.timeout != 0 {
            writeln!(f)?;
            write!(
                f,
                "{:indent$}{:BEACON_FIELD_WIDTH$}: {} ms",
                "", "timeout", self.value.timeout
            )?;
        }

        Ok(())
    }
}

impl fmt::Display for Displayer<&AdvertiseData> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let indent = self.indent;

        if self.value.include_device_name {
            writeln!(f)?;
            write!(
                f,
                "{:indent$}{:BEACON_FIELD_WIDTH$}: {}",
                "", "include device name", self.value.include_device_name
            )?;
        }

        if self.value.include_tx_power_level {
            writeln!(f)?;
            write!(
                f,
                "{:indent$}{:BEACON_FIELD_WIDTH$}: {}",
                "", "include tx power level", self.value.include_tx_power_level
            )?;
        }

        if !self.value.manufacturer_data.is_empty() {
            writeln!(f)?;
            write!(
                f,
                "{:indent$}{:BEACON_FIELD_WIDTH$}: {}",
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

        match self.value {
            Interval::Milliseconds(interval) => {
                write!(f, "{:indent$}{:BEACON_FIELD_WIDTH$}: {interval} ms", "", "interval")
            }
            Interval::AdvertiseMode(mode) => {
                write!(f, "{:indent$}{:BEACON_FIELD_WIDTH$}: {mode:?}", "", "advertise mode")
            }
        }
    }
}

impl fmt::Display for Displayer<&TxPower> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let indent = self.indent;

        match self.value {
            TxPower::Dbm(dbm) => {
                write!(f, "{:indent$}{:BEACON_FIELD_WIDTH$}: {dbm} dBm", "", "tx power")
            }
            TxPower::TxPowerLevel(level) => {
                write!(f, "{:indent$}{:BEACON_FIELD_WIDTH$}: {level:?}", "", "tx power level")
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

pub(crate) struct LinkTable<'a> {
    pub(crate) links: &'a ListLinkResponse,
    pub(crate) devices: &'a ListDeviceResponse,
}

impl fmt::Display for Displayer<LinkTable<'_>> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let indent = self.indent;
        if self.value.links.links.is_empty() {
            return write!(f, "{:indent$}No links with properties are currently set.", "");
        }

        let chip_to_device: HashMap<ChipId, Cow<'_, str>> = self
            .value
            .devices
            .devices
            .iter()
            .flat_map(|device| {
                let label = device_label(device);
                device.chips.iter().map(move |chip| (ChipId(chip.id), label.clone()))
            })
            .collect();
        let endpoint = |id: ChipId| -> Cow<'_, str> {
            chip_to_device.get(&id).cloned().unwrap_or_else(|| format_link_chip_id(id))
        };

        let mut id_width = 4;
        let mut sender_width = 10;
        let mut receiver_width = 10;
        let mut phykind_width = 12;
        let mut rssi_width = 6;
        let mut rows = Vec::with_capacity(self.value.links.links.len() + 1);
        rows.push((
            Cow::Borrowed("ID"),
            Cow::Borrowed("Sender"),
            Cow::Borrowed("Receiver"),
            Cow::Borrowed("Type"),
            Cow::Borrowed("RSSI"),
        ));
        for link in &self.value.links.links {
            let id: Cow<'_, str> = Cow::Owned(link.id.0.to_string());
            let sender = endpoint(link.sender);
            let receiver = endpoint(link.receiver);
            let kind: Cow<'_, str> = Cow::Owned(format!("{:?}", link.kind));
            let rssi: Cow<'_, str> = Cow::Owned(link.rssi.to_string());
            id_width = max(id_width, id.chars().count());
            sender_width = max(sender_width, sender.chars().count());
            receiver_width = max(receiver_width, receiver.chars().count());
            phykind_width = max(phykind_width, kind.chars().count());
            rssi_width = max(rssi_width, rssi.chars().count());
            rows.push((id, sender, receiver, kind, rssi));
        }

        for (idx, (id, sender, receiver, kind, rssi)) in rows.iter().enumerate() {
            if idx > 0 {
                writeln!(f)?;
            }
            if self.verbose {
                write!(f, "{:indent$}{id:<id_width$} | ", "")?;
            } else {
                write!(f, "{:indent$}", "")?;
            }
            write!(
                f,
                "{sender:<sender_width$} | {receiver:<receiver_width$} | {kind:<phykind_width$} | {rssi:<rssi_width$}"
            )?;
            if idx == 0 {
                writeln!(f)?;
                if self.verbose {
                    write!(f, "{:indent$}{:-<id_width$}-+-", "", "")?;
                } else {
                    write!(f, "{:indent$}", "")?;
                }
                write!(
                    f,
                    "{:-<sender_width$}-+-{:-<receiver_width$}-+-{:-<phykind_width$}-+-{:-<rssi_width$}",
                    "", "", "", ""
                )?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use netsim_model::{
        AdvertiseMode, AdvertiseTxPower, BeaconParams, Bluetooth, Call, CallDirection, CallState,
        Cell, CellularData, Chip, ChipVariant, Ethernet, Nfc, Radio, RegistrationStatus, Service,
        Uwb, Wifi,
    };

    use super::*;

    fn radio(state: bool, rx_count: u64, tx_count: u64) -> Radio {
        Radio { state: Some(state), tx_count, rx_count, ..Default::default() }
    }

    fn format_indented_chip(chip: &Chip) -> String {
        let mut displayer = Displayer::new(chip, false);
        displayer.indent = INDENT_WIDTH;
        format!("{displayer}")
    }

    fn format_variant(variant: ChipVariant) -> String {
        format_indented_chip(&Chip { variant: Some(variant), ..Default::default() })
    }

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
        let devices = ListDeviceResponse { devices: vec![] };

        let output =
            format!("{}", Displayer::new(LinkTable { links: &resp, devices: &devices }, false));
        let expected = concat!(
            "Sender     | Receiver   | Type         | RSSI  \n",
            "-----------+------------+--------------+-------\n",
            "1000       | 1001       | BLUETOOTH    | -65   "
        );
        assert_eq!(output, expected);
    }

    #[test]
    fn test_display_rest_list_link_response_with_devices() {
        let resp = ListLinkResponse {
            links: vec![netsim_model::Link {
                id: netsim_model::LinkId(1),
                sender: netsim_model::ChipId(1000),
                receiver: netsim_model::ChipId(1001),
                kind: netsim_model::ChipKind::CELLULAR_DATA,
                rssi: -70,
            }],
        };
        let devices = ListDeviceResponse {
            devices: vec![
                Device {
                    id: 1,
                    name: "Pixel_8".to_string(),
                    chips: vec![Chip { id: 1000, ..Default::default() }],
                    device_info: Some(netsim_model::DeviceInfo {
                        adb_serial: "emulator-5554".to_string(),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
                Device {
                    id: 2,
                    name: "Long_Device_Name".to_string(),
                    chips: vec![Chip { id: 1001, ..Default::default() }],
                    ..Default::default()
                },
            ],
        };

        let table = LinkTable { links: &resp, devices: &devices };
        let expected_normal = concat!(
            "Sender                  | Receiver         | Type          | RSSI  \n",
            "------------------------+------------------+---------------+-------\n",
            "Pixel_8 (emulator-5554) | Long_Device_Name | CELLULAR_DATA | -70   "
        );
        assert_eq!(format!("{}", Displayer::new(table, false)), expected_normal);

        let table_verbose = LinkTable { links: &resp, devices: &devices };
        let expected_verbose = concat!(
            "ID   | Sender                  | Receiver         | Type          | RSSI  \n",
            "-----+-------------------------+------------------+---------------+-------\n",
            "1    | Pixel_8 (emulator-5554) | Long_Device_Name | CELLULAR_DATA | -70   "
        );
        assert_eq!(format!("{}", Displayer::new(table_verbose, true)), expected_verbose);
    }

    #[test]
    fn test_display_rest_list_link_response_fallback() {
        let resp = ListLinkResponse {
            links: vec![
                netsim_model::Link {
                    id: netsim_model::LinkId(1),
                    sender: netsim_model::ChipId(1002),
                    receiver: netsim_model::ChipId(0),
                    kind: netsim_model::ChipKind::WIFI,
                    rssi: -50,
                },
                netsim_model::Link {
                    id: netsim_model::LinkId(2),
                    sender: netsim_model::ChipId(9999),
                    receiver: netsim_model::ChipId(1002),
                    kind: netsim_model::ChipKind::BLUETOOTH,
                    rssi: -65,
                },
            ],
        };
        let devices = ListDeviceResponse {
            devices: vec![Device {
                id: 3,
                name: "Watch".to_string(),
                chips: vec![Chip { id: 1002, ..Default::default() }],
                ..Default::default()
            }],
        };

        let output =
            format!("{}", Displayer::new(LinkTable { links: &resp, devices: &devices }, false));
        let expected = concat!(
            "Sender     | Receiver   | Type         | RSSI  \n",
            "-----------+------------+--------------+-------\n",
            "Watch      | ALL        | WIFI         | -50   \n",
            "9999       | Watch      | BLUETOOTH    | -65   "
        );
        assert_eq!(output, expected);
    }

    #[test]
    fn test_display_device_with_adb_serial() {
        let device = Device {
            id: 1,
            name: "Pixel_6".to_string(),
            device_info: Some(netsim_model::DeviceInfo {
                adb_serial: "emulator-5554".to_string(),
                ..Default::default()
            }),
            ..Default::default()
        };
        assert_eq!(format!("{}", Displayer::new(&device, false)), "Pixel_6 (emulator-5554) ");
    }

    #[test]
    fn test_display_device_without_device_info() {
        let missing_info = Device { id: 1, name: "Pixel_6".to_string(), ..Default::default() };
        assert_eq!(format!("{}", Displayer::new(&missing_info, false)), "Pixel_6   ");
    }

    #[test]
    fn test_display_device_with_empty_adb_serial() {
        let empty_serial = Device {
            id: 2,
            name: "Pixel_6".to_string(),
            device_info: Some(netsim_model::DeviceInfo::default()),
            ..Default::default()
        };
        assert_eq!(format!("{}", Displayer::new(&empty_serial, false)), "Pixel_6   ");
    }

    #[test]
    fn test_display_device_matching_name_and_adb_serial() {
        let device = Device {
            id: 1,
            name: "emulator-5554".to_string(),
            device_info: Some(netsim_model::DeviceInfo {
                adb_serial: "emulator-5554".to_string(),
                ..Default::default()
            }),
            ..Default::default()
        };
        assert_eq!(format!("{}", Displayer::new(&device, false)), "emulator-5554 ");
    }

    #[test]
    fn test_display_cellular_chip_joins_multiple_active_calls() {
        let output = format_variant(ChipVariant::Cell(Cell {
            state: "active".to_string(),
            rssi: 15,
            voice_registration: RegistrationStatus::RegisteredHome,
            data_registration: RegistrationStatus::NotRegistered,
            active_calls: vec![
                Call {
                    number: "12345".to_string(),
                    state: CallState::Active,
                    direction: CallDirection::MobileOriginated,
                },
                Call {
                    number: "67890".to_string(),
                    state: CallState::Holding,
                    direction: CallDirection::MobileTerminated,
                },
            ],
            ..Default::default()
        }));

        assert_eq!(
            output,
            "\n  cellular: state: active | rssi: 15 | voice: home | data: unregistered | active_calls: 12345(active), 67890(holding)"
        );
    }

    #[test]
    fn test_display_bluetooth_device_chip() {
        let output = format_variant(ChipVariant::Bluetooth(Box::new(Bluetooth {
            low_energy: radio(true, 3, 7),
            classic: radio(false, 11, 13),
            ..Default::default()
        })));

        let expected = concat!(
            "\n  ble:     up       | rx_count:         3 | tx_count:         7",
            "\n  classic: down     | rx_count:        11 | tx_count:        13",
        );
        assert_eq!(output, expected);
    }

    #[test]
    fn test_display_wifi_chip() {
        let output = format_variant(ChipVariant::Wifi(Wifi { radio: radio(true, 12, 34) }));

        assert_eq!(output, "\n  wifi:    up       | rx_count:        12 | tx_count:        34");
    }

    #[test]
    fn test_display_uwb_chip() {
        let output = format_variant(ChipVariant::Uwb(Uwb {
            radio: radio(true, 8, 16),
            ..Default::default()
        }));

        assert_eq!(output, "\n  uwb:     up       | rx_count:         8 | tx_count:        16");
    }

    #[test]
    fn test_display_bluetooth_beacon_chip() {
        let chip = Chip {
            name: "b1".to_string(),
            variant: Some(ChipVariant::Bluetooth(Box::new(Bluetooth {
                low_energy: radio(true, 4, 9),
                mode: BluetoothMode::Beacon(Box::<BeaconParams>::default()),
                ..Default::default()
            }))),
            ..Default::default()
        };

        assert_eq!(
            format_indented_chip(&chip),
            "\n  beacon-ble (b1):          up       | rx_count:         4 | tx_count:         9"
        );
    }

    #[test]
    fn test_display_list_device_response_separates_multiple_devices() {
        let response = ListDeviceResponse {
            devices: vec![
                Device { id: 1, name: "Pixel_7".to_string(), ..Default::default() },
                Device { id: 2, name: "Pixel_8".to_string(), ..Default::default() },
            ],
        };

        let displayer = Displayer::new(&response, false);
        assert_eq!(format!("{displayer}"), "Pixel_7   \nPixel_8   ");
    }

    #[test]
    fn test_display_position_non_default_formats_coordinates() {
        let pos = Position { x: 1.5, y: 0.0, z: -2.25 };
        let displayer = Displayer::new(&pos, false);
        assert_eq!(format!("{displayer}"), " position: 1.50, 0.00, -2.25");
    }

    #[test]
    fn test_display_ble_beacon_omits_unset_settings_and_data_fields() {
        let beacon = BleBeacon {
            address: "02:00:00:00:00:01".to_string(),
            settings: Some(AdvertiseSettings {
                interval: Some(Interval::Milliseconds(250)),
                tx_power: Some(TxPower::Dbm(-15)),
                scannable: false,
                timeout: 0,
            }),
            adv_data: Some(AdvertiseData {
                include_device_name: false,
                include_tx_power_level: false,
                manufacturer_data: vec![],
                services: vec![Service { uuid: "180D".to_string(), data: vec![] }],
            }),
            scan_response: None,
        };

        let displayer = Displayer::new(&beacon, true);
        let expected = concat!(
            "address: 02:00:00:00:00:01\n",
            "advertise settings:\n",
            "  tx power                 : -15 dBm\n",
            "  interval                 : 250 ms\n",
            "advertise packet data:",
        );
        assert_eq!(format!("{displayer}"), expected);
    }

    #[test]
    fn test_display_ble_beacon_formats_populated_settings_and_data() {
        let beacon = BleBeacon {
            address: "02:00:00:00:00:01".to_string(),
            settings: Some(AdvertiseSettings {
                interval: Some(Interval::AdvertiseMode(AdvertiseMode::LowLatency)),
                tx_power: Some(TxPower::TxPowerLevel(AdvertiseTxPower::High)),
                scannable: true,
                timeout: 500,
            }),
            adv_data: Some(AdvertiseData {
                include_device_name: true,
                include_tx_power_level: true,
                manufacturer_data: vec![0xAA, 0xBB, 0xCC],
                services: vec![],
            }),
            scan_response: None,
        };

        let displayer = Displayer::new(&beacon, true);
        let expected = concat!(
            "address: 02:00:00:00:00:01\n",
            "advertise settings:\n",
            "  tx power level           : High\n",
            "  advertise mode           : LowLatency\n",
            "  scannable                : true\n",
            "  timeout                  : 500 ms\n",
            "advertise packet data:\n",
            "  include device name      : true\n",
            "  include tx power level   : true\n",
            "  manufacturer data bytes  : 3",
        );
        assert_eq!(format!("{displayer}"), expected);
    }

    #[test]
    fn test_format_optional_link_chip_id() {
        let cases = [(None, "ALL"), (Some(ChipId(0)), "ALL"), (Some(ChipId(7)), "7")];
        for (input, expected) in cases {
            assert_eq!(format_optional_link_chip_id(input), expected);
        }
    }
}
