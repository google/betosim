// Copyright 2026 The Android Open Source Project
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

use std::sync::Arc;

use device_actor::{DeviceClient, DeviceError};
use futures::{FutureExt, SinkExt};
use grpcio::{RpcContext, RpcStatus, RpcStatusCode, ServerStreamingSink, UnarySink, WriteFlags};
use link_api::{LinkClient, LinkCreate, LinkId, LinkUpdate};
use netsim_model::{ChipId, ClientError, Pose};
use netsim_proto::{
    empty::Empty,
    frontend::{
        GetCaptureRequest, GetCaptureResponse, ListCaptureResponse, ListDeviceResponse,
        ListLinkResponse, PatchCaptureRequest,
    },
    frontend_grpc::FrontendService,
    protobuf,
};
use tokio::io::AsyncReadExt;

use crate::frontend_converter::to_proto_device;

#[derive(Clone)]
pub(crate) struct FrontendClient {
    device_client: DeviceClient,
    link_client: Arc<dyn LinkClient>,
    ap_client: ap_actor::ApClient,
    capture_client: capture_actor::CaptureClient,
    version: String,
    frontend_stats: Arc<netsim_model::FrontendStats>,
    tokio_handle: tokio::runtime::Handle,
}

impl FrontendClient {
    pub(crate) fn new(
        device_client: DeviceClient,
        link_client: Arc<dyn LinkClient>,
        ap_client: ap_actor::ApClient,
        capture_client: capture_actor::CaptureClient,
        version: String,
        frontend_stats: Arc<netsim_model::FrontendStats>,
    ) -> Self {
        Self {
            device_client,
            link_client,
            ap_client,
            capture_client,
            version,
            frontend_stats,
            tokio_handle: tokio::runtime::Handle::current(),
        }
    }

    async fn handle_create_link(
        client: Arc<dyn LinkClient>,
        req: netsim_proto::frontend::CreateLinkRequest,
    ) -> Result<netsim_proto::frontend::CreateLinkResponse, RpcStatus> {
        let proto_link = req.link.into_option().ok_or_else(|| {
            RpcStatus::with_message(RpcStatusCode::INVALID_ARGUMENT, "No link provided".to_string())
        })?;

        let link = crate::frontend_converter::from_proto_link(proto_link).ok_or_else(|| {
            RpcStatus::with_message(
                RpcStatusCode::INVALID_ARGUMENT,
                "Invalid link parameters".to_string(),
            )
        })?;

        let create = LinkCreate { sender: link.sender, receiver: link.receiver, rssi: link.rssi };

        let id = client
            .create(create)
            .await
            .map_err(|e| RpcStatus::with_message(RpcStatusCode::INTERNAL, e.to_string()))?;

        let mut response = netsim_proto::frontend::CreateLinkResponse::new();
        let mut response_link = crate::frontend_converter::to_proto_link(link);
        response_link.id = id.0;
        response.link = protobuf::MessageField::some(response_link);
        Ok(response)
    }

    async fn handle_patch_link(
        client: Arc<dyn LinkClient>,
        req: netsim_proto::frontend::PatchLinkRequest,
    ) -> Result<(), RpcStatus> {
        let proto_link = req.link.into_option().ok_or_else(|| {
            RpcStatus::with_message(RpcStatusCode::INVALID_ARGUMENT, "No link provided".to_string())
        })?;

        let link = crate::frontend_converter::from_proto_link(proto_link).ok_or_else(|| {
            RpcStatus::with_message(
                RpcStatusCode::INVALID_ARGUMENT,
                "Invalid link parameters".to_string(),
            )
        })?;

        let update = LinkUpdate { rssi: Some(link.rssi) };
        client
            .update(LinkId(req.id), update)
            .await
            .map_err(|e| RpcStatus::with_message(RpcStatusCode::INTERNAL, e.to_string()))
    }

    async fn handle_delete_link(
        client: Arc<dyn LinkClient>,
        req: netsim_proto::frontend::DeleteLinkRequest,
    ) -> Result<(), RpcStatus> {
        if req.id != 0 {
            client
                .delete(LinkId(req.id))
                .await
                .map_err(|e| RpcStatus::with_message(RpcStatusCode::INTERNAL, e.to_string()))?;
            return Ok(());
        }

        Err(RpcStatus::with_message(
            RpcStatusCode::INVALID_ARGUMENT,
            "Link ID required".to_string(),
        ))
    }

