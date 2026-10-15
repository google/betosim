// Copyright 2022 The Android Open Source Project
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

use clap::builder::{PossibleValue, TypedValueParser};
use clap::{Args, Parser, Subcommand, ValueEnum};
use hex::{FromHexError, decode as hex_to_bytes};
use netsim_proto::model::chip::ble_beacon::advertise_settings::{
    AdvertiseMode as AdvertiseModeProto, AdvertiseTxPower as AdvertiseTxPowerProto,
    Interval as IntervalProto, Tx_power as TxPowerProto,
};
use netsim_proto::model::chip::ble_beacon::{
    AdvertiseData as AdvertiseDataProto, AdvertiseSettings as AdvertiseSettingsProto,
};

use std::fmt;
use std::iter;
use std::str::FromStr;

#[derive(Debug, Parser)]
pub struct NetsimArgs {
    #[command(subcommand)]
    pub command: Command,
    /// Set verbose mode
    #[arg(short, long)]
    pub verbose: bool,
    /// Set custom grpc port
    #[arg(short, long)]
    pub port: Option<i32>,
    /// Set netsimd instance number to connect
    #[arg(short, long)]
    pub instance: Option<u16>,
    /// Set vsock cid:port pair
    #[arg(long)]
    pub vsock: Option<String>,
}

#[derive(Debug, Subcommand, PartialEq)]
#[command(infer_subcommands = true)]
pub enum Command {
    /// Print Netsim version information
    Version,
    /// Control the radio state of a device
    Radio(Radio),
    /// Set the device location
    Move(Move),
    /// Display device(s) information
    Devices(Devices),
    /// Reset Netsim device scene
    Reset,
    /// Open netsim Web UI
    Gui,
    /// Control the packet capture functionalities with commands: list, patch, get
    #[command(subcommand, visible_alias("pcap"))]
    Capture(Capture),
    /// Opens netsim artifacts directory (log, pcaps)
    Artifact,
    /// A chip that sends advertisements at a set interval
    #[command(subcommand)]
    Beacon(Beacon),
    /// Open Bumble Hive Web Page
    Bumble,
    /// Manage Link properties
    #[command(subcommand)]
    Link(Link),
}

