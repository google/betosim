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

/// request packets flow into netsim
/// response packets flow out of netsim
/// packet transports read requests and write response packets over gRPC or Fds.
use super::h4;
use super::h4::PacketError;
use super::uci;
use crate::devices::chip;
use crate::devices::chip::ChipIdentifier;
use crate::devices::device::DeviceIdentifier;
use crate::devices::devices_handler::{add_chip, remove_chip};
use crate::ffi::ffi_transport;
use crate::wireless;
use crate::wireless::packet::{Response, register_transport, unregister_transport};
use bytes::Bytes;
use log::{error, info, warn};
use netsim_proto::common::ChipKind;
use netsim_proto::hci_packet::{HCIPacket, hcipacket::PacketType};
use netsim_proto::packet_streamer::PacketRequest;
use netsim_proto::startup::{ChipInfo as ChipInfoProto, StartupInfo as StartupInfoProto};
use protobuf::{Enum, EnumOrUnknown, Message, MessageField};
use std::collections::HashMap;
use std::fs::File;
use std::io::{ErrorKind, Write};
use std::os::fd::FromRawFd;
use std::sync::{Arc, OnceLock, RwLock};
use std::thread;
use std::thread::JoinHandle;

struct FdTransport {
    file: File,
}

impl Response for FdTransport {
    fn response(&mut self, packet: Bytes, packet_type: u8) {
        let mut buffer = Vec::<u8>::new();
        if packet_type != (PacketType::HCI_PACKET_UNSPECIFIED.value() as u8) {
            buffer.push(packet_type);
        }
        buffer.extend(packet);
        if let Err(e) = self.file.write_all(&buffer[..]) {
            error!("netsimd: error writing {e}");
        }
    }
}

/// read from the raw fd and pass to the packet hub.
///
/// # Safety
///
/// `fd_rx` must be a valid and open file descriptor.
unsafe fn fd_reader(
    fd_rx: i32,
    kind: ChipKind,
    device_id: DeviceIdentifier,
    chip_id: ChipIdentifier,
) -> JoinHandle<()> {
    thread::Builder::new()
        .name(format!("fd_reader_{fd_rx}"))
        .spawn(move || {
            // SAFETY: The caller promises that `fd_rx` is valid and open.
            let mut rx = unsafe { File::from_raw_fd(fd_rx) };

            info!("Handling fd={fd_rx} for kind: {kind:?} chip_id: {chip_id:?}");

            loop {
                match kind {
                    ChipKind::UWB => match uci::read_uci_packet(&mut rx) {
                        Err(e) => {
                            error!("End reader connection with fd={fd_rx}. Failed to reading uci control packet: {e:?}");
                            break;
                        }
                        Ok(uci::Packet { payload }) => {
                            wireless::handle_request(chip_id, &payload, 0);
                        }
                    },
                    ChipKind::BLUETOOTH => match h4::read_h4_packet(&mut rx) {
                        Ok(h4::Packet { h4_type, payload }) => {
                            wireless::handle_request(chip_id, &payload, h4_type);
                        }
                        Err(PacketError::IoError(e))
                            if e.kind() == ErrorKind::UnexpectedEof =>
                        {
                            info!("End reader connection with fd={fd_rx}.");
                            break;
                        }
                        Err(e) => {
                            error!("End reader connection with fd={fd_rx}. Failed to reading hci control packet: {e:?}");
                            break;
                        }
                    },
                    _ => {
                        error!("unknown control packet chip_kind: {kind:?}");
                        break;
                    }
                };
            }

            // unregister before remove_chip because facade may re-use facade_id
            // on an intertwining create_chip and the unregister here might remove
            // the recently added chip creating a disconnected transport.
            unregister_transport(chip_id);

            if let Err(err) = remove_chip(device_id, chip_id) {
                warn!("{err}");
            }
        })
        .unwrap()
}

