# Copyright 2025 The Android Open Source Project
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

load("@rules_rust//rust:defs.bzl", "rust_library", "rust_test")

package(default_visibility = ["//visibility:public"])

rust_library(
    name = "netsim_capture",
    srcs = glob(["src/**/*.rs"]),
    aliases = {
        "//rust/libslirp-rs:netsim_libslirp_rs": "libslirp_rs",
    },
    crate_name = "netsim_capture",
    crate_root = "src/lib.rs",
    edition = "2021",
    proc_macro_deps = [
        "@zerocopy-derive",
    ],
    deps = [
        "@tokio",
        "@zerocopy",
    ],
)

rust_test(
    name = "netsim_capture_tests",
    crate = ":netsim_capture",
    tags = ["general_tests"],
)
