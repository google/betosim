// Copyright 2023-2025 The Android Open Source Project
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

//! Uses the tracing crate that allows control of logging through
//! the RUST_LOG environment variable.

/// Initiating the environment for logging with given prefix
///
/// The current log format follows the same format as Android Emulator team.
pub(crate) fn init(prefix: &'static str, is_verbose: bool) {
    common::util::netsim_logger::init(prefix, is_verbose);
}