    async fn handle_list_link(client: Arc<dyn LinkClient>) -> Result<ListLinkResponse, RpcStatus> {
        let links = client.list().await.map_err(|e| {
            RpcStatus::with_message(RpcStatusCode::INTERNAL, format!("Failed to list links: {}", e))
        })?;
        let mut response = ListLinkResponse::new();
        for link in links {
            response.links.push(crate::frontend_converter::to_proto_link(link));
        }
        Ok(response)
    }

    async fn handle_list_device(client: DeviceClient) -> Result<ListDeviceResponse, RpcStatus> {
        // Trigger stats collection to update internal cache
        if let Err(error) = client.get_radio_stats().await {
            tracing::warn!(%error, "Failed to get radio stats");
        }

        let devices = client.list().await.map_err(|e| {
            RpcStatus::with_message(
                RpcStatusCode::INTERNAL,
                format!("Failed to list devices: {}", e),
            )
        })?;
        let mut proto_response = ListDeviceResponse::new();
        for device in devices {
            proto_response.devices.push(to_proto_device(device));
        }
        Ok(proto_response)
    }

    async fn handle_create_device(
        client: DeviceClient,
        req: netsim_proto::frontend::CreateDeviceRequest,
    ) -> Result<netsim_proto::frontend::CreateDeviceResponse, RpcStatus> {
        // We only support creating a device with a single chip (Beacon) for now.
        if let Some(proto_chip_ref) = req.device.chips.first() {
            let mut proto_chip = proto_chip_ref.clone();
            // Fallback: Use device name if chip name is missing.
            if proto_chip.name.is_empty() {
                proto_chip.name = req.device.name.clone();
            }
            if let Some(chip_config) = crate::frontend_converter::from_proto_chip_create(proto_chip)
            {
                let device_config = device_api::DeviceConfig {
                    name: req.device.name.clone(),
                    visible: true, // Default to true as proto doesn't have this field
                    pose: Pose {
                        position: crate::frontend_converter::from_proto_position(
                            req.device.position.clone().unwrap_or_default(),
                        ),
                        orientation: crate::frontend_converter::from_proto_orientation(
                            req.device.orientation.clone().unwrap_or_default(),
                        ),
                    },
                    builtin: false,
                    device_info: None,
                };

                let device_create = device_api::DeviceCreate { device_config, chip: chip_config };

                let id = client
                    .create_device(device_create)
                    .await
                    .map_err(|e| RpcStatus::with_message(RpcStatusCode::INTERNAL, e.to_string()))?;
                let device = client
                    .get(id)
                    .await
                    .map_err(|e| RpcStatus::with_message(RpcStatusCode::INTERNAL, e.to_string()))?
                    .ok_or_else(|| {
                        RpcStatus::with_message(
                            RpcStatusCode::INTERNAL,
                            "Device not found after creation".into(),
                        )
                    })?;

                Ok(netsim_proto::frontend::CreateDeviceResponse {
                    device: protobuf::MessageField::some(
                        crate::frontend_converter::to_proto_device(device),
                    ),
                    ..Default::default()
                })
            } else {
                Err(RpcStatus::with_message(
                    RpcStatusCode::INVALID_ARGUMENT,
                    "Unsupported chip kind or invalid config".to_string(),
                ))
            }
        } else {
            Err(RpcStatus::with_message(
                RpcStatusCode::INVALID_ARGUMENT,
                "No chips provided".to_string(),
            ))
        }
    }

    async fn handle_patch_device(
        client: DeviceClient,
        req: netsim_proto::frontend::PatchDeviceRequest,
    ) -> Result<(), RpcStatus> {
        let id = req.id.unwrap_or(0);

        let update = device_api::DeviceUpdate {
            id,
            name: req.device.name.clone(),
            visible: req.device.visible,
            pose: device_api::PoseUpdate {
                position: req
                    .device
                    .position
                    .clone()
                    .into_option()
                    .map(crate::frontend_converter::from_proto_position),
                orientation: req
                    .device
                    .orientation
                    .clone()
                    .into_option()
                    .map(crate::frontend_converter::from_proto_orientation),
            },
            chips: if !req.device.chips.is_empty() {
                Some(
                    req.device
                        .chips
                        .iter()
                        .cloned()
                        .map(crate::frontend_converter::from_proto_chip_update)
                        .collect(),
                )
            } else {
                None
            },
        };

        let name_opt = req.device.name.as_deref();
        client.patch(req.id, name_opt, update).await.map_err(|e| match e {
            ClientError::Framework(ref framework)
                if matches!(
                    framework.downcast_ref::<DeviceError>(),
                    Some(DeviceError::NotFound(_) | DeviceError::DeviceNotFound(_))
                ) =>
            {
                RpcStatus::with_message(
                    RpcStatusCode::NOT_FOUND,
                    format!("Device not found or patch failed: {}", e),
                )
            }
            _ => RpcStatus::with_message(
                RpcStatusCode::INTERNAL,
                format!("Failed to patch device: {}", e),
            ),
        })
    }

