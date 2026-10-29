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
load(
    "//next:defs.bzl",
    "netsim_rust_binary",
    "stripped_binaries",
)

package(default_visibility = ["//:__subpackages__"])

netsim_rust_binary(
    name = "netsim",
    srcs = glob(["src/**/*.rs"]),
    edition = "2024",
    tags = ["no-sandbox"],
    deps = [
        "//next/common",
        "//next/model",
        "//next/packets",
        "//next/rest-api",
        "//proto",
        "@clap",
        "@futures",
        "@futures-util",
        "@grpcio",
        "@hex",
        "@httparse",
        "@protobuf-rust",
        "@serde",
        "@serde_json",
        "@tokio",
        "@tracing",
        "@tungstenite",
    ],
)

rust_test(
    name = "netsim_test",
    srcs = glob(["src/**/*.rs"]),
    crate_root = "src/main.rs",
    edition = "2024",
    deps = [
        "//next/common",
        "//next/model",
        "//next/packets",
        "//next/rest-api",
        "//proto",
        "@clap",
        "@futures",
        "@futures-util",
        "@grpcio",
        "@hex",
        "@httparse",
        "@protobuf-rust",
        "@serde",
        "@serde_json",
        "@tokio",
        "@tracing",
        "@tungstenite",
    ],
)

stripped_binaries(
    name = "netsim_stripped",
    srcs = [":netsim"],
)

test_suite(
    name = "test",
    tests = [":netsim_test"],
)
