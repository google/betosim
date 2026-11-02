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

load("@rules_rust//rust:defs.bzl", "rust_test")
load("//src:defs.bzl", "netsim_rust_library")

package(default_visibility = ["//visibility:public"])

BLUETOOTH_DEPS = [
    "//src/actor-framework",
    "//src/device-actor",
    "//src/model",
    "//src/rootcanal",
    "//src/packets",
    "@bytes",
    "@futures",
    "@hex",
    "@parking_lot",
    "@tracing",
    "@serde",
    "@thiserror",
    "@tokio",
    "@tokio-stream",
    "@zerocopy",
]

netsim_rust_library(
    name = "bluetooth-actor",
    srcs = glob(["src/**/*.rs"]),
    crate_name = "bluetooth_actor",
    edition = "2024",
    enable_integration_test = False,
    proc_macro_deps = [
        "@async-trait",
    ],
    deps = BLUETOOTH_DEPS,
)

rust_test(
    name = "integration-test",
    size = "small",
    srcs = glob(["tests/**/*.rs"]),
    crate_root = "tests/mod.rs",
    edition = "2024",
    proc_macro_deps = ["@async-trait"],
    rustc_flags = [
        "-C",
        "link-arg=-lc",
    ],
    tags = ["general_tests"],
    deps = [
        ":testing",
        "//proto",
        "//src/actor-framework:testing",
        "//src/common",
        "//src/device-actor:testing",
        "//src/device-api:testing",
        "//src/model:testing",
        "//src/packets",
        "//src/rootcanal:testing",
        "//src/testing",
        "@bytes",
        "@futures",
        "@hex",
        "@protobuf-rust",
        "@rand",
        "@rootcanal//:lib_rootcanal_ffi",
        "@serde",
        "@thiserror",
        "@tokio",
        "@tokio-stream",
        "@tracing",
        "@tracing-subscriber",
        "@zerocopy",
    ],
)