    #[deprecated(note = "Use handle_delete_device instead")]
    #[allow(deprecated)]
    fn handle_delete_chip(
        _client: DeviceClient,
        _req: netsim_proto::frontend::DeleteChipRequest,
    ) -> Result<(), RpcStatus> {
        Err(RpcStatus::new(RpcStatusCode::UNIMPLEMENTED))
    }

    async fn handle_delete_device(
        client: DeviceClient,
        req: netsim_proto::frontend::DeleteDeviceRequest,
    ) -> Result<(), RpcStatus> {
        let device_id = device_api::DeviceId(req.id);
        client.delete_device(device_id).await.map_err(|e| {
            RpcStatus::with_message(
                RpcStatusCode::INTERNAL,
                format!("Failed to delete device: {}", e),
            )
        })
    }

    async fn handle_reset(
        client: DeviceClient,
        ap_client: ap_actor::ApClient,
    ) -> Result<(), RpcStatus> {
        client.reset(None).await.map_err(|e| {
            RpcStatus::with_message(
                RpcStatusCode::INTERNAL,
                format!("Failed to reset devices: {}", e),
            )
        })?;
        let aps = ap_client.list_aps().await.map_err(|e| {
            RpcStatus::with_message(RpcStatusCode::INTERNAL, format!("Failed to list APs: {}", e))
        })?;

        let default_ap_bssid = ap_actor::expected_default_ap_bssid();
        for (id, state) in aps {
            // The default AP is considered built-in.
            // TODO(b/499058772): reset default AP back to its original state.
            if state.config.bssid == default_ap_bssid {
                continue;
            }
            ap_client.destroy_ap(id).await.map_err(|e| {
                RpcStatus::with_message(
                    RpcStatusCode::INTERNAL,
                    format!("Failed to destroy AP {}: {}", id, e),
                )
            })?;
        }
        Ok(())
    }

    async fn handle_list_capture(
        client: capture_actor::CaptureClient,
    ) -> Result<ListCaptureResponse, RpcStatus> {
        let captures = client.list_captures().await.map_err(to_rpc_status)?;
        let mut response = ListCaptureResponse::new();
        for info in captures {
            let mut capture = netsim_proto::model::Capture::new();
            capture.id = info.chip_id.0;
            capture.chip_kind = protobuf::EnumOrUnknown::new(
                crate::frontend_converter::to_proto_chip_kind(info.chip_kind),
            );
            capture.device_name = info.device_name;
            capture.state = Some(info.enabled);
            capture.size = info.bytes_written.min(i32::MAX as u64) as i32;
            capture.records = info.records_written.min(i32::MAX as u64) as i32;
            let mut timestamp = protobuf::well_known_types::timestamp::Timestamp::new();
            timestamp.seconds = info.seconds;
            timestamp.nanos = info.nanos;
            capture.timestamp = protobuf::MessageField::some(timestamp);
            capture.valid = true;
            response.captures.push(capture);
        }
        Ok(response)
    }

    async fn handle_patch_capture(
        client: capture_actor::CaptureClient,
        req: PatchCaptureRequest,
    ) -> Result<(), RpcStatus> {
        let id = ChipId(req.id);
        let patch = req.patch.into_option().ok_or_else(|| {
            RpcStatus::with_message(
                RpcStatusCode::INVALID_ARGUMENT,
                "No patch provided".to_string(),
            )
        })?;
        let state = patch.state.ok_or_else(|| {
            RpcStatus::with_message(
                RpcStatusCode::INVALID_ARGUMENT,
                "Capture patch state not provided".to_string(),
            )
        })?;
        client.update_capture(id, state).await.map(|_| ()).map_err(to_rpc_status)
    }

