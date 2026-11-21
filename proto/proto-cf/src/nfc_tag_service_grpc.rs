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
#![allow(unused_imports)]
#![allow(unused_results)]

const METHOD_NFC_TAG_SERVICE_CREATE_TAG: ::grpcio::Method<
    super::nfc_tag_service::CreateTagRequest,
    super::nfc_tag_service::NfcTag,
> = ::grpcio::Method {
    ty: ::grpcio::MethodType::Unary,
    name: "/netsim.nfc_tag_service.NfcTagService/CreateTag",
    req_mar: ::grpcio::Marshaller { ser: ::grpcio::pb_ser, de: ::grpcio::pb_de },
    resp_mar: ::grpcio::Marshaller { ser: ::grpcio::pb_ser, de: ::grpcio::pb_de },
};

const METHOD_NFC_TAG_SERVICE_GET_TAG: ::grpcio::Method<
    super::nfc_tag_service::GetTagRequest,
    super::nfc_tag_service::NfcTag,
> = ::grpcio::Method {
    ty: ::grpcio::MethodType::Unary,
    name: "/netsim.nfc_tag_service.NfcTagService/GetTag",
    req_mar: ::grpcio::Marshaller { ser: ::grpcio::pb_ser, de: ::grpcio::pb_de },
    resp_mar: ::grpcio::Marshaller { ser: ::grpcio::pb_ser, de: ::grpcio::pb_de },
};

const METHOD_NFC_TAG_SERVICE_LIST_TAGS: ::grpcio::Method<
    super::empty::Empty,
    super::nfc_tag_service::ListTagsResponse,
> = ::grpcio::Method {
    ty: ::grpcio::MethodType::Unary,
    name: "/netsim.nfc_tag_service.NfcTagService/ListTags",
    req_mar: ::grpcio::Marshaller { ser: ::grpcio::pb_ser, de: ::grpcio::pb_de },
    resp_mar: ::grpcio::Marshaller { ser: ::grpcio::pb_ser, de: ::grpcio::pb_de },
};

const METHOD_NFC_TAG_SERVICE_UPDATE_TAG: ::grpcio::Method<
    super::nfc_tag_service::UpdateTagRequest,
    super::nfc_tag_service::NfcTag,
> = ::grpcio::Method {
    ty: ::grpcio::MethodType::Unary,
    name: "/netsim.nfc_tag_service.NfcTagService/UpdateTag",
    req_mar: ::grpcio::Marshaller { ser: ::grpcio::pb_ser, de: ::grpcio::pb_de },
    resp_mar: ::grpcio::Marshaller { ser: ::grpcio::pb_ser, de: ::grpcio::pb_de },
};

const METHOD_NFC_TAG_SERVICE_DELETE_TAG: ::grpcio::Method<
    super::nfc_tag_service::DeleteTagRequest,
    super::empty::Empty,
> = ::grpcio::Method {
    ty: ::grpcio::MethodType::Unary,
    name: "/netsim.nfc_tag_service.NfcTagService/DeleteTag",
    req_mar: ::grpcio::Marshaller { ser: ::grpcio::pb_ser, de: ::grpcio::pb_de },
    resp_mar: ::grpcio::Marshaller { ser: ::grpcio::pb_ser, de: ::grpcio::pb_de },
};

const METHOD_NFC_TAG_SERVICE_RESET_TAGS: ::grpcio::Method<
    super::empty::Empty,
    super::empty::Empty,
> = ::grpcio::Method {
    ty: ::grpcio::MethodType::Unary,
    name: "/netsim.nfc_tag_service.NfcTagService/ResetTags",
    req_mar: ::grpcio::Marshaller { ser: ::grpcio::pb_ser, de: ::grpcio::pb_de },
    resp_mar: ::grpcio::Marshaller { ser: ::grpcio::pb_ser, de: ::grpcio::pb_de },
};

#[derive(Clone)]
pub struct NfcTagServiceClient {
    pub client: ::grpcio::Client,
}