/// start_fd_transport
///
/// Create threads to read and write to file descriptors
///
/// # Safety
///
/// The file descriptors in the JSON must be valid and open.
pub unsafe fn run_fd_transport(startup_json: &String) {
    info!("Running fd transport with {startup_json}");
    let startup_info =
        match protobuf_json_mapping::parse_from_str::<StartupInfoProto>(startup_json.as_str()) {
            Ok(startup_info) => startup_info,
            Err(e) => {
                error!("Error parsing startup info: {e:?}");
                return;
            }
        };
    // Vector for getting all fd_in, fd_out, chip_kind, device_id, chip_id information
    // of adding all chips to frontend resource.
    let mut fd_vec: Vec<(i32, i32, ChipKind, DeviceIdentifier, ChipIdentifier)> = Vec::new();
    let chip_count = startup_info.devices.len();
    for device in startup_info.devices {
        info!("Processing startup device {}", device.name);
        for chip in &device.chips {
            info!("Processing chip {chip:?}");
            let chip_kind = chip.kind.enum_value_or_default();
            // TODO(b/323899010): Avoid having cfg(test) in mainline code
            #[cfg(not(test))]
            let wireless_create_param = match chip_kind {
                ChipKind::BLUETOOTH => {
                    wireless::CreateParam::Bluetooth(wireless::bluetooth::CreateParams {
                        address: chip.address.clone(),
                        bt_properties: Some(chip.bt_properties.clone()),
                    })
                }
                ChipKind::WIFI => wireless::CreateParam::Wifi(wireless::wifi_chip::CreateParams {}),
                ChipKind::UWB => wireless::CreateParam::Uwb(wireless::uwb::CreateParams {
                    address: chip.address.clone(),
                }),
                _ => {
                    warn!("The provided chip kind is unsupported: {chip_kind:?}");
                    return;
                }
            };
            #[cfg(test)]
            let wireless_create_param =
                wireless::CreateParam::Mock(wireless::mocked::CreateParams { chip_kind });
            let chip_create_params = chip::CreateParams {
                kind: chip_kind,
                address: chip.address.clone(),
                name: Some(chip.id.clone()),
                manufacturer: chip.manufacturer.clone(),
                product_name: chip.product_name.clone(),
            };
            let result = match add_chip(
                &format!("fd-device-{}", &device.name.clone()),
                &device.name.clone(),
                &chip_create_params,
                &wireless_create_param,
                device.device_info.clone().unwrap_or_default(),
            ) {
                Ok(chip_result) => chip_result,
                Err(err) => {
                    warn!("{err}");
                    return;
                }
            };
            fd_vec.push((chip.fd_in, chip.fd_out, chip_kind, result.device_id, result.chip_id));
        }
    }

    // See https://tokio.rs/tokio/topics/bridging
    // This code is synchronous hosting asynchronous until main is converted to rust.
    thread::Builder::new()
        .name("fd_transport".to_string())
        .spawn(move || {
            let mut handles = Vec::with_capacity(chip_count);
            for (fd_in, fd_out, kind, device_id, chip_id) in fd_vec {
                // Cf writes to fd_out and reads from fd_in
                // SAFETY: Our caller promises that the file descriptors in the JSON are valid
                // and open.
                let file_in = unsafe { File::from_raw_fd(fd_in) };

                register_transport(chip_id, Box::new(FdTransport { file: file_in }));
                // TODO: switch to runtime.spawn once FIFOs are available in Tokio
                // SAFETY: Our caller promises that the file descriptors in the JSON are valid
                // and open.
                handles.push(unsafe { fd_reader(fd_out, kind, device_id, chip_id) });
            }
            // Wait for all of them to complete.
            for handle in handles {
                // The `spawn` method returns a `JoinHandle`. A `JoinHandle` is
                // a future, so we can wait for it using `block_on`.
                // runtime.block_on(handle).unwrap();
                // TODO: use runtime.block_on once FIFOs are available in Tokio
                handle.join().unwrap();
            }
            info!("done with all fd handlers");
        })
        .unwrap();
}

/// Read from the raw fd and pass to the grpc server.
///
/// # Safety
///
/// `fd_rx` must be a valid and open file descriptor.
unsafe fn connector_fd_reader(fd_rx: i32, kind: ChipKind, stream_id: u32) -> JoinHandle<()> {
    info!("Connecting fd reader for stream_id: {stream_id}, fd_rx: {fd_rx}");
    thread::Builder::new()
        .name(format!("fd_connector_{stream_id}_{fd_rx}"))
        .spawn(move || {
            // SAFETY: The caller promises that `fd_rx` is valid and open.
            let mut rx = unsafe { File::from_raw_fd(fd_rx) };
            info!("Handling fd={fd_rx} for kind: {kind:?} stream_id: {stream_id:?}");

            loop {
                match kind {
                    ChipKind::UWB => match uci::read_uci_packet(&mut rx) {
                        Err(e) => {
                            error!(
                                "End reader connection with fd={fd_rx}. Failed to read \
                                     uci control packet: {e:?}"
                            );
                            break;
                        }
                        Ok(uci::Packet { payload }) => {
                            let mut request = PacketRequest::new();
                            request.set_packet(payload.to_vec());
                            let proto_bytes = request.write_to_bytes().unwrap();
                            ffi_transport::write_packet_request(stream_id, &proto_bytes);
                        }
                    },
                    ChipKind::BLUETOOTH => match h4::read_h4_packet(&mut rx) {
                        Ok(h4::Packet { h4_type, payload }) => {
                            let mut request = PacketRequest::new();
                            let hci_packet = HCIPacket {
                                packet_type: EnumOrUnknown::from_i32(h4_type as i32),
                                packet: payload.to_vec(),
                                ..Default::default()
                            };
                            request.set_hci_packet(hci_packet);
                            let proto_bytes = request.write_to_bytes().unwrap();
                            ffi_transport::write_packet_request(stream_id, &proto_bytes);
                        }
                        Err(PacketError::IoError(e)) if e.kind() == ErrorKind::UnexpectedEof => {
                            info!("End reader connection with fd={fd_rx}.");
                            break;
                        }
                        Err(e) => {
                            error!(
                                "End reader connection with fd={fd_rx}. Failed to read \
                                     hci control packet: {e:?}"
                            );
                            break;
                        }
                    },
                    _ => {
                        error!("unknown control packet chip_kind: {kind:?}");
                        break;
                    }
                };
            }
        })
        .unwrap()
}

