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

load(
    "@rules_rust//rust:defs.bzl",
    "rustfmt_test",
)

exports_files([
    "rustfmt.toml",
    "netsimd.cc",
])

rustfmt_test(
    name = "fmt",
    tags = ["manual"],
    targets = [
        "//rust/capture:netsim_capture",
        "//rust/cli:netsim_cli",
        "//rust/common:netsim_common",
        "//rust/daemon:netsim_daemon",
        "//rust/hostapd-rs:netsim_hostapd_rs",
        "//rust/http-proxy:netsim_http_proxy",
        "//rust/libslirp-rs:netsim_libslirp_rs",
        "//rust/packets:netsim_packets",
    ],
)