impl NfcTagServiceClient {
    pub fn new(channel: ::grpcio::Channel) -> Self {
        NfcTagServiceClient { client: ::grpcio::Client::new(channel) }
    }

    pub fn create_tag_opt(
        &self,
        req: &super::nfc_tag_service::CreateTagRequest,
        opt: ::grpcio::CallOption,
    ) -> ::grpcio::Result<super::nfc_tag_service::NfcTag> {
        self.client.unary_call(&METHOD_NFC_TAG_SERVICE_CREATE_TAG, req, opt)
    }

    pub fn create_tag(
        &self,
        req: &super::nfc_tag_service::CreateTagRequest,
    ) -> ::grpcio::Result<super::nfc_tag_service::NfcTag> {
        self.create_tag_opt(req, ::grpcio::CallOption::default())
    }

    pub fn create_tag_async_opt(
        &self,
        req: &super::nfc_tag_service::CreateTagRequest,
        opt: ::grpcio::CallOption,
    ) -> ::grpcio::Result<::grpcio::ClientUnaryReceiver<super::nfc_tag_service::NfcTag>> {
        self.client.unary_call_async(&METHOD_NFC_TAG_SERVICE_CREATE_TAG, req, opt)
    }

    pub fn create_tag_async(
        &self,
        req: &super::nfc_tag_service::CreateTagRequest,
    ) -> ::grpcio::Result<::grpcio::ClientUnaryReceiver<super::nfc_tag_service::NfcTag>> {
        self.create_tag_async_opt(req, ::grpcio::CallOption::default())
    }

    pub fn get_tag_opt(
        &self,
        req: &super::nfc_tag_service::GetTagRequest,
        opt: ::grpcio::CallOption,
    ) -> ::grpcio::Result<super::nfc_tag_service::NfcTag> {
        self.client.unary_call(&METHOD_NFC_TAG_SERVICE_GET_TAG, req, opt)
    }

    pub fn get_tag(
        &self,
        req: &super::nfc_tag_service::GetTagRequest,
    ) -> ::grpcio::Result<super::nfc_tag_service::NfcTag> {
        self.get_tag_opt(req, ::grpcio::CallOption::default())
    }

    pub fn get_tag_async_opt(
        &self,
        req: &super::nfc_tag_service::GetTagRequest,
        opt: ::grpcio::CallOption,
    ) -> ::grpcio::Result<::grpcio::ClientUnaryReceiver<super::nfc_tag_service::NfcTag>> {
        self.client.unary_call_async(&METHOD_NFC_TAG_SERVICE_GET_TAG, req, opt)
    }

    pub fn get_tag_async(
        &self,
        req: &super::nfc_tag_service::GetTagRequest,
    ) -> ::grpcio::Result<::grpcio::ClientUnaryReceiver<super::nfc_tag_service::NfcTag>> {
        self.get_tag_async_opt(req, ::grpcio::CallOption::default())
    }

    pub fn list_tags_opt(
        &self,
        req: &super::empty::Empty,
        opt: ::grpcio::CallOption,
    ) -> ::grpcio::Result<super::nfc_tag_service::ListTagsResponse> {
        self.client.unary_call(&METHOD_NFC_TAG_SERVICE_LIST_TAGS, req, opt)
    }

    pub fn list_tags(
        &self,
        req: &super::empty::Empty,
    ) -> ::grpcio::Result<super::nfc_tag_service::ListTagsResponse> {
        self.list_tags_opt(req, ::grpcio::CallOption::default())
    }

    pub fn list_tags_async_opt(
        &self,
        req: &super::empty::Empty,
        opt: ::grpcio::CallOption,
    ) -> ::grpcio::Result<::grpcio::ClientUnaryReceiver<super::nfc_tag_service::ListTagsResponse>>
    {
        self.client.unary_call_async(&METHOD_NFC_TAG_SERVICE_LIST_TAGS, req, opt)
    }

