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

use std::{env, path::PathBuf};

use bindgen;

pub fn main() {
    println!("cargo:rerun-if-changed=bindings/wrapper.h");
    println!("cargo:rerun-if-changed=build.rs");

    let bindings = bindgen::Builder::default()
        .header("bindings/wrapper.h")
        // The Windows MCI sequencer API is unused, and its names trip
        // go/cross inclusive-language checks.
        .blocklist_item("MCI_SEQ_.*")
        .blocklist_type(".*MCI_SEQ_.*")
        .generate()
        .expect("Unable to generate bindings");

    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap();
    let out_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap())
        .join("src")
        .join("libslirp_sys")
        .join(target_os);

    std::fs::create_dir_all(&out_dir).expect("Failed to create output directory");

    let out_path = out_dir.join("bindings.rs");
    bindings.write_to_file(out_path).expect("Couldn't write bindings!");
}
