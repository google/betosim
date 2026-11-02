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

load("//src:defs.bzl", "netsim_rust_library")

package(default_visibility = ["//visibility:public"])

netsim_rust_library(
    name = "grpc-server",
    srcs = glob(["src/*.rs"]),
    crate_name = "grpc_server",
    edition = "2024",
    proc_macro_deps = [
        "@async-trait",
    ],
    test_deps = [
        "//src/actor-framework:testing",
        "@tempfile",
    ],
    deps = [
        "//proto",
        "//src/ap-actor",
        "//src/capture-actor",
        "//src/cell-actor",
        "//src/device-actor",
        "//src/device-api",
        "//src/link-actor",
        "//src/link-api",
        "//src/model",
        "//src/nfc-actor",
        "//src/packet-stream",
        "//src/packets",
        "//src/wifi-actor",
        "@bytes",
        "@futures",
        "@grpcio",
        "@hex",
        "@pdl-runtime",
        "@protobuf-rust",
        "@tokio",
        "@tokio-stream",
        "@tracing",
    ],
)
