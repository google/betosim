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

const METHOD_CASIMIR_CONTROL_SERVICE_SEND_APDU: ::grpcio::Method<
    super::casimir_control::SendApduRequest,
    super::casimir_control::SendApduReply,
> = ::grpcio::Method {
    ty: ::grpcio::MethodType::Unary,
    name: "/casimircontrolserver.CasimirControlService/SendApdu",
    req_mar: ::grpcio::Marshaller { ser: ::grpcio::pb_ser, de: ::grpcio::pb_de },
    resp_mar: ::grpcio::Marshaller { ser: ::grpcio::pb_ser, de: ::grpcio::pb_de },
};

const METHOD_CASIMIR_CONTROL_SERVICE_POLL_A: ::grpcio::Method<
    super::casimir_control::Void,
    super::casimir_control::SenderId,
> = ::grpcio::Method {
    ty: ::grpcio::MethodType::Unary,
    name: "/casimircontrolserver.CasimirControlService/PollA",
    req_mar: ::grpcio::Marshaller { ser: ::grpcio::pb_ser, de: ::grpcio::pb_de },
    resp_mar: ::grpcio::Marshaller { ser: ::grpcio::pb_ser, de: ::grpcio::pb_de },
};

const METHOD_CASIMIR_CONTROL_SERVICE_SET_RADIO_STATE: ::grpcio::Method<
    super::casimir_control::RadioState,
    super::casimir_control::Void,
> = ::grpcio::Method {
    ty: ::grpcio::MethodType::Unary,
    name: "/casimircontrolserver.CasimirControlService/SetRadioState",
    req_mar: ::grpcio::Marshaller { ser: ::grpcio::pb_ser, de: ::grpcio::pb_de },
    resp_mar: ::grpcio::Marshaller { ser: ::grpcio::pb_ser, de: ::grpcio::pb_de },
};

const METHOD_CASIMIR_CONTROL_SERVICE_SET_POWER_LEVEL: ::grpcio::Method<
    super::casimir_control::PowerLevel,
    super::casimir_control::Void,
> = ::grpcio::Method {
    ty: ::grpcio::MethodType::Unary,
    name: "/casimircontrolserver.CasimirControlService/SetPowerLevel",
    req_mar: ::grpcio::Marshaller { ser: ::grpcio::pb_ser, de: ::grpcio::pb_de },
    resp_mar: ::grpcio::Marshaller { ser: ::grpcio::pb_ser, de: ::grpcio::pb_de },
};

const METHOD_CASIMIR_CONTROL_SERVICE_SEND_BROADCAST: ::grpcio::Method<
    super::casimir_control::SendBroadcastRequest,
    super::casimir_control::SendBroadcastResponse,
> = ::grpcio::Method {
    ty: ::grpcio::MethodType::Unary,
    name: "/casimircontrolserver.CasimirControlService/SendBroadcast",
    req_mar: ::grpcio::Marshaller { ser: ::grpcio::pb_ser, de: ::grpcio::pb_de },
    resp_mar: ::grpcio::Marshaller { ser: ::grpcio::pb_ser, de: ::grpcio::pb_de },
};

const METHOD_CASIMIR_CONTROL_SERVICE_INIT: ::grpcio::Method<
    super::casimir_control::Void,
    super::casimir_control::Void,
> = ::grpcio::Method {
    ty: ::grpcio::MethodType::Unary,
    name: "/casimircontrolserver.CasimirControlService/Init",
    req_mar: ::grpcio::Marshaller { ser: ::grpcio::pb_ser, de: ::grpcio::pb_de },
    resp_mar: ::grpcio::Marshaller { ser: ::grpcio::pb_ser, de: ::grpcio::pb_de },
};

const METHOD_CASIMIR_CONTROL_SERVICE_CLOSE: ::grpcio::Method<
    super::casimir_control::Void,
    super::casimir_control::Void,
> = ::grpcio::Method {
    ty: ::grpcio::MethodType::Unary,
    name: "/casimircontrolserver.CasimirControlService/Close",
    req_mar: ::grpcio::Marshaller { ser: ::grpcio::pb_ser, de: ::grpcio::pb_de },
    resp_mar: ::grpcio::Marshaller { ser: ::grpcio::pb_ser, de: ::grpcio::pb_de },
};

#[derive(Clone)]
pub struct CasimirControlServiceClient {
    pub client: ::grpcio::Client,
}