#[derive(Debug, Args, PartialEq)]
pub struct Radio {
    /// Radio type
    #[arg(value_enum, ignore_case = true)]
    pub radio_type: RadioType,
    /// Radio status
    #[arg(value_enum, ignore_case = true)]
    pub status: UpDownStatus,
    /// Device name
    pub name: String,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
pub enum RadioType {
    Ble,
    Classic,
    Wifi,
    Uwb,
}

impl fmt::Display for RadioType {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
pub enum UpDownStatus {
    Up,
    Down,
}

impl fmt::Display for UpDownStatus {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

#[derive(Debug, Args, PartialEq)]
pub struct Move {
    /// Device name
    pub name: String,
    /// x position of device
    pub x: f32,
    /// y position of device
    pub y: f32,
    /// Optional z position of device
    pub z: Option<f32>,
}

#[derive(Debug, Args, PartialEq)]
pub struct Devices {
    /// Continuously print device(s) information every second
    #[arg(short, long)]
    pub continuous: bool,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum, Default)]
pub enum OnOffState {
    #[default]
    On,
    Off,
}

#[derive(Debug, Subcommand, PartialEq)]
pub enum Beacon {
    /// Create a beacon chip
    #[command(subcommand)]
    Create(BeaconCreate),
    /// Modify a beacon chip
    #[command(subcommand)]
    Patch(BeaconPatch),
    /// Remove a beacon chip
    Remove(BeaconRemove),
}

#[derive(Debug, Subcommand, PartialEq)]
pub enum BeaconCreate {
    /// Create a Bluetooth low-energy beacon chip
    Ble(BeaconCreateBle),
}

#[derive(Debug, Args, PartialEq, Default)]
pub struct BeaconCreateBle {
    /// Name of the device to create
    pub device_name: Option<String>,
    /// Name of the beacon chip to create within the new device. May only be specified if device_name is specified
    pub chip_name: Option<String>,
    /// Bluetooth address of the beacon. Must be a 6-byte hexadecimal string with each byte separated by a colon. Will be generated if not provided
    #[arg(long)]
    pub address: Option<String>,
    #[command(flatten)]
    pub settings: BeaconBleSettings,
    #[command(flatten)]
    pub advertise_data: BeaconBleAdvertiseData,
    #[command(flatten)]
    pub scan_response_data: BeaconBleScanResponseData,
}

#[derive(Debug, Subcommand, PartialEq)]
pub enum BeaconPatch {
    /// Modify a Bluetooth low-energy beacon chip
    Ble(BeaconPatchBle),
}

#[derive(Debug, Args, PartialEq, Default)]
pub struct BeaconPatchBle {
    /// Name of the device that contains the chip
    pub device_name: String,
    /// Name of the beacon chip to modify
    pub chip_name: String,
    /// Bluetooth address of the beacon. Must be a 6-byte hexadecimal string with each byte separated by a colon
    #[arg(long)]
    pub address: Option<String>,
    #[command(flatten)]
    pub settings: BeaconBleSettings,
    #[command(flatten)]
    pub advertise_data: BeaconBleAdvertiseData,
    #[command(flatten)]
    pub scan_response_data: BeaconBleScanResponseData,
}

#[derive(Debug, Args, PartialEq)]
pub struct BeaconRemove {
    /// Name of the device to remove
    pub device_name: String,
    /// Name of the beacon chip to remove. Can be omitted if the device has exactly 1 chip
    pub chip_name: Option<String>,
}

#[derive(Debug, Args, PartialEq, Default)]
pub struct BeaconBleAdvertiseData {
    /// Whether the device name should be included in the advertise packet
    #[arg(long, required = false)]
    pub include_device_name: bool,
    /// Whether the transmission power level should be included in the advertise packet
    #[arg(long, required = false)]
    pub include_tx_power_level: bool,
    /// Manufacturer-specific data given as bytes in hexadecimal
    #[arg(long)]
    pub manufacturer_data: Option<ParsableBytes>,
}

#[derive(Debug, Subcommand, PartialEq)]
pub enum Link {
    /// List all current links and their properties
    List,
    /// Add or modify link properties
    Patch(LinkPatchCommand),
    /// Remove link properties
    Delete(LinkDeleteCommand),
}

#[derive(Debug, Args, PartialEq)]
pub struct LinkPatchCommand {
    #[command(subcommand)]
    pub command: LinkPatch,
}

#[derive(Debug, Subcommand, PartialEq)]
pub enum LinkPatch {
    /// Patch RSSI (Received Signal Strength Indication) for a link.
    Rssi(RssiPatch),
}

#[derive(Debug, Args, PartialEq)]
pub struct LinkDeleteCommand {
    #[command(subcommand)]
    pub command: LinkDelete,
}

#[derive(Debug, Subcommand, PartialEq)]
pub enum LinkDelete {
    /// Delete RSSI (Received Signal Strength Indication) for a link.
    Rssi(RssiDelete),
}

#[derive(Debug, Args, PartialEq)]
pub struct RssiPatch {
    /// Radio type for the link.
    #[arg(value_enum, ignore_case = true)]
    pub radio_type: RadioType,
    /// RSSI value in dBm (e.g., -60). Must be between -128 and 127.
    #[arg(allow_hyphen_values = true)]
    pub value: i8,
    /// Identifier for the sender chip. Defaults to 0 (ANY_CHIP), affecting all senders to the specified receiver.
    pub sender_id: Option<u32>,
    /// Identifier for the receiver chip. Defaults to 0 (ANY_CHIP), affecting all receivers from the specified sender.
    pub receiver_id: Option<u32>,
}

#[derive(Debug, Args, PartialEq)]
pub struct RssiDelete {
    /// Radio type for the link.
    #[arg(value_enum, ignore_case = true)]
    pub radio_type: RadioType,
    /// Identifier for the sender chip. Defaults to 0 (ANY_CHIP).
    pub sender_id: Option<u32>,
    /// Identifier for the receiver chip. Defaults to 0 (ANY_CHIP).
    pub receiver_id: Option<u32>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ParsableBytes(pub Vec<u8>);

impl ParsableBytes {
    fn unwrap(self) -> Vec<u8> {
        self.0
    }
}

impl FromStr for ParsableBytes {
    type Err = FromHexError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        hex_to_bytes(s.strip_prefix("0x").unwrap_or(s)).map(ParsableBytes)
    }
}

#[derive(Debug, Args, PartialEq, Default)]
pub struct BeaconBleScanResponseData {
    /// Whether the device name should be included in the scan response packet
    #[arg(long, required = false)]
    pub scan_response_include_device_name: bool,
    /// Whether the transmission power level should be included in the scan response packet
    #[arg(long, required = false)]
    pub scan_response_include_tx_power_level: bool,
    /// Manufacturer-specific data to include in the scan response packet given as bytes in hexadecimal
    #[arg(long, value_name = "MANUFACTURER_DATA")]
    pub scan_response_manufacturer_data: Option<ParsableBytes>,
}

#[derive(Debug, Args, PartialEq, Default)]
pub struct BeaconBleSettings {
    /// Set advertise mode to control the advertising latency
    #[arg(long, value_parser = IntervalParser)]
    pub advertise_mode: Option<Interval>,
    /// Set advertise TX power level to control the beacon's transmission power
    #[arg(long, value_parser = TxPowerParser, allow_hyphen_values = true)]
    pub tx_power_level: Option<TxPower>,
    /// Set whether the beacon will respond to scan requests
    #[arg(long)]
    pub scannable: bool,
    /// Limit advertising to an amount of time given in milliseconds
    #[arg(long, value_name = "MS")]
    pub timeout: Option<u64>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Interval {
    Mode(AdvertiseMode),
    Milliseconds(u64),
}

#[derive(Clone)]
struct IntervalParser;

impl TypedValueParser for IntervalParser {
    type Value = Interval;

