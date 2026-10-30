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

#![allow(clippy::print_stdout, clippy::print_stderr)]

mod ap;
mod args;
mod ble;
mod browser;
mod capture_stream;
mod cell_helper;
mod display;
mod error;
mod file_handler;
mod grpc_client;
mod gsm;
mod nfc;
mod requests;
mod response;
mod sms;

use std::{env, fs::File, path::PathBuf, process::ExitCode};

use args::{GetCapture, NetsimArgs};
use clap::Parser;
use common::util::{
    ini_file::{get_http_server_address, get_server_address},
    netsim_logger,
    os_utils::get_instance,
};
use file_handler::FileHandler;
use grpcio::{ChannelBuilder, EnvBuilder};
use netsim_proto::{
    cell_grpc::CellServiceClient, frontend, frontend_grpc::FrontendServiceClient,
    nfc_service_grpc::NfcServiceClient,
};
use netsim_rest_api::{ListDeviceResponse, NetsimRestClient};
use tracing::error;

use crate::{
    display::DisplayExt,
    error::{Error, Result},
    grpc_client::{ClientResponseReader, GrpcRequest, GrpcResponse},
};
// helper function to process streaming Grpc request
fn perform_streaming_request(
    client: &FrontendServiceClient,
    cmd: &mut GetCapture,
    req: &frontend::GetCaptureRequest,
    filename: &str,
) -> Result<()> {
    let dir = if let Some(location) = cmd.location.as_ref() {
        PathBuf::from(location)
    } else {
        env::current_dir()?
    };
    let output_file = dir.join(filename);
    cmd.current_file = output_file.display().to_string();
    grpc_client::get_capture(
        client,
        req,
        &mut ClientResponseReader {
            handler: Box::new(FileHandler {
                file: File::create(&output_file).map_err(|e| {
                    Error::from(format!("Failed to create file {}: {}", output_file.display(), e))
                })?,
                path: output_file,
            }),
        },
    )
}

/// helper function to send the Grpc request(s) and handle the response(s) per
/// the given command
fn perform_command(
    command: &mut args::Command,
    client: FrontendServiceClient,
    verbose: bool,
) -> Result<()> {
    // Get command's gRPC request(s)
    let requests = match command {
        args::Command::Capture(args::Capture::Patch(_) | args::Capture::Get(_))
        | args::Command::Link(_) => command.get_requests(&client),
        _ => Ok(vec![command.get_request()?]),
    }?;
    let mut process_error = false;
    // Process each request
    for (i, req) in requests.iter().enumerate() {
        let result = match command {
            // Continuous option sends the gRPC call every second
            &mut args::Command::Capture(args::Capture::List(ref cmd)) if cmd.continuous => {
                continuous_perform_command(command, &client, req, verbose)?;
                error!("Continuous command unexpectedly terminated without error");
                return Ok(());
            }
            // Get Capture use streaming gRPC reader request
            args::Command::Capture(args::Capture::Get(cmd)) => {
                let GrpcRequest::GetCapture(request) = req else {
                    return Err(format!("Expected GetCaptureRequest. Got: {req:?}").into());
                };
                let filename = cmd.filenames.get(i).cloned().ok_or_else(|| {
                    Error::from(format!("Missing filename for request index {i}"))
                })?;
                perform_streaming_request(&client, cmd, request, &filename)?;
                Ok(None)
            }
            // All other commands use a single gRPC call
            _ => {
                let response = grpc_client::send_grpc(&client, req)?;
                Ok(Some(response))
            }
        };
        if let Err(error) = process_result(command, result, Some(req), verbose) {
            error!(%error);
            process_error = true;
        };
    }
    if process_error {
        return Err("Not all requests were processed successfully.".into());
    }
    Ok(())
}

fn find_device<'a>(
    response: &'a ListDeviceResponse,
    device_name: &str,
) -> Result<&'a netsim_model::Device> {
    response
        .devices
        .iter()
        .find(|device| device.name == device_name)
        .ok_or_else(|| format!("Device not found: {device_name}").into())
}

fn find_device_id(response: &ListDeviceResponse, device_name: &str) -> Result<u32> {
    Ok(find_device(response, device_name)?.id)
}

