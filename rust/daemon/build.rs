//  Copyright 2023 The Android Open Source Project
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

//! Build script for linking `netsim-daemon` with dependencies.

use std::env;
use std::fs;
use std::path::PathBuf;

/// Adds all archive library dependencies from the specified `objs/archives` directory.
fn _all_archives_dependencies(objs_path: &str) {
    let archives_path = format!("{objs_path}/archives");
    if let Ok(entry_lst) = fs::read_dir(&archives_path) {
        println!("cargo:rustc-link-search=all={archives_path}");
        for entry in entry_lst {
            let entry = entry.unwrap();
            let path = entry.path();
            if path.is_file() {
                if let Some(filename) = path.file_name() {
                    if let Some(filename_str) = filename.to_str() {
                        let lib_name = &filename_str[3..filename.len() - 2];
                        // "rootcanal.configuration.ControllerFeatures" conflicting symbols
                        if lib_name == "librootcanal_config" {
                            println!(
                                "cargo:warning=skip linking librootcanal_config to avoid conflicting symbols on rootcanal.configuration.ControllerFeatures"
                            );
                            continue;
                        }
                        println!("cargo:rustc-link-lib=static={lib_name}");
                    }
                }
            }
        }
    }
}
/// Configures linking for Linux test builds, including prebuilt PDL files and Rootcanal library.
fn _run_test_link() {
    // Linking libraries in objs/archives & objs/lib64
    let objs_path = std::env::var("OBJS_PATH").unwrap_or("../objs".to_string());
    println!("cargo:rustc-link-arg=-Wl,--allow-multiple-definition");
    _all_archives_dependencies(&objs_path);
    println!("cargo:rustc-link-lib=dylib=abseil_dll");

    // Locate prebuilt pdl generated rust packet definition files
    let prebuilts: [[&str; 2]; 5] = [
        ["LINK_LAYER_PACKETS_PREBUILT", "link_layer_packets.rs"],
        ["MAC80211_HWSIM_PACKETS_PREBUILT", "mac80211_hwsim_packets.rs"],
        ["IEEE80211_PACKETS_PREBUILT", "ieee80211_packets.rs"],
        ["LLC_PACKETS_PREBUILT", "llc_packets.rs"],
        ["NETLINK_PACKETS_PREBUILT", "netlink_packets.rs"],
    ];
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    for [var, name] in prebuilts {
        let env_prebuilt = env::var(var);
        let out_file = out_dir.join(name);
        // Check and use prebuilt pdl generated rust file from env var
        if let Ok(prebuilt_path) = env_prebuilt {
            println!("cargo:rerun-if-changed={}", prebuilt_path);
            std::fs::copy(prebuilt_path.as_str(), out_file.as_os_str().to_str().unwrap()).unwrap();
        // Prebuilt env var not set - check and use pdl generated file that is already present in out_dir
        } else if out_file.exists() {
            println!(
                "cargo:warning=env var {} not set. Using prebuilt found at: {}",
                var,
                out_file.display()
            );
        } else {
            panic!("Unable to find env var or prebuilt pdl generated rust file for: {}.", name);
        };
    }

    // Linking Rootcanal Rust Library
    if std::path::Path::new(&format!("{objs_path}/rootcanal/rust")).exists() {
        println!("cargo:rustc-link-search=all={objs_path}/rootcanal/rust");
        println!("cargo:rustc-link-lib=static=rootcanal_rs");
    }
}

/// Configures C++ FFI bindings and Linux test linking.
fn main() {
    // Linking for FFI
    let _build = cxx_build::bridge("src/ffi.rs");
    println!("cargo:rerun-if-changed=src/ffi.rs");

    // TODO(379708365): Link libraries for Mac and Windows for integration test
    #[cfg(target_os = "linux")]
    _run_test_link()
}
