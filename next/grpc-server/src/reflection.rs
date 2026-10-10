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

use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use futures::{SinkExt, StreamExt};
use grpcio::{DuplexSink, RequestStream, RpcContext, WriteFlags};
use netsim_proto::{
    reflection::{
        ErrorResponse, FileDescriptorResponse, ListServiceResponse, ServerReflectionRequest,
        ServerReflectionResponse, ServiceResponse, server_reflection_request,
        server_reflection_response,
    },
    reflection_grpc::ServerReflection,
};
use tracing::warn;

// Standard gRPC status codes (https://grpc.github.io/grpc/core/md_doc_statuscodes.html)
const GRPC_STATUS_NOT_FOUND: i32 = 5;
const GRPC_STATUS_UNIMPLEMENTED: i32 = 12;

#[derive(Clone)]
pub(crate) struct ServerReflectionImpl {
    inner: Arc<ReflectionData>,
}

struct ReflectionData {
    services: Vec<String>,
    symbol_to_file: HashMap<String, String>,
    file_aliases: HashMap<String, String>,
    file_bytes: HashMap<String, Vec<u8>>,
    file_deps: HashMap<String, Vec<String>>,
}

impl ReflectionData {
    fn get_descriptors_for_file(&self, file_name: &str) -> Option<Vec<Vec<u8>>> {
        let canonical_name = self.file_aliases.get(file_name).map_or(file_name, |s| s.as_str());
        if !self.file_bytes.contains_key(canonical_name) {
            return None;
        }

        let mut visited = HashSet::new();
        let mut ordered_files = Vec::new();

        fn collect_deps(
            file: &str,
            reflection: &ReflectionData,
            visited: &mut HashSet<String>,
            ordered: &mut Vec<String>,
        ) {
            let canonical = reflection.file_aliases.get(file).map_or(file, |s| s.as_str());
            if !visited.insert(canonical.to_string()) {
                return;
            }
            if let Some(deps) = reflection.file_deps.get(canonical) {
                for dep in deps {
                    collect_deps(dep, reflection, visited, ordered);
                }
            }
            ordered.push(canonical.to_string());
        }

        collect_deps(canonical_name, self, &mut visited, &mut ordered_files);

        let mut result = Vec::new();
        for file in ordered_files {
            if let Some(bytes) = self.file_bytes.get(&file) {
                result.push(bytes.clone());
            }
        }
        Some(result)
    }

    fn get_descriptors_for_symbol(&self, symbol: &str) -> Option<Vec<Vec<u8>>> {
        let clean_symbol = symbol.strip_prefix('.').unwrap_or(symbol);
        let file_name = self.symbol_to_file.get(clean_symbol)?;
        self.get_descriptors_for_file(file_name)
    }
}

fn join_name(prefix: &str, name: &str) -> String {
    if prefix.is_empty() { name.to_string() } else { format!("{}.{}", prefix, name) }
}

fn index_message(
    msg: &protobuf::descriptor::DescriptorProto,
    prefix: &str,
    file_name: &str,
    symbol_to_file: &mut HashMap<String, String>,
) {
    let msg_name = join_name(prefix, msg.name());
    symbol_to_file.insert(msg_name.clone(), file_name.to_string());

    for f in &msg.field {
        let f_name = format!("{}.{}", msg_name, f.name());
        symbol_to_file.insert(f_name, file_name.to_string());
    }

    for nested in &msg.nested_type {
        index_message(nested, &msg_name, file_name, symbol_to_file);
    }

    for e in &msg.enum_type {
        let e_name = format!("{}.{}", msg_name, e.name());
        symbol_to_file.insert(e_name.clone(), file_name.to_string());
        for val in &e.value {
            let val_name = format!("{}.{}", e_name, val.name());
            symbol_to_file.insert(val_name, file_name.to_string());
            let msg_val_name = format!("{}.{}", msg_name, val.name());
            symbol_to_file.insert(msg_val_name, file_name.to_string());
        }
    }
}