fn find_device_and_chip_id(
    response: &ListDeviceResponse,
    device_name: &str,
    chip_name: &str,
) -> Result<(u32, u32)> {
    let device = find_device(response, device_name)?;
    let chip = device
        .chips
        .iter()
        .find(|chip| chip.name == chip_name)
        .ok_or_else(|| format!("Chip not found: {chip_name}"))?;
    Ok((device.id, chip.id))
}

fn run_beacon_create_ble(
    cmd: &args::BeaconCreateBle,
    rest: &NetsimRestClient,
    verbose: bool,
) -> Result<()> {
    let req = netsim_model::DeviceCreate::from(cmd);
    let device = rest.create_device(&req).send_blocking()?;
    if verbose {
        let [chip] = device.chips.as_slice() else {
            return Err(concat!(
                "the REST request completed successfully but the response ",
                "contained an unexpected number of chips"
            )
            .into());
        };
        println!("Created device '{}' with ble beacon chip '{}'", device.name, chip.name);
    }
    Ok(())
}

fn run_beacon_patch_ble(
    cmd: &args::BeaconPatchBle,
    rest: &NetsimRestClient,
    verbose: bool,
) -> Result<()> {
    let devices = rest.get_devices().send_blocking()?;
    let (device_id, chip_id) = find_device_and_chip_id(&devices, &cmd.device_name, &cmd.chip_name)?;
    let patch = cmd.to_device_update(device_id, chip_id);
    rest.patch_device(device_id, &patch).send_blocking()?;
    if verbose {
        response::print_beacon_patch_ble_verbose(cmd);
    }
    Ok(())
}

fn run_beacon_remove(
    cmd: &args::BeaconRemove,
    rest: &NetsimRestClient,
    verbose: bool,
) -> Result<()> {
    let devices = rest.get_devices().send_blocking()?;
    let id = find_device_id(&devices, &cmd.device_name)?;
    rest.delete_device(id).send_blocking()?;
    if verbose {
        println!("Removed device '{}'", cmd.device_name);
    }
    Ok(())
}

/// Continuously execute the command every second
fn continuous_perform_command(
    command: &args::Command,
    client: &FrontendServiceClient,
    grpc_request: &GrpcRequest,
    verbose: bool,
) -> Result<()> {
    repeat(true, || {
        let response = grpc_client::send_grpc(client, grpc_request)?;
        process_result(command, Ok(Some(response)), Some(grpc_request), verbose)
    })
}
fn patch_device_by_name(
    rest: &NetsimRestClient,
    device_name: &str,
    patch: &netsim_model::DeviceUpdate,
) -> Result<()> {
    let devices = rest.get_devices().send_blocking()?;
    let id = find_device_id(&devices, device_name)?;
    rest.patch_device(id, patch).send_blocking()?;
    Ok(())
}

