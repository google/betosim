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

load(":netsimd.bzl", "netsimd_rust_library")

package(default_visibility = ["//:__subpackages__"])

exports_files(["src/ffi.rs"])

netsimd_rust_library(
    name = "netsim_daemon",
    srcs = glob(["src/**/*.rs"]),
    aliases = {
        "//rust/hostapd-rs:netsim_hostapd_rs": "hostapd_rs",
        "//rust/http-proxy:netsim_http_proxy": "http_proxy",
        "//rust/libslirp-rs:netsim_libslirp_rs": "libslirp_rs",
    },
    compile_data = [
        "src/uwb/sample_ranging_data.csv",
    ],
    crate_name = "netsim_daemon",
    crate_root = "src/lib.rs",
    edition = "2021",
    rustc_flags = [
        "--cfg=feature=\"local_ssl\"",
    ],
    deps = [
        "//proto",
        "//rust/common:netsim_common",
        "//rust/hostapd-rs:netsim_hostapd_rs",
        "//rust/http-proxy:netsim_http_proxy",
        "//rust/libslirp-rs:netsim_libslirp_rs",
        "//rust/packets:netsim_packets",
        "@anyhow",
        "@bytes",
        "@c-ares//:ares",
        "@clap",
        "@cxx.rs//:cxx",
        "@data-encoding",
        "@futures",
        "@futures-channel",
        "@futures-executor",
        "@futures-util",
        "@glam",
        "@grpcio",
        "@http",
        "@httparse",
        "@libc",
        "@log",
        "@pdl-runtime",
        "@pica",
        "@protobuf-json-mapping",
        "@protobuf-rust",
        "@rand",
        "@regex",
        "@socket2",
        "@thiserror",
        "@tokio",
        "@tokio-stream",
        "@tungstenite",
    ],
)
