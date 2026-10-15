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

load("//next:defs.bzl", "netsim_rust_library")

package(default_visibility = ["//visibility:public"])

DEPS = [
    "//next/actor-framework",
    "//next/capture-api",
    "//next/common",
    "//next/model",
    "//next/packets",
    "@bytes",
    "@futures",
    "@serde",
    "@thiserror",
    "@tokio",
    "@tokio-stream",
    "@tracing",
    "@zerocopy",
]

netsim_rust_library(
    name = "capture-actor",
    srcs = glob(["src/**/*.rs"]),
    crate_features = [],
    crate_name = "capture_actor",
    crate_root = "src/lib.rs",
    edition = "2024",
    proc_macro_deps = [
        "@async-trait",
    ],
    deps = DEPS,
)
