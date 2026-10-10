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

netsim_rust_library(
    name = "netsim-testing",
    testonly = True,
    srcs = glob(["src/**/*.rs"]),
    crate_features = ["testing"],
    crate_name = "netsim_testing",
    edition = "2024",
    enable_rustfmt = False,
    visibility = ["//next:__subpackages__"],
    deps = [
        "//next/common",
        "@bytes",
        "@futures",
        "@tokio",
        "@tokio-stream",
    ],
)
