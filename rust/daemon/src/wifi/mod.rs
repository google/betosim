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

// [cfg(test)] gets compiled during local Rust unit tests
// [cfg(not(test))] avoids getting compiled during local Rust unit tests

pub(crate) mod error;
pub(crate) mod frame;
#[cfg_attr(feature = "cuttlefish", path = "hostapd_cf.rs")]
pub(crate) mod hostapd;
pub(crate) mod hwsim_attr_set;
#[cfg_attr(feature = "cuttlefish", path = "libslirp_cf.rs")]
pub(crate) mod libslirp;
#[cfg(not(feature = "cuttlefish"))]
pub(crate) mod mdns_forwarder;
pub(crate) mod medium;
pub(crate) mod radiotap;
pub(crate) mod stats;
