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

SELECT_DEPS = select({
    "@platforms//os:linux": [
        "@nix",
        "@libc",
    ],
    "//conditions:default": [],
})

netsim_rust_library(
    name = "wifi-actor",
    srcs = glob(["src/**/*.rs"]),
    crate_name = "wifi_actor",
    edition = "2024",
    proc_macro_deps = [
        "@async-trait",
    ],
    select_deps = SELECT_DEPS,
    test_deps = [
        "//src/device-api:testing",
        "@tracing-subscriber",
    ],
    deps = [
        "//proto",
        "//src/actor-framework",
        "//src/ap-actor",
        "//src/device-actor",
        "//src/model",
        "//src/packets",
        "//src/slirp-actor",
        "@bytes",
        "@futures",
        "@socket2",
        "@thiserror",
        "@tokio",
        "@tokio-stream",
        "@tracing",
        "@zerocopy",
    ],
)
