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

use std::time::Duration;

pub const LABEL_WIDTH: usize = 20;

#[derive(Debug, Clone)]
pub struct ClientParams {
    pub payload_size: usize,
    pub proto: String,
    pub target: String,
    pub expect_eof: bool,
    pub timeout_ms: u64,
}

#[derive(Debug, Clone, Copy)]
pub struct Throughput {
    pub bytes: usize,
    pub duration: Duration,
}
