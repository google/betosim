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

"""Provides a wrapper for rust_library and rust_static_library for netsimd on different platforms."""

load("@rules_rust//rust:defs.bzl", "rust_library", "rust_static_library")

# The correct way to created an rust archive for static linking into a c++ binary is to use rust_static_library.
# However, on Linux and Mac, rust_library works - probably because we set @rules_rust//rust/settings:experimental_use_cc_common_link.
# But that flag doesn't work on Windows.
# This rust library has C++ dependencies and is used to produce a C++ binary: C++ -> Rust -> C++
# On Linux and Mac with rust_static_library and when compiling for release (-c opt), rustc fails to create the archive when clang is newer (LLVM incompatibility with thinlto).
# When we upgrade rustc to be newer then clang/lld has similar complaints when linking the binary containing this rust archive.
# So for now, we leave Linux and Mac to use the "wrong" but working rust_library.
# This should be rectified when we can upgrade our rust toolchain to match our clang toolchain but that is currently blocked by other issues.
def netsimd_rust_library(name, **kwargs):
    rust_library(
        name = name,
        target_compatible_with = select({
            "@platforms//os:windows": ["@platforms//:incompatible"],
            "//conditions:default": [],
        }),
        **kwargs
    )
    windows_kwargs = dict(kwargs)
    windows_flags = windows_kwargs.pop("rustc_flags", [])
    rust_static_library(
        name = name + "_windows",
        target_compatible_with = ["@platforms//os:windows"],
        # Force Static CRT linking (+crt-static) to avoid ABI mismatches with the Emulator's prebuilt DLLs.
        rustc_flags = windows_flags + ["-C", "target-feature=+crt-static"],
        **windows_kwargs
    )
