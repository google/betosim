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

load("@rules_rust//rust:defs.bzl", "rust_test")
load("//next:defs.bzl", "netsim_rust_library")

package(default_visibility = ["//:__subpackages__"])

netsim_rust_library(
    name = "packets",
    srcs = glob(["src/**/*.rs"]) + [
        "//:netsim_link_layer_packets_rust_gen",
    ],
    compile_data = glob([
        "src/**/test_data/**",
    ]),
    crate_name = "netsim_packets",
    edition = "2024",
    enable_doc_test = False,
    proc_macro_deps = [
        "@num-derive",
    ],
    rustc_env = {
        "OUT_DIR": "../../..",
    },
    rustc_flags = ["--cap-lints=allow"],
    deps = [
        "@bytes",
        "@hex",
        "@num-traits",
        "@pdl-runtime",
        "@serde",
        "@serde_json",
        "@zerocopy",
    ],
)

rust_test(
    name = "packets_test",
    crate = ":packets",
    data = glob(["src/**/test_data/**"]),
    proc_macro_deps = [
        "@num-derive",
    ],
    deps = [
        "@num-traits",
        "@zerocopy",
    ],
)

rust_test(
    name = "tests",
    srcs = glob(["tests/**/*.rs"]),
    crate_root = "tests/mod.rs",
    edition = "2024",
    deps = [
        ":packets",
        "@num-traits",
        "@zerocopy",
    ],
)
