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

const METHOD_ACCESS_POINT_SERVICE_CREATE: ::grpcio::Method<
    super::access_point::CreateAccessPointRequest,
    super::access_point::AccessPoint,
> = ::grpcio::Method {
    ty: ::grpcio::MethodType::Unary,
    name: "/netsim.access_point.AccessPointService/Create",
    req_mar: ::grpcio::Marshaller { ser: ::grpcio::pb_ser, de: ::grpcio::pb_de },
    resp_mar: ::grpcio::Marshaller { ser: ::grpcio::pb_ser, de: ::grpcio::pb_de },
};

const METHOD_ACCESS_POINT_SERVICE_GET: ::grpcio::Method<
    super::access_point::GetAccessPointRequest,
    super::access_point::AccessPoint,
> = ::grpcio::Method {
    ty: ::grpcio::MethodType::Unary,
    name: "/netsim.access_point.AccessPointService/Get",
    req_mar: ::grpcio::Marshaller { ser: ::grpcio::pb_ser, de: ::grpcio::pb_de },
    resp_mar: ::grpcio::Marshaller { ser: ::grpcio::pb_ser, de: ::grpcio::pb_de },
};

const METHOD_ACCESS_POINT_SERVICE_UPDATE: ::grpcio::Method<
    super::access_point::UpdateAccessPointRequest,
    super::access_point::AccessPoint,
> = ::grpcio::Method {
    ty: ::grpcio::MethodType::Unary,
    name: "/netsim.access_point.AccessPointService/Update",
    req_mar: ::grpcio::Marshaller { ser: ::grpcio::pb_ser, de: ::grpcio::pb_de },
    resp_mar: ::grpcio::Marshaller { ser: ::grpcio::pb_ser, de: ::grpcio::pb_de },
};

const METHOD_ACCESS_POINT_SERVICE_DELETE: ::grpcio::Method<
    super::access_point::DeleteAccessPointRequest,
    super::empty::Empty,
> = ::grpcio::Method {
    ty: ::grpcio::MethodType::Unary,
    name: "/netsim.access_point.AccessPointService/Delete",
    req_mar: ::grpcio::Marshaller { ser: ::grpcio::pb_ser, de: ::grpcio::pb_de },
    resp_mar: ::grpcio::Marshaller { ser: ::grpcio::pb_ser, de: ::grpcio::pb_de },
};

const METHOD_ACCESS_POINT_SERVICE_LIST: ::grpcio::Method<
    super::access_point::ListAccessPointsRequest,
    super::access_point::ListAccessPointsResponse,
> = ::grpcio::Method {
    ty: ::grpcio::MethodType::Unary,
    name: "/netsim.access_point.AccessPointService/List",
    req_mar: ::grpcio::Marshaller { ser: ::grpcio::pb_ser, de: ::grpcio::pb_de },
    resp_mar: ::grpcio::Marshaller { ser: ::grpcio::pb_ser, de: ::grpcio::pb_de },
};

const METHOD_ACCESS_POINT_SERVICE_EXECUTE: ::grpcio::Method<
    super::access_point::ExecuteAccessPointRequest,
    super::empty::Empty,
> = ::grpcio::Method {
    ty: ::grpcio::MethodType::Unary,
    name: "/netsim.access_point.AccessPointService/Execute",
    req_mar: ::grpcio::Marshaller { ser: ::grpcio::pb_ser, de: ::grpcio::pb_de },
    resp_mar: ::grpcio::Marshaller { ser: ::grpcio::pb_ser, de: ::grpcio::pb_de },
};

#[derive(Clone)]
pub struct AccessPointServiceClient {
    pub client: ::grpcio::Client,
}

impl AccessPointServiceClient {
    pub fn new(channel: ::grpcio::Channel) -> Self {
        AccessPointServiceClient { client: ::grpcio::Client::new(channel) }
    }

    pub fn create_opt(
        &self,
        req: &super::access_point::CreateAccessPointRequest,
        opt: ::grpcio::CallOption,
    ) -> ::grpcio::Result<super::access_point::AccessPoint> {
        self.client.unary_call(&METHOD_ACCESS_POINT_SERVICE_CREATE, req, opt)
    }

    pub fn create(
        &self,
        req: &super::access_point::CreateAccessPointRequest,
    ) -> ::grpcio::Result<super::access_point::AccessPoint> {
        self.create_opt(req, ::grpcio::CallOption::default())
    }

    pub fn create_async_opt(
        &self,
        req: &super::access_point::CreateAccessPointRequest,
        opt: ::grpcio::CallOption,
    ) -> ::grpcio::Result<::grpcio::ClientUnaryReceiver<super::access_point::AccessPoint>> {
        self.client.unary_call_async(&METHOD_ACCESS_POINT_SERVICE_CREATE, req, opt)
    }

    pub fn create_async(
        &self,
        req: &super::access_point::CreateAccessPointRequest,
    ) -> ::grpcio::Result<::grpcio::ClientUnaryReceiver<super::access_point::AccessPoint>> {
        self.create_async_opt(req, ::grpcio::CallOption::default())
    }

