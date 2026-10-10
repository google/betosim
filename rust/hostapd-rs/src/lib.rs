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

//! # hostapd-rs
//!
//! This crate provides a Rust interface to the `hostapd` C library, allowing you to manage WiFi access points
//! and perform various wireless networking tasks directly from your Rust code.
//!
//! It consists of two main modules:
//!
//! * **`hostapd`:** This module provides a high-level and safe interface to interact with the `hostapd` process.
//!   It uses separate threads for managing the `hostapd` process and handling its responses, ensuring efficient
//!   and non-blocking communication.
//! * **`hostapd_sys`:** This module contains the low-level C FFI bindings to the `hostapd` library. It is
//!   automatically generated using `rust-bindgen` and provides platform-specific bindings for Linux, macOS, and Windows.
//!

pub mod hostapd;
pub mod hostapd_sys;
