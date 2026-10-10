# Copyright 2026 The Android Open Source Project
#
# Licensed under the Apache License, Version 2.0 (the "License");
# you may not use this file except in compliance with the License.
# You may obtain a copy of the License at
#
#     http://www.apache.org/licenses/LICENSE-2.0
#
# Unless required by applicable law or agreed to in writing, software
# distributed under the License is distributed on an "AS IS" BASIS,
# WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
# See the License for the specific language governing permissions and
# limitations under the License.

load("@rules_rust//rust:defs.bzl", "rust_library")

package(default_visibility = ["//:__subpackages__"])

rust_library(
    name = "netsim_packets",
    srcs = glob(["src/*.rs"]) + [
        "//:netsim_ieee80211_rust_gen",
        "//:netsim_link_layer_packets_rust_gen",
        "//:netsim_llc_rust_gen",
        "//:netsim_mac80211_hwsim_rust_gen",
        "//:netsim_netlink_rust_gen",
    ],
    crate_name = "netsim_packets",
    edition = "2021",
    proc_macro_deps = [
        "@async-trait",
    ],
    rustc_env = {
        "OUT_DIR": "../../..",
    },
    deps = [
        "@anyhow",
        "@bytes",
        "@pdl-runtime",
    ],
)
