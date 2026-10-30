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
    rest_client: &NetsimRestClient,
    client: FrontendServiceClient,
    verbose: bool,
) -> Result<()> {
    // Get command's gRPC request(s)
    let requests = match command {
        args::Command::Capture(args::Capture::Patch(_) | args::Capture::Get(_))
        | args::Command::Link(_) => command.get_requests(&client),
        args::Command::Beacon(args::Beacon::Remove(cmd)) => {
            let response = rest_client.get_devices().send_blocking()?;
            let id = find_device_id(&response, &cmd.device_name)?;
            Ok(vec![GrpcRequest::DeleteDevice(frontend::DeleteDeviceRequest {
                id,
                ..Default::default()
            })])
        }
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

fn find_device_id(response: &ListDeviceResponse, device_name: &str) -> Result<u32> {
    let device = response
        .devices
        .iter()
        .find(|device| device.name == device_name)
        .ok_or_else(|| format!("Device not found: {device_name}"))?;
    Ok(device.id)
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

/// Dispatches `args.command` to the browser, the REST API, or gRPC.
fn run(args: NetsimArgs) -> Result<()> {
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
        mut command => perform_command(
            &mut command,
            &rest_client,
            FrontendServiceClient::new(channel),
            verbose,
        ),
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
    use netsim_model::Device;
    use netsim_rest_api::ListDeviceResponse;

    use crate::find_device_id;

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
}
