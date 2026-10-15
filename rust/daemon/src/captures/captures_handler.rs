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

//! Packet Capture handlers and singleton for HTTP and gRPC server.
//!
//! This module implements a handler for GET, PATCH, LIST capture
//!
//! /v1/captures --> handle_capture_list
//!
//! /v1/captures/{id} --> handle_capture_patch, handle_capture_get
//!
//! handle_packet_request and handle_packet_response is invoked by packet_hub
//! to write packets to files if capture state is on.

// TODO(b/274506882): Implement gRPC status proto on error responses. Also write better
// and more descriptive error messages with proper error codes.

use bytes::Bytes;
use http::Request;
use log::warn;
use netsim_common::util::{proto_print_options::JSON_PRINT_OPTION, time_display::TimeDisplay};
use netsim_proto::common::ChipKind;
use netsim_proto::frontend::ListCaptureResponse;
use protobuf_json_mapping::print_to_string_with_options;
use std::fs::File;
use std::io::{Read, Result};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::devices::chip::ChipIdentifier;
use crate::http_server::server_response::ResponseWritable;
use crate::resource::clone_captures;
use crate::wifi::radiotap;

use anyhow::anyhow;

use super::PCAP_MIME_TYPE;
use super::pcap_util::{PacketDirection, append_record, append_record_pcapng, wrap_bt_packet};

/// Max Chunk length of capture file during get_capture
pub const CHUNK_LEN: usize = 1024;

/// Helper function for getting file name from the given fields.
fn get_file(id: ChipIdentifier, device_name: String, chip_kind: ChipKind) -> Result<File> {
    let mut filename = netsim_common::system::netsimd_temp_dir();
    let extension = match chip_kind {
        ChipKind::UWB => "pcapng",
        _ => "pcap",
    };
    filename.push("pcaps");
    filename.push(format!("netsim-{id:?}-{device_name:}-{chip_kind:?}.{extension}"));
    File::open(filename)
}

/// Getting capture file of the provided ChipIdentifier
pub fn get_capture(id: ChipIdentifier) -> anyhow::Result<File> {
    let captures_arc = clone_captures();
    let mut captures = captures_arc.write().unwrap();
    let capture = captures
        .get(id)
        .ok_or(anyhow!("Cannot access Capture Resource"))?
        .lock()
        .map_err(|_| anyhow!("Failed to lock Capture"))?;

    if capture.size == 0 {
        return Err(anyhow!(
            "Capture file not found for {:?}-{}-{:?}",
            id,
            capture.device_name,
            capture.chip_kind
        ));
    }
    Ok(get_file(id, capture.device_name.clone(), capture.chip_kind)?)
}

// TODO: GetCapture should return the information of the capture. Need to reconsider
// uri hierarchy.
// GET /captures/id/{id} --> Get Capture information
// GET /captures/contents/{id} --> Download Pcap file
/// Performs GetCapture to download pcap file and write to writer.
fn handle_capture_get(writer: ResponseWritable, id: ChipIdentifier) -> anyhow::Result<()> {
    let captures_arc = clone_captures();
    let mut captures = captures_arc.write().unwrap();
    let capture = captures
        .get(id)
        .ok_or(anyhow!("Cannot access Capture Resource"))?
        .lock()
        .map_err(|_| anyhow!("Failed to lock Capture"))?;

    if capture.size == 0 {
        return Err(anyhow!(
            "Capture file not found for {:?}-{}-{:?}",
            id,
            capture.device_name,
            capture.chip_kind
        ));
    }
    let mut file = get_file(id, capture.device_name.clone(), capture.chip_kind)?;
    let mut buffer = [0u8; CHUNK_LEN];
    let time_display = TimeDisplay::new(capture.seconds, capture.nanos as u32);
    let header_value = format!(
        "attachment; filename=\"netsim-{:?}-{:}-{:?}-{}.{}\"",
        id,
        capture.device_name.clone(),
        capture.chip_kind,
        time_display.utc_display(),
        capture.extension
    );
    writer.put_ok_with_length(
        PCAP_MIME_TYPE,
        capture.size,
        vec![("Content-Disposition".to_string(), header_value)],
    );
    loop {
        let length = file.read(&mut buffer)?;
        if length == 0 {
            break;
        }
        writer.put_chunk(&buffer[..length]);
    }
    Ok(())
}

/// List capture information of all chips
pub fn list_capture() -> anyhow::Result<ListCaptureResponse> {
    let captures_arc = clone_captures();
    let captures = captures_arc.write().unwrap();
    // Instantiate ListCaptureResponse and add Captures
    let mut response = ListCaptureResponse::new();
    for capture in captures.values() {
        response.captures.push(
            capture.lock().expect("Failed to acquire lock on CaptureInfo").get_capture_proto(),
        );
    }
    Ok(response)
}

/// Performs ListCapture to get the list of CaptureInfos and write to writer.
fn handle_capture_list(writer: ResponseWritable) -> anyhow::Result<()> {
    let response = list_capture()?;

    // Perform protobuf-json-mapping with the given protobuf
    let json_response = print_to_string_with_options(&response, &JSON_PRINT_OPTION)
        .map_err(|e| anyhow!("proto to JSON mapping failure: {}", e))?;
    writer.put_ok("text/json", &json_response, vec![]);
    Ok(())
}

