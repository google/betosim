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

use crate::captures::captures_handler;
use crate::devices::chip::ChipIdentifier;
use crate::devices::devices_handler;
use crate::links::link::{Link, LinkManager, PhyKind};
use futures_util::{FutureExt as _, SinkExt as _, TryFutureExt as _};
use grpcio::{RpcContext, RpcStatus, RpcStatusCode, UnarySink, WriteFlags};
use log::warn;
use netsim_proto::frontend::{
    DeleteLinkRequest, ListLinkResponse, PatchLinkRequest, VersionResponse,
};
use netsim_proto::frontend_grpc::FrontendService;
use netsim_proto::model::{Link as ProtoLink, PhyKind as ProtoPhyKind};
use protobuf::well_known_types::empty::Empty;

use std::io::Read;
use std::sync::Arc;

#[derive(Clone)]
pub struct FrontendClient {
    link_manager: Arc<LinkManager>,
}

impl FrontendClient {
    pub fn new(link_manager: Arc<LinkManager>) -> Self {
        FrontendClient { link_manager }
    }
}

/// Processes and validates fields from a Link protobuf message.
///
/// This function ensures:
/// 1. The `link` field is present.
/// 2. `link_kind` is specific (not `PhyKind::NONE`).
///
/// Returns a tuple of (sender_id, receiver_id, link_kind, rssi) if valid,
/// otherwise an `RpcStatus` indicating the error.
fn process_link_data(
    link_opt: Option<&ProtoLink>,
) -> Result<(ChipIdentifier, ChipIdentifier, PhyKind, i32), RpcStatus> {
    let link_ref = link_opt.ok_or_else(|| {
        RpcStatus::with_message(RpcStatusCode::INVALID_ARGUMENT, "Missing link field".to_string())
    })?;

    let proto_link_kind = link_ref.link_kind.enum_value_or_default();
    if proto_link_kind == ProtoPhyKind::NONE {
        return Err(RpcStatus::with_message(
            RpcStatusCode::INVALID_ARGUMENT,
            "Specific link_kind (other than NONE) must be provided for link".to_string(),
        ));
    }

    let link_kind = PhyKind::try_from(proto_link_kind)
        .map_err(|e| RpcStatus::with_message(RpcStatusCode::INVALID_ARGUMENT, e.to_string()))?;

    Ok((
        ChipIdentifier(link_ref.sender_id),
        ChipIdentifier(link_ref.receiver_id),
        link_kind,
        link_ref.rssi,
    ))
}

fn validate_patch_link_request(
    req: &PatchLinkRequest,
) -> Result<(ChipIdentifier, ChipIdentifier, PhyKind, i8), RpcStatus> {
    let (sender_id, receiver_id, link_kind, rssi_opt) = process_link_data(req.link.0.as_deref())?;
    // Validate and convert RSSI
    let rssi_i8: i8 = rssi_opt.try_into().map_err(|_| {
        RpcStatus::with_message(
            RpcStatusCode::INVALID_ARGUMENT,
            format!("RSSI value {} is out of range for i8 [{}, {}]", rssi_opt, i8::MIN, i8::MAX),
        )
    })?;
    Ok((sender_id, receiver_id, link_kind, rssi_i8))
}

fn validate_delete_link_request(
    req: &DeleteLinkRequest,
) -> Result<(ChipIdentifier, ChipIdentifier, PhyKind), RpcStatus> {
    let (sender_id, receiver_id, link_kind, _rssi_opt) = process_link_data(req.link.0.as_deref())?;
    Ok((sender_id, receiver_id, link_kind))
}

impl FrontendService for FrontendClient {
    fn get_version(&mut self, ctx: RpcContext<'_>, req: Empty, sink: UnarySink<VersionResponse>) {
        let response =
            VersionResponse { version: crate::version::get_version(), ..Default::default() };
        let f = sink
            .success(response)
            .map_err(move |e| eprintln!("client error {req:?}: {e:?}"))
            .map(|_| ());
        ctx.spawn(f)
    }

