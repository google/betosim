// Copyright 2023 The Android Open Source Project
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

//! Netsim daemon libraries.

use std::sync::OnceLock;
use tokio::runtime::{Handle, Runtime};

static RUNTIME: OnceLock<Runtime> = OnceLock::new();

/// Retrieves a handle to a shared, lazily initialized Tokio runtime.
pub fn get_runtime() -> Handle {
    RUNTIME.get_or_init(|| Runtime::new().unwrap()).handle().clone()
}

mod args;
mod bluetooth;
pub mod captures;
mod config_file;
mod devices;
mod events;
mod ffi;
mod grpc_server;
mod http_server;
mod links;
mod proto_mapping;
mod ranging;
mod resource;
mod rust_main;
mod service;
mod session;
mod transport;
mod uwb;
mod version;
mod websocket_server;
mod wifi;
mod wireless;

// This feature is enabled only for CMake builds
#[cfg(feature = "local_ssl")]
mod openssl;
