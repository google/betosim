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
pub mod libslirp;
pub mod libslirp_config;
pub mod libslirp_sys;
