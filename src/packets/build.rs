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

use std::{env, fs::File, io::Write, path::PathBuf};

fn main() {
    let rootcanal_dir =
        PathBuf::from(env::var("DEP_ROOTCANAL_ROOT").expect("DEP_ROOTCANAL_ROOT not set"));
    let pdl_path = rootcanal_dir.join("packets/link_layer_packets.pdl");
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR not set"));

    println!("cargo:rerun-if-changed={}", pdl_path.display());

    let mut out_file = File::create(out_dir.join("link_layer_packets.rs"))
        .expect("Could not create link_layer_packets.rs");

    let mut sources = pdl_compiler::ast::SourceDatabase::new();
    let in_file_str = pdl_path.to_str().expect("Filename is not UTF-8");
    let parsed_file =
        pdl_compiler::parser::parse_file(&mut sources, in_file_str).expect("PDL parse failed");
    let analyzed_file = pdl_compiler::analyzer::analyze(&parsed_file).expect("PDL analysis failed");
    let rust_source = pdl_compiler::backends::rust::generate(&sources, &analyzed_file, &[]);
    out_file.write_all(rust_source.as_bytes()).expect("Could not write to output file");
}
