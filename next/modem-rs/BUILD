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
#
# Common Bazel commands for this package:
#
# Build the library:
#   bazel build //next/modem-rs
#
# Run all tests:
#   bazel test //next/modem-rs:tests
#
# Run the linter (uses bazel test):
#   bazel test //next/modem-rs:clippy

load("//next:defs.bzl", "netsim_rust_library")

package(default_visibility = ["//visibility:public"])

netsim_rust_library(
    name = "modem-rs",
    srcs = glob(["src/**/*.rs"]),
    compile_data = [
        "src/profiles/iccprofile_for_sim0.xml",
        "src/profiles/iccprofile_for_sim0_for_CtsCarrierApiTestCases.xml",
        "src/profiles/iccprofile_for_sim_tel_alaska.xml",
    ],
    crate_name = "modem_rs",
    edition = "2024",
    features = ["test-utils"],
    proc_macro_deps = [
        "//next/modem-rs-derive",
    ],
    test_deps = [
        "@serde_json",
        "@tempfile",
        "@tracing-subscriber",
    ],
    deps = [
        "//next/model",
        "@bytes",
        "@hex",
        "@jiff",
        "@nom",
        "@parking_lot",
        "@serde",
        "@serde-xml-rs",
        "@tokio",
        "@tracing",
    ],
)