    fn list_device(
        &mut self,
        ctx: grpcio::RpcContext,
        req: Empty,
        sink: grpcio::UnarySink<netsim_proto::frontend::ListDeviceResponse>,
    ) {
        let response = match devices_handler::list_device() {
            Ok(response) => sink.success(response),
            Err(e) => {
                warn!("failed to list device: {e}");
                sink.fail(RpcStatus::with_message(RpcStatusCode::INTERNAL, e))
            }
        };

        ctx.spawn(response.map_err(move |e| warn!("client error {req:?}: {e:?}")).map(|_| ()))
    }

    fn patch_device(
        &mut self,
        ctx: grpcio::RpcContext,
        req: netsim_proto::frontend::PatchDeviceRequest,
        sink: grpcio::UnarySink<Empty>,
    ) {
        let response = match devices_handler::patch_device(req) {
            Ok(_) => sink.success(Empty::new()),
            Err(e) => {
                warn!("failed to patch device: {e}");
                sink.fail(RpcStatus::with_message(RpcStatusCode::INTERNAL, e))
            }
        };

        ctx.spawn(response.map_err(move |e| warn!("client error: {e:?}")).map(|_| ()))
    }

    fn reset(&mut self, ctx: grpcio::RpcContext, _req: Empty, sink: grpcio::UnarySink<Empty>) {
        let response = match devices_handler::reset_all() {
            Ok(_) => sink.success(Empty::new()),
            Err(e) => {
                warn!("failed to reset: {e}");
                sink.fail(RpcStatus::with_message(RpcStatusCode::INTERNAL, e))
            }
        };

        ctx.spawn(response.map_err(move |e| warn!("client error: {e:?}")).map(|_| ()))
    }

    fn patch_capture(
        &mut self,
        ctx: grpcio::RpcContext,
        req: netsim_proto::frontend::PatchCaptureRequest,
        sink: grpcio::UnarySink<Empty>,
    ) {
        let id = req.id;
        let state = match req.patch.state {
            Some(v) => v,
            None => {
                let error_msg = "Capture patch state not provided";
                warn!("{error_msg}");
                sink.fail(RpcStatus::with_message(
                    RpcStatusCode::INVALID_ARGUMENT,
                    error_msg.to_string(),
                ));
                return;
            }
        };

        let response = match captures_handler::patch_capture(ChipIdentifier(id), state) {
            Ok(_) => sink.success(Empty::new()),
            Err(e) => {
                warn!("failed to patch capture: {e}");
                sink.fail(RpcStatus::with_message(RpcStatusCode::INTERNAL, e.to_string()))
            }
        };

        ctx.spawn(response.map_err(move |e| warn!("client error: {e:?}")).map(|_| ()))
    }

    fn list_capture(
        &mut self,
        ctx: grpcio::RpcContext,
        req: Empty,
        sink: grpcio::UnarySink<netsim_proto::frontend::ListCaptureResponse>,
    ) {
        let response = match captures_handler::list_capture() {
            Ok(response) => sink.success(response),
            Err(e) => {
                warn!("failed to list capture: {e}");
                sink.fail(RpcStatus::with_message(RpcStatusCode::INTERNAL, e.to_string()))
            }
        };

        ctx.spawn(response.map_err(move |e| warn!("client error {req:?}: {e:?}")).map(|_| ()))
    }

    fn get_capture(
        &mut self,
        ctx: grpcio::RpcContext,
        req: netsim_proto::frontend::GetCaptureRequest,
        mut sink: grpcio::ServerStreamingSink<netsim_proto::frontend::GetCaptureResponse>,
    ) {
        let mut file = match captures_handler::get_capture(ChipIdentifier(req.id)) {
            Ok(f) => f,
            Err(e) => {
                warn!("failed to get capture: {e}");
                return ctx.spawn(
                    sink.fail(RpcStatus::with_message(RpcStatusCode::INTERNAL, e.to_string()))
                        .map_err(move |e| warn!("client error {req:?}: {e:?}"))
                        .map(|_| ()),
                );
            }
        };

        let mut buffer = [0u8; captures_handler::CHUNK_LEN];

        let f = async move {
            loop {
                let length = match file.read(&mut buffer) {
                    Ok(l) => l,
                    Err(e) => {
                        warn!("failed to read file: {e}");
                        sink.fail(RpcStatus::with_message(RpcStatusCode::INTERNAL, e.to_string()))
                            .await?;
                        return Ok(());
                    }
                };
                if length == 0 {
                    break;
                }
                let mut response = netsim_proto::frontend::GetCaptureResponse::new();
                response.capture_stream = buffer[..length].to_vec(); // Send only read data
                sink.send((response, WriteFlags::default())).await?;
            }
            sink.close().await?;
            Ok(())
        }
        .map_err(|e: grpcio::Error| log::error!("failed to handle get_capture request: {e:?}"))
        .map(|_| ());
        ctx.spawn(f)
    }

