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

use std::sync::Once;

static INIT: Once = Once::new();

/// Initialize the logger for testing.
/// This function ensures that the logger is initialized only once.
/// It uses `tracing-subscriber` to print logs to stdout/stderr.
pub fn setup(_level: Option<&str>) {
    INIT.call_once(|| {
        // For now, we just call init_for_test which uses "info"
        // If we need custom level, we should update common::util::netsim_logger
        common::util::netsim_logger::init_for_test();
    });
}
