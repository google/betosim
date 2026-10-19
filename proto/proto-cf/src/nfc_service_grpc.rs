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

const METHOD_NFC_SERVICE_GET_STATUS: ::grpcio::Method<
    super::nfc_service::GetStatusRequest,
    super::nfc_service::GetStatusResponse,
> = ::grpcio::Method {
    ty: ::grpcio::MethodType::Unary,
    name: "/netsim.nfc_service.NfcService/GetStatus",
    req_mar: ::grpcio::Marshaller { ser: ::grpcio::pb_ser, de: ::grpcio::pb_de },
    resp_mar: ::grpcio::Marshaller { ser: ::grpcio::pb_ser, de: ::grpcio::pb_de },
};

const METHOD_NFC_SERVICE_SET_POWER: ::grpcio::Method<
    super::nfc_service::SetPowerRequest,
    super::nfc_service::SetPowerResponse,
> = ::grpcio::Method {
    ty: ::grpcio::MethodType::Unary,
    name: "/netsim.nfc_service.NfcService/SetPower",
    req_mar: ::grpcio::Marshaller { ser: ::grpcio::pb_ser, de: ::grpcio::pb_de },
    resp_mar: ::grpcio::Marshaller { ser: ::grpcio::pb_ser, de: ::grpcio::pb_de },
};

const METHOD_NFC_SERVICE_POLL: ::grpcio::Method<
    super::nfc_service::PollRequest,
    super::nfc_service::PollResponse,
> = ::grpcio::Method {
    ty: ::grpcio::MethodType::ServerStreaming,
    name: "/netsim.nfc_service.NfcService/Poll",
    req_mar: ::grpcio::Marshaller { ser: ::grpcio::pb_ser, de: ::grpcio::pb_de },
    resp_mar: ::grpcio::Marshaller { ser: ::grpcio::pb_ser, de: ::grpcio::pb_de },
};

const METHOD_NFC_SERVICE_SEND_APDU: ::grpcio::Method<
    super::nfc_service::SendApduRequest,
    super::nfc_service::SendApduResponse,
> = ::grpcio::Method {
    ty: ::grpcio::MethodType::Unary,
    name: "/netsim.nfc_service.NfcService/SendApdu",
    req_mar: ::grpcio::Marshaller { ser: ::grpcio::pb_ser, de: ::grpcio::pb_de },
    resp_mar: ::grpcio::Marshaller { ser: ::grpcio::pb_ser, de: ::grpcio::pb_de },
};

#[derive(Clone)]
pub struct NfcServiceClient {
    pub client: ::grpcio::Client,
}

impl NfcServiceClient {
    pub fn new(channel: ::grpcio::Channel) -> Self {
        NfcServiceClient { client: ::grpcio::Client::new(channel) }
    }

    pub fn get_status_opt(
        &self,
        req: &super::nfc_service::GetStatusRequest,
        opt: ::grpcio::CallOption,
    ) -> ::grpcio::Result<super::nfc_service::GetStatusResponse> {
        self.client.unary_call(&METHOD_NFC_SERVICE_GET_STATUS, req, opt)
    }

    pub fn get_status(
        &self,
        req: &super::nfc_service::GetStatusRequest,
    ) -> ::grpcio::Result<super::nfc_service::GetStatusResponse> {
        self.get_status_opt(req, ::grpcio::CallOption::default())
    }

    pub fn get_status_async_opt(
        &self,
        req: &super::nfc_service::GetStatusRequest,
        opt: ::grpcio::CallOption,
    ) -> ::grpcio::Result<::grpcio::ClientUnaryReceiver<super::nfc_service::GetStatusResponse>>
    {
        self.client.unary_call_async(&METHOD_NFC_SERVICE_GET_STATUS, req, opt)
    }

    pub fn get_status_async(
        &self,
        req: &super::nfc_service::GetStatusRequest,
    ) -> ::grpcio::Result<::grpcio::ClientUnaryReceiver<super::nfc_service::GetStatusResponse>>
    {
        self.get_status_async_opt(req, ::grpcio::CallOption::default())
    }

    pub fn set_power_opt(
        &self,
        req: &super::nfc_service::SetPowerRequest,
        opt: ::grpcio::CallOption,
    ) -> ::grpcio::Result<super::nfc_service::SetPowerResponse> {
        self.client.unary_call(&METHOD_NFC_SERVICE_SET_POWER, req, opt)
    }

    pub fn set_power(
        &self,
        req: &super::nfc_service::SetPowerRequest,
    ) -> ::grpcio::Result<super::nfc_service::SetPowerResponse> {
        self.set_power_opt(req, ::grpcio::CallOption::default())
    }

    pub fn set_power_async_opt(
        &self,
        req: &super::nfc_service::SetPowerRequest,
        opt: ::grpcio::CallOption,
    ) -> ::grpcio::Result<::grpcio::ClientUnaryReceiver<super::nfc_service::SetPowerResponse>> {
        self.client.unary_call_async(&METHOD_NFC_SERVICE_SET_POWER, req, opt)
    }

