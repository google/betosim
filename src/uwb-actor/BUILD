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

DEPS = [
    "//src/actor-framework",
    "//src/device-actor",
    "//src/model",
    "//proto",
    "@bytes",
    "@thiserror",
    "@futures",
    "@tracing",
    "@parking_lot",
    "@pdl-runtime",
    "@pica",
    "@rand",
    "@tokio",
    "@serde",
    "@glam",
]

netsim_rust_library(
    name = "uwb-actor",
    srcs = glob(["src/**/*.rs"]),
    compile_data = ["src/sample_ranging_data.csv"],
    crate_features = ["testing"],
    crate_name = "uwb_actor",
    crate_root = "src/lib.rs",
    edition = "2024",
    proc_macro_deps = [
        "@async-trait",
    ],
    test_deps = [
        "//src/device-api:testing",
        "//src/testing",
    ],
    deps = DEPS,
)
