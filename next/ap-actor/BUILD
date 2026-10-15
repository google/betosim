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

load("@cxx.rs//tools/bazel:rust_cxx_bridge.bzl", "rust_cxx_bridge")
load("@rules_cc//cc:defs.bzl", "cc_library")
load("//next:defs.bzl", "netsim_rust_library")

package(default_visibility = ["//visibility:public"])

genrule(
    name = "cxx-bridge-header",
    outs = ["cxx.h"],
    cmd = "$(execpath @cxx.rs//:codegen) --header > $(OUTS)",
    tools = ["@cxx.rs//:codegen"],
    visibility = ["//visibility:public"],
)

cc_library(
    name = "hostap_crypto_shim",
    srcs = ["src/crypto_ffi.cc"],
    hdrs = [
        "src/crypto_ffi.h",
        ":cxx-bridge-header",
    ],
    defines = [
        "CONFIG_OPENSSL_INTERNAL_AES_WRAP",
    ],
    includes = [
        ".",
        "src",
    ],
    # netsimd is installed as a standalone binary next to the emulator, where no
    # netsim .so files are shipped. Without linkstatic the cc_library is linked
    # dynamically and the daemon aborts at startup with
    # "error while loading shared libraries: libhostap_crypto_shim.so".
    linkstatic = True,
    deps = [
        "@boringssl//:crypto",
        "@cxx.rs//:core",
    ],
)

netsim_rust_library(
    name = "ap-actor",
    srcs = glob(["src/**/*.rs"]),
    crate_name = "ap_actor",
    edition = "2024",
    test_deps = [
        "//next/testing",
        "@rand",
    ],
    deps = [
        ":ap_actor_cxx",
        ":hostap_crypto_shim",
        "//next/actor-framework",
        "//next/model",
        "//next/packets",
        "@bytes",
        "@cxx.rs//:cxx",
        "@futures",
        "@parking_lot",
        "@serde",
        "@thiserror",
        "@tokio",
        "@tokio-stream",
        "@tracing",
        "@zerocopy",
    ],
)

# Architecture Note:
# ap-actor uses C++ BoringSSL via cxx bridge for WPA/EAPOL Authentication to ensure
# strict compliance and correctness with hostapd/wpa_supplicant behavior.
# Native Rust crypto crates (hmac, sha1, etc) required for Auth are not available
# in the Netsim workspace configuration.
#
# wifi-actor (Data Plane) uses the available 'ccm' and 'aes' Rust crates for
# high-performance, zero-copy packet encryption/decryption.
rust_cxx_bridge(
    name = "ap_actor_bridge",
    src = "src/ffi.rs",
    deps = [":hostap_crypto_shim"],
)

cc_library(
    name = "ap_actor_cxx",
    srcs = [":ap_actor_bridge"],
    # Linked statically for the same reason as :hostap_crypto_shim above.
    linkstatic = True,
    deps = [
        ":hostap_crypto_shim",
        "@cxx.rs//:core",
    ],
    # Deliberately not alwayslink. The generated bridge shims are called from
    # ap-actor's Rust code, so the linker already has references that pull the
    # objects out of the archive. Forcing them in instead drags them into every
    # downstream binary, including test binaries that never touch the bridge.
    # Those objects reference the cxx runtime, and its Rust half is only linked
    # when some crate in the Rust graph actually uses it, which for an unrelated
    # test it does not: //next/rootcanal-server:test then failed on Windows with
    # 20 undefined cxxbridge1$string$*, cxxbridge1$str$* and cxxbridge1$slice$*
    # symbols. Unix linkers tolerated it; lld-link does not.
)