// For connector.
static CONNECTOR_FILES: OnceLock<Arc<RwLock<HashMap<u32, File>>>> = OnceLock::new();

fn get_connector_files() -> Arc<RwLock<HashMap<u32, File>>> {
    CONNECTOR_FILES.get_or_init(|| Arc::new(RwLock::new(HashMap::new()))).clone()
}

/// This function is called when a packet is received from the gRPC server.
fn connector_grpc_read_callback(stream_id: u32, proto_bytes: &[u8]) {
    let request = PacketRequest::parse_from_bytes(proto_bytes).unwrap();

    let mut buffer = Vec::<u8>::new();
    if request.has_hci_packet() {
        buffer.push(request.hci_packet().packet_type.enum_value_or_default().value() as u8);
        buffer.extend(&request.hci_packet().packet);
    } else if request.has_packet() {
        buffer.extend(request.packet());
    }

    if let Some(mut file_in) = get_connector_files().read().unwrap().get(&stream_id) {
        if let Err(e) = file_in.write_all(&buffer[..]) {
            error!("Failed to write: {e}");
        }
    } else {
        warn!("Unable to find file with stream_id {stream_id}");
    }
}

/// Read from grpc server and write back to file descriptor.
fn connector_grpc_reader(chip_kind: ChipKind, stream_id: u32, file_in: File) -> JoinHandle<()> {
    info!("Connecting grpc reader for stream_id: {stream_id}");
    thread::Builder::new()
        .name(format!("grpc_reader_{stream_id}"))
        .spawn(move || {
            {
                let connector_files = get_connector_files();
                let mut binding = connector_files.write().unwrap();
                if binding.contains_key(&stream_id) {
                    error!(
                        "register_connector: key already present for \
                                 stream_id: {stream_id}"
                    );
                }
                binding.insert(stream_id, file_in);
            }
            if (chip_kind != ChipKind::BLUETOOTH) && (chip_kind != ChipKind::UWB) {
                warn!("Unable to register connector for chip type {chip_kind:?}");
            }
            // Read packet from grpc and send to file_in.
            ffi_transport::read_packet_response_loop(stream_id, connector_grpc_read_callback);

            get_connector_files().write().unwrap().remove(&stream_id);
        })
        .unwrap()
}

/// Create threads to forward file descriptors to another netsim daemon.
pub fn run_fd_connector(startup_json: &String, server: &str) -> Result<(), String> {
    info!("Running fd connector with {startup_json}");
    let startup_info =
        match protobuf_json_mapping::parse_from_str::<StartupInfoProto>(startup_json.as_str()) {
            Ok(startup_info) => startup_info,
            Err(e) => {
                return Err(format!("Error parsing startup info: {:?}", e.to_string()));
            }
        };
    let server = server.to_owned();

    let chip_count = startup_info.devices.len();
    let mut handles = Vec::with_capacity(chip_count);

    for device in startup_info.devices {
        for chip in device.chips {
            let chip_kind = chip.kind.enum_value_or_default();
            // Cf writes to fd_out and reads from fd_in
            // SAFETY: Our caller promises that the file descriptors in the JSON are valid
            // and open.
            let file_in = unsafe { File::from_raw_fd(chip.fd_in) };

            let stream_id = ffi_transport::stream_packets(&server);
            // Send out initial info of PacketRequest to grpc server.
            let mut initial_request = PacketRequest::new();
            initial_request.set_initial_info(ChipInfoProto {
                name: device.name.clone(),
                chip: MessageField::some(chip.clone()),
                device_info: device.device_info.clone(),
                ..Default::default()
            });
            ffi_transport::write_packet_request(
                stream_id,
                &initial_request.write_to_bytes().unwrap(),
            );
            info!("Sent initial request to grpc for stream_id: {stream_id}");

            handles.push(connector_grpc_reader(chip_kind, stream_id, file_in));

            // TODO: switch to runtime.spawn once FIFOs are available in Tokio
            // SAFETY: Our caller promises that the file descriptors in the JSON are valid
            // and open.
            handles.push(unsafe { connector_fd_reader(chip.fd_out, chip_kind, stream_id) });
        }
    }
    // Wait for all of them to complete.
    for handle in handles {
        // The `spawn` method returns a `JoinHandle`. A `JoinHandle` is
        // a future, so we can wait for it using `block_on`.
        // runtime.block_on(handle).unwrap();
        // TODO: use runtime.block_on once FIFOs are available in Tokio
        handle.join().unwrap();
    }
    Ok(())
}
