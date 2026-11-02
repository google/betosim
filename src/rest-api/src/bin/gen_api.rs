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

//! Code generator that validates `openapi.yaml` and emits Rust, C++, or Python
//! types and route tables.

#![allow(unreachable_pub)]
#![allow(clippy::indexing_slicing)]
#![allow(clippy::print_stdout, clippy::print_stderr)]

use std::{env, fs, process::ExitCode};

#[allow(dead_code)]
#[path = "../openapi.rs"]
mod openapi;

fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();
    let [_, lang, path] = args.as_slice() else {
        eprintln!("Usage: gen_api <--rust|--cpp|--python> <openapi.yaml>");
        return ExitCode::FAILURE;
    };
    let mut doc: serde_json::Value = match fs::read_to_string(path)
        .map_err(|e| e.to_string())
        .and_then(|s| serde_yaml::from_str::<serde_yaml::Value>(&s).map_err(|e| e.to_string()))
        .and_then(|mut v| {
            v.apply_merge().map_err(|e| e.to_string())?;
            serde_yaml::from_value(v).map_err(|e| e.to_string())
        }) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("{path}: {e}");
            return ExitCode::FAILURE;
        }
    };
    openapi::expand(&mut doc);
    let errors = openapi::validate(&doc);
    if !errors.is_empty() {
        errors.iter().for_each(|err| eprintln!("{path}: {err}"));
        return ExitCode::FAILURE;
    }
    match lang.as_str() {
        "--rust" => print!("{}", openapi::generate_rust(&doc)),
        "--cpp" => print!("{}", openapi::generate_cpp(&doc)),
        "--python" => print!("{}", openapi::generate_python(&doc)),
        _ => {
            eprintln!("Unknown language flag {lang:?}; expected --rust, --cpp, or --python");
            return ExitCode::FAILURE;
        }
    }
    ExitCode::SUCCESS
}
