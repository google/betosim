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

# Rootcanal Legacy Control Server

netsim_rust_library(
    name = "rootcanal-server",
    srcs = glob(["src/**/*.rs"]),
    crate_name = "rootcanal_server",
    edition = "2024",
    test_deps = [
        "//src/daemon:testing",
    ],
    visibility = ["//src:__subpackages__"],
    deps = [
        "//src/device-actor",
        "//src/model",
        "@rand",
        "@tokio",
        "@tracing",
    ],
)
