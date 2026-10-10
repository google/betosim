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
    name = "slirp-host",
    srcs = glob(["src/**/*.rs"]),
    crate_name = "slirp_host",
    edition = "2024",
    # The integration tests bind loopback TCP listeners and send real ICMP
    # echo requests to 127.0.0.1, which the sandbox's network namespace blocks.
    tags = ["no-sandbox"],
    test_deps = ["@zerocopy"],
    deps = [
        "//next/packets",
        "//next/slirp",
        "@bytes",
        "@log",
        "@parking_lot",
        "@socket2",
        "@tokio",
    ],
)
