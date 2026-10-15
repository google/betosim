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

use std::cmp::max;

use common::util::time_display::TimeDisplay;
use netsim_proto::{common::ChipKind, frontend, model};
use protobuf::MessageField;
use protobuf_json_mapping::print_to_string;

use crate::{
    args::{self, Beacon, BeaconCreate, BeaconPatch, Capture, Command, Link, OnOffState},
    display::{Displayer, LinkChipIdDisplay},
    grpc_client::{GrpcRequest, GrpcResponse},
};

impl args::Command {
    pub(crate) fn print_response(
        &self,
        response: &GrpcResponse,
        cells: Option<&[netsim_proto::cell::Cell]>,
        request: Option<&GrpcRequest>,
        verbose: bool,
    ) -> crate::error::Result<()> {
        match self {
            Command::Radio(cmd) => {
                if verbose {
                    println!(
                        "Radio {} is {} for {}",
                        cmd.radio_type,
                        cmd.status,
                        cmd.name.to_owned()
                    );
                }
            }
            Command::Move(cmd) => {
                if verbose {
                    println!(
                        "Moved device:{} to x: {:.2}, y: {:.2}, z: {:.2}",
                        cmd.name,
                        cmd.x,
                        cmd.y,
                        cmd.z.unwrap_or_default()
                    )
                }
            }
            Command::Devices(cmd) => {
                let GrpcResponse::ListDevice(res) = response else {
                    return Err(
                        format!("Expected to print ListDeviceResponse. Got: {response:?}").into()
                    );
                };
                if cmd.json {
                    let json = print_to_string(res).map_err(|e| {
                        crate::error::Error::from(format!("JSON mapping error: {e}"))
                    })?;
                    println!("{json}");
                } else {
                    let mut displayer = Displayer::new(res, verbose);
                    if let Some(cells) = cells {
                        displayer = displayer.with_cells(cells);
                    }
                    println!("{}", displayer);
                }
            }
            Command::Reset => {
                if verbose {
                    println!("All devices have been reset.");
                }
            }
            Command::Capture(Capture::List(cmd)) => {
                let GrpcResponse::ListCapture(res) = response else {
                    return Err(format!(
                        "Expected to print ListCaptureResponse. Got: {response:?}"
                    )
                    .into());
                };
                Self::print_list_capture_response(
                    &mut res.clone(),
                    verbose,
                    cmd.patterns.to_owned(),
                )
            }
            Command::Capture(Capture::Patch(cmd)) => {
                if verbose {
                    println!(
                        "Patched Capture state to {}",
                        Self::on_off_state_to_string(cmd.state),
                    );
                }
            }
            Command::Capture(Capture::Get(cmd)) => {
                if verbose {
                    println!("Successfully downloaded file: {}", cmd.current_file);
                }
            }

            Command::Beacon(action) => match action {
                Beacon::Create(kind) => match kind {
                    BeaconCreate::Ble(_) | BeaconCreate::Uwb(_) => {
                        if !verbose {
                            return Ok(());
                        }
                        let GrpcResponse::CreateDevice(res) = response else {
                            return Err(format!(
                                "Expected to print CreateDeviceResponse. Got: {response:?}"
                            )
                            .into());
                        };
                        let device = &res.device;
                        match device.chips.as_slice() {
                            [chip] => {
                                let kind_str = match kind {
                                    BeaconCreate::Ble(_) => "ble beacon",
                                    BeaconCreate::Uwb(_) => "uwb anchor",
                                };
                                println!(
                                    "Created device '{}' with {kind_str} chip '{}'",
                                    device.name, chip.name
                                );
                            }
                            _ => {
                                return Err("the gRPC request completed successfully but the response contained an unexpected number of chips".into());
                            }
                        }
                    }
                },
                Beacon::Patch(kind) => match kind {
                    BeaconPatch::Ble(args) => {
                        if !verbose {
                            return Ok(());
                        }
                        if let Some(advertise_mode) = &args.settings.advertise_mode {
                            match advertise_mode {
                                args::Interval::Mode(mode) => {
                                    println!("Set advertise mode to {mode:#?}")
                                }
                                args::Interval::Milliseconds(ms) => {
                                    println!("Set advertise interval to {ms} ms")
                                }
                            }
                        }
                        if let Some(tx_power_level) = &args.settings.tx_power_level {
                            match tx_power_level {
                                args::TxPower::Level(level) => {
                                    println!("Set transmit power level to {level:#?}")
                                }
                                args::TxPower::Dbm(dbm) => {
                                    println!("Set transmit power level to {dbm} dBm")
                                }
                            }
                        }
                        if args.settings.scannable {
                            println!("Set scannable to true");
                        }
                        if let Some(timeout) = args.settings.timeout {
                            println!("Set timeout to {timeout} ms");
                        }
                        if args.advertise_data.include_device_name {
                            println!("Added the device's name to the advertise packet")
                        }
                        if args.advertise_data.include_tx_power_level {
                            println!(
                                "Added the beacon's transmit power level to the advertise packet"
                            )
                        }
                        if args.advertise_data.manufacturer_data.is_some() {
                            println!("Added manufacturer data to the advertise packet")
                        }
                        if args.settings.scannable {
                            println!("Set scannable to true");
                        }
                        if let Some(timeout) = args.settings.timeout {
                            println!("Set timeout to {timeout} ms");
                        }
                    }
                },
                Beacon::Remove(args) => {
                    if !verbose {
                        return Ok(());
                    }
                    println!("Removed device '{}'", args.device_name)
                }
            },

            Command::Link(link_cmd) => match link_cmd {
                Link::Create(_) => {
                    if verbose
                        && let GrpcResponse::CreateLink(frontend::CreateLinkResponse {
                            link: MessageField(Some(match_link)),
                            ..
                        }) = response
                    {
                        println!(
                            "Successfully created link (Sender: {}, Receiver: {}, Type: {:?})",
                            LinkChipIdDisplay(match_link.sender_id),
                            LinkChipIdDisplay(match_link.receiver_id),
                            match_link.kind
                        );
                    }
                }
                Link::List => {
                    return Err("Link::List uses the REST API and is handled in main.rs.".into());
                }
                Link::Patch(args) => {
                    if let Some(rssi) = args.rssi
                        && verbose
                    {
                        // fall back to args (wildcards)
                        let (sender, receiver) = match request {
                            Some(GrpcRequest::PatchLink(req)) => {
                                match &req.link.clone().into_option() {
                                    Some(link) => (Some(link.sender_id), Some(link.receiver_id)),
                                    None => (args.sender, args.receiver),
                                }
                            }
                            _ => (args.sender, args.receiver),
                        };

                        println!(
                            "Successfully patched RSSI for link (Sender: {}, Receiver: {}, Type: {}) to {}.",
                            LinkChipIdDisplay(sender.unwrap_or(0)),
                            LinkChipIdDisplay(receiver.unwrap_or(0)),
                            args.chip_kind,
                            rssi
                        );
                    }
                }
                Link::Delete(args) => {
                    if verbose {
                        // Extract specific link info from request if available
                        let (sender, receiver) = if let Some(GrpcRequest::DeleteLink(req)) = request
                        {
                            #[allow(deprecated)]
                            if let Some(link) = &req.link.clone().into_option() {
                                (Some(link.sender_id), Some(link.receiver_id))
                            } else {
                                (args.sender, args.receiver)
                            }
                        } else {
                            (args.sender, args.receiver)
                        };

                        println!(
                            "Successfully deleted RSSI for link (Sender: {}, Receiver: {}, Type: {}).",
                            LinkChipIdDisplay(sender.unwrap_or(0)),
                            LinkChipIdDisplay(receiver.unwrap_or(0)),
                            args.chip_kind
                        );
                    }
                }
            },
            // These commands are intercepted early in main.rs and have no direct gRPC pipeline.
            _ => {
                return Err("No Grpc Response for this Command.".into());
            }
        }
        Ok(())
    }