impl ServerReflectionImpl {
    pub(crate) fn from_descriptors(
        descriptors: &[&'static protobuf::reflect::FileDescriptor],
    ) -> Self {
        let mut services = Vec::new();
        let mut symbol_to_file = HashMap::new();
        let mut file_aliases = HashMap::new();
        let mut file_bytes = HashMap::new();
        let mut file_deps = HashMap::new();

        let mut queue: Vec<_> = descriptors.to_vec();
        let mut visited: HashSet<&'static str> = HashSet::new();

        while let Some(desc) = queue.pop() {
            let proto = desc.proto();
            let file_name = proto.name();
            if !visited.insert(file_name) {
                continue;
            }
            queue.extend(desc.deps());

            let raw_bytes =
                protobuf::Message::write_to_bytes(proto).expect("Failed to serialize descriptor");
            file_bytes.insert(file_name.to_string(), raw_bytes);
            file_aliases.insert(file_name.to_string(), file_name.to_string());
            if let Some(base_name) = file_name.strip_prefix("netsim/") {
                file_aliases.insert(base_name.to_string(), file_name.to_string());
            }

            file_deps.insert(file_name.to_string(), proto.dependency.to_vec());

            let pkg = proto.package();
            for s in &proto.service {
                let s_name = join_name(pkg, s.name());
                services.push(s_name.clone());
                symbol_to_file.insert(s_name.clone(), file_name.to_string());

                for m in &s.method {
                    let m_name = format!("{}.{}", s_name, m.name());
                    symbol_to_file.insert(m_name, file_name.to_string());
                }
            }

            for e in &proto.enum_type {
                let e_name = join_name(pkg, e.name());
                symbol_to_file.insert(e_name.clone(), file_name.to_string());
                for val in &e.value {
                    let val_name = format!("{}.{}", e_name, val.name());
                    symbol_to_file.insert(val_name, file_name.to_string());
                    if !pkg.is_empty() {
                        let pkg_val_name = format!("{}.{}", pkg, val.name());
                        symbol_to_file.insert(pkg_val_name, file_name.to_string());
                    }
                }
            }

            for msg in &proto.message_type {
                index_message(msg, pkg, file_name, &mut symbol_to_file);
            }
        }

        Self {
            inner: Arc::new(ReflectionData {
                services,
                symbol_to_file,
                file_aliases,
                file_bytes,
                file_deps,
            }),
        }
    }
}

impl ServerReflection for ServerReflectionImpl {
    fn server_reflection_info(
        &mut self,
        ctx: RpcContext,
        mut stream: RequestStream<ServerReflectionRequest>,
        sink: DuplexSink<ServerReflectionResponse>,
    ) {
        let inner = self.inner.clone();
        ctx.spawn(async move {
            let mut sink = sink.sink_map_err(|e| warn!("Reflection sink error: {e}"));
            while let Some(req_res) = stream.next().await {
                match req_res {
                    Ok(req) => {
                        let mut resp = ServerReflectionResponse::new();
                        resp.valid_host = req.host.clone();
                        resp.original_request = protobuf::MessageField::some(req.clone());

                        match req.message_request {
                            Some(server_reflection_request::Message_request::ListServices(_)) => {
                                let mut list_resp = ListServiceResponse::new();
                                list_resp.service = inner
                                    .services
                                    .iter()
                                    .map(|name| {
                                        let mut s = ServiceResponse::new();
                                        s.name = name.clone();
                                        s
                                    })
                                    .collect();
                                resp.message_response = Some(
                                    server_reflection_response::Message_response::ListServicesResponse(
                                        list_resp,
                                    ),
                                );
                            }
                            Some(
                                server_reflection_request::Message_request::FileContainingSymbol(
                                    ref symbol,
                                ),
                            ) => {
                                if let Some(descs) = inner.get_descriptors_for_symbol(symbol) {
                                    let mut fd_resp = FileDescriptorResponse::new();
                                    fd_resp.file_descriptor_proto = descs;
                                    resp.message_response = Some(
                                        server_reflection_response::Message_response::FileDescriptorResponse(
                                            fd_resp,
                                        ),
                                    );
                                } else {
                                    let mut err_resp = ErrorResponse::new();
                                    err_resp.error_code = GRPC_STATUS_NOT_FOUND;
                                    err_resp.error_message = format!("Symbol not found: {symbol}");
                                    resp.message_response = Some(
                                        server_reflection_response::Message_response::ErrorResponse(
                                            err_resp,
                                        ),
                                    );
                                }
                            }
                            Some(
                                server_reflection_request::Message_request::FileByFilename(
                                    ref filename,
                                ),
                            ) => {
                                if let Some(descs) = inner.get_descriptors_for_file(filename) {
                                    let mut fd_resp = FileDescriptorResponse::new();
                                    fd_resp.file_descriptor_proto = descs;
                                    resp.message_response = Some(
                                        server_reflection_response::Message_response::FileDescriptorResponse(
                                            fd_resp,
                                        ),
                                    );
                                } else {
                                    let mut err_resp = ErrorResponse::new();
                                    err_resp.error_code = GRPC_STATUS_NOT_FOUND;
                                    err_resp.error_message =
                                        format!("File not found: {filename}");
                                    resp.message_response = Some(
                                        server_reflection_response::Message_response::ErrorResponse(
                                            err_resp,
                                        ),
                                    );
                                }
                            }
                            _ => {
                                let mut err_resp = ErrorResponse::new();
                                err_resp.error_code = GRPC_STATUS_UNIMPLEMENTED;
                                err_resp.error_message =
                                    "Unsupported reflection request".to_string();
                                resp.message_response = Some(
                                    server_reflection_response::Message_response::ErrorResponse(
                                        err_resp,
                                    ),
                                );
                            }
                        }

                        if sink.send((resp, WriteFlags::default())).await.is_err() {
                            break;
                        }
                    }
                    Err(e) => {
                        warn!("Error in reflection stream: {e}");
                        break;
                    }
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_from_descriptors_indexing() {
        let reflection = ServerReflectionImpl::from_descriptors(&[
            netsim_proto::reflection::file_descriptor(),
            netsim_proto::casimir_control::file_descriptor(),
            netsim_proto::common::file_descriptor(),
        ]);

        let inner = &reflection.inner;
        // Verify service is listed
        assert!(inner.services.contains(&"grpc.reflection.v1alpha.ServerReflection".to_string()));
        assert!(inner.services.contains(&"casimircontrolserver.CasimirControlService".to_string()));

        // Verify symbol lookup
        assert!(inner.symbol_to_file.contains_key("casimircontrolserver.CasimirControlService"));
        assert!(inner.symbol_to_file.contains_key("grpc.reflection.v1alpha.ServerReflection"));

        // Verify field indexing
        assert!(
            inner
                .symbol_to_file
                .contains_key("casimircontrolserver.SendApduRequest.apdu_hex_strings")
        );

        // Verify package-scoped and qualified enum values
        assert!(inner.symbol_to_file.contains_key("netsim.common.ChipKind.BLUETOOTH"));
        assert!(inner.symbol_to_file.contains_key("netsim.common.BLUETOOTH"));

        // Verify transitive descriptor retrieval
        let descs = inner.get_descriptors_for_symbol(".casimircontrolserver.CasimirControlService");
        assert!(descs.is_some());
        assert!(!descs.unwrap().is_empty());

        let file_descs = inner.get_descriptors_for_file("netsim/reflection.proto");
        assert!(file_descs.is_some());
        assert!(!file_descs.unwrap().is_empty());
    }
}