/// Patch capture state of a chip with provided ChipIdentifier
pub fn patch_capture(id: ChipIdentifier, state: bool) -> anyhow::Result<()> {
    let captures_arc = clone_captures();
    let mut captures = captures_arc.write().unwrap();
    if let Some(mut capture) = captures
        .get(id)
        .map(|arc_capture| arc_capture.lock().expect("Failed to acquire lock on CaptureInfo"))
    {
        if state {
            capture.start_capture()?;
        } else {
            capture.stop_capture();
        }
    }
    Ok(())
}

/// Performs PatchCapture to patch a CaptureInfo with id.
/// Writes the result of PatchCapture to writer.
fn handle_capture_patch(
    writer: ResponseWritable,
    id: ChipIdentifier,
    state: bool,
) -> anyhow::Result<()> {
    let captures_arc = clone_captures();
    let mut captures = captures_arc.write().unwrap();
    if let Some(mut capture) = captures
        .get(id)
        .map(|arc_capture| arc_capture.lock().expect("Failed to acquire lock on CaptureInfo"))
    {
        if state {
            capture.start_capture()?;
        } else {
            capture.stop_capture();
        }
        // Perform protobuf-json-mapping with the given protobuf
        let json_response =
            print_to_string_with_options(&capture.get_capture_proto(), &JSON_PRINT_OPTION)
                .map_err(|e| anyhow!("proto to JSON mapping failure: {}", e))?;
        writer.put_ok("text/json", &json_response, vec![]);
    }
    Ok(())
}

/// The Rust capture handler used directly by Http frontend for LIST, GET, and PATCH
pub fn handle_capture(request: &Request<Vec<u8>>, param: &str, writer: ResponseWritable) {
    if let Err(e) = handle_capture_internal(request, param, writer) {
        writer.put_error(404, &e.to_string());
    }
}

fn get_id(param: &str) -> anyhow::Result<ChipIdentifier> {
    param
        .parse::<u32>()
        .map_err(|_| anyhow!("Capture ID must be u32, found {}", param))
        .map(ChipIdentifier)
}

fn handle_capture_internal(
    request: &Request<Vec<u8>>,
    param: &str,
    writer: ResponseWritable,
) -> anyhow::Result<()> {
    if request.uri() == "/v1/captures" {
        match request.method().as_str() {
            "GET" => handle_capture_list(writer),
            _ => Err(anyhow!("Not found.")),
        }
    } else {
        match request.method().as_str() {
            "GET" => handle_capture_get(writer, get_id(param)?),
            "PATCH" => {
                let id = get_id(param)?;
                let body = request.body();
                let state = String::from_utf8(body.to_vec()).unwrap();
                match state.as_str() {
                    "0" => handle_capture_patch(writer, id, false),
                    "1" => handle_capture_patch(writer, id, true),
                    _ => Err(anyhow!("Incorrect state for PatchCapture")),
                }
            }
            _ => Err(anyhow!("Not found.")),
        }
    }
}

/// A common code for handle_request and handle_response methods.
pub(super) fn handle_packet(
    chip_id: ChipIdentifier,
    packet: &[u8],
    packet_type: u32,
    direction: PacketDirection,
) {
    let captures_arc = clone_captures();
    let captures = captures_arc.write().unwrap();
    if let Some(mut capture) = captures
        .chip_id_to_capture
        .get(&chip_id)
        .map(|arc_capture| arc_capture.lock().expect("Failed to acquire lock on CaptureInfo"))
    {
        let chip_kind = capture.chip_kind;
        if let Some(ref mut file) = capture.file {
            let timestamp =
                SystemTime::now().duration_since(UNIX_EPOCH).expect("Time went backwards");
            let packet_buf = match chip_kind {
                ChipKind::BLUETOOTH => wrap_bt_packet(direction, packet_type, packet),
                ChipKind::WIFI => match radiotap::into_pcap(packet) {
                    Some(buffer) => buffer,
                    None => return,
                },
                ChipKind::UWB => {
                    match append_record_pcapng(timestamp, file, packet) {
                        Ok(size) => {
                            capture.size += size;
                            capture.records += 1;
                        }
                        Err(err) => warn!("{err:?}"),
                    }
                    return;
                }
                _ => {
                    warn!("Unknown capture type");
                    return;
                }
            };
            match append_record(timestamp, file, packet_buf.as_slice()) {
                Ok(size) => {
                    capture.size += size;
                    capture.records += 1;
                }
                Err(err) => {
                    warn!("{err:?}");
                }
            }
        }
    };
}

/// Method for dispatcher to invoke (Host to Controller Packet Flow)
pub fn host_to_controller(chip_id: ChipIdentifier, packet: &Bytes, packet_type: u32) {
    clone_captures().read().unwrap().send(
        chip_id,
        packet,
        packet_type,
        PacketDirection::HostToController,
    );
}

/// Method for dispatcher to invoke (Controller to Host Packet Flow)
pub fn controller_to_host(chip_id: ChipIdentifier, packet: &Bytes, packet_type: u32) {
    clone_captures().read().unwrap().send(
        chip_id,
        packet,
        packet_type,
        PacketDirection::ControllerToHost,
    );
}

/// Method for clearing pcap files in temp directory
pub fn clear_pcap_files() -> bool {
    let mut path = netsim_common::system::netsimd_temp_dir();
    path.push("pcaps");

    // Check if the directory exists.
    if std::fs::metadata(&path).is_err() {
        return false;
    }

    // Delete the directory.
    std::fs::remove_dir_all(&path).is_ok()
}
