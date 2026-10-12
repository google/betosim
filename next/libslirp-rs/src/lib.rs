// Copyright 2024 The Android Open Source Project
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

//! This crate is a wrapper for libslirp C library.
//!
//! All calls into libslirp are routed to and handled by a dedicated
//! thread.
//!
//! Rust struct LibslirpConfig for conversion between Rust and C types
//! (IpV4Addr, SocketAddrV4, etc.).
//!
//! Callbacks for libslirp send_packet are delivered on Channel.

#![allow(clippy::cast_possible_wrap)]

mod libslirp;
mod libslirp_config;
// Keep libslirp_sys public to bypass dead code warnings for unused
// auto-generated FFI bindings.
pub mod libslirp_sys;

pub use libslirp::{LibSlirp, LibSlirpInputHandle, PacketSender};
pub use libslirp_config::{SlirpConfig, lookup_host_dns};
pub use slirp::{ProxyConnect, ProxyManager};