    pub fn get_opt(
        &self,
        req: &super::access_point::GetAccessPointRequest,
        opt: ::grpcio::CallOption,
    ) -> ::grpcio::Result<super::access_point::AccessPoint> {
        self.client.unary_call(&METHOD_ACCESS_POINT_SERVICE_GET, req, opt)
    }

    pub fn get(
        &self,
        req: &super::access_point::GetAccessPointRequest,
    ) -> ::grpcio::Result<super::access_point::AccessPoint> {
        self.get_opt(req, ::grpcio::CallOption::default())
    }

    pub fn get_async_opt(
        &self,
        req: &super::access_point::GetAccessPointRequest,
        opt: ::grpcio::CallOption,
    ) -> ::grpcio::Result<::grpcio::ClientUnaryReceiver<super::access_point::AccessPoint>> {
        self.client.unary_call_async(&METHOD_ACCESS_POINT_SERVICE_GET, req, opt)
    }

    pub fn get_async(
        &self,
        req: &super::access_point::GetAccessPointRequest,
    ) -> ::grpcio::Result<::grpcio::ClientUnaryReceiver<super::access_point::AccessPoint>> {
        self.get_async_opt(req, ::grpcio::CallOption::default())
    }

    pub fn update_opt(
        &self,
        req: &super::access_point::UpdateAccessPointRequest,
        opt: ::grpcio::CallOption,
    ) -> ::grpcio::Result<super::access_point::AccessPoint> {
        self.client.unary_call(&METHOD_ACCESS_POINT_SERVICE_UPDATE, req, opt)
    }

    pub fn update(
        &self,
        req: &super::access_point::UpdateAccessPointRequest,
    ) -> ::grpcio::Result<super::access_point::AccessPoint> {
        self.update_opt(req, ::grpcio::CallOption::default())
    }

    pub fn update_async_opt(
        &self,
        req: &super::access_point::UpdateAccessPointRequest,
        opt: ::grpcio::CallOption,
    ) -> ::grpcio::Result<::grpcio::ClientUnaryReceiver<super::access_point::AccessPoint>> {
        self.client.unary_call_async(&METHOD_ACCESS_POINT_SERVICE_UPDATE, req, opt)
    }

    pub fn update_async(
        &self,
        req: &super::access_point::UpdateAccessPointRequest,
    ) -> ::grpcio::Result<::grpcio::ClientUnaryReceiver<super::access_point::AccessPoint>> {
        self.update_async_opt(req, ::grpcio::CallOption::default())
    }

    pub fn delete_opt(
        &self,
        req: &super::access_point::DeleteAccessPointRequest,
        opt: ::grpcio::CallOption,
    ) -> ::grpcio::Result<super::empty::Empty> {
        self.client.unary_call(&METHOD_ACCESS_POINT_SERVICE_DELETE, req, opt)
    }

    pub fn delete(
        &self,
        req: &super::access_point::DeleteAccessPointRequest,
    ) -> ::grpcio::Result<super::empty::Empty> {
        self.delete_opt(req, ::grpcio::CallOption::default())
    }

    pub fn delete_async_opt(
        &self,
        req: &super::access_point::DeleteAccessPointRequest,
        opt: ::grpcio::CallOption,
    ) -> ::grpcio::Result<::grpcio::ClientUnaryReceiver<super::empty::Empty>> {
        self.client.unary_call_async(&METHOD_ACCESS_POINT_SERVICE_DELETE, req, opt)
    }

    pub fn delete_async(
        &self,
        req: &super::access_point::DeleteAccessPointRequest,
    ) -> ::grpcio::Result<::grpcio::ClientUnaryReceiver<super::empty::Empty>> {
        self.delete_async_opt(req, ::grpcio::CallOption::default())
    }

    pub fn list_opt(
        &self,
        req: &super::access_point::ListAccessPointsRequest,
        opt: ::grpcio::CallOption,
    ) -> ::grpcio::Result<super::access_point::ListAccessPointsResponse> {
        self.client.unary_call(&METHOD_ACCESS_POINT_SERVICE_LIST, req, opt)
    }

    pub fn list(
        &self,
        req: &super::access_point::ListAccessPointsRequest,
    ) -> ::grpcio::Result<super::access_point::ListAccessPointsResponse> {
        self.list_opt(req, ::grpcio::CallOption::default())
    }

    pub fn list_async_opt(
        &self,
        req: &super::access_point::ListAccessPointsRequest,
        opt: ::grpcio::CallOption,
    ) -> ::grpcio::Result<
        ::grpcio::ClientUnaryReceiver<super::access_point::ListAccessPointsResponse>,
    > {
        self.client.unary_call_async(&METHOD_ACCESS_POINT_SERVICE_LIST, req, opt)
    }