    pub fn list_tags_async(
        &self,
        req: &super::empty::Empty,
    ) -> ::grpcio::Result<::grpcio::ClientUnaryReceiver<super::nfc_tag_service::ListTagsResponse>>
    {
        self.list_tags_async_opt(req, ::grpcio::CallOption::default())
    }

    pub fn update_tag_opt(
        &self,
        req: &super::nfc_tag_service::UpdateTagRequest,
        opt: ::grpcio::CallOption,
    ) -> ::grpcio::Result<super::nfc_tag_service::NfcTag> {
        self.client.unary_call(&METHOD_NFC_TAG_SERVICE_UPDATE_TAG, req, opt)
    }

    pub fn update_tag(
        &self,
        req: &super::nfc_tag_service::UpdateTagRequest,
    ) -> ::grpcio::Result<super::nfc_tag_service::NfcTag> {
        self.update_tag_opt(req, ::grpcio::CallOption::default())
    }

    pub fn update_tag_async_opt(
        &self,
        req: &super::nfc_tag_service::UpdateTagRequest,
        opt: ::grpcio::CallOption,
    ) -> ::grpcio::Result<::grpcio::ClientUnaryReceiver<super::nfc_tag_service::NfcTag>> {
        self.client.unary_call_async(&METHOD_NFC_TAG_SERVICE_UPDATE_TAG, req, opt)
    }

    pub fn update_tag_async(
        &self,
        req: &super::nfc_tag_service::UpdateTagRequest,
    ) -> ::grpcio::Result<::grpcio::ClientUnaryReceiver<super::nfc_tag_service::NfcTag>> {
        self.update_tag_async_opt(req, ::grpcio::CallOption::default())
    }

    pub fn delete_tag_opt(
        &self,
        req: &super::nfc_tag_service::DeleteTagRequest,
        opt: ::grpcio::CallOption,
    ) -> ::grpcio::Result<super::empty::Empty> {
        self.client.unary_call(&METHOD_NFC_TAG_SERVICE_DELETE_TAG, req, opt)
    }

    pub fn delete_tag(
        &self,
        req: &super::nfc_tag_service::DeleteTagRequest,
    ) -> ::grpcio::Result<super::empty::Empty> {
        self.delete_tag_opt(req, ::grpcio::CallOption::default())
    }

    pub fn delete_tag_async_opt(
        &self,
        req: &super::nfc_tag_service::DeleteTagRequest,
        opt: ::grpcio::CallOption,
    ) -> ::grpcio::Result<::grpcio::ClientUnaryReceiver<super::empty::Empty>> {
        self.client.unary_call_async(&METHOD_NFC_TAG_SERVICE_DELETE_TAG, req, opt)
    }

    pub fn delete_tag_async(
        &self,
        req: &super::nfc_tag_service::DeleteTagRequest,
    ) -> ::grpcio::Result<::grpcio::ClientUnaryReceiver<super::empty::Empty>> {
        self.delete_tag_async_opt(req, ::grpcio::CallOption::default())
    }

    pub fn reset_tags_opt(
        &self,
        req: &super::empty::Empty,
        opt: ::grpcio::CallOption,
    ) -> ::grpcio::Result<super::empty::Empty> {
        self.client.unary_call(&METHOD_NFC_TAG_SERVICE_RESET_TAGS, req, opt)
    }

    pub fn reset_tags(&self, req: &super::empty::Empty) -> ::grpcio::Result<super::empty::Empty> {
        self.reset_tags_opt(req, ::grpcio::CallOption::default())
    }

    pub fn reset_tags_async_opt(
        &self,
        req: &super::empty::Empty,
        opt: ::grpcio::CallOption,
    ) -> ::grpcio::Result<::grpcio::ClientUnaryReceiver<super::empty::Empty>> {
        self.client.unary_call_async(&METHOD_NFC_TAG_SERVICE_RESET_TAGS, req, opt)
    }

