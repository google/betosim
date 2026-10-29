# Copyright 2023-2026 The Android Open Source Project
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
# Build the binary:
#   bazel build //next/daemon
#
# Run the binary:
#   bazel run //next/daemon
#
# Run the tests:
#   bazel test //next/daemon:tests

load(
    "//next:defs.bzl",
    "netsim_rust_binary",
    "netsim_rust_library",
    "stripped_binaries",
)

package(default_visibility = ["//visibility:public"])

DAEMON_SRCS = [
    "src/args.rs",
    "src/avd_config.rs",
    "src/cuttlefish.rs",
    "src/ini_file.rs",
    "src/lib.rs",
    "src/logger.rs",
    "src/netsimd.rs",
    "src/platform/cuttlefish.rs",
    "src/platform/mod.rs",
    "src/platform/standard.rs",
    "src/test_beacons.rs",
    "src/version.rs",
]

netsim_rust_library(
    name = "daemon-lib",
    srcs = DAEMON_SRCS,
    crate_name = "daemon",
    edition = "2024",
    proc_macro_deps = [],
    rustc_env = {
        "NETSIM_VERSION_SUFFIX": "{STABLE_NETSIM_VERSION_SUFFIX}",
    },
    select_deps = select({
        "@bazel_tools//src/conditions:windows": ["@winapi-util"],
        "//conditions:default": [],
    }),
    stamp = 1,
    test_deps = [
        "//proto",
        "@bytes",
        "@rand",
        "@regex",
        "@tempfile",
    ],
    deps = [
        "//next/ap-actor",
        "//next/bluetooth-actor",
        "//next/capture-actor",
        "//next/cell-actor",
        "//next/common",
        "//next/device-actor",
        "//next/device-api",
        "//next/ethernet-actor",
        "//next/grpc-server",
        "//next/hci-server",
        "//next/link-actor",
        "//next/link-api",
        "//next/model",
        "//next/nfc-actor",
        "//next/packet-stream",
        "//next/rest-api",
        "//next/slirp-actor",
        "//next/uwb-actor",
        "//next/websocket-server",
        "//next/wifi-actor",
        "@clap",
        "@futures",
        "@grpcio",
        "@serde",
        "@serde_json",
        "@tokio",
        "@tracing",
    ],
)

netsim_rust_binary(
    name = "daemon",
    srcs = ["src/main.rs"],
    edition = "2024",
    rustc_flags = [
        "-C",
        "link-arg=-lc",
        "-Cforce-frame-pointers=yes",
    ],
    tags = ["no-sandbox"],
    deps = [
        ":daemon-lib",
    ],
)

stripped_binaries(
    name = "daemon_stripped",
    srcs = [":daemon"],
)
