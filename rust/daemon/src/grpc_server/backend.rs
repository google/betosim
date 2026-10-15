// Copyright 2024 The Android Open Source Project
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

use crate::devices::chip;
use crate::devices::device::AddChipResult;
use crate::devices::devices_handler;
use crate::grpc_server::avd_config::{get_or_create_bluetooth_mac, set_bluetooth_mac};
use crate::transport::grpc::RustGrpcTransport;
use crate::wireless;
use crate::wireless::packet::{register_transport, unregister_transport};
use anyhow::{Context, Result, anyhow};
use bytes::Bytes;
use futures_util::{FutureExt as _, TryFutureExt as _, TryStreamExt as _};
use log::{info, warn};
use netsim_proto::common::ChipKind as ProtoChipKind;
use netsim_proto::hci_packet::hcipacket::PacketType;
use netsim_proto::packet_streamer::{PacketRequest, PacketResponse};
use netsim_proto::packet_streamer_grpc::PacketStreamer;
use netsim_proto::startup::ChipInfo;
use protobuf::Enum;

fn add_chip(initial_info: &ChipInfo, device_guid: &str) -> Result<AddChipResult> {
    let chip_kind =
        initial_info.chip.kind.enum_value().map_err(|v| anyhow!("unknown chip kind {v}"))?;
    let chip = &initial_info.chip;
    // TODO(b/323899010): Avoid having cfg(test) in mainline code
    #[cfg(not(test))]
    let wireless_create_param = match &chip_kind {
        ProtoChipKind::BLUETOOTH => {
            wireless::CreateParam::Bluetooth(wireless::bluetooth::CreateParams {
                address: chip.address.clone(),
                bt_properties: Some(chip.bt_properties.clone()),
            })
        }
        ProtoChipKind::WIFI => wireless::CreateParam::Wifi(wireless::wifi_chip::CreateParams {}),
        ProtoChipKind::UWB => wireless::CreateParam::Uwb(wireless::uwb::CreateParams {
            address: chip.address.clone(),
        }),
        _ => return Err(anyhow!("The provided chip kind is unsupported: {:?}", chip.kind)),
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

    devices_handler::add_chip(
        device_guid,
        &initial_info.device_info.as_ref().unwrap_or_default().name,
        &chip_create_params,
        &wireless_create_param,
        initial_info.device_info.clone().unwrap_or_default(),
    )
    .map_err(|err| anyhow!(err))
}

// Helper function to manage Bluetooth address for the AVD.
// This function either retrieves an existing MAC address for the AVD
// or generates and stores a new one if it doesn't exist.
// If the initial_info already contains a MAC address, it stores that address.
fn handle_bluetooth_address(initial_info: &mut ChipInfo) -> Result<()> {
    let device_info = initial_info
        .device_info
        .as_ref()
        .ok_or_else(|| anyhow!("Device info is missing for Bluetooth chip"))?;

    let avd_path = &device_info.avd_path;
    if avd_path.is_empty() {
        // This case should ideally not happen if device_info is present
        warn!("AVD path is empty, skipping Bluetooth address management.");
        return Ok(());
    }

    let chip_address = &initial_info.chip.address;
    if chip_address.is_empty() {
        // No address provided, get or create one for this AVD.
        let mac = get_or_create_bluetooth_mac(avd_path)
            .map_err(|e| anyhow!("Failed to get/create MAC for {}: {}", avd_path, e))?;
        info!("Assigned MAC {} to AVD {}", mac, avd_path);
        initial_info.chip.as_mut().unwrap().address = mac;
    } else {
        // Address is provided, store it as the address for this AVD.
        set_bluetooth_mac(avd_path, chip_address)
            .map_err(|e| anyhow!("Failed to store MAC for {}: {}", avd_path, e))?;
        info!("Stored provided MAC {} for AVD {}", chip_address, avd_path);
    }
    Ok(())
}

#[derive(Clone)]
pub struct PacketStreamerService;
impl PacketStreamer for PacketStreamerService {
    fn stream_packets(
        &mut self,
        ctx: ::grpcio::RpcContext,
        mut packet_request: ::grpcio::RequestStream<PacketRequest>,
        sink: ::grpcio::DuplexSink<PacketResponse>,
    ) {
        let peer = ctx.peer().clone();
        let f = async move {
            info!("grpc_server new packet_stream for peer {}", &peer);

            let request = packet_request.try_next().await?.context("initial info")?;
            let mut initial_info: ChipInfo = request.initial_info().clone(); // Clone to make mutable

            // Handle AVD-specific Bluetooth address persistence.
            match initial_info.chip.kind.enum_value() {
                Ok(ProtoChipKind::BLUETOOTH) => {
                    if let Err(e) = handle_bluetooth_address(&mut initial_info) {
                        warn!("Failed to handle Bluetooth address: {e}");
                    }
                }
                Ok(_) => { /* Other chip kinds, no special address handling needed here */ }
                Err(val) => warn!("Received unknown ChipKind value: {}", val),
            }

            let result = add_chip(&initial_info, &peer)?; // Use the potentially modified initial_info

            register_transport(result.chip_id, Box::new(RustGrpcTransport { sink }));

            while let Some(request) = packet_request.try_next().await? {
                let chip_kind =
                    initial_info.chip.kind.enum_value().unwrap_or(ProtoChipKind::UNSPECIFIED);
                match chip_kind {
                    ProtoChipKind::WIFI | ProtoChipKind::UWB => {
                        if !request.has_packet() {
                            warn!("unknown packet type from chip_id: {}", result.chip_id);
                            continue;
                        }
                        let packet: Bytes = request.packet().to_vec().into();
                        wireless::handle_request(
                            result.chip_id,
                            &packet,
                            PacketType::HCI_PACKET_UNSPECIFIED.value() as u8,
                        );
                    }
                    ProtoChipKind::BLUETOOTH => {
                        if !request.has_hci_packet() {
                            warn!("unknown packet type from chip_id: {}", result.chip_id);
                            continue;
                        }
                        let packet: Bytes = request.hci_packet().packet.to_vec().into();
                        wireless::handle_request(
                            result.chip_id,
                            &packet,
                            request.hci_packet().packet_type.unwrap().value() as u8,
                        );
                    }
                    _ => {
                        warn!("unknown control packet chip_kind: {chip_kind:?}");
                        break;
                    }
                };
            }
            unregister_transport(result.chip_id);
            if let Err(e) = devices_handler::remove_chip(result.device_id, result.chip_id) {
                warn!("failed to remove chip: {e}");
            }
            Ok(())
        }
        .map_err(|e: anyhow::Error| warn!("failed to stream packets: {e:?}"))
        .map(|_| ());
        ctx.spawn(f)
    }
}
