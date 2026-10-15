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

//! Netsim HTTP REST API module.
//!
//! Provides REST API request and response models and the HTTP endpoint
//! dispatcher for managing devices, links, and access points.

#![allow(clippy::indexing_slicing)]

use hyper::Method;
use serde::{Deserialize, Serialize};

use crate::openapi::Route;

pub mod ap_api;
pub mod client;
pub mod device_api;
pub mod link_api;
pub mod openapi;
pub mod server;

pub use ap_api::*;
pub use client::*;
pub use device_api::*;
pub use link_api::*;
pub use server::*;

/// Endpoint paths shared by the REST server and client, so the two cannot
/// drift apart. Usable as `match` patterns.
pub mod path {
    pub const OPENAPI: &str = "/v1/openapi.json";
    pub const VERSION: &str = "/v1/version";
    pub const DEVICES: &str = "/v1/devices";
    pub const LINKS: &str = "/v1/links";
    pub const APS: &str = "/v1/aps";
}

include!(concat!(env!("OUT_DIR"), "/openapi_generated.rs"));