/// Check and handle the gRPC call result
fn process_result(
    command: &args::Command,
    result: Result<Option<GrpcResponse>>,
    request: Option<&GrpcRequest>,
    verbose: bool,
) -> Result<()> {
    match result {
        Ok(grpc_response) => {
            let response = grpc_response.unwrap_or(GrpcResponse::Unknown);
            command.print_response(&response, request, verbose)
        }
        Err(e) => Err(format!("Grpc call error: {e}").into()),
    }
}
/// Runs `command` if it is served over the REST/HTTP API; returns `None`
/// otherwise.
fn run_rest(
    command: &args::Command,
    rest: &NetsimRestClient,
    http_server: &str,
    verbose: bool,
) -> Option<Result<()>> {
    let result = match command {
        args::Command::Version => rest
            .get_version()
            .send_blocking()
            .map(|resp| println!("Netsim version: {}", resp.version)),
        args::Command::Radio(cmd) => {
            return Some(
                patch_device_by_name(rest, &cmd.name, &requests::radio_to_device_update(cmd)).map(
                    |()| {
                        if verbose {
                            println!("Radio {} is {} for {}", cmd.radio_type, cmd.status, cmd.name);
                        }
                    },
                ),
            );
        }
        args::Command::Move(cmd) => {
            return Some(
                patch_device_by_name(rest, &cmd.name, &requests::move_to_device_update(cmd)).map(
                    |()| {
                        if verbose {
                            println!(
                                "Moved device:{} to x: {:.2}, y: {:.2}, z: {:.2}",
                                cmd.name,
                                cmd.x,
                                cmd.y,
                                cmd.z.unwrap_or_default()
                            );
                        }
                    },
                ),
            );
        }
        args::Command::Beacon(args::Beacon::Create(args::BeaconCreate::Ble(cmd))) => {
            return Some(run_beacon_create_ble(cmd, rest, verbose));
        }
        args::Command::Beacon(args::Beacon::Patch(args::BeaconPatch::Ble(cmd))) => {
            return Some(run_beacon_patch_ble(cmd, rest, verbose));
        }
        args::Command::Beacon(args::Beacon::Remove(cmd)) => {
            return Some(run_beacon_remove(cmd, rest, verbose));
        }
        args::Command::Devices(cmd) => {
            return Some(repeat(cmd.continuous, || {
                let resp = rest.get_devices().send_blocking()?;
                if cmd.json {
                    let json = serde_json::to_string(&resp)
                        .map_err(|e| Error::from(format!("JSON mapping error: {e}")))?;
                    println!("{json}");
                } else {
                    println!("{}", resp.display(verbose));
                }
                Ok(())
            }));
        }
        args::Command::Reset => rest.reset().send_blocking().map(|()| {
            if verbose {
                println!("All devices have been reset.");
            }
        }),
        args::Command::Link(args::Link::List) => {
            rest.get_links().send_blocking().map(|resp| println!("{}", resp.display(verbose)))
        }
        args::Command::Capture(args::Capture::Stream(cmd)) => {
            return Some(capture_stream::stream_capture(http_server, cmd));
        }
        args::Command::Ap(cmd) => {
            return Some(crate::ap::client::execute(cmd, rest, verbose));
        }
        args::Command::Ble(cmd) => {
            return Some(crate::ble::client::execute(cmd, http_server, verbose));
        }
        _ => return None,
    };
    Some(result.map_err(Error::from))
}

/// Standard Netsim CLI entry point
fn main() -> ExitCode {
    let args = NetsimArgs::parse();
    netsim_logger::init("netsim", args.verbose);
    match run(args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            error!(%error);
            ExitCode::FAILURE
        }
    }
}

fn validate_beacon_rest_args(args: &NetsimArgs) -> Result<()> {
    if args.vsock.is_none() && args.port.is_none() {
        return Ok(());
    }
    let subcommand = match &args.command {
        args::Command::Beacon(args::Beacon::Create(args::BeaconCreate::Ble(_))) => {
            Some("beacon create ble")
        }
        args::Command::Beacon(args::Beacon::Patch(args::BeaconPatch::Ble(_))) => {
            Some("beacon patch ble")
        }
        args::Command::Beacon(args::Beacon::Remove(_)) => Some("beacon remove"),
        _ => None,
    };
    if let Some(cmd) = subcommand {
        return Err(format!(
            "--vsock and --port are not supported for '{cmd}'; it uses the REST API"
        )
        .into());
    }
    Ok(())
}

/// Dispatches `args.command` to the browser, the REST API, or gRPC.
fn run(args: NetsimArgs) -> Result<()> {
    validate_beacon_rest_args(&args)?;
    let verbose = args.verbose;
    match &args.command {
        args::Command::Artifact => {
            let artifact_dir = common::system::netsimd_temp_dir();
            println!("netsim artifact directory: {}", artifact_dir.display());
            browser::open(artifact_dir);
            return Ok(());
        }
        args::Command::Bumble => {
            println!("Opening Bumble Hive on default web browser");
            browser::open("https://google.github.io/bumble/hive/index.html");
            return Ok(());
        }
        _ => {}
    }

    let instance_num = get_instance(args.instance);
    let http_server = get_http_server_address(instance_num).unwrap_or_default();
    let rest_client = NetsimRestClient::from_addr(http_server.clone());
    if let Some(result) = run_rest(&args.command, &rest_client, &http_server, verbose) {
        return result;
    }

    let server = match (args.vsock, args.port) {
        (Some(vsock), _) => format!("vsock:{vsock}"),
        (_, Some(port)) => format!("localhost:{port}"),
        _ => get_server_address(instance_num).unwrap_or_default(),
    };
    let channel =
        ChannelBuilder::new(std::sync::Arc::new(EnvBuilder::new().build())).connect(&server);
    match args.command {
        args::Command::Gsm(cmd) => {
            crate::gsm::client::execute(cmd, &CellServiceClient::new(channel), verbose)
        }
        args::Command::Sms(cmd) => {
            crate::sms::client::execute(cmd, &CellServiceClient::new(channel), verbose)
        }
        args::Command::Nfc(cmd) => {
            crate::nfc::client::execute(&cmd, &NfcServiceClient::new(channel), verbose)
        }
        mut command => perform_command(&mut command, FrontendServiceClient::new(channel), verbose),
    }
}

