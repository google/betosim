// Copyright 2024 The Android Open Source Project
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

//! Build script for linking `libslirp-rs` with dependencies.

/// The main function of the build script.
///
/// Configures the build process to link against `libslirp` and other
/// OS-dependent libraries.
pub fn main() {
    let objs_path = std::env::var("OBJS_PATH").unwrap_or("../objs".to_string());

    println!("cargo:rustc-link-search={objs_path}/archives");
    println!("cargo:rustc-link-search={objs_path}/lib64");
    println!("cargo:rustc-link-lib=libslirp");
    #[cfg(target_os = "linux")]
    println!("cargo:rustc-link-lib=glib2_linux-x86_64");
    #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
    println!("cargo:rustc-link-lib=glib2_darwin-x86_64");
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    println!("cargo:rustc-link-lib=glib2_darwin-aarch64");
    #[cfg(target_os = "windows")]
    println!("cargo:rustc-link-lib=glib2_windows_msvc-x86_64");
    #[cfg(target_os = "windows")]
    println!("cargo:rustc-link-lib=iphlpapi");
}
