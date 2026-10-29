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

//! gRPC frontend client library for netsim.
use futures_util::StreamExt;
use netsim_proto::{frontend, frontend_grpc::FrontendServiceClient};
use protobuf::well_known_types::empty;

use crate::error::Result;

/// Wrapper struct for application defined ClientResponseReader
pub(crate) struct ClientResponseReader {
    /// Delegated handler for reading responses
    pub handler: Box<dyn ClientResponseReadable>,
}

/// Delegating functions to handler
impl ClientResponseReader {
    fn handle_chunk(&mut self, chunk: &[u8]) -> Result<()> {
        self.handler.handle_chunk(chunk)
    }
}

/// Trait for ClientResponseReader handler functions
pub(crate) trait ClientResponseReadable {
    /// Process each chunk of streaming response
    fn handle_chunk(&mut self, chunk: &[u8]) -> Result<()>;
}

// Enum of Grpc Requests holding the request proto as applicable
#[derive(Debug, PartialEq)]
pub(crate) enum GrpcRequest {
    ListDevice,
    ListCapture,
    CreateDevice(frontend::CreateDeviceRequest),
    DeleteChip(frontend::DeleteChipRequest),
    DeleteDevice(frontend::DeleteDeviceRequest),
    PatchDevice(frontend::PatchDeviceRequest),
    PatchCapture(frontend::PatchCaptureRequest),
    GetCapture(frontend::GetCaptureRequest),
    ListLink,
    CreateLink(frontend::CreateLinkRequest),
    PatchLink(frontend::PatchLinkRequest),
    DeleteLink(frontend::DeleteLinkRequest),
}

// Enum of Grpc Responses holding the response proto as applicable
#[derive(Debug, PartialEq)]
pub(crate) enum GrpcResponse {
    ListDevice(frontend::ListDeviceResponse),
    ListCapture(frontend::ListCaptureResponse),
    CreateDevice(frontend::CreateDeviceResponse),
    DeleteChip,
    DeleteDevice,
    PatchDevice,
    PatchCapture,
    ListLink(frontend::ListLinkResponse),
    CreateLink(frontend::CreateLinkResponse),
    PatchLink,
    DeleteLink,
    Unknown,
}

pub(crate) fn get_capture(
    client: &FrontendServiceClient,
    req: &frontend::GetCaptureRequest,
    client_reader: &mut ClientResponseReader,
) -> Result<()> {
    let mut stream = client.get_capture(req)?;
    // Use block_on to run the async block handling all chunks
    futures::executor::block_on(async {
        while let Some(chunk_res) = stream.next().await {
            let chunk = chunk_res?;
            let bytes = chunk.capture_stream;
            client_reader.handle_chunk(&bytes)?;
        }
        Ok(())
    })
}

pub(crate) trait GrpcMethodExecutor {
    fn send_grpc(&self, grpc_request: &GrpcRequest) -> Result<GrpcResponse>;
}

impl GrpcMethodExecutor for FrontendServiceClient {
    fn send_grpc(&self, grpc_request: &GrpcRequest) -> Result<GrpcResponse> {
        match grpc_request {
            GrpcRequest::ListDevice => {
                Ok(GrpcResponse::ListDevice(self.list_device(&empty::Empty::new())?))
            }
            GrpcRequest::ListCapture => {
                Ok(GrpcResponse::ListCapture(self.list_capture(&empty::Empty::new())?))
            }
            GrpcRequest::CreateDevice(req) => {
                Ok(GrpcResponse::CreateDevice(self.create_device(req)?))
            }
            GrpcRequest::DeleteChip(req) => {
                self.delete_chip(req)?;
                Ok(GrpcResponse::DeleteChip)
            }
            GrpcRequest::DeleteDevice(req) => {
                self.delete_device(req)?;
                Ok(GrpcResponse::DeleteDevice)
            }
            GrpcRequest::PatchDevice(req) => {
                self.patch_device(req)?;
                Ok(GrpcResponse::PatchDevice)
            }
            GrpcRequest::PatchCapture(req) => {
                self.patch_capture(req)?;
                Ok(GrpcResponse::PatchCapture)
            }
            GrpcRequest::ListLink => {
                Ok(GrpcResponse::ListLink(self.list_link(&empty::Empty::new())?))
            }
            GrpcRequest::CreateLink(req) => Ok(GrpcResponse::CreateLink(self.create_link(req)?)),
            GrpcRequest::PatchLink(req) => {
                self.patch_link(req)?;
                Ok(GrpcResponse::PatchLink)
            }
            GrpcRequest::DeleteLink(req) => {
                self.delete_link(req)?;
                Ok(GrpcResponse::DeleteLink)
            }
            _ => Err(grpcio::Error::RpcFailure(grpcio::RpcStatus::new(
                grpcio::RpcStatusCode::INVALID_ARGUMENT,
            ))
            .into()),
        }
    }
}

pub(crate) fn send_grpc(
    client: &FrontendServiceClient,
    grpc_request: &GrpcRequest,
) -> Result<GrpcResponse> {
    client.send_grpc(grpc_request)
}
