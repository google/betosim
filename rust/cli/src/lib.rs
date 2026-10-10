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

//! Command Line Interface for Netsim

mod args;
mod browser;
mod display;
mod error;
mod file_handler;
mod grpc_client;
mod requests;
mod response;

use netsim_common::util::ini_file::get_server_address;
use netsim_common::util::os_utils::get_instance;
use netsim_proto::frontend;

use grpcio::{ChannelBuilder, EnvBuilder};
use log::error;
use std::env;
use std::fs::File;
use std::path::PathBuf;

use crate::error::{Error, Result};
use crate::grpc_client::{ClientResponseReader, GrpcRequest, GrpcResponse};
use netsim_proto::frontend_grpc::FrontendServiceClient;

use args::{GetCapture, NetsimArgs};
use clap::Parser;
use file_handler::FileHandler;
use netsim_common::util::netsim_logger;

// helper function to process streaming Grpc request
fn perform_streaming_request(
    client: &FrontendServiceClient,
    cmd: &mut GetCapture,
    req: &frontend::GetCaptureRequest,
    filename: &str,
) -> Result<()> {
    let dir = if cmd.location.is_some() {
        PathBuf::from(cmd.location.to_owned().unwrap())
    } else {
        env::current_dir().unwrap()
    };
    let output_file = dir.join(filename);
    cmd.current_file = output_file.display().to_string();
    grpc_client::get_capture(
        client,
        req,
        &mut ClientResponseReader {
            handler: Box::new(FileHandler {
                file: File::create(&output_file).unwrap_or_else(|_| {
                    panic!("Failed to create file: {}", &output_file.display())
                }),
                path: output_file,
            }),
        },
    )
}

/// helper function to send the Grpc request(s) and handle the response(s) per the given command
fn perform_command(
    command: &mut args::Command,
    client: FrontendServiceClient,
    verbose: bool,
) -> Result<()> {
    // Get command's gRPC request(s)
    let requests = match command {
        args::Command::Capture(args::Capture::Patch(_) | args::Capture::Get(_)) => {
            command.get_requests(&client)
        }
        args::Command::Beacon(args::Beacon::Remove(_)) => {
            vec![args::Command::Devices(args::Devices { continuous: false }).get_request()]
        }
        _ => vec![command.get_request()],
    };
    let mut process_error = false;
    // Process each request
    for (i, req) in requests.iter().enumerate() {
        let result = match command {
            // Continuous option sends the gRPC call every second
            &mut args::Command::Devices(ref cmd) if cmd.continuous => {
                continuous_perform_command(command, &client, req, verbose)?;
                panic!("Continuous command interrupted. Exiting.");
            }
            &mut args::Command::Capture(args::Capture::List(ref cmd)) if cmd.continuous => {
                continuous_perform_command(command, &client, req, verbose)?;
                panic!("Continuous command interrupted. Exiting.");
            }
            // Get Capture use streaming gRPC reader request
            args::Command::Capture(args::Capture::Get(cmd)) => {
                let GrpcRequest::GetCapture(request) = req else {
                    panic!("Expected to find GetCaptureRequest. Got: {req:?}");
                };
                perform_streaming_request(&client, cmd, request, &cmd.filenames[i].to_owned())?;
                Ok(None)
            }
            &mut args::Command::Beacon(args::Beacon::Remove(ref cmd)) => {
                let response = grpc_client::send_grpc(&client, &GrpcRequest::ListDevice)?;
                let GrpcResponse::ListDevice(response) = response else {
                    panic!("Expected to find ListDeviceResponse. Got: {response:?}");
                };
                let id = find_id_for_remove(response, cmd)?;
                let res = grpc_client::send_grpc(
                    &client,
                    &GrpcRequest::DeleteChip(frontend::DeleteChipRequest {
                        id,
                        ..Default::default()
                    }),
                )?;
                Ok(Some(res))
            }
            // All other commands use a single gRPC call
            _ => {
                let response = grpc_client::send_grpc(&client, req)?;
                Ok(Some(response))
            }
        };
        if let Err(e) = process_result(command, result, verbose) {
            error!("{e}");
            process_error = true;
        };
    }
    if process_error {
        return Err("Not all requests were processed successfully.".into());
    }
    Ok(())
}

fn find_id_for_remove(
    response: frontend::ListDeviceResponse,
    cmd: &args::BeaconRemove,
) -> Result<u32> {
    let devices = response.devices;
    let id = devices
        .iter()
        .find(|device| device.name == cmd.device_name)
        .and_then(|device| cmd.chip_name.as_ref().map_or(
            (device.chips.len() == 1).then_some(&device.chips[0]),
            |chip_name| device.chips.iter().find(|chip| &chip.name == chip_name)
        ))
        .ok_or_else(|| {
            cmd.chip_name
                .as_ref()
                .map_or_else(
                    || Error::from(format!("failed to delete chip: device '{}' has multiple possible candidates, please specify a chip name", cmd.device_name)),
                    |chip_name| {
                        Error::from(format!(
                            "failed to delete chip: could not find chip '{}' on device '{}'",
                            chip_name, cmd.device_name
                        ))
                    },
                )
        })?
        .id;

    Ok(id)
}

