// Copyright 2023 The Android Open Source Project
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

pub mod ble_beacon;
pub mod bluetooth;
pub mod mocked;
pub mod packet;
pub mod uwb;
pub mod wifi_chip;
pub mod wifi_manager;
pub mod wireless_chip;
pub mod wireless_manager;

pub use crate::wireless::packet::{handle_request, handle_request_cxx, handle_response_cxx};
pub use crate::wireless::wireless_chip::WirelessChip;
pub use crate::wireless::wireless_chip::WirelessChipImpl;
pub use crate::wireless::wireless_manager::{CreateParam, add_chip};
