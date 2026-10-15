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

const METHOD_CELL_SERVICE_GET: ::grpcio::Method<super::cell::GetCellRequest, super::cell::Cell> =
    ::grpcio::Method {
        ty: ::grpcio::MethodType::Unary,
        name: "/netsim.cell.CellService/Get",
        req_mar: ::grpcio::Marshaller { ser: ::grpcio::pb_ser, de: ::grpcio::pb_de },
        resp_mar: ::grpcio::Marshaller { ser: ::grpcio::pb_ser, de: ::grpcio::pb_de },
    };

const METHOD_CELL_SERVICE_LIST: ::grpcio::Method<
    super::cell::ListCellsRequest,
    super::cell::ListCellsResponse,
> = ::grpcio::Method {
    ty: ::grpcio::MethodType::Unary,
    name: "/netsim.cell.CellService/List",
    req_mar: ::grpcio::Marshaller { ser: ::grpcio::pb_ser, de: ::grpcio::pb_de },
    resp_mar: ::grpcio::Marshaller { ser: ::grpcio::pb_ser, de: ::grpcio::pb_de },
};

const METHOD_CELL_SERVICE_EXECUTE: ::grpcio::Method<
    super::cell::ExecuteCellRequest,
    super::empty::Empty,
> = ::grpcio::Method {
    ty: ::grpcio::MethodType::Unary,
    name: "/netsim.cell.CellService/Execute",
    req_mar: ::grpcio::Marshaller { ser: ::grpcio::pb_ser, de: ::grpcio::pb_de },
    resp_mar: ::grpcio::Marshaller { ser: ::grpcio::pb_ser, de: ::grpcio::pb_de },
};

#[derive(Clone)]
pub struct CellServiceClient {
    pub client: ::grpcio::Client,
}

impl CellServiceClient {
    pub fn new(channel: ::grpcio::Channel) -> Self {
        CellServiceClient { client: ::grpcio::Client::new(channel) }
    }

    pub fn get_opt(
        &self,
        req: &super::cell::GetCellRequest,
        opt: ::grpcio::CallOption,
    ) -> ::grpcio::Result<super::cell::Cell> {
        self.client.unary_call(&METHOD_CELL_SERVICE_GET, req, opt)
    }

    pub fn get(&self, req: &super::cell::GetCellRequest) -> ::grpcio::Result<super::cell::Cell> {
        self.get_opt(req, ::grpcio::CallOption::default())
    }

    pub fn get_async_opt(
        &self,
        req: &super::cell::GetCellRequest,
        opt: ::grpcio::CallOption,
    ) -> ::grpcio::Result<::grpcio::ClientUnaryReceiver<super::cell::Cell>> {
        self.client.unary_call_async(&METHOD_CELL_SERVICE_GET, req, opt)
    }

    pub fn get_async(
        &self,
        req: &super::cell::GetCellRequest,
    ) -> ::grpcio::Result<::grpcio::ClientUnaryReceiver<super::cell::Cell>> {
        self.get_async_opt(req, ::grpcio::CallOption::default())
    }

    pub fn list_opt(
        &self,
        req: &super::cell::ListCellsRequest,
        opt: ::grpcio::CallOption,
    ) -> ::grpcio::Result<super::cell::ListCellsResponse> {
        self.client.unary_call(&METHOD_CELL_SERVICE_LIST, req, opt)
    }

    pub fn list(
        &self,
        req: &super::cell::ListCellsRequest,
    ) -> ::grpcio::Result<super::cell::ListCellsResponse> {
        self.list_opt(req, ::grpcio::CallOption::default())
    }

    pub fn list_async_opt(
        &self,
        req: &super::cell::ListCellsRequest,
        opt: ::grpcio::CallOption,
    ) -> ::grpcio::Result<::grpcio::ClientUnaryReceiver<super::cell::ListCellsResponse>> {
        self.client.unary_call_async(&METHOD_CELL_SERVICE_LIST, req, opt)
    }

    pub fn list_async(
        &self,
        req: &super::cell::ListCellsRequest,
    ) -> ::grpcio::Result<::grpcio::ClientUnaryReceiver<super::cell::ListCellsResponse>> {
        self.list_async_opt(req, ::grpcio::CallOption::default())
    }

    pub fn execute_opt(
        &self,
        req: &super::cell::ExecuteCellRequest,
        opt: ::grpcio::CallOption,
    ) -> ::grpcio::Result<super::empty::Empty> {
        self.client.unary_call(&METHOD_CELL_SERVICE_EXECUTE, req, opt)
    }

    pub fn execute(
        &self,
        req: &super::cell::ExecuteCellRequest,
    ) -> ::grpcio::Result<super::empty::Empty> {
        self.execute_opt(req, ::grpcio::CallOption::default())
    }

    pub fn execute_async_opt(
        &self,
        req: &super::cell::ExecuteCellRequest,
        opt: ::grpcio::CallOption,
    ) -> ::grpcio::Result<::grpcio::ClientUnaryReceiver<super::empty::Empty>> {
        self.client.unary_call_async(&METHOD_CELL_SERVICE_EXECUTE, req, opt)
    }

    pub fn execute_async(
        &self,
        req: &super::cell::ExecuteCellRequest,
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

pub trait CellService {
    fn get(
        &mut self,
        ctx: ::grpcio::RpcContext,
        _req: super::cell::GetCellRequest,
        sink: ::grpcio::UnarySink<super::cell::Cell>,
    ) {
        grpcio::unimplemented_call!(ctx, sink)
    }
    fn list(
        &mut self,
        ctx: ::grpcio::RpcContext,
        _req: super::cell::ListCellsRequest,
        sink: ::grpcio::UnarySink<super::cell::ListCellsResponse>,
    ) {
        grpcio::unimplemented_call!(ctx, sink)
    }
    fn execute(
        &mut self,
        ctx: ::grpcio::RpcContext,
        _req: super::cell::ExecuteCellRequest,
        sink: ::grpcio::UnarySink<super::empty::Empty>,
    ) {
        grpcio::unimplemented_call!(ctx, sink)
    }
}

pub fn create_cell_service<S: CellService + Send + Clone + 'static>(s: S) -> ::grpcio::Service {
    let mut builder = ::grpcio::ServiceBuilder::new();
    let mut instance = s.clone();
    builder = builder.add_unary_handler(&METHOD_CELL_SERVICE_GET, move |ctx, req, resp| {
        instance.get(ctx, req, resp)
    });
    let mut instance = s.clone();
    builder = builder.add_unary_handler(&METHOD_CELL_SERVICE_LIST, move |ctx, req, resp| {
        instance.list(ctx, req, resp)
    });
    let mut instance = s;
    builder = builder.add_unary_handler(&METHOD_CELL_SERVICE_EXECUTE, move |ctx, req, resp| {
        instance.execute(ctx, req, resp)
    });
    builder.build()
}
