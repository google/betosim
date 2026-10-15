// Copyright 2025-2026 The Android Open Source Project
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

fn main() {
    cxx_build::bridge("src/ffi.rs")
        .file("src/crypto_ffi.cc")
        .include("src")
        .flag_if_supported("-std=c++17")
        .compile("hostap_crypto");

    println!("cargo:rerun-if-changed=src/ffi.rs");
    println!("cargo:rerun-if-changed=src/crypto_ffi.cc");
    println!("cargo:rerun-if-changed=src/crypto_ffi.h");

    // Link against libcrypto (BoringSSL/OpenSSL)
    // In Android build this is handled by Soong/Bazel, but for Cargo:
    println!("cargo:rustc-link-lib=crypto");
}
