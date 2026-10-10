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

netsim_rust_library(
    name = "hci-server",
    srcs = glob(["src/**/*.rs"]),
    crate_name = "hci_server",
    edition = "2024",
    deps = [
        "//next/device-actor",
        "//next/device-api",
        "//next/model",
        "//next/packet-stream",
        "@bytes",
        "@futures",
        "@tokio",
        "@tokio-stream",
        "@tokio-util",
        "@tracing",
    ],
)
