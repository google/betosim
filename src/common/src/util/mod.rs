//  Copyright 2023 The Android Open Source Project
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

//! # netsim utilities.

#[deny(unsafe_code)]
pub mod bluetooth_mac;
#[deny(unsafe_code)]
pub mod ini_file;
#[deny(unsafe_code)]
pub mod net;
#[deny(unsafe_code)]
pub mod netsim_logger;
#[cfg(not(windows))]
#[deny(unsafe_code)]
pub mod os_utils;
#[cfg(windows)]
pub mod os_utils;
#[deny(unsafe_code)]
pub mod proto_print_options;
#[deny(unsafe_code)]
pub mod scanner_util;
#[deny(unsafe_code)]
pub mod time_display;
#[deny(unsafe_code)]
pub mod websocket;
#[deny(unsafe_code)]
pub mod zip_artifact;