    async fn handle_get_capture(
        client: capture_actor::CaptureClient,
        req: GetCaptureRequest,
        mut sink: ServerStreamingSink<GetCaptureResponse>,
    ) {
        let id = ChipId(req.id);
        let info = match client.get_capture(id).await {
            Ok(Some(info)) => info,
            Ok(None) => {
                let _ = sink
                    .fail(RpcStatus::with_message(
                        RpcStatusCode::NOT_FOUND,
                        format!("Capture not found for chip id {}", req.id),
                    ))
                    .await;
                return;
            }
            Err(e) => {
                let _ = sink.fail(to_rpc_status(e)).await;
                return;
            }
        };

        let Some(filepath) = info.filepath else {
            let _ = sink
                .fail(RpcStatus::with_message(
                    RpcStatusCode::NOT_FOUND,
                    format!("Capture file path not found for chip id {}", req.id),
                ))
                .await;
            return;
        };

        let file = match tokio::fs::File::open(&filepath).await {
            Ok(f) => f,
            Err(e) => {
                let _ = sink
                    .fail(RpcStatus::with_message(
                        RpcStatusCode::NOT_FOUND,
                        format!("Failed to open capture file {}: {e}", filepath.display()),
                    ))
                    .await;
                return;
            }
        };
        let mut reader = tokio::io::BufReader::new(file);

        const CHUNK_LEN: usize = 16384;
        let mut buffer = vec![0u8; CHUNK_LEN];

        loop {
            let length = match reader.read(&mut buffer).await {
                Ok(l) => l,
                Err(e) => {
                    let _ = sink
                        .fail(RpcStatus::with_message(
                            RpcStatusCode::INTERNAL,
                            format!("Failed to read capture file: {e}"),
                        ))
                        .await;
                    return;
                }
            };
            if length == 0 {
                break;
            }
            let Some(chunk) = buffer.get(..length) else {
                let _ = sink
                    .fail(RpcStatus::with_message(
                        RpcStatusCode::INTERNAL,
                        "Read length exceeds buffer capacity".to_string(),
                    ))
                    .await;
                return;
            };
            let mut response = GetCaptureResponse::new();
            response.capture_stream = chunk.to_vec();
            if let Err(error) = sink.send((response, WriteFlags::default())).await {
                tracing::warn!(%error, "Failed to send get_capture response chunk");
                return;
            }
        }
        if let Err(error) = sink.close().await {
            tracing::warn!(%error, "Failed to close get_capture sink");
        }
    }
}

fn to_rpc_status(e: ClientError) -> RpcStatus {
    match e {
        ClientError::Chip(netsim_model::ChipError::ChipNotFound(id)) => {
            RpcStatus::with_message(RpcStatusCode::NOT_FOUND, format!("Chip not found: {}", id.0))
        }
        ClientError::Framework(ref framework)
            if matches!(
                framework.downcast_ref::<netsim_model::ChipError>(),
                Some(netsim_model::ChipError::ChipNotFound(_))
            ) =>
        {
            RpcStatus::with_message(RpcStatusCode::NOT_FOUND, format!("Chip not found: {e}"))
        }
        _ => RpcStatus::with_message(RpcStatusCode::INTERNAL, format!("{e}")),
    }
}

async fn reply<T>(sink: UnarySink<T>, res: Result<T, RpcStatus>) {
    match res {
        Ok(msg) => {
            let _ = sink.success(msg).await;
        }
        Err(status) => {
            let _ = sink.fail(status).await;
        }
    }
}

impl FrontendService for FrontendClient {
    fn delete_device(
        &mut self,
        ctx: RpcContext,
        _req: netsim_proto::frontend::DeleteDeviceRequest,
        sink: UnarySink<Empty>,
    ) {
        self.frontend_stats.delete_device.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let client = self.device_client.clone();
        ctx.spawn(async move {
            let res = Self::handle_delete_device(client, _req).await.map(|_| Empty::new());
            reply(sink, res).await;
        });
    }

    fn get_version(
        &mut self,
        ctx: RpcContext,
        _req: Empty,
        sink: UnarySink<netsim_proto::frontend::VersionResponse>,
    ) {
        self.frontend_stats.get_version.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let mut response = netsim_proto::frontend::VersionResponse::new();
        response.version = self.version.clone();
        let f = sink.success(response).map(|_| ());
        ctx.spawn(f)
    }

    fn list_device(&mut self, ctx: RpcContext, _req: Empty, sink: UnarySink<ListDeviceResponse>) {
        self.frontend_stats.list_device.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let client = self.device_client.clone();
        ctx.spawn(async move {
            let res = Self::handle_list_device(client).await;
            reply(sink, res).await;
        });
    }

    fn subscribe_device(
        &mut self,
        ctx: RpcContext,
        _req: netsim_proto::frontend::SubscribeDeviceRequest,
        sink: UnarySink<netsim_proto::frontend::SubscribeDeviceResponse>,
    ) {
        self.frontend_stats.subscribe_device.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let f = sink.fail(RpcStatus::new(RpcStatusCode::UNIMPLEMENTED)).map(|_| ());
        ctx.spawn(f)
    }