    pub fn set_power_async(
        &self,
        req: &super::nfc_service::SetPowerRequest,
    ) -> ::grpcio::Result<::grpcio::ClientUnaryReceiver<super::nfc_service::SetPowerResponse>> {
        self.set_power_async_opt(req, ::grpcio::CallOption::default())
    }

    pub fn poll_opt(
        &self,
        req: &super::nfc_service::PollRequest,
        opt: ::grpcio::CallOption,
    ) -> ::grpcio::Result<::grpcio::ClientSStreamReceiver<super::nfc_service::PollResponse>> {
        self.client.server_streaming(&METHOD_NFC_SERVICE_POLL, req, opt)
    }

    pub fn poll(
        &self,
        req: &super::nfc_service::PollRequest,
    ) -> ::grpcio::Result<::grpcio::ClientSStreamReceiver<super::nfc_service::PollResponse>> {
        self.poll_opt(req, ::grpcio::CallOption::default())
    }

    pub fn send_apdu_opt(
        &self,
        req: &super::nfc_service::SendApduRequest,
        opt: ::grpcio::CallOption,
    ) -> ::grpcio::Result<super::nfc_service::SendApduResponse> {
        self.client.unary_call(&METHOD_NFC_SERVICE_SEND_APDU, req, opt)
    }

    pub fn send_apdu(
        &self,
        req: &super::nfc_service::SendApduRequest,
    ) -> ::grpcio::Result<super::nfc_service::SendApduResponse> {
        self.send_apdu_opt(req, ::grpcio::CallOption::default())
    }

    pub fn send_apdu_async_opt(
        &self,
        req: &super::nfc_service::SendApduRequest,
        opt: ::grpcio::CallOption,
    ) -> ::grpcio::Result<::grpcio::ClientUnaryReceiver<super::nfc_service::SendApduResponse>> {
        self.client.unary_call_async(&METHOD_NFC_SERVICE_SEND_APDU, req, opt)
    }

    pub fn send_apdu_async(
        &self,
        req: &super::nfc_service::SendApduRequest,
    ) -> ::grpcio::Result<::grpcio::ClientUnaryReceiver<super::nfc_service::SendApduResponse>> {
        self.send_apdu_async_opt(req, ::grpcio::CallOption::default())
    }
    pub fn spawn<F>(&self, f: F)
    where
        F: ::std::future::Future<Output = ()> + Send + 'static,
    {
        self.client.spawn(f)
    }
}

pub trait NfcService {
    fn get_status(
        &mut self,
        ctx: ::grpcio::RpcContext,
        _req: super::nfc_service::GetStatusRequest,
        sink: ::grpcio::UnarySink<super::nfc_service::GetStatusResponse>,
    ) {
        grpcio::unimplemented_call!(ctx, sink)
    }
    fn set_power(
        &mut self,
        ctx: ::grpcio::RpcContext,
        _req: super::nfc_service::SetPowerRequest,
        sink: ::grpcio::UnarySink<super::nfc_service::SetPowerResponse>,
    ) {
        grpcio::unimplemented_call!(ctx, sink)
    }
    fn poll(
        &mut self,
        ctx: ::grpcio::RpcContext,
        _req: super::nfc_service::PollRequest,
        sink: ::grpcio::ServerStreamingSink<super::nfc_service::PollResponse>,
    ) {
        grpcio::unimplemented_call!(ctx, sink)
    }
    fn send_apdu(
        &mut self,
        ctx: ::grpcio::RpcContext,
        _req: super::nfc_service::SendApduRequest,
        sink: ::grpcio::UnarySink<super::nfc_service::SendApduResponse>,
    ) {
        grpcio::unimplemented_call!(ctx, sink)
    }
}

pub fn create_nfc_service<S: NfcService + Send + Clone + 'static>(s: S) -> ::grpcio::Service {
    let mut builder = ::grpcio::ServiceBuilder::new();
    let mut instance = s.clone();
    builder = builder.add_unary_handler(&METHOD_NFC_SERVICE_GET_STATUS, move |ctx, req, resp| {
        instance.get_status(ctx, req, resp)
    });
    let mut instance = s.clone();
    builder = builder.add_unary_handler(&METHOD_NFC_SERVICE_SET_POWER, move |ctx, req, resp| {
        instance.set_power(ctx, req, resp)
    });
    let mut instance = s.clone();
    builder = builder
        .add_server_streaming_handler(&METHOD_NFC_SERVICE_POLL, move |ctx, req, resp| {
            instance.poll(ctx, req, resp)
        });
    let mut instance = s;
    builder = builder.add_unary_handler(&METHOD_NFC_SERVICE_SEND_APDU, move |ctx, req, resp| {
        instance.send_apdu(ctx, req, resp)
    });
    builder.build()
}
