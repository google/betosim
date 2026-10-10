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

load("@rules_rust//rust:defs.bzl", "rust_test_suite")
load("//next:defs.bzl", "netsim_rust_library")

# Rootcanal

# Note: This is rootcanal-rs -- the rust wrapper for lib_rootcanal_ffi
# but that name taken by a rootcanal rust component.

netsim_rust_library(
    name = "rootcanal",
    srcs = glob(["src/**/*.rs"]),
    crate_name = "rootcanal",
    edition = "2024",
    enable_integration_test = False,
    test_deps = [
        "@pdl-runtime",
    ],
    visibility = ["//next:__subpackages__"],
    deps = [
        "//next/packets",
        "@bytes",
        "@parking_lot",
        "@rootcanal//:lib_rootcanal_ffi",
        "@thiserror",
        "@tracing",
    ],
)

rust_test_suite(
    name = "tests",
    size = "small",
    srcs = glob(["tests/**/*.rs"]),
    edition = "2024",
    rustc_flags = [
        "-C",
        "link-arg=-lc",
    ],
    tags = ["general_tests"],
    deps = [
        ":rootcanal",
        "//next/packets",
        "@bytes",
        "@cxx.rs//:cxx",
        "@pdl-runtime",
        "@rootcanal//:rootcanal_rs",
        "@tokio",
        "@tracing",
        "@tracing-subscriber",
    ],
)