    fn patch_device(
        &mut self,
        ctx: RpcContext,
        req: netsim_proto::frontend::PatchDeviceRequest,
        sink: UnarySink<Empty>,
    ) {
        self.frontend_stats.patch_device.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let client = self.device_client.clone();
        ctx.spawn(async move {
            let res = Self::handle_patch_device(client, req).await.map(|_| Empty::new());
            reply(sink, res).await;
        });
    }

    fn reset(&mut self, ctx: RpcContext, _req: Empty, sink: UnarySink<Empty>) {
        self.frontend_stats.reset.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let client = self.device_client.clone();
        let ap_client = self.ap_client.clone();
        ctx.spawn(async move {
            let res = Self::handle_reset(client, ap_client).await.map(|_| Empty::new());
            reply(sink, res).await;
        });
    }

    fn create_device(
        &mut self,
        ctx: RpcContext,
        req: netsim_proto::frontend::CreateDeviceRequest,
        sink: UnarySink<netsim_proto::frontend::CreateDeviceResponse>,
    ) {
        self.frontend_stats.create_device.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let client = self.device_client.clone();
        ctx.spawn(async move {
            let res = Self::handle_create_device(client, req).await;
            reply(sink, res).await;
        });
    }

    #[allow(deprecated)]
    fn delete_chip(
        &mut self,
        ctx: RpcContext,
        req: netsim_proto::frontend::DeleteChipRequest,
        sink: UnarySink<Empty>,
    ) {
        self.frontend_stats.delete_chip.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let client = self.device_client.clone();
        ctx.spawn(async move {
            let res = Self::handle_delete_chip(client, req).map(|_| Empty::new());
            reply(sink, res).await;
        });
    }

    fn create_link(
        &mut self,
        ctx: RpcContext,
        req: netsim_proto::frontend::CreateLinkRequest,
        sink: UnarySink<netsim_proto::frontend::CreateLinkResponse>,
    ) {
        let client = self.link_client.clone();
        ctx.spawn(async move {
            let res = Self::handle_create_link(client, req).await;
            reply(sink, res).await;
        });
    }

    fn list_link(&mut self, ctx: RpcContext, _req: Empty, sink: UnarySink<ListLinkResponse>) {
        let client = self.link_client.clone();
        ctx.spawn(async move {
            let res = Self::handle_list_link(client).await;
            reply(sink, res).await;
        });
    }

    fn patch_link(
        &mut self,
        ctx: RpcContext,
        req: netsim_proto::frontend::PatchLinkRequest,
        sink: UnarySink<Empty>,
    ) {
        let client = self.link_client.clone();
        ctx.spawn(async move {
            let res = Self::handle_patch_link(client, req).await.map(|_| Empty::new());
            reply(sink, res).await;
        });
    }

    fn delete_link(
        &mut self,
        ctx: RpcContext,
        req: netsim_proto::frontend::DeleteLinkRequest,
        sink: UnarySink<Empty>,
    ) {
        let client = self.link_client.clone();
        ctx.spawn(async move {
            let res = Self::handle_delete_link(client, req).await.map(|_| Empty::new());
            reply(sink, res).await;
        });
    }

    fn list_capture(&mut self, ctx: RpcContext, _req: Empty, sink: UnarySink<ListCaptureResponse>) {
        self.frontend_stats.list_capture.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let client = self.capture_client.clone();
        ctx.spawn(async move {
            let res = Self::handle_list_capture(client).await;
            reply(sink, res).await;
        });
    }

    fn patch_capture(&mut self, ctx: RpcContext, req: PatchCaptureRequest, sink: UnarySink<Empty>) {
        self.frontend_stats.patch_capture.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let client = self.capture_client.clone();
        ctx.spawn(async move {
            let res = Self::handle_patch_capture(client, req).await.map(|_| Empty::new());
            reply(sink, res).await;
        });
    }

    fn get_capture(
        &mut self,
        _ctx: RpcContext,
        req: GetCaptureRequest,
        sink: ServerStreamingSink<GetCaptureResponse>,
    ) {
        self.frontend_stats.get_capture.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let client = self.capture_client.clone();
        self.tokio_handle.spawn(async move {
            Self::handle_get_capture(client, req, sink).await;
        });
    }
}

#[cfg(test)]
mod tests {
    use actor_framework::MockActorClient;
    use device_actor::DeviceActor;
    use link_api::{Link, LinkId, MockLinkClient};
    use netsim_model::{ChipId, ChipKind};
    use protobuf::EnumOrUnknown;

    use super::*;