    fn create_device(
        &mut self,
        ctx: ::grpcio::RpcContext,
        req: netsim_proto::frontend::CreateDeviceRequest,
        sink: ::grpcio::UnarySink<netsim_proto::frontend::CreateDeviceResponse>,
    ) {
        let response = match devices_handler::create_device(&req) {
            Ok(device_proto) => sink.success(netsim_proto::frontend::CreateDeviceResponse {
                device: protobuf::MessageField::some(device_proto),
                ..Default::default()
            }),
            Err(e) => {
                warn!("failed to create chip: {e}");
                sink.fail(RpcStatus::with_message(RpcStatusCode::INTERNAL, e.to_string()))
            }
        };
        ctx.spawn(response.map_err(move |e| warn!("client error: {e:?}")).map(|_| ()))
    }

    fn delete_chip(
        &mut self,
        ctx: ::grpcio::RpcContext,
        req: netsim_proto::frontend::DeleteChipRequest,
        sink: ::grpcio::UnarySink<Empty>,
    ) {
        let response = match devices_handler::delete_chip(&req) {
            Ok(()) => sink.success(Empty::new()),
            Err(e) => {
                warn!("failed to delete chip: {e}");
                sink.fail(RpcStatus::with_message(RpcStatusCode::INTERNAL, e.to_string()))
            }
        };

        ctx.spawn(response.map_err(move |e| warn!("client error: {e:?}")).map(|_| ()))
    }

    fn patch_link(
        &mut self,
        ctx: grpcio::RpcContext,
        req: PatchLinkRequest,
        sink: grpcio::UnarySink<Empty>,
    ) {
        // Validate request data
        let validated_data = validate_patch_link_request(&req);
        let response = match validated_data {
            Ok((sender_id, receiver_id, link_kind, rssi)) => {
                self.link_manager.set_rssi(sender_id, receiver_id, link_kind, rssi);
                sink.success(Empty::new())
            }
            Err(status) => {
                warn!("Invalid patch link request: {}", status.message());
                sink.fail(status)
            }
        };

        ctx.spawn(response.map_err(move |e| log::error!("client sink error: {e:?}")).map(|_| ()))
    }

    fn delete_link(
        &mut self,
        ctx: grpcio::RpcContext,
        req: DeleteLinkRequest,
        sink: grpcio::UnarySink<Empty>,
    ) {
        let validated_data = validate_delete_link_request(&req);

        let response = match validated_data {
            Ok((sender_id, receiver_id, link_kind)) => {
                if self.link_manager.delete_rssi(sender_id, receiver_id, link_kind) {
                    sink.success(Empty::new())
                } else {
                    let msg = format!(
                        "Failed to delete link with sender: {sender_id}, receiver: {receiver_id}, link_kind: {link_kind:?}"
                    );
                    warn!("{msg}");
                    sink.fail(RpcStatus::with_message(RpcStatusCode::NOT_FOUND, msg))
                }
            }
            Err(status) => {
                warn!("Invalid delete link request: {}", status.message());
                sink.fail(status)
            }
        };

        ctx.spawn(response.map_err(move |e| warn!("client error: {e:?}")).map(|_| ()))
    }

    fn list_link(
        &mut self,
        ctx: grpcio::RpcContext,
        req: Empty,
        sink: grpcio::UnarySink<ListLinkResponse>,
    ) {
        let links: Vec<Link> = self.link_manager.list();
        let proto_links: Vec<ProtoLink> = links.into_iter().map(ProtoLink::from).collect();
        let response = sink.success(ListLinkResponse { links: proto_links, ..Default::default() });
        ctx.spawn(response.map_err(move |e| warn!("client error {req:?}: {e:?}")).map(|_| ()))
    }
}