    pub fn reset_tags_async(
        &self,
        req: &super::empty::Empty,
    ) -> ::grpcio::Result<::grpcio::ClientUnaryReceiver<super::empty::Empty>> {
        self.reset_tags_async_opt(req, ::grpcio::CallOption::default())
    }
    pub fn spawn<F>(&self, f: F)
    where
        F: ::std::future::Future<Output = ()> + Send + 'static,
    {
        self.client.spawn(f)
    }
}

pub trait NfcTagService {
    fn create_tag(
        &mut self,
        ctx: ::grpcio::RpcContext,
        _req: super::nfc_tag_service::CreateTagRequest,
        sink: ::grpcio::UnarySink<super::nfc_tag_service::NfcTag>,
    ) {
        grpcio::unimplemented_call!(ctx, sink)
    }
    fn get_tag(
        &mut self,
        ctx: ::grpcio::RpcContext,
        _req: super::nfc_tag_service::GetTagRequest,
        sink: ::grpcio::UnarySink<super::nfc_tag_service::NfcTag>,
    ) {
        grpcio::unimplemented_call!(ctx, sink)
    }
    fn list_tags(
        &mut self,
        ctx: ::grpcio::RpcContext,
        _req: super::empty::Empty,
        sink: ::grpcio::UnarySink<super::nfc_tag_service::ListTagsResponse>,
    ) {
        grpcio::unimplemented_call!(ctx, sink)
    }
    fn update_tag(
        &mut self,
        ctx: ::grpcio::RpcContext,
        _req: super::nfc_tag_service::UpdateTagRequest,
        sink: ::grpcio::UnarySink<super::nfc_tag_service::NfcTag>,
    ) {
        grpcio::unimplemented_call!(ctx, sink)
    }
    fn delete_tag(
        &mut self,
        ctx: ::grpcio::RpcContext,
        _req: super::nfc_tag_service::DeleteTagRequest,
        sink: ::grpcio::UnarySink<super::empty::Empty>,
    ) {
        grpcio::unimplemented_call!(ctx, sink)
    }
    fn reset_tags(
        &mut self,
        ctx: ::grpcio::RpcContext,
        _req: super::empty::Empty,
        sink: ::grpcio::UnarySink<super::empty::Empty>,
    ) {
        grpcio::unimplemented_call!(ctx, sink)
    }
}

pub fn create_nfc_tag_service<S: NfcTagService + Send + Clone + 'static>(
    s: S,
) -> ::grpcio::Service {
    let mut builder = ::grpcio::ServiceBuilder::new();
    let mut instance = s.clone();
    builder = builder
        .add_unary_handler(&METHOD_NFC_TAG_SERVICE_CREATE_TAG, move |ctx, req, resp| {
            instance.create_tag(ctx, req, resp)
        });
    let mut instance = s.clone();
    builder = builder.add_unary_handler(&METHOD_NFC_TAG_SERVICE_GET_TAG, move |ctx, req, resp| {
        instance.get_tag(ctx, req, resp)
    });
    let mut instance = s.clone();
    builder = builder
        .add_unary_handler(&METHOD_NFC_TAG_SERVICE_LIST_TAGS, move |ctx, req, resp| {
            instance.list_tags(ctx, req, resp)
        });
    let mut instance = s.clone();
    builder = builder
        .add_unary_handler(&METHOD_NFC_TAG_SERVICE_UPDATE_TAG, move |ctx, req, resp| {
            instance.update_tag(ctx, req, resp)
        });
    let mut instance = s.clone();
    builder = builder
        .add_unary_handler(&METHOD_NFC_TAG_SERVICE_DELETE_TAG, move |ctx, req, resp| {
            instance.delete_tag(ctx, req, resp)
        });
    let mut instance = s;
    builder = builder
        .add_unary_handler(&METHOD_NFC_TAG_SERVICE_RESET_TAGS, move |ctx, req, resp| {
            instance.reset_tags(ctx, req, resp)
        });
    builder.build()
}