    #[test]
    #[allow(deprecated)]
    fn test_handle_delete_chip() {
        let mock_client = MockActorClient::<DeviceActor>::new();
        let client = DeviceClient::new(Box::new(mock_client));
        let mut req = netsim_proto::frontend::DeleteChipRequest::new();
        req.id = 3;

        let res = FrontendClient::handle_delete_chip(client, req);
        assert_eq!(res.unwrap_err().code(), RpcStatusCode::UNIMPLEMENTED);
    }

    #[tokio::test]
    async fn test_create_link() {
        let mut client = MockLinkClient::new();
        client
            .expect_create()
            .withf(|params| params.sender.0 == 1 && params.receiver.0 == 2 && params.rssi == -50)
            .times(1)
            .returning(|_| Ok(LinkId(100)));

        let client = Arc::new(client);
        let mut req = netsim_proto::frontend::CreateLinkRequest::new();
        let mut link = netsim_proto::model::Link::new();
        link.sender_id = 1;
        link.receiver_id = 2;
        link.rssi = -50;
        link.kind = EnumOrUnknown::new(netsim_proto::common::ChipKind::BLUETOOTH);
        req.link = protobuf::MessageField::some(link);

        let resp = FrontendClient::handle_create_link(client.clone(), req).await.unwrap();
        assert_eq!(resp.link.id, 100);
    }

    #[tokio::test]
    async fn test_patch_link() {
        let mut client = MockLinkClient::new();
        // Update()
        client
            .expect_update()
            .withf(|id, patch| id.0 == 10 && patch.rssi == Some(-60))
            .times(1)
            .returning(|_, _| Ok(()));

        let client = Arc::new(client);
        let mut req = netsim_proto::frontend::PatchLinkRequest::new();
        req.id = 10;
        let mut link = netsim_proto::model::Link::new();
        link.sender_id = 1;
        link.receiver_id = 2;
        link.rssi = -60;
        link.kind = EnumOrUnknown::new(netsim_proto::common::ChipKind::BLUETOOTH);
        req.link = protobuf::MessageField::some(link);

        FrontendClient::handle_patch_link(client.clone(), req).await.unwrap();
    }

    #[tokio::test]
    async fn test_delete_link() {
        let mut client = MockLinkClient::new();
        // delete()
        client.expect_delete().withf(|id| id.0 == 20).times(1).returning(|_| Ok(()));

        let client = Arc::new(client);
        let mut req = netsim_proto::frontend::DeleteLinkRequest::new();
        req.id = 20;

        FrontendClient::handle_delete_link(client.clone(), req).await.unwrap();
    }

    #[tokio::test]
    async fn test_list_link() {
        let mut client = MockLinkClient::new();
        client.expect_list().times(1).returning(|| {
            Ok(vec![Link {
                id: LinkId(1),
                sender: ChipId(10),
                receiver: ChipId(11),
                kind: ChipKind::BLUETOOTH,
                rssi: -70,
            }])
        });

        let client = Arc::new(client);
        let resp = FrontendClient::handle_list_link(client.clone()).await.unwrap();
        assert_eq!(resp.links.len(), 1);
        assert_eq!(resp.links[0].id, 1);
        assert_eq!(resp.links[0].sender_id, 10);
        assert_eq!(resp.links[0].receiver_id, 11);
    }

    #[tokio::test]
    async fn test_list_capture() {
        let (capture_runner, capture_client) = capture_actor::new();
        let capture_actor_state = capture_actor::CaptureActor::new(false, None);
        tokio::spawn(capture_runner.run(capture_actor_state));

        // 1. Initial capture created disabled -> timestamp is 0
        let enabled_flag = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let create = capture_actor::CaptureCreate {
            chip_kind: ChipKind::BLUETOOTH,
            device_name: "test-device".to_string(),
            enabled_flag,
        };
        capture_client.create_capture(ChipId(10), create).await.unwrap();

        let resp = FrontendClient::handle_list_capture(capture_client.clone()).await.unwrap();
        assert_eq!(resp.captures.len(), 1);
        assert_eq!(resp.captures[0].id, 10);
        assert_eq!(resp.captures[0].device_name, "test-device");
        assert_eq!(
            resp.captures[0].chip_kind,
            protobuf::EnumOrUnknown::new(netsim_proto::common::ChipKind::BLUETOOTH)
        );
        assert_eq!(resp.captures[0].state, Some(false));
        assert!(resp.captures[0].valid);
        assert_eq!(resp.captures[0].records, 0);
        assert_eq!(resp.captures[0].size, 0);
        assert_eq!(resp.captures[0].timestamp.seconds, 0);
        assert_eq!(resp.captures[0].timestamp.nanos, 0);

        // 2. Patch capture to ON -> verify state is true and timestamp is populated
        let mut req = netsim_proto::frontend::PatchCaptureRequest::new();
        req.id = 10;
        let mut patch = netsim_proto::frontend::patch_capture_request::PatchCapture::new();
        patch.state = Some(true);
        req.patch = protobuf::MessageField::some(patch);
        FrontendClient::handle_patch_capture(capture_client.clone(), req).await.unwrap();

        let resp = FrontendClient::handle_list_capture(capture_client.clone()).await.unwrap();
        assert_eq!(resp.captures[0].state, Some(true));
        assert!(resp.captures[0].timestamp.seconds > 0);
    }

