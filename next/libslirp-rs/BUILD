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

load("//next:defs.bzl", "netsim_rust_library")

package(default_visibility = ["//visibility:public"])

netsim_rust_library(
    name = "libslirp-rs",
    srcs = glob(["src/**/*.rs"]),
    crate_name = "libslirp_rs",
    crate_root = "src/lib.rs",
    edition = "2024",
    enable_integration_test = True,
    # Tests monitor process-level file descriptor counts (/proc/self/fd) to verify zero
    # resource leaks on Drop of World. Running integration tests in parallel causes FD
    # count race conditions, so serial execution (--test-threads=1) is strictly required.
    integration_test_args = ["--test-threads=1"],
    select_deps = select({
        "//:standalone": [],
        "//conditions:default": [
            "@glib//glib",
            "@libslirp",
        ],
    }) + select({
        "@bazel_tools//src/conditions:windows": ["@windows-sys"],
        "@platforms//os:linux": ["@libc"],
        "@platforms//os:macos": ["@libc"],
    }),
    target_compatible_with = select({
        "//:standalone": ["@platforms//:incompatible"],
        "//conditions:default": [],
    }),
    test_deps = ["//next/packets"],
    deps = [
        "//next/slirp",
        "@bytes",
        "@tokio",
        "@tracing",
    ],
)