impl CasimirControlServiceClient {
    pub fn new(channel: ::grpcio::Channel) -> Self {
        CasimirControlServiceClient { client: ::grpcio::Client::new(channel) }
    }

    pub fn send_apdu_opt(
        &self,
        req: &super::casimir_control::SendApduRequest,
        opt: ::grpcio::CallOption,
    ) -> ::grpcio::Result<super::casimir_control::SendApduReply> {
        self.client.unary_call(&METHOD_CASIMIR_CONTROL_SERVICE_SEND_APDU, req, opt)
    }

    pub fn send_apdu(
        &self,
        req: &super::casimir_control::SendApduRequest,
    ) -> ::grpcio::Result<super::casimir_control::SendApduReply> {
        self.send_apdu_opt(req, ::grpcio::CallOption::default())
    }

    pub fn send_apdu_async_opt(
        &self,
        req: &super::casimir_control::SendApduRequest,
        opt: ::grpcio::CallOption,
    ) -> ::grpcio::Result<::grpcio::ClientUnaryReceiver<super::casimir_control::SendApduReply>>
    {
        self.client.unary_call_async(&METHOD_CASIMIR_CONTROL_SERVICE_SEND_APDU, req, opt)
    }

    pub fn send_apdu_async(
        &self,
        req: &super::casimir_control::SendApduRequest,
    ) -> ::grpcio::Result<::grpcio::ClientUnaryReceiver<super::casimir_control::SendApduReply>>
    {
        self.send_apdu_async_opt(req, ::grpcio::CallOption::default())
    }

    pub fn poll_a_opt(
        &self,
        req: &super::casimir_control::Void,
        opt: ::grpcio::CallOption,
    ) -> ::grpcio::Result<super::casimir_control::SenderId> {
        self.client.unary_call(&METHOD_CASIMIR_CONTROL_SERVICE_POLL_A, req, opt)
    }

    pub fn poll_a(
        &self,
        req: &super::casimir_control::Void,
    ) -> ::grpcio::Result<super::casimir_control::SenderId> {
        self.poll_a_opt(req, ::grpcio::CallOption::default())
    }

    pub fn poll_a_async_opt(
        &self,
        req: &super::casimir_control::Void,
        opt: ::grpcio::CallOption,
    ) -> ::grpcio::Result<::grpcio::ClientUnaryReceiver<super::casimir_control::SenderId>> {
        self.client.unary_call_async(&METHOD_CASIMIR_CONTROL_SERVICE_POLL_A, req, opt)
    }

    pub fn poll_a_async(
        &self,
        req: &super::casimir_control::Void,
    ) -> ::grpcio::Result<::grpcio::ClientUnaryReceiver<super::casimir_control::SenderId>> {
        self.poll_a_async_opt(req, ::grpcio::CallOption::default())
    }

    pub fn set_radio_state_opt(
        &self,
        req: &super::casimir_control::RadioState,
        opt: ::grpcio::CallOption,
    ) -> ::grpcio::Result<super::casimir_control::Void> {
        self.client.unary_call(&METHOD_CASIMIR_CONTROL_SERVICE_SET_RADIO_STATE, req, opt)
    }

    pub fn set_radio_state(
        &self,
        req: &super::casimir_control::RadioState,
    ) -> ::grpcio::Result<super::casimir_control::Void> {
        self.set_radio_state_opt(req, ::grpcio::CallOption::default())
    }

    pub fn set_radio_state_async_opt(
        &self,
        req: &super::casimir_control::RadioState,
        opt: ::grpcio::CallOption,
    ) -> ::grpcio::Result<::grpcio::ClientUnaryReceiver<super::casimir_control::Void>> {
        self.client.unary_call_async(&METHOD_CASIMIR_CONTROL_SERVICE_SET_RADIO_STATE, req, opt)
    }

    pub fn set_radio_state_async(
        &self,
        req: &super::casimir_control::RadioState,
    ) -> ::grpcio::Result<::grpcio::ClientUnaryReceiver<super::casimir_control::Void>> {
        self.set_radio_state_async_opt(req, ::grpcio::CallOption::default())
    }

    pub fn set_power_level_opt(
        &self,
        req: &super::casimir_control::PowerLevel,
        opt: ::grpcio::CallOption,
    ) -> ::grpcio::Result<super::casimir_control::Void> {
        self.client.unary_call(&METHOD_CASIMIR_CONTROL_SERVICE_SET_POWER_LEVEL, req, opt)
    }

