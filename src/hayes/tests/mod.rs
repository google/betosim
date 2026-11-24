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

pub mod common;
pub mod steps;

mod boot_test;
mod call_service_test;
mod cellular_network_test;
mod data_service_test;
mod integration_test;
mod misc_service_test;
mod modem_action_test;
mod multi_call_test;
mod network_service_test;
mod sim_filesystem_test;
mod sim_pin_test;
mod sim_service_test;
mod sms_service_test;
mod stk_service_test;
mod sup_service_test;
mod time_service_test;
pub mod world;