/// Runs `f` once, or once a second until it fails when `continuous` is set.
pub(crate) fn repeat(continuous: bool, mut f: impl FnMut() -> Result<()>) -> Result<()> {
    loop {
        f()?;
        if !continuous {
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
}

#[cfg(test)]
mod tests {
    use clap::Parser;
    use netsim_model::{Chip, Device};
    use netsim_rest_api::ListDeviceResponse;

    use crate::{
        args::NetsimArgs, find_device_and_chip_id, find_device_id, run, validate_beacon_rest_args,
    };

    #[test]
    fn test_remove_device() {
        let device_name = String::from("a-device");
        let device_id = 42;

        let response = ListDeviceResponse {
            devices: vec![Device {
                id: device_id,
                name: device_name.clone(),
                ..Default::default()
            }],
        };

        let id = find_device_id(&response, &device_name);
        assert!(id.is_ok(), "{}", id.unwrap_err());
        let id = id.unwrap();

        assert_eq!(device_id, id);
    }

    #[test]
    fn test_find_device_id_not_found() {
        let response = ListDeviceResponse { devices: vec![] };
        let err = find_device_id(&response, "missing").unwrap_err();
        assert_eq!(err.to_string(), "Device not found: missing");
    }

    #[test]
    fn test_find_device_and_chip_id() {
        let response = ListDeviceResponse {
            devices: vec![Device {
                id: 7,
                name: "beacon-dev".to_string(),
                chips: vec![Chip {
                    id: 1001,
                    name: "beacon-chip".to_string(),
                    ..Default::default()
                }],
                ..Default::default()
            }],
        };
        assert_eq!(
            find_device_and_chip_id(&response, "beacon-dev", "beacon-chip").unwrap(),
            (7, 1001)
        );
        let err = find_device_and_chip_id(&response, "beacon-dev", "wrong-chip").unwrap_err();
        assert_eq!(err.to_string(), "Chip not found: wrong-chip");
        let err = find_device_and_chip_id(&response, "wrong-dev", "beacon-chip").unwrap_err();
        assert_eq!(err.to_string(), "Device not found: wrong-dev");
    }

    #[test]
    fn test_beacon_rest_rejects_vsock_and_port() {
        for (argv, expected_subcmd) in [
            (vec!["netsim-cli", "--port", "8554", "beacon", "create", "ble"], "beacon create ble"),
            (
                vec!["netsim-cli", "--vsock", "3:8554", "beacon", "create", "ble"],
                "beacon create ble",
            ),
            (
                vec!["netsim-cli", "--port", "8554", "beacon", "patch", "ble", "dev", "chip"],
                "beacon patch ble",
            ),
            (
                vec!["netsim-cli", "--vsock", "3:8554", "beacon", "patch", "ble", "dev", "chip"],
                "beacon patch ble",
            ),
            (vec!["netsim-cli", "--port", "8554", "beacon", "remove", "dev"], "beacon remove"),
            (vec!["netsim-cli", "--vsock", "3:8554", "beacon", "remove", "dev"], "beacon remove"),
        ] {
            let args = NetsimArgs::parse_from(argv);
            let err = run(args).unwrap_err();
            let expected = format!(
                "--vsock and --port are not supported for '{expected_subcmd}'; it uses the REST API"
            );
            assert_eq!(err.to_string(), expected);
        }

        let uwb_args =
            NetsimArgs::parse_from(["netsim-cli", "--port", "8554", "beacon", "create", "uwb"]);
        assert!(validate_beacon_rest_args(&uwb_args).is_ok());

        let ble_args = NetsimArgs::parse_from(["netsim-cli", "beacon", "create", "ble"]);
        assert!(validate_beacon_rest_args(&ble_args).is_ok());
    }
}