    pub fn set_power_level(
        &self,
        req: &super::casimir_control::PowerLevel,
    ) -> ::grpcio::Result<super::casimir_control::Void> {
        self.set_power_level_opt(req, ::grpcio::CallOption::default())
    }

    pub fn set_power_level_async_opt(
        &self,
        req: &super::casimir_control::PowerLevel,
        opt: ::grpcio::CallOption,
    ) -> ::grpcio::Result<::grpcio::ClientUnaryReceiver<super::casimir_control::Void>> {
        self.client.unary_call_async(&METHOD_CASIMIR_CONTROL_SERVICE_SET_POWER_LEVEL, req, opt)
    }

    pub fn set_power_level_async(
        &self,
        req: &super::casimir_control::PowerLevel,
    ) -> ::grpcio::Result<::grpcio::ClientUnaryReceiver<super::casimir_control::Void>> {
        self.set_power_level_async_opt(req, ::grpcio::CallOption::default())
    }

    pub fn send_broadcast_opt(
        &self,
        req: &super::casimir_control::SendBroadcastRequest,
        opt: ::grpcio::CallOption,
    ) -> ::grpcio::Result<super::casimir_control::SendBroadcastResponse> {
        self.client.unary_call(&METHOD_CASIMIR_CONTROL_SERVICE_SEND_BROADCAST, req, opt)
    }

    pub fn send_broadcast(
        &self,
        req: &super::casimir_control::SendBroadcastRequest,
    ) -> ::grpcio::Result<super::casimir_control::SendBroadcastResponse> {
        self.send_broadcast_opt(req, ::grpcio::CallOption::default())
    }

    pub fn send_broadcast_async_opt(
        &self,
        req: &super::casimir_control::SendBroadcastRequest,
        opt: ::grpcio::CallOption,
    ) -> ::grpcio::Result<
        ::grpcio::ClientUnaryReceiver<super::casimir_control::SendBroadcastResponse>,
    > {
        self.client.unary_call_async(&METHOD_CASIMIR_CONTROL_SERVICE_SEND_BROADCAST, req, opt)
    }

    pub fn send_broadcast_async(
        &self,
        req: &super::casimir_control::SendBroadcastRequest,
    ) -> ::grpcio::Result<
        ::grpcio::ClientUnaryReceiver<super::casimir_control::SendBroadcastResponse>,
    > {
        self.send_broadcast_async_opt(req, ::grpcio::CallOption::default())
    }

    pub fn init_opt(
        &self,
        req: &super::casimir_control::Void,
        opt: ::grpcio::CallOption,
    ) -> ::grpcio::Result<super::casimir_control::Void> {
        self.client.unary_call(&METHOD_CASIMIR_CONTROL_SERVICE_INIT, req, opt)
    }

    pub fn init(
        &self,
        req: &super::casimir_control::Void,
    ) -> ::grpcio::Result<super::casimir_control::Void> {
        self.init_opt(req, ::grpcio::CallOption::default())
    }

    pub fn init_async_opt(
        &self,
        req: &super::casimir_control::Void,
        opt: ::grpcio::CallOption,
    ) -> ::grpcio::Result<::grpcio::ClientUnaryReceiver<super::casimir_control::Void>> {
        self.client.unary_call_async(&METHOD_CASIMIR_CONTROL_SERVICE_INIT, req, opt)
    }

    pub fn init_async(
        &self,
        req: &super::casimir_control::Void,
    ) -> ::grpcio::Result<::grpcio::ClientUnaryReceiver<super::casimir_control::Void>> {
        self.init_async_opt(req, ::grpcio::CallOption::default())
    }

    pub fn close_opt(
        &self,
        req: &super::casimir_control::Void,
        opt: ::grpcio::CallOption,
    ) -> ::grpcio::Result<super::casimir_control::Void> {
        self.client.unary_call(&METHOD_CASIMIR_CONTROL_SERVICE_CLOSE, req, opt)
    }

    pub fn close(
        &self,
        req: &super::casimir_control::Void,
    ) -> ::grpcio::Result<super::casimir_control::Void> {
        self.close_opt(req, ::grpcio::CallOption::default())
    }

    pub fn close_async_opt(
        &self,
        req: &super::casimir_control::Void,
        opt: ::grpcio::CallOption,
    ) -> ::grpcio::Result<::grpcio::ClientUnaryReceiver<super::casimir_control::Void>> {
        self.client.unary_call_async(&METHOD_CASIMIR_CONTROL_SERVICE_CLOSE, req, opt)
    }

