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
#   bazel build //next/packet-stream
#
# Run all tests:
#   bazel test //next/packet-stream:tests
#
# Run the linter (uses bazel test):
#   bazel test //next/packet-stream:clippy

load("//next:defs.bzl", "netsim_rust_library")

package(default_visibility = ["//visibility:public"])

DEPS = [
    "//next/types",
    "@bytes",
    "@futures",
    "@serde",
    "@serde_json",
    "@thiserror",
    "@tokio",
    "@tokio-util",
    "@tracing",
]

SELECT_DEPS = select({
    "@platforms//os:linux": [
        "@command-fds",
        "@nix",
    ],
    "//conditions:default": [],
})

netsim_rust_library(
    name = "packet-stream",
    srcs = glob(["src/**/*.rs"]),
    crate_features = select({
        "@platforms//os:linux": ["cuttlefish"],
        "//conditions:default": [],
    }),
    crate_name = "packet_stream",
    edition = "2024",
    proc_macro_deps = [
        "@async-trait",
    ],
    select_deps = SELECT_DEPS,
    deps = DEPS,
)
