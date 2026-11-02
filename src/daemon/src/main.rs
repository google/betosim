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

use std::process;

#[cfg(all(target_os = "linux", feature = "cuttlefish"))]
use command_fds::inherited::init_inherited_fds;
use daemon::{RunResult, run};

#[allow(clippy::print_stderr)]
fn main() {
    // SAFETY: This is the first line of the main function before anything opens
    // files or otherwise, takes ownership of any file descriptors.
    #[cfg(all(target_os = "linux", feature = "cuttlefish"))]
    if let Err(e) = unsafe { init_inherited_fds() } {
        eprintln!("Failed to initialize inherited FDs: {}", e);
        process::exit(1);
    }

    match run() {
        RunResult::ExitedNormally => process::exit(0),
        RunResult::InitializationError(e) => {
            eprintln!("Netsim daemon failed to start: {}", e);
            process::exit(1);
        }
    }
}