    fn parse_ref(
        &self,
        cmd: &clap::Command,
        arg: Option<&clap::Arg>,
        value: &std::ffi::OsStr,
    ) -> Result<Self::Value, clap::Error> {
        let millis_parser = clap::value_parser!(u64);
        let mode_parser = clap::value_parser!(AdvertiseMode);

        mode_parser
            .parse_ref(cmd, arg, value)
            .map(Self::Value::Mode)
            .or(millis_parser.parse_ref(cmd, arg, value).map(Self::Value::Milliseconds))
    }

    fn possible_values(&self) -> Option<Box<dyn Iterator<Item = PossibleValue> + '_>> {
        Some(Box::new(
            AdvertiseMode::value_variants().iter().map(|v| v.to_possible_value().unwrap()).chain(
                iter::once(
                    PossibleValue::new("<MS>").help("An exact advertise interval in milliseconds"),
                ),
            ),
        ))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum TxPower {
    Level(TxPowerLevel),
    Dbm(i8),
}

#[derive(Clone)]
struct TxPowerParser;

impl TypedValueParser for TxPowerParser {
    type Value = TxPower;

    fn parse_ref(
        &self,
        cmd: &clap::Command,
        arg: Option<&clap::Arg>,
        value: &std::ffi::OsStr,
    ) -> Result<Self::Value, clap::Error> {
        let dbm_parser = clap::value_parser!(i8);
        let level_parser = clap::value_parser!(TxPowerLevel);

        level_parser
            .parse_ref(cmd, arg, value)
            .map(Self::Value::Level)
            .or(dbm_parser.parse_ref(cmd, arg, value).map(Self::Value::Dbm))
    }

