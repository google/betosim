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

// This file is generated. Do not edit
// @generated

// https://github.com/Manishearth/rust-clippy/issues/702
#![allow(unknown_lints)]
#![allow(clippy::all)]
#![allow(dead_code)]
#![allow(missing_docs)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]
#![allow(trivial_casts)]
#![allow(unsafe_code)]
#![allow(unused_imports)]
#![allow(unused_results)]

const METHOD_BLE_SERVICE_SCAN: ::grpcio::Method<
    super::ble_service::ScanRequest,
    super::ble_service::ScanResponse,
> = ::grpcio::Method {
    ty: ::grpcio::MethodType::ServerStreaming,
    name: "/netsim.ble_service.BleService/Scan",
    req_mar: ::grpcio::Marshaller { ser: ::grpcio::pb_ser, de: ::grpcio::pb_de },
    resp_mar: ::grpcio::Marshaller { ser: ::grpcio::pb_ser, de: ::grpcio::pb_de },
};

const METHOD_BLE_SERVICE_SNIFF: ::grpcio::Method<
    super::ble_service::SniffRequest,
    super::ble_service::SniffResponse,
> = ::grpcio::Method {
    ty: ::grpcio::MethodType::ServerStreaming,
    name: "/netsim.ble_service.BleService/Sniff",
    req_mar: ::grpcio::Marshaller { ser: ::grpcio::pb_ser, de: ::grpcio::pb_de },
    resp_mar: ::grpcio::Marshaller { ser: ::grpcio::pb_ser, de: ::grpcio::pb_de },
};

#[derive(Clone)]
pub struct BleServiceClient {
    pub client: ::grpcio::Client,
}

impl BleServiceClient {
    pub fn new(channel: ::grpcio::Channel) -> Self {
        BleServiceClient { client: ::grpcio::Client::new(channel) }
    }

    pub fn scan_opt(
        &self,
        req: &super::ble_service::ScanRequest,
        opt: ::grpcio::CallOption,
    ) -> ::grpcio::Result<::grpcio::ClientSStreamReceiver<super::ble_service::ScanResponse>> {
        self.client.server_streaming(&METHOD_BLE_SERVICE_SCAN, req, opt)
    }

    pub fn scan(
        &self,
        req: &super::ble_service::ScanRequest,
    ) -> ::grpcio::Result<::grpcio::ClientSStreamReceiver<super::ble_service::ScanResponse>> {
        self.scan_opt(req, ::grpcio::CallOption::default())
    }

    pub fn sniff_opt(
        &self,
        req: &super::ble_service::SniffRequest,
        opt: ::grpcio::CallOption,
    ) -> ::grpcio::Result<::grpcio::ClientSStreamReceiver<super::ble_service::SniffResponse>> {
        self.client.server_streaming(&METHOD_BLE_SERVICE_SNIFF, req, opt)
    }

    pub fn sniff(
        &self,
        req: &super::ble_service::SniffRequest,
    ) -> ::grpcio::Result<::grpcio::ClientSStreamReceiver<super::ble_service::SniffResponse>> {
        self.sniff_opt(req, ::grpcio::CallOption::default())
    }
    pub fn spawn<F>(&self, f: F)
    where
        F: ::std::future::Future<Output = ()> + Send + 'static,
    {
        self.client.spawn(f)
    }
}

pub trait BleService {
    fn scan(
        &mut self,
        ctx: ::grpcio::RpcContext,
        _req: super::ble_service::ScanRequest,
        sink: ::grpcio::ServerStreamingSink<super::ble_service::ScanResponse>,
    ) {
        grpcio::unimplemented_call!(ctx, sink)
    }
    fn sniff(
        &mut self,
        ctx: ::grpcio::RpcContext,
        _req: super::ble_service::SniffRequest,
        sink: ::grpcio::ServerStreamingSink<super::ble_service::SniffResponse>,
    ) {
        grpcio::unimplemented_call!(ctx, sink)
    }
}

pub fn create_ble_service<S: BleService + Send + Clone + 'static>(s: S) -> ::grpcio::Service {
    let mut builder = ::grpcio::ServiceBuilder::new();
    let mut instance = s.clone();
    builder = builder
        .add_server_streaming_handler(&METHOD_BLE_SERVICE_SCAN, move |ctx, req, resp| {
            instance.scan(ctx, req, resp)
        });
    let mut instance = s;
    builder = builder
        .add_server_streaming_handler(&METHOD_BLE_SERVICE_SNIFF, move |ctx, req, resp| {
            instance.sniff(ctx, req, resp)
        });
    builder.build()
}
