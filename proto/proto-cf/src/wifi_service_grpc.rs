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

const METHOD_WIFI_SERVICE_GET_STATUS: ::grpcio::Method<
    super::wifi_service::GetStatusRequest,
    super::wifi_service::GetStatusResponse,
> = ::grpcio::Method {
    ty: ::grpcio::MethodType::Unary,
    name: "/netsim.wifi_service.WifiService/GetStatus",
    req_mar: ::grpcio::Marshaller { ser: ::grpcio::pb_ser, de: ::grpcio::pb_de },
    resp_mar: ::grpcio::Marshaller { ser: ::grpcio::pb_ser, de: ::grpcio::pb_de },
};

const METHOD_WIFI_SERVICE_SET_POWER: ::grpcio::Method<
    super::wifi_service::SetPowerRequest,
    super::wifi_service::SetPowerResponse,
> = ::grpcio::Method {
    ty: ::grpcio::MethodType::Unary,
    name: "/netsim.wifi_service.WifiService/SetPower",
    req_mar: ::grpcio::Marshaller { ser: ::grpcio::pb_ser, de: ::grpcio::pb_de },
    resp_mar: ::grpcio::Marshaller { ser: ::grpcio::pb_ser, de: ::grpcio::pb_de },
};

#[derive(Clone)]
pub struct WifiServiceClient {
    pub client: ::grpcio::Client,
}

impl WifiServiceClient {
    pub fn new(channel: ::grpcio::Channel) -> Self {
        WifiServiceClient { client: ::grpcio::Client::new(channel) }
    }

    pub fn get_status_opt(
        &self,
        req: &super::wifi_service::GetStatusRequest,
        opt: ::grpcio::CallOption,
    ) -> ::grpcio::Result<super::wifi_service::GetStatusResponse> {
        self.client.unary_call(&METHOD_WIFI_SERVICE_GET_STATUS, req, opt)
    }

    pub fn get_status(
        &self,
        req: &super::wifi_service::GetStatusRequest,
    ) -> ::grpcio::Result<super::wifi_service::GetStatusResponse> {
        self.get_status_opt(req, ::grpcio::CallOption::default())
    }

    pub fn get_status_async_opt(
        &self,
        req: &super::wifi_service::GetStatusRequest,
        opt: ::grpcio::CallOption,
    ) -> ::grpcio::Result<::grpcio::ClientUnaryReceiver<super::wifi_service::GetStatusResponse>>
    {
        self.client.unary_call_async(&METHOD_WIFI_SERVICE_GET_STATUS, req, opt)
    }

    pub fn get_status_async(
        &self,
        req: &super::wifi_service::GetStatusRequest,
    ) -> ::grpcio::Result<::grpcio::ClientUnaryReceiver<super::wifi_service::GetStatusResponse>>
    {
        self.get_status_async_opt(req, ::grpcio::CallOption::default())
    }

    pub fn set_power_opt(
        &self,
        req: &super::wifi_service::SetPowerRequest,
        opt: ::grpcio::CallOption,
    ) -> ::grpcio::Result<super::wifi_service::SetPowerResponse> {
        self.client.unary_call(&METHOD_WIFI_SERVICE_SET_POWER, req, opt)
    }

    pub fn set_power(
        &self,
        req: &super::wifi_service::SetPowerRequest,
    ) -> ::grpcio::Result<super::wifi_service::SetPowerResponse> {
        self.set_power_opt(req, ::grpcio::CallOption::default())
    }

    pub fn set_power_async_opt(
        &self,
        req: &super::wifi_service::SetPowerRequest,
        opt: ::grpcio::CallOption,
    ) -> ::grpcio::Result<::grpcio::ClientUnaryReceiver<super::wifi_service::SetPowerResponse>>
    {
        self.client.unary_call_async(&METHOD_WIFI_SERVICE_SET_POWER, req, opt)
    }

    pub fn set_power_async(
        &self,
        req: &super::wifi_service::SetPowerRequest,
    ) -> ::grpcio::Result<::grpcio::ClientUnaryReceiver<super::wifi_service::SetPowerResponse>>
    {
        self.set_power_async_opt(req, ::grpcio::CallOption::default())
    }
    pub fn spawn<F>(&self, f: F)
    where
        F: ::std::future::Future<Output = ()> + Send + 'static,
    {
        self.client.spawn(f)
    }
}

pub trait WifiService {
    fn get_status(
        &mut self,
        ctx: ::grpcio::RpcContext,
        _req: super::wifi_service::GetStatusRequest,
        sink: ::grpcio::UnarySink<super::wifi_service::GetStatusResponse>,
    ) {
        grpcio::unimplemented_call!(ctx, sink)
    }
    fn set_power(
        &mut self,
        ctx: ::grpcio::RpcContext,
        _req: super::wifi_service::SetPowerRequest,
        sink: ::grpcio::UnarySink<super::wifi_service::SetPowerResponse>,
    ) {
        grpcio::unimplemented_call!(ctx, sink)
    }
}

pub fn create_wifi_service<S: WifiService + Send + Clone + 'static>(s: S) -> ::grpcio::Service {
    let mut builder = ::grpcio::ServiceBuilder::new();
    let mut instance = s.clone();
    builder = builder.add_unary_handler(&METHOD_WIFI_SERVICE_GET_STATUS, move |ctx, req, resp| {
        instance.get_status(ctx, req, resp)
    });
    let mut instance = s;
    builder = builder.add_unary_handler(&METHOD_WIFI_SERVICE_SET_POWER, move |ctx, req, resp| {
        instance.set_power(ctx, req, resp)
    });
    builder.build()
}