    pub fn list_async(
        &self,
        req: &super::access_point::ListAccessPointsRequest,
    ) -> ::grpcio::Result<
        ::grpcio::ClientUnaryReceiver<super::access_point::ListAccessPointsResponse>,
    > {
        self.list_async_opt(req, ::grpcio::CallOption::default())
    }

    pub fn execute_opt(
        &self,
        req: &super::access_point::ExecuteAccessPointRequest,
        opt: ::grpcio::CallOption,
    ) -> ::grpcio::Result<super::empty::Empty> {
        self.client.unary_call(&METHOD_ACCESS_POINT_SERVICE_EXECUTE, req, opt)
    }

    pub fn execute(
        &self,
        req: &super::access_point::ExecuteAccessPointRequest,
    ) -> ::grpcio::Result<super::empty::Empty> {
        self.execute_opt(req, ::grpcio::CallOption::default())
    }

    pub fn execute_async_opt(
        &self,
        req: &super::access_point::ExecuteAccessPointRequest,
        opt: ::grpcio::CallOption,
    ) -> ::grpcio::Result<::grpcio::ClientUnaryReceiver<super::empty::Empty>> {
        self.client.unary_call_async(&METHOD_ACCESS_POINT_SERVICE_EXECUTE, req, opt)
    }

    pub fn execute_async(
        &self,
        req: &super::access_point::ExecuteAccessPointRequest,
    ) -> ::grpcio::Result<::grpcio::ClientUnaryReceiver<super::empty::Empty>> {
        self.execute_async_opt(req, ::grpcio::CallOption::default())
    }
    pub fn spawn<F>(&self, f: F)
    where
        F: ::std::future::Future<Output = ()> + Send + 'static,
    {
        self.client.spawn(f)
    }
}

pub trait AccessPointService {
    fn create(
        &mut self,
        ctx: ::grpcio::RpcContext,
        _req: super::access_point::CreateAccessPointRequest,
        sink: ::grpcio::UnarySink<super::access_point::AccessPoint>,
    ) {
        grpcio::unimplemented_call!(ctx, sink)
    }
    fn get(
        &mut self,
        ctx: ::grpcio::RpcContext,
        _req: super::access_point::GetAccessPointRequest,
        sink: ::grpcio::UnarySink<super::access_point::AccessPoint>,
    ) {
        grpcio::unimplemented_call!(ctx, sink)
    }
    fn update(
        &mut self,
        ctx: ::grpcio::RpcContext,
        _req: super::access_point::UpdateAccessPointRequest,
        sink: ::grpcio::UnarySink<super::access_point::AccessPoint>,
    ) {
        grpcio::unimplemented_call!(ctx, sink)
    }
    fn delete(
        &mut self,
        ctx: ::grpcio::RpcContext,
        _req: super::access_point::DeleteAccessPointRequest,
        sink: ::grpcio::UnarySink<super::empty::Empty>,
    ) {
        grpcio::unimplemented_call!(ctx, sink)
    }
    fn list(
        &mut self,
        ctx: ::grpcio::RpcContext,
        _req: super::access_point::ListAccessPointsRequest,
        sink: ::grpcio::UnarySink<super::access_point::ListAccessPointsResponse>,
    ) {
        grpcio::unimplemented_call!(ctx, sink)
    }
    fn execute(
        &mut self,
        ctx: ::grpcio::RpcContext,
        _req: super::access_point::ExecuteAccessPointRequest,
        sink: ::grpcio::UnarySink<super::empty::Empty>,
    ) {
        grpcio::unimplemented_call!(ctx, sink)
    }
}

pub fn create_access_point_service<S: AccessPointService + Send + Clone + 'static>(
    s: S,
) -> ::grpcio::Service {
    let mut builder = ::grpcio::ServiceBuilder::new();
    let mut instance = s.clone();
    builder = builder
        .add_unary_handler(&METHOD_ACCESS_POINT_SERVICE_CREATE, move |ctx, req, resp| {
            instance.create(ctx, req, resp)
        });
    let mut instance = s.clone();
    builder = builder.add_unary_handler(&METHOD_ACCESS_POINT_SERVICE_GET, move |ctx, req, resp| {
        instance.get(ctx, req, resp)
    });
    let mut instance = s.clone();
    builder = builder
        .add_unary_handler(&METHOD_ACCESS_POINT_SERVICE_UPDATE, move |ctx, req, resp| {
            instance.update(ctx, req, resp)
        });
    let mut instance = s.clone();
    builder = builder
        .add_unary_handler(&METHOD_ACCESS_POINT_SERVICE_DELETE, move |ctx, req, resp| {
            instance.delete(ctx, req, resp)
        });
    let mut instance = s.clone();
    builder = builder
        .add_unary_handler(&METHOD_ACCESS_POINT_SERVICE_LIST, move |ctx, req, resp| {
            instance.list(ctx, req, resp)
        });
    let mut instance = s;
    builder = builder
        .add_unary_handler(&METHOD_ACCESS_POINT_SERVICE_EXECUTE, move |ctx, req, resp| {
            instance.execute(ctx, req, resp)
        });
    builder.build()
}
