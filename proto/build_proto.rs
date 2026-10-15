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
use std::{
    env, fs,
    path::Path,
    process::{Command, exit},
};
fn main() {
    // Entry point for build_proto

    let args: Vec<String> = env::args().collect();
    let mut protoc = String::new();
    let mut plugin = String::new();
    let mut out_dir = String::new();
    let mut proto_paths = Vec::new();
    let mut grpc_srcs = Vec::new();
    let mut protos = Vec::new();

    // Simple argument parsing without 'clap' to avoid deps
    let mut i = 1;
    while i < args.len() {
        if args[i].starts_with("--protoc=") {
            protoc = args[i].splitn(2, '=').nth(1).unwrap().to_string();
        } else if args[i].starts_with("--plugin=") {
            plugin = args[i].splitn(2, '=').nth(1).unwrap().to_string();
        } else if args[i].starts_with("--out-dir=") {
            out_dir = args[i].splitn(2, '=').nth(1).unwrap().to_string();
        } else if args[i].starts_with("--proto-path=") {
            proto_paths.push(args[i].splitn(2, '=').nth(1).unwrap().to_string());
        } else if args[i] == "--grpc-srcs" {
            // Handle space-separated list until next flag
            i += 1;
            while i < args.len() && !args[i].starts_with("--") {
                grpc_srcs.push(args[i].clone());
                i += 1;
            }
            continue; // Skip the increment at end of loop since we consumed args
        } else if !args[i].starts_with("--") {
            protos.push(args[i].clone());
        }
        i += 1;
    }

    if protoc.is_empty() || plugin.is_empty() || out_dir.is_empty() {
        eprintln!("Usage: build_proto --protoc=... --plugin=... --out-dir=... [protos...]");
        exit(1);
    }

    let out_path = Path::new(&out_dir);
    if !out_path.exists() {
        fs::create_dir_all(out_path).expect("Failed to create out dir");
    }

    // Create subdirectories
    for subdir in &["netsim", "rootcanal", "google/protobuf", "google"] {
        let p = out_path.join(subdir);
        if !p.exists() {
            fs::create_dir_all(&p).expect("Failed to create subdir");
        }
    }

    // Copy gRPC sources
    for src in &grpc_srcs {
        if src.ends_with("_grpc.rs") {
            let path = Path::new(src);
            if let Some(name) = path.file_name() {
                let dst = out_path.join("netsim").join(name);
                if path != dst {
                    let _ = fs::remove_file(&dst);
                    fs::copy(path, &dst).expect("Failed to copy gRPC file");
                }
            }
        }
    }

    // Run protoc
    let mut cmd = Command::new(&protoc);
    cmd.arg(format!("--plugin=protoc-gen-rust_community={}", plugin))
        .arg(format!("--rust_community_out={}", out_dir))
        .arg("--experimental_allow_proto3_optional");

    for p in &proto_paths {
        cmd.arg(format!("--proto_path={}", p));
    }

    cmd.args(&protos);

    // Print command for debugging (Bazel captures stderr)
    eprintln!("Running: {:?}", cmd);

    let status = cmd.status().expect("Failed to execute protoc");
    if !status.success() {
        eprintln!("Protoc failed with status: {}", status);
        exit(status.code().unwrap_or(1));
    }

    // Move files and patch imports
    let entries = fs::read_dir(out_path).expect("Failed to read out dir");
    for entry in entries {
        let entry = entry.expect("Inaccessible entry");
        let path = entry.path();

        if path.is_file() {
            let file_name = entry.file_name().into_string().unwrap();

            if file_name == "lib.rs"
                || file_name.ends_with("_grpc.rs")
                || !file_name.ends_with(".rs")
            {
                continue;
            }

            let dest_subdir = if file_name == "configuration.rs" {
                "rootcanal"
            } else if file_name == "mod.rs" {
                // skip generated mod.rs, we write our own lib.rs
                continue;
            } else {
                "netsim"
            };

            let dest = out_path.join(dest_subdir).join(&file_name);
            fs::rename(&path, &dest).expect("Failed to move file");

            // Patch rootcanal imports if moving to netsim (though generally config stays in rootcanal)
            if dest_subdir == "netsim" {
                // Nothing specific needed for netsim files usually unless they ref rootcanal
            }

            // Patch configuration.rs specifically if needed (from Python logic)
            if file_name == "configuration.rs" {
                let crg = "crate::rootcanal::configuration::";
                let content = fs::read_to_string(&dest).expect("Read config");
                let new_content = content
                    .replace("super::super::configuration::", crg)
                    .replace("super::configuration::", crg);
                fs::write(&dest, new_content).expect("Write config");
            }
        }
    }

    // Write lib.rs
    let lib_rs = out_path.join("lib.rs");
    let content = r#"
#![allow(ambiguous_glob_reexports)]
pub mod netsim {
  pub mod common;
  pub mod config;
  pub mod access_point;
  pub mod ble_service;
  pub mod cell;
  pub mod nfc_service;
  pub mod frontend;
  pub mod hci_packet;
  pub mod model;
  pub mod packet_streamer;
  pub mod startup;
  pub mod stats;
  pub mod casimir_control;
  pub mod wifi_service;
  pub mod reflection;
  pub use crate::rootcanal::configuration;
  pub mod packet {
      pub use super::hci_packet::*;
      pub use super::packet_streamer::*;
  }
}
pub mod rootcanal {
  pub mod configuration;
}
pub mod google {
    pub mod protobuf {
        pub use crate::protobuf::well_known_types::empty::Empty;
        pub use crate::protobuf::well_known_types::timestamp::Timestamp;
    }
}
pub mod grpc {
    pub mod reflection {
        pub mod v1alpha {
            pub use crate::netsim::reflection::*;
        }
    }
}
pub use netsim::common;
pub use netsim::config;
pub use netsim::access_point;
pub use netsim::ble_service;
pub use netsim::cell;
pub use netsim::nfc_service;
pub use netsim::frontend;
pub use netsim::hci_packet;
pub use netsim::model;
pub use netsim::packet_streamer;
pub use netsim::startup;
pub use netsim::stats;
pub use netsim::casimir_control;
pub use netsim::wifi_service;
pub use netsim::reflection;
pub use netsim::reflection as grpc_reflection;
pub use netsim::casimir_control as casimircontrolserver;
pub use rootcanal::configuration;
#[path = "netsim/frontend_grpc.rs"]
pub mod frontend_grpc;
#[path = "netsim/packet_streamer_grpc.rs"]
pub mod packet_streamer_grpc;
#[path = "netsim/access_point_grpc.rs"]
pub mod access_point_grpc;
#[path = "netsim/ble_service_grpc.rs"]
pub mod ble_service_grpc;
#[path = "netsim/cell_grpc.rs"]
pub mod cell_grpc;
#[path = "netsim/casimir_control_grpc.rs"]
pub mod casimir_control_grpc;
#[path = "netsim/nfc_service_grpc.rs"]
pub mod nfc_service_grpc;
#[path = "netsim/wifi_service_grpc.rs"]
pub mod wifi_service_grpc;
#[path = "netsim/reflection_grpc.rs"]
pub mod reflection_grpc;

pub use protobuf;
pub use protobuf::well_known_types::empty;
"#;
    fs::write(&lib_rs, content).expect("Failed to write lib.rs");
}
