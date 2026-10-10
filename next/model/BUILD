# Copyright 2023 The Android Open Source Project
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

NETSIM_API_DEPS = [
    "@bytes",
    "@futures",
    "//next/types",
    "@serde",
    "@thiserror",
    "@tokio",
    "@tokio-stream",
    "@tracing",
    "//next/actor-framework",
]

netsim_rust_library(
    name = "model",
    srcs = glob(["src/**/*.rs"]),
    crate_name = "netsim_model",
    edition = "2024",
    proc_macro_deps = [
        "@async-trait",
    ],
    test_deps = ["@serde_json"],
    testing_deps = ["@mockall"],
    deps = NETSIM_API_DEPS,
)
