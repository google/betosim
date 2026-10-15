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
#   bazel build //next/modem-rs-derive
#
# Run all tests:
#   bazel test //next/modem-rs-derive:tests
#
# Run the linter (uses bazel test):
#   bazel test //next/modem-rs-derive:clippy

load(
    "@rules_rust//rust:defs.bzl",
    "rust_clippy",
    "rust_doc",
    "rust_doc_test",
    "rust_proc_macro",
)

package(default_visibility = ["//visibility:public"])

rust_proc_macro(
    name = "modem-rs-derive",
    srcs = glob(["src/**/*.rs"]),
    crate_name = "modem_rs_derive",
    crate_root = "src/lib.rs",
    edition = "2024",
    deps = [
        "@nom",
        "@proc-macro2",
        "@quote",
        "@syn",
    ],
)

rust_clippy(
    name = "clippy",
    deps = [":modem-rs-derive"],
)

rust_doc(
    name = "doc",
    crate = ":modem-rs-derive",
)

rust_doc_test(
    name = "doc_test",
    crate = ":modem-rs-derive",
    target_compatible_with = ["@platforms//os:linux"],
)