    fn capture_state_to_string(state: Option<bool>) -> String {
        state.map(|value| if value { "on" } else { "off" }).unwrap_or("unknown").to_string()
    }

    fn on_off_state_to_string(state: OnOffState) -> String {
        match state {
            OnOffState::On => "on".to_string(),
            OnOffState::Off => "off".to_string(),
        }
    }

    fn print_list_capture_response(
        response: &mut frontend::ListCaptureResponse,
        verbose: bool,
        patterns: Vec<String>,
    ) {
        if response.captures.is_empty() {
            if verbose {
                println!("No available Capture found.");
            }
            return;
        }
        if patterns.is_empty() {
            println!("List of Captures:");
        } else {
            // Filter out list of captures with matching patterns
            Self::filter_captures(&mut response.captures, &patterns);
            if response.captures.is_empty() {
                if verbose {
                    println!("No available Capture found matching pattern(s) `{patterns:?}`:");
                }
                return;
            }
            println!("List of Captures matching pattern(s) `{patterns:?}`:");
        }
        // Create the header row and determine column widths
        let id_hdr = "ID";
        let name_hdr = "Device Name";
        let chipkind_hdr = "Chip Kind";
        let state_hdr = "State";
        let time_hdr = "Timestamp";
        let records_hdr = "Records";
        let size_hdr = "Size (bytes)";
        let id_width = 4; // ID width of 4 since capture id (=chip_id) starts at 1000
        let state_width = 8; // State width of 8 for 'detached' if device is disconnected
        let chipkind_width = 11; // ChipKind width 11 for 'UNSPECIFIED'
        let time_width = 9; // Timestamp width 9 for header (value format set to HH:MM:SS)
        let name_width = max(
            (response.captures.iter().max_by_key(|x| x.device_name.len()))
                .unwrap_or_default()
                .device_name
                .len(),
            name_hdr.len(),
        );
        let records_width = max(
            (response.captures.iter().max_by_key(|x| x.records))
                .unwrap_or_default()
                .records
                .to_string()
                .len(),
            records_hdr.len(),
        );
        let size_width = max(
            (response.captures.iter().max_by_key(|x| x.size))
                .unwrap_or_default()
                .size
                .to_string()
                .len(),
            size_hdr.len(),
        );
        // Print header for capture list
        println!(
            "{}",
            if verbose {
                format!(
                    "{id_hdr:id_width$} | {name_hdr:name_width$} | {chipkind_hdr:chipkind_width$} | {state_hdr:state_width$} | {time_hdr:time_width$} | {records_hdr:records_width$} | {size_hdr:size_width$} |",
                )
            } else {
                format!(
                    "{name_hdr:name_width$} | {chipkind_hdr:chipkind_width$} | {state_hdr:state_width$} | {records_hdr:records_width$} |"
                )
            }
        );
        // Print information of each Capture
        for capture in &response.captures {
            println!(
                "{}",
                if verbose {
                    format!(
                        "{:id_width$} | {:name_width$} | {:chipkind_width$} | {:state_width$} | {:time_width$} | {:records_width$} | {:size_width$} |",
                        capture.id.to_string(),
                        capture.device_name,
                        Self::chip_kind_to_string(capture.chip_kind.enum_value_or_default()),
                        if capture.valid {
                            Self::capture_state_to_string(capture.state)
                        } else {
                            "detached".to_string()
                        },
                        TimeDisplay::new(
                            capture.timestamp.get_or_default().seconds,
                            capture.timestamp.get_or_default().nanos as u32,
                        )
                        .utc_display_hms(),
                        capture.records,
                        capture.size,
                    )
                } else {
                    format!(
                        "{:name_width$} | {:chipkind_width$} | {:state_width$} | {:records_width$} |",
                        capture.device_name,
                        Self::chip_kind_to_string(capture.chip_kind.enum_value_or_default()),
                        if capture.valid {
                            Self::capture_state_to_string(capture.state)
                        } else {
                            "detached".to_string()
                        },
                        capture.records,
                    )
                }
            );
        }
    }