    #[tokio::test]
    async fn test_patch_capture() {
        let (capture_runner, capture_client) = capture_actor::new();
        let capture_actor_state = capture_actor::CaptureActor::new(false, None);
        tokio::spawn(capture_runner.run(capture_actor_state));

        let enabled_flag = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let create = capture_actor::CaptureCreate {
            chip_kind: ChipKind::WIFI,
            device_name: "test-wifi".to_string(),
            enabled_flag,
        };
        let chip_id = ChipId(20);
        capture_client.create_capture(chip_id, create).await.unwrap();

        // Enable capture
        let mut req = netsim_proto::frontend::PatchCaptureRequest::new();
        req.id = chip_id.0;
        let mut patch = netsim_proto::frontend::patch_capture_request::PatchCapture::new();
        patch.state = Some(true);
        req.patch = protobuf::MessageField::some(patch);
        FrontendClient::handle_patch_capture(capture_client.clone(), req).await.unwrap();

        let info = capture_client.get_capture(chip_id).await.unwrap().unwrap();
        assert!(info.enabled);
        assert!(info.seconds > 0);

        // Disable capture
        let mut req = netsim_proto::frontend::PatchCaptureRequest::new();
        req.id = chip_id.0;
        let mut patch = netsim_proto::frontend::patch_capture_request::PatchCapture::new();
        patch.state = Some(false);
        req.patch = protobuf::MessageField::some(patch);
        FrontendClient::handle_patch_capture(capture_client.clone(), req).await.unwrap();

        let info = capture_client.get_capture(chip_id).await.unwrap().unwrap();
        assert!(!info.enabled);
    }

    #[tokio::test]
    async fn test_patch_capture_missing_state() {
        let (capture_runner, capture_client) = capture_actor::new();
        let capture_actor_state = capture_actor::CaptureActor::new(false, None);
        tokio::spawn(capture_runner.run(capture_actor_state));

        // Missing patch message
        let mut req = netsim_proto::frontend::PatchCaptureRequest::new();
        req.id = 1;
        let res = FrontendClient::handle_patch_capture(capture_client.clone(), req).await;
        assert_eq!(res.unwrap_err().code(), RpcStatusCode::INVALID_ARGUMENT);

        // Patch message without state field
        let mut req = netsim_proto::frontend::PatchCaptureRequest::new();
        req.id = 1;
        let patch = netsim_proto::frontend::patch_capture_request::PatchCapture::new();
        req.patch = protobuf::MessageField::some(patch);
        let res = FrontendClient::handle_patch_capture(capture_client, req).await;
        assert_eq!(res.unwrap_err().code(), RpcStatusCode::INVALID_ARGUMENT);
    }

    #[test]
    fn test_to_rpc_status() {
        let err = ClientError::Chip(netsim_model::ChipError::ChipNotFound(ChipId(42)));
        let status = to_rpc_status(err);
        assert_eq!(status.code(), RpcStatusCode::NOT_FOUND);

        let err =
            ClientError::Framework(Box::new(netsim_model::ChipError::ChipNotFound(ChipId(42))));
        let status = to_rpc_status(err);
        assert_eq!(status.code(), RpcStatusCode::NOT_FOUND);

        let err = ClientError::Framework(Box::new(std::io::Error::other("err")));
        let status = to_rpc_status(err);
        assert_eq!(status.code(), RpcStatusCode::INTERNAL);
    }