/// Check and handle the gRPC call result
fn continuous_perform_command(
    command: &args::Command,
    client: &FrontendServiceClient,
    grpc_request: &GrpcRequest,
    verbose: bool,
) -> Result<()> {
    loop {
        let response = grpc_client::send_grpc(client, grpc_request)?;
        process_result(command, Ok(Some(response)), verbose)?;
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
}
/// Check and handle the gRPC call result
fn process_result(
    command: &args::Command,
    result: Result<Option<GrpcResponse>>,
    verbose: bool,
) -> Result<()> {
    match result {
        Ok(grpc_response) => {
            let response = grpc_response.unwrap_or(GrpcResponse::Unknown);
            command.print_response(&response, verbose);
            Ok(())
        }
        Err(e) => Err(format!("Grpc call error: {e}").into()),
    }
}
#[unsafe(no_mangle)]
/// main Rust netsim CLI function to be called by C wrapper netsim.cc
pub extern "C" fn rust_main() {
    let mut args = NetsimArgs::parse();
    netsim_logger::init("netsim", args.verbose);
    if matches!(args.command, args::Command::Gui) {
        println!("Opening netsim web UI on default web browser");
        browser::open("http://localhost:7681/");
        return;
    } else if matches!(args.command, args::Command::Artifact) {
        let artifact_dir = netsim_common::system::netsimd_temp_dir();
        println!("netsim artifact directory: {}", artifact_dir.display());
        browser::open(artifact_dir);
        return;
    } else if matches!(args.command, args::Command::Bumble) {
        println!("Opening Bumble Hive on default web browser");
        browser::open("https://google.github.io/bumble/hive/index.html");
        return;
    }
    let server = match (args.vsock, args.port) {
        (Some(vsock), _) => format!("vsock:{vsock}"),
        (_, Some(port)) => format!("localhost:{port}"),
        _ => get_server_address(get_instance(args.instance)).unwrap_or_default(),
    };
    let channel =
        ChannelBuilder::new(std::sync::Arc::new(EnvBuilder::new().build())).connect(&server);
    let client = FrontendServiceClient::new(channel);
    if let Err(e) = perform_command(&mut args.command, client, args.verbose) {
        error!("{e}");
    }
}

#[cfg(test)]
mod tests {
    use crate::args::BeaconRemove;
    use netsim_proto::{
        frontend::ListDeviceResponse,
        model::{Chip as ChipProto, Device as DeviceProto},
    };

    use crate::find_id_for_remove;

    #[test]
    fn test_remove_device() {
        let device_name = String::from("a-device");
        let chip_id = 7;

        let cmd = &BeaconRemove { device_name: device_name.clone(), chip_name: None };

        let response = ListDeviceResponse {
            devices: vec![DeviceProto {
                id: 0,
                name: device_name,
                chips: vec![ChipProto { id: chip_id, ..Default::default() }],
                ..Default::default()
            }],
            ..Default::default()
        };

        let id = find_id_for_remove(response, cmd);
        assert!(id.is_ok(), "{}", id.unwrap_err());
        let id = id.unwrap();

        assert_eq!(chip_id, id);
    }

    #[test]
    fn test_remove_chip() {
        let device_name = String::from("a-device");
        let chip_name = String::from("should-be-deleted");
        let device_id = 4;
        let chip_id = 2;

        let cmd =
            &BeaconRemove { device_name: device_name.clone(), chip_name: Some(chip_name.clone()) };

        let response = ListDeviceResponse {
            devices: vec![DeviceProto {
                id: device_id,
                name: device_name,
                chips: vec![
                    ChipProto { id: chip_id, name: chip_name, ..Default::default() },
                    ChipProto {
                        id: chip_id + 1,
                        name: String::from("shouldnt-be-deleted"),
                        ..Default::default()
                    },
                ],
                ..Default::default()
            }],
            ..Default::default()
        };

        let id = find_id_for_remove(response, cmd);
        assert!(id.is_ok(), "{}", id.unwrap_err());
        let id = id.unwrap();

        assert_eq!(chip_id, id);
    }

    #[test]
    fn test_remove_multiple_chips_fails() {
        let device_name = String::from("a-device");
        let device_id = 3;

        let cmd = &BeaconRemove { device_name: device_name.clone(), chip_name: None };

        let response = ListDeviceResponse {
            devices: vec![DeviceProto {
                id: device_id,
                name: device_name,
                chips: vec![
                    ChipProto { id: 1, name: String::from("chip-1"), ..Default::default() },
                    ChipProto { id: 2, name: String::from("chip-2"), ..Default::default() },
                ],
                ..Default::default()
            }],
            ..Default::default()
        };

        let id = find_id_for_remove(response, cmd);
        assert!(id.is_err());
    }

    #[test]
    fn test_remove_nonexistent_chip_fails() {
        let device_name = String::from("a-device");
        let device_id = 1;

        let cmd = &BeaconRemove {
            device_name: device_name.clone(),
            chip_name: Some(String::from("nonexistent-chip")),
        };

        let response = ListDeviceResponse {
            devices: vec![DeviceProto {
                id: device_id,
                name: device_name,
                chips: vec![ChipProto {
                    id: 1,
                    name: String::from("this-chip-exists"),
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        };

        let id = find_id_for_remove(response, cmd);
        assert!(id.is_err());
    }
}