    pub(crate) fn chip_kind_to_string(chip_kind: ChipKind) -> String {
        match chip_kind {
            ChipKind::UNSPECIFIED => "UNSPECIFIED",
            ChipKind::BLUETOOTH => "BLUETOOTH",
            ChipKind::WIFI => "WIFI",
            ChipKind::UWB => "UWB",
            ChipKind::BLUETOOTH_BEACON => "BLUETOOTH_BEACON",
            ChipKind::NFC => "NFC",
            ChipKind::CELLULAR => "CELLULAR",
            ChipKind::CELLULAR_DATA => "CELLULAR_DATA",
            ChipKind::ETHERNET => "ETHERNET",
        }
        .into()
    }

    pub(crate) fn filter_captures(captures: &mut Vec<model::Capture>, keys: &[String]) {
        // Filter out list of captures with matching pattern
        captures.retain(|capture| {
            keys.iter().map(|key| key.to_uppercase()).all(|key| {
                capture.id.to_string().contains(&key)
                    || capture.device_name.to_uppercase().contains(&key)
                    || Self::chip_kind_to_string(capture.chip_kind.enum_value_or_default())
                        .contains(&key)
            })
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn test_filter_captures_helper(patterns: Vec<String>, expected_captures: Vec<model::Capture>) {
        let mut captures = all_test_captures();
        Command::filter_captures(&mut captures, &patterns);
        assert_eq!(captures, expected_captures);
    }

    fn capture_1() -> model::Capture {
        model::Capture {
            id: 4001,
            chip_kind: ChipKind::BLUETOOTH.into(),
            device_name: "device 1".to_string(),
            ..Default::default()
        }
    }
    fn capture_1_wifi() -> model::Capture {
        model::Capture {
            id: 4002,
            chip_kind: ChipKind::WIFI.into(),
            device_name: "device 1".to_string(),
            ..Default::default()
        }
    }
    fn capture_2() -> model::Capture {
        model::Capture {
            id: 4003,
            chip_kind: ChipKind::BLUETOOTH.into(),
            device_name: "device 2".to_string(),
            ..Default::default()
        }
    }
    fn capture_3() -> model::Capture {
        model::Capture {
            id: 4004,
            chip_kind: ChipKind::WIFI.into(),
            device_name: "device 3".to_string(),
            ..Default::default()
        }
    }
    fn capture_4_uwb() -> model::Capture {
        model::Capture {
            id: 4005,
            chip_kind: ChipKind::UWB.into(),
            device_name: "device 4".to_string(),
            ..Default::default()
        }
    }
    fn all_test_captures() -> Vec<model::Capture> {
        vec![capture_1(), capture_1_wifi(), capture_2(), capture_3(), capture_4_uwb()]
    }

    #[test]
    fn test_no_match() {
        test_filter_captures_helper(vec!["test".to_string()], vec![]);
    }

    #[test]
    fn test_all_match() {
        test_filter_captures_helper(vec!["device".to_string()], all_test_captures());
    }

    #[test]
    fn test_match_capture_id() {
        test_filter_captures_helper(vec!["4001".to_string()], vec![capture_1()]);
        test_filter_captures_helper(vec!["03".to_string()], vec![capture_2()]);
        test_filter_captures_helper(vec!["40".to_string()], all_test_captures());
    }

    #[test]
    fn test_match_device_name() {
        test_filter_captures_helper(
            vec!["device 1".to_string()],
            vec![capture_1(), capture_1_wifi()],
        );
        test_filter_captures_helper(vec![" 2".to_string()], vec![capture_2()]);
    }

    #[test]
    fn test_match_device_name_case_insensitive() {
        test_filter_captures_helper(
            vec!["DEVICE 1".to_string()],
            vec![capture_1(), capture_1_wifi()],
        );
    }

    #[test]
    fn test_match_wifi() {
        test_filter_captures_helper(vec!["wifi".to_string()], vec![capture_1_wifi(), capture_3()]);
        test_filter_captures_helper(vec!["WIFI".to_string()], vec![capture_1_wifi(), capture_3()]);
    }

    #[test]
    fn test_match_uwb() {
        test_filter_captures_helper(vec!["uwb".to_string()], vec![capture_4_uwb()]);
        test_filter_captures_helper(vec!["UWB".to_string()], vec![capture_4_uwb()]);
    }

    #[test]
    fn test_match_bt() {
        test_filter_captures_helper(vec!["BLUETOOTH".to_string()], vec![capture_1(), capture_2()]);
        test_filter_captures_helper(vec!["blue".to_string()], vec![capture_1(), capture_2()]);
    }

    #[test]
    fn test_match_name_and_chip() {
        test_filter_captures_helper(
            vec!["device 1".to_string(), "wifi".to_string()],
            vec![capture_1_wifi()],
        );
    }
}