    fn possible_values(&self) -> Option<Box<dyn Iterator<Item = PossibleValue> + '_>> {
        Some(Box::new(
            TxPowerLevel::value_variants().iter().map(|v| v.to_possible_value().unwrap()).chain(
                iter::once(
                    PossibleValue::new("<DBM>").help("An exact transmit power level in dBm"),
                ),
            ),
        ))
    }
}

#[derive(Debug, Clone, ValueEnum, PartialEq)]
pub enum AdvertiseMode {
    /// Lowest power consumption, preferred advertising mode
    LowPower,
    /// Balanced between advertising frequency and power consumption
    Balanced,
    /// Highest power consumption
    LowLatency,
}

#[derive(Debug, Clone, ValueEnum, PartialEq)]
pub enum TxPowerLevel {
    /// Lowest transmission power level
    UltraLow,
    /// Low transmission power level
    Low,
    /// Medium transmission power level
    Medium,
    /// High transmission power level
    High,
}

#[derive(Debug, Subcommand, PartialEq)]
pub enum Capture {
    /// List currently available Captures (packet captures)
    List(ListCapture),
    /// Patch a Capture source to turn packet capture on/off
    Patch(PatchCapture),
    /// Download the packet capture content
    Get(GetCapture),
}

#[derive(Debug, Args, PartialEq, Default)]
pub struct ListCapture {
    /// Optional strings of pattern for captures to list. Possible filter fields include Capture ID, Device Name, and Chip Kind
    pub patterns: Vec<String>,
    /// Continuously print Capture information every second
    #[arg(short, long)]
    pub continuous: bool,
}

#[derive(Debug, Args, PartialEq, Default)]
pub struct PatchCapture {
    /// Packet capture state
    #[arg(value_enum, ignore_case = true)]
    pub state: OnOffState,
    /// Optional strings of pattern for captures to patch. Possible filter fields include Capture ID, Device Name, and Chip Kind
    pub patterns: Vec<String>,
}

#[derive(Debug, Args, PartialEq, Default)]
pub struct GetCapture {
    /// Optional strings of pattern for captures to get. Possible filter fields include Capture ID, Device Name, and Chip Kind
    pub patterns: Vec<String>,
    /// Directory to store downloaded capture file(s)
    #[arg(short = 'o', long)]
    pub location: Option<String>,
    #[arg(skip)]
    pub filenames: Vec<String>,
    #[arg(skip)]
    pub current_file: String,
}

impl From<&BeaconBleSettings> for AdvertiseSettingsProto {
    fn from(value: &BeaconBleSettings) -> Self {
        AdvertiseSettingsProto {
            interval: value.advertise_mode.as_ref().map(IntervalProto::from),
            tx_power: value.tx_power_level.as_ref().map(TxPowerProto::from),
            scannable: value.scannable,
            timeout: value.timeout.unwrap_or_default(),
            ..Default::default()
        }
    }
}

impl From<&Interval> for IntervalProto {
    fn from(value: &Interval) -> Self {
        match value {
            Interval::Mode(mode) => IntervalProto::AdvertiseMode(
                match mode {
                    AdvertiseMode::LowPower => AdvertiseModeProto::LOW_POWER,
                    AdvertiseMode::Balanced => AdvertiseModeProto::BALANCED,
                    AdvertiseMode::LowLatency => AdvertiseModeProto::LOW_LATENCY,
                }
                .into(),
            ),
            Interval::Milliseconds(ms) => IntervalProto::Milliseconds(*ms),
        }
    }
}

impl From<&TxPower> for TxPowerProto {
    fn from(value: &TxPower) -> Self {
        match value {
            TxPower::Level(level) => TxPowerProto::TxPowerLevel(
                match level {
                    TxPowerLevel::UltraLow => AdvertiseTxPowerProto::ULTRA_LOW,
                    TxPowerLevel::Low => AdvertiseTxPowerProto::LOW,
                    TxPowerLevel::Medium => AdvertiseTxPowerProto::MEDIUM,
                    TxPowerLevel::High => AdvertiseTxPowerProto::HIGH,
                }
                .into(),
            ),
            TxPower::Dbm(dbm) => TxPowerProto::Dbm((*dbm).into()),
        }
    }
}

impl From<&BeaconBleAdvertiseData> for AdvertiseDataProto {
    fn from(value: &BeaconBleAdvertiseData) -> Self {
        AdvertiseDataProto {
            include_device_name: value.include_device_name,
            include_tx_power_level: value.include_tx_power_level,
            manufacturer_data: value
                .manufacturer_data
                .clone()
                .map(ParsableBytes::unwrap)
                .unwrap_or_default(),
            ..Default::default()
        }
    }
}

impl From<&BeaconBleScanResponseData> for AdvertiseDataProto {
    fn from(value: &BeaconBleScanResponseData) -> Self {
        AdvertiseDataProto {
            include_device_name: value.scan_response_include_device_name,
            include_tx_power_level: value.scan_response_include_tx_power_level,
            manufacturer_data: value
                .scan_response_manufacturer_data
                .clone()
                .map(ParsableBytes::unwrap)
                .unwrap_or_default(),
            ..Default::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hex_parser_succeeds() {
        let hex = ParsableBytes::from_str("beef1234");
        assert!(hex.is_ok(), "{}", hex.unwrap_err());
        let hex = hex.unwrap().unwrap();

        assert_eq!(vec![0xbeu8, 0xef, 0x12, 0x34], hex);
    }

    #[test]
    fn test_hex_parser_prefix_succeeds() {
        let hex = ParsableBytes::from_str("0xabcd");
        assert!(hex.is_ok(), "{}", hex.unwrap_err());
        let hex = hex.unwrap().unwrap();

        assert_eq!(vec![0xabu8, 0xcd], hex);
    }

    #[test]
    fn test_hex_parser_empty_str_succeeds() {
        let hex = ParsableBytes::from_str("");
        assert!(hex.is_ok(), "{}", hex.unwrap_err());
        let hex = hex.unwrap().unwrap();

        assert_eq!(Vec::<u8>::new(), hex);
    }

    #[test]
    fn test_hex_parser_bad_digit_fails() {
        assert!(ParsableBytes::from_str("0xabcdefg").is_err());
    }
}
