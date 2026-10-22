// Copyright 2026 The Android Open Source Project
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

use std::{env, fs, path::PathBuf};

#[allow(dead_code, unused_imports)]
#[path = "src/openapi.rs"]
mod openapi;

fn main() {
    println!("cargo:rerun-if-changed=openapi.yaml");
    println!("cargo:rerun-if-changed=src/openapi.rs");
    println!("cargo:rerun-if-changed=src/openapi/cpp_gen.rs");
    println!("cargo:rerun-if-changed=src/openapi/py_gen.rs");
    println!("cargo:rerun-if-changed=src/openapi/rust_gen.rs");
    println!("cargo:rerun-if-changed=src/openapi/spec.rs");
    println!("cargo:rerun-if-changed=src/openapi/reserved_idents.inc");

    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR not set"));
    let yaml_content = fs::read_to_string("openapi.yaml").expect("Failed to read openapi.yaml");
    let mut doc: serde_json::Value = serde_yaml::from_str::<serde_yaml::Value>(&yaml_content)
        .map_err(|e| e.to_string())
        .and_then(|mut v| {
            v.apply_merge().map_err(|e| e.to_string())?;
            serde_yaml::from_value(v).map_err(|e| e.to_string())
        })
        .expect("Failed to parse openapi.yaml");

    openapi::expand(&mut doc);
    let errors = openapi::validate(&doc);
    if !errors.is_empty() {
        panic!("OpenAPI validation errors: {errors:?}");
    }

    let rust_code = openapi::generate_rust(&doc);
    fs::write(out_dir.join("openapi_generated.rs"), rust_code)
        .expect("Failed to write openapi_generated.rs");
}
