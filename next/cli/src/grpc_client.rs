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
use netsim_proto::{frontend, frontend_grpc::FrontendServiceClient};
use protobuf::well_known_types::empty;

use crate::error::Result;

// Enum of Grpc Requests holding the request proto as applicable
#[derive(Debug, PartialEq)]
pub(crate) enum GrpcRequest {
    ListDevice,
    CreateDevice(frontend::CreateDeviceRequest),
    ListLink,
    CreateLink(frontend::CreateLinkRequest),
    PatchLink(frontend::PatchLinkRequest),
    DeleteLink(frontend::DeleteLinkRequest),
}

// Enum of Grpc Responses holding the response proto as applicable
#[derive(Debug, PartialEq)]
pub(crate) enum GrpcResponse {
    ListDevice(frontend::ListDeviceResponse),
    CreateDevice(frontend::CreateDeviceResponse),
    ListLink(frontend::ListLinkResponse),
    CreateLink(frontend::CreateLinkResponse),
    PatchLink,
    DeleteLink,
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
            GrpcRequest::CreateDevice(req) => {
                Ok(GrpcResponse::CreateDevice(self.create_device(req)?))
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
        }
    }
}

pub(crate) fn send_grpc(
    client: &FrontendServiceClient,
    grpc_request: &GrpcRequest,
) -> Result<GrpcResponse> {
    client.send_grpc(grpc_request)
}
