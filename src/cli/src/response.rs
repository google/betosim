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
use netsim_proto::frontend;
use netsim_rest_api::CaptureInfo;
use protobuf::MessageField;

use crate::{
    args::{self, Beacon, BeaconCreate, Command, Link, OnOffState},
    display::LinkChipIdDisplay,
    grpc_client::{GrpcRequest, GrpcResponse},
};

pub(crate) fn print_beacon_patch_ble_verbose(args: &args::BeaconPatchBle) {
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
        println!("Added the beacon's transmit power level to the advertise packet")
    }
    if args.advertise_data.manufacturer_data.is_some() {
        println!("Added manufacturer data to the advertise packet")
    }
}

impl args::Command {
    pub(crate) fn print_response(
        &self,
        response: &GrpcResponse,
        request: Option<&GrpcRequest>,
        verbose: bool,
    ) -> crate::error::Result<()> {
        match self {
            Command::Beacon(Beacon::Create(BeaconCreate::Uwb(_))) => {
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
                        println!(
                            "Created device '{}' with uwb anchor chip '{}'",
                            device.name, chip.name
                        );
                    }
                    _ => {
                        return Err("the gRPC request completed successfully but the response contained an unexpected number of chips".into());
                    }
                }
            }

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

    pub(crate) fn on_off_state_to_string(state: OnOffState) -> String {
        match state {
            OnOffState::On => "on".to_string(),
            OnOffState::Off => "off".to_string(),
        }
    }
}

pub(crate) fn print_capture_list(captures: &[CaptureInfo], patterns: &[String], verbose: bool) {
    if captures.is_empty() {
        if verbose {
            println!("No available Capture found.");
        }
        return;
    }
    if patterns.is_empty() {
        println!("List of Captures:");
    } else {
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
    let state_width = state_hdr.len();
    let chipkind_width = 11; // ChipKind width 11 for 'UNSPECIFIED'
    let time_width = 9; // Timestamp width 9 for header (value format set to HH:MM:SS)
    let column_width = |hdr: &str, len: fn(&CaptureInfo) -> usize| {
        max(captures.iter().map(len).max().unwrap_or_default(), hdr.len())
    };
    let name_width = column_width(name_hdr, |c| c.device_name.len());
    let records_width = column_width(records_hdr, |c| c.records_written.to_string().len());
    let size_width = column_width(size_hdr, |c| c.bytes_written.to_string().len());
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
    for capture in captures {
        let chip_kind_str = capture.chip_kind.as_str();
        let state_str = if capture.enabled { "on" } else { "off" };
        println!(
            "{}",
            if verbose {
                format!(
                    "{:id_width$} | {:name_width$} | {:chipkind_width$} | {:state_width$} | {:time_width$} | {:records_width$} | {:size_width$} |",
                    capture.chip_id.0.to_string(),
                    capture.device_name,
                    chip_kind_str,
                    state_str,
                    TimeDisplay::new(capture.seconds, capture.nanos as u32).utc_display_hms(),
                    capture.records_written,
                    capture.bytes_written,
                )
            } else {
                format!(
                    "{:name_width$} | {:chipkind_width$} | {:state_width$} | {:records_width$} |",
                    capture.device_name, chip_kind_str, state_str, capture.records_written,
                )
            }
        );
    }
}

pub(crate) fn filter_captures(captures: &mut Vec<CaptureInfo>, keys: &[String]) {
    let upper_keys: Vec<String> = keys.iter().map(|k| k.to_uppercase()).collect();
    captures.retain(|capture| {
        let chip_id_str = capture.chip_id.0.to_string();
        let device_upper = capture.device_name.to_uppercase();
        let chip_kind_str = capture.chip_kind.as_str();
        upper_keys.iter().all(|key| {
            chip_id_str.contains(key) || device_upper.contains(key) || chip_kind_str.contains(key)
        })
    });
}

#[cfg(test)]
mod tests {
    use netsim_model::{ChipId, ChipKind};

    use super::*;
    fn test_filter_captures_helper(patterns: Vec<String>, expected_captures: Vec<CaptureInfo>) {
        let mut captures = all_test_captures();
        filter_captures(&mut captures, &patterns);
        assert_eq!(captures, expected_captures);
    }

    fn make_capture(id: u32, chip_kind: ChipKind, device_name: &str) -> CaptureInfo {
        CaptureInfo {
            chip_id: ChipId(id),
            chip_kind,
            device_name: device_name.to_string(),
            enabled: false,
            records_written: 0,
            bytes_written: 0,
            seconds: 0,
            nanos: 0,
            filepath: None,
        }
    }

    fn capture_1() -> CaptureInfo {
        make_capture(4001, ChipKind::BLUETOOTH, "device 1")
    }
    fn capture_1_wifi() -> CaptureInfo {
        make_capture(4002, ChipKind::WIFI, "device 1")
    }
    fn capture_2() -> CaptureInfo {
        make_capture(4003, ChipKind::BLUETOOTH, "device 2")
    }
    fn capture_3() -> CaptureInfo {
        make_capture(4004, ChipKind::WIFI, "device 3")
    }
    fn capture_4_uwb() -> CaptureInfo {
        make_capture(4005, ChipKind::UWB, "device 4")
    }
    fn all_test_captures() -> Vec<CaptureInfo> {
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
