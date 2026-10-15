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
use crate::error::Result;
use futures_util::StreamExt;
use netsim_proto::frontend;
use netsim_proto::frontend_grpc::FrontendServiceClient;
use protobuf::well_known_types::empty;

/// Wrapper struct for application defined ClientResponseReader
pub struct ClientResponseReader {
    /// Delegated handler for reading responses
    pub handler: Box<dyn ClientResponseReadable>,
}

/// Delegating functions to handler
impl ClientResponseReader {
    fn handle_chunk(&self, chunk: &[u8]) {
        self.handler.handle_chunk(chunk);
    }
}

/// Trait for ClientResponseReader handler functions
pub trait ClientResponseReadable {
    /// Process each chunk of streaming response
    fn handle_chunk(&self, chunk: &[u8]);
}

// Enum of Grpc Requests holding the request proto as applicable
#[derive(Debug, PartialEq)]
pub enum GrpcRequest {
    GetVersion,
    ListDevice,
    Reset,
    ListCapture,
    CreateDevice(frontend::CreateDeviceRequest),
    DeleteChip(frontend::DeleteChipRequest),
    PatchDevice(frontend::PatchDeviceRequest),
    PatchCapture(frontend::PatchCaptureRequest),
    GetCapture(frontend::GetCaptureRequest),
    ListLink,
    PatchLink(frontend::PatchLinkRequest),
    DeleteLink(frontend::DeleteLinkRequest),
}

// Enum of Grpc Responses holding the response proto as applicable
#[derive(Debug, PartialEq)]
pub enum GrpcResponse {
    GetVersion(frontend::VersionResponse),
    ListDevice(frontend::ListDeviceResponse),
    Reset,
    ListCapture(frontend::ListCaptureResponse),
    CreateDevice(frontend::CreateDeviceResponse),
    DeleteChip,
    PatchDevice,
    PatchCapture,
    ListLink(frontend::ListLinkResponse),
    PatchLink,
    DeleteLink,
    Unknown,
}

pub fn get_capture(
    client: &FrontendServiceClient,
    req: &frontend::GetCaptureRequest,
    client_reader: &mut ClientResponseReader,
) -> Result<()> {
    let mut stream = client.get_capture(req)?;
    // Use block_on to run the async block handling all chunks
    futures::executor::block_on(async {
        // Read every available chunk from gRPC stream
        while let Some(Ok(chunk)) = stream.next().await {
            let bytes = chunk.capture_stream;
            client_reader.handle_chunk(&bytes);
        }
    });

    Ok(())
}

pub fn send_grpc(
    client: &FrontendServiceClient,
    grpc_request: &GrpcRequest,
) -> Result<GrpcResponse> {
    match grpc_request {
        GrpcRequest::GetVersion => {
            Ok(GrpcResponse::GetVersion(client.get_version(&empty::Empty::new())?))
        }
        GrpcRequest::ListDevice => {
            Ok(GrpcResponse::ListDevice(client.list_device(&empty::Empty::new())?))
        }
        GrpcRequest::Reset => {
            client.reset(&empty::Empty::new())?;
            Ok(GrpcResponse::Reset)
        }
        GrpcRequest::ListCapture => {
            Ok(GrpcResponse::ListCapture(client.list_capture(&empty::Empty::new())?))
        }
        GrpcRequest::CreateDevice(req) => {
            Ok(GrpcResponse::CreateDevice(client.create_device(req)?))
        }
        GrpcRequest::DeleteChip(req) => {
            client.delete_chip(req)?;
            Ok(GrpcResponse::DeleteChip)
        }
        GrpcRequest::PatchDevice(req) => {
            client.patch_device(req)?;
            Ok(GrpcResponse::PatchDevice)
        }
        GrpcRequest::PatchCapture(req) => {
            client.patch_capture(req)?;
            Ok(GrpcResponse::PatchCapture)
        }
        GrpcRequest::ListLink => {
            Ok(GrpcResponse::ListLink(client.list_link(&empty::Empty::new())?))
        }
        GrpcRequest::PatchLink(req) => {
            client.patch_link(req)?;
            Ok(GrpcResponse::PatchLink)
        }
        GrpcRequest::DeleteLink(req) => {
            client.delete_link(req)?;
            Ok(GrpcResponse::DeleteLink)
        }
        _ => Err(grpcio::Error::RpcFailure(grpcio::RpcStatus::new(
            grpcio::RpcStatusCode::INVALID_ARGUMENT,
        ))
        .into()),
    }
}
