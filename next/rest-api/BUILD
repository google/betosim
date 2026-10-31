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

load("//next:defs.bzl", "netsim_rust_binary", "netsim_rust_library")

package(default_visibility = ["//visibility:public"])

netsim_rust_binary(
    name = "gen_api",
    srcs = [
        "src/bin/gen_api.rs",
        "src/openapi.rs",
        "src/openapi/cpp_gen.rs",
        "src/openapi/py_gen.rs",
        "src/openapi/rust_gen.rs",
        "src/openapi/spec.rs",
    ],
    compile_data = ["src/openapi/reserved_idents.inc"],
    crate_root = "src/bin/gen_api.rs",
    edition = "2024",
    rustc_flags = [
        "-Aunreachable_pub",
    ],
    deps = [
        "@hyper",
        "@serde_json",
        "@serde_yaml",
    ],
)

genrule(
    name = "openapi_rust_gen",
    srcs = ["openapi.yaml"],
    outs = ["openapi_generated.rs"],
    cmd = "$(execpath :gen_api) --rust $(SRCS) > $(OUTS)",
    tools = [":gen_api"],
)

netsim_rust_library(
    name = "rest-api",
    srcs = glob(
        ["src/**/*.rs"],
        exclude = ["src/bin/**"],
    ) + [
        ":openapi_rust_gen",
    ],
    compile_data = [
        "src/openapi/reserved_idents.inc",
        "tests/openapi.yaml",
    ],
    crate_name = "netsim_rest_api",
    crate_root = "src/lib.rs",
    edition = "2024",
    enable_doc_test = False,
    proc_macro_test_deps = [
        "@async-trait",
    ],
    rustc_env = {
        "OUT_DIR": "..",
    },
    test_deps = [
        "//next/actor-framework",
        "//next/capture-api",
        "//next/device-api",
        "@serde_yaml",
    ],
    deps = [
        "//next/ap-actor",
        "//next/capture-actor",
        "//next/device-actor",
        "//next/link-api",
        "//next/model",
        "@base64",
        "@bytes",
        "@form_urlencoded",
        "@futures",
        "@http-body-util",
        "@hyper",
        "@hyper-util",
        "@ring",
        "@serde",
        "@serde_json",
        "@thiserror",
        "@tokio",
        "@tokio-stream",
        "@tokio-util",
        "@tracing",
    ],
)