    pub fn close_async(
        &self,
        req: &super::casimir_control::Void,
    ) -> ::grpcio::Result<::grpcio::ClientUnaryReceiver<super::casimir_control::Void>> {
        self.close_async_opt(req, ::grpcio::CallOption::default())
    }
    pub fn spawn<F>(&self, f: F)
    where
        F: ::std::future::Future<Output = ()> + Send + 'static,
    {
        self.client.spawn(f)
    }
}

pub trait CasimirControlService {
    fn send_apdu(
        &mut self,
        ctx: ::grpcio::RpcContext,
        _req: super::casimir_control::SendApduRequest,
        sink: ::grpcio::UnarySink<super::casimir_control::SendApduReply>,
    ) {
        grpcio::unimplemented_call!(ctx, sink)
    }
    fn poll_a(
        &mut self,
        ctx: ::grpcio::RpcContext,
        _req: super::casimir_control::Void,
        sink: ::grpcio::UnarySink<super::casimir_control::SenderId>,
    ) {
        grpcio::unimplemented_call!(ctx, sink)
    }
    fn set_radio_state(
        &mut self,
        ctx: ::grpcio::RpcContext,
        _req: super::casimir_control::RadioState,
        sink: ::grpcio::UnarySink<super::casimir_control::Void>,
    ) {
        grpcio::unimplemented_call!(ctx, sink)
    }
    fn set_power_level(
        &mut self,
        ctx: ::grpcio::RpcContext,
        _req: super::casimir_control::PowerLevel,
        sink: ::grpcio::UnarySink<super::casimir_control::Void>,
    ) {
        grpcio::unimplemented_call!(ctx, sink)
    }
    fn send_broadcast(
        &mut self,
        ctx: ::grpcio::RpcContext,
        _req: super::casimir_control::SendBroadcastRequest,
        sink: ::grpcio::UnarySink<super::casimir_control::SendBroadcastResponse>,
    ) {
        grpcio::unimplemented_call!(ctx, sink)
    }
    fn init(
        &mut self,
        ctx: ::grpcio::RpcContext,
        _req: super::casimir_control::Void,
        sink: ::grpcio::UnarySink<super::casimir_control::Void>,
    ) {
        grpcio::unimplemented_call!(ctx, sink)
    }
    fn close(
        &mut self,
        ctx: ::grpcio::RpcContext,
        _req: super::casimir_control::Void,
        sink: ::grpcio::UnarySink<super::casimir_control::Void>,
    ) {
        grpcio::unimplemented_call!(ctx, sink)
    }
}

pub fn create_casimir_control_service<S: CasimirControlService + Send + Clone + 'static>(
    s: S,
) -> ::grpcio::Service {
    let mut builder = ::grpcio::ServiceBuilder::new();
    let mut instance = s.clone();
    builder = builder
        .add_unary_handler(&METHOD_CASIMIR_CONTROL_SERVICE_SEND_APDU, move |ctx, req, resp| {
            instance.send_apdu(ctx, req, resp)
        });
    let mut instance = s.clone();
    builder = builder
        .add_unary_handler(&METHOD_CASIMIR_CONTROL_SERVICE_POLL_A, move |ctx, req, resp| {
            instance.poll_a(ctx, req, resp)
        });
    let mut instance = s.clone();
    builder = builder.add_unary_handler(
        &METHOD_CASIMIR_CONTROL_SERVICE_SET_RADIO_STATE,
        move |ctx, req, resp| instance.set_radio_state(ctx, req, resp),
    );
    let mut instance = s.clone();
    builder = builder.add_unary_handler(
        &METHOD_CASIMIR_CONTROL_SERVICE_SET_POWER_LEVEL,
        move |ctx, req, resp| instance.set_power_level(ctx, req, resp),
    );
    let mut instance = s.clone();
    builder = builder.add_unary_handler(
        &METHOD_CASIMIR_CONTROL_SERVICE_SEND_BROADCAST,
        move |ctx, req, resp| instance.send_broadcast(ctx, req, resp),
    );
    let mut instance = s.clone();
    builder = builder
        .add_unary_handler(&METHOD_CASIMIR_CONTROL_SERVICE_INIT, move |ctx, req, resp| {
            instance.init(ctx, req, resp)
        });
    let mut instance = s;
    builder = builder
        .add_unary_handler(&METHOD_CASIMIR_CONTROL_SERVICE_CLOSE, move |ctx, req, resp| {
            instance.close(ctx, req, resp)
        });
    builder.build()
}