    #[tokio::test]
    async fn test_handle_create_device_uwb() {
        let mut mock_client = MockActorClient::<DeviceActor>::new();
        let expected_id = device_api::DeviceId(100);

        mock_client
            .expect_create()
            .withf(|params| {
                matches!(
                    &params.chip.chip,
                    device_api::ChipCreateVariant::Uwb(netsim_model::UwbCreate::Anchor {
                        address: Some(addr)
                    }) if addr == "00:01"
                )
            })
            .times(1)
            .returning(move |_| Ok(expected_id));

        mock_client.expect_get().withf(move |id| *id == expected_id).times(1).returning(
            move |_| {
                Ok(Some(device_api::Device {
                    id: 100,
                    name: "uwb-device".to_string(),
                    visible: true,
                    pose: Default::default(),
                    builtin: false,
                    device_info: None,
                    chips: vec![netsim_model::Chip {
                        id: 101,
                        kind: netsim_model::ChipKind::UWB,
                        name: "uwb-anchor".to_string(),
                        manufacturer: "".to_string(),
                        product_name: "".to_string(),
                        pose: Default::default(),
                        variant: Some(netsim_model::ChipVariant::Uwb(Default::default())),
                        ..Default::default()
                    }],
                }))
            },
        );

        let client = DeviceClient::new(Box::new(mock_client));
        let mut req = netsim_proto::frontend::CreateDeviceRequest::new();
        let mut device_proto = netsim_proto::model::DeviceCreate::new();
        device_proto.name = "uwb-device".to_string();
        let mut chip_proto = netsim_proto::model::ChipCreate::new();
        chip_proto.name = "uwb-anchor".to_string();
        chip_proto.kind = EnumOrUnknown::new(netsim_proto::common::ChipKind::UWB);
        chip_proto.address = "00:01".to_string();
        device_proto.chips.push(chip_proto);
        req.device = protobuf::MessageField::some(device_proto);

        let resp = FrontendClient::handle_create_device(client, req).await.unwrap();
        assert_eq!(resp.device.name, "uwb-device");
        assert_eq!(resp.device.chips.len(), 1);
        assert_eq!(
            resp.device.chips[0].kind,
            EnumOrUnknown::new(netsim_proto::common::ChipKind::UWB)
        );
        assert_eq!(resp.device.chips[0].name, "uwb-anchor");
    }

    #[tokio::test]
    async fn test_get_capture_over_grpc_server() {
        let temp_dir = tempfile::tempdir().unwrap();

        let (capture_runner, capture_client) = capture_actor::new();
        let capture_actor_state =
            capture_actor::CaptureActor::new(false, Some(temp_dir.path().to_path_buf()));
        tokio::spawn(capture_runner.run(capture_actor_state));

        let enabled_flag = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let create = capture_actor::CaptureCreate {
            chip_kind: ChipKind::BLUETOOTH,
            device_name: "test-bt".to_string(),
            enabled_flag,
        };
        let chip_id = ChipId(10);
        capture_client.create_capture(chip_id, create).await.unwrap();

        // Enable capture to flush PCAP header to disk, then disable
        capture_client.update_capture(chip_id, true).await.unwrap();
        capture_client.update_capture(chip_id, false).await.unwrap();

        let info = capture_client.get_capture(chip_id).await.unwrap().unwrap();
        let pcap_filepath = info.filepath.as_ref().unwrap();
        let expected_bytes = std::fs::read(pcap_filepath).unwrap();

        let (_, device_client) = device_actor::new();
        let (_, ap_client) = ap_actor::new();
        let frontend = FrontendClient::new(
            device_client,
            Arc::new(MockLinkClient::new()),
            ap_client,
            capture_client.clone(),
            "1.0.0-test".to_string(),
            Arc::new(netsim_model::FrontendStats::default()),
        );

        let server_env = Arc::new(grpcio::Environment::new(1));
        let service = netsim_proto::frontend_grpc::create_frontend_service(frontend);
        let mut server =
            grpcio::ServerBuilder::new(server_env).register_service(service).build().unwrap();
        let port = server
            .add_listening_port("127.0.0.1:0", grpcio::ServerCredentials::insecure())
            .unwrap();
        server.start();
        let client_env = Arc::new(grpcio::Environment::new(1));
        let channel = grpcio::ChannelBuilder::new(client_env).connect(&format!("127.0.0.1:{port}"));
        let client = netsim_proto::frontend_grpc::FrontendServiceClient::new(channel);

        let mut req = GetCaptureRequest::new();
        req.id = chip_id.0;
        let mut rx = client.get_capture(&req).unwrap();

        let mut streamed_bytes = Vec::new();
        while let Some(chunk) = futures::StreamExt::next(&mut rx).await {
            let res = chunk.unwrap();
            streamed_bytes.extend_from_slice(&res.capture_stream);
        }
        assert_eq!(streamed_bytes, expected_bytes);

        // Verify "grpc-poll-0" is alive and can process subsequent requests
        let ver = client.get_version_async(&Empty::new()).unwrap().await.unwrap();
        assert_eq!(ver.version, "1.0.0-test");

        drop(client);
        let shutdown_fut = server.shutdown();
        server.cancel_all_calls();
        let _ = shutdown_fut.await;
    }
}
