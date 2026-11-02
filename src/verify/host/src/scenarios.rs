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

//! # Netsim HDD Scenarios
//!
//! This module defines the sequence of operations for the integration test
//! suite.
//!
//! ## Variable Definition Pattern
//! To avoid "Ghost Variable" errors in dry-run mode, we follow a strict
//! definition pattern:
//! 1. **Definition**: Use literal names in quotes (e.g., `on "port"`) during
//!    the step that populates the variable.
//! 2. **Usage**: Use curly braces (e.g., `to 10.0.2.2:{port}`) in subsequent
//!    steps that consume the resolved value.

use crate::orchestrator::TestContext;

async fn run_feature(
    features: &mut features::Features<TestContext>,
    ctx: &mut TestContext,
    name: &str,
    content: &str,
) -> Result<(), String> {
    if let Some(f) = &ctx.filter {
        if !name.contains(f) {
            return Ok(());
        }
    }
    if ctx.is_dry_run {
        features.dry_run_list(content);
        return Ok(());
    }

    for is_retry in [false, true] {
        if is_retry {
            ctx.reset_actors(true).await?;
            println!("INFO: Retrying feature in isolation...");
        }

        match features.execute_from_memory(content, ctx).await {
            Ok(_) => {
                if is_retry {
                    println!("INFO: Feature succeeded on retry after hard reset.");
                }
                ctx.reset_actors(false).await?;
                break;
            }
            Err(e) => {
                if !is_retry {
                    println!("WARN: Feature failed: {}. Attempting isolation recovery...", e);
                } else {
                    println!("ERROR: Feature failed again on retry: {}", e);
                    // Reset actors again (hard) to be safe for next scenarios if keep_going is true
                    ctx.reset_actors(true).await?;
                    return Err(e);
                }
            }
        }
    }
    Ok(())
}

fn find_feature_files(
    dir: &std::path::Path,
    entries: &mut Vec<std::path::PathBuf>,
    visited: &mut std::collections::HashSet<std::path::PathBuf>,
) {
    let read_dir = match std::fs::read_dir(dir) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("WARN: Failed to read directory {:?}: {}", dir, e);
            return;
        }
    };

    for entry in read_dir {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                eprintln!("WARN: Failed to read entry: {}", e);
                continue;
            }
        };
        let path = entry.path();
        let file_name = entry.file_name();
        if file_name.as_encoded_bytes().starts_with(b".") {
            continue;
        }

        if path.is_dir() {
            let canonical = match path.canonicalize() {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("WARN: Failed to canonicalize path {:?}: {}", path, e);
                    continue;
                }
            };
            if visited.insert(canonical.clone()) {
                find_feature_files(&path, entries, visited);
            }
        } else if path.is_file() && path.extension().map_or(false, |ext| ext == "feature") {
            entries.push(path);
        }
    }
}

pub async fn run_suite(
    ctx: &mut TestContext,
    mut features: features::Features<TestContext>,
    spec_dir: Option<String>,
) -> Result<(), String> {
    let dir_path = match spec_dir {
        Some(path) => path,
        None => return Err("No spec directory provided. Use --spec-dir.".to_string()),
    };
    let dir = std::path::Path::new(&dir_path);
    if !dir.is_dir() {
        return Err(format!("Features directory not found: {}", dir_path));
    }

    let mut entries = Vec::new();
    let mut visited = std::collections::HashSet::new();
    visited.insert(dir.canonicalize().map_err(|e| e.to_string())?);
    find_feature_files(&dir, &mut entries, &mut visited);
    // Sort for deterministic order
    entries.sort();

    let mut error_messages = Vec::new();

    for path in entries {
        let filename = path.file_name().unwrap().to_string_lossy().into_owned();
        let content = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
        match run_feature(&mut features, ctx, &filename, &content).await {
            Ok(_) => {}
            Err(e) => {
                error_messages.push(format!("{}: {:?}", filename, e));
                if !ctx.keep_going {
                    return Err(e);
                }
            }
        }
    }

    if !error_messages.is_empty() {
        println!("\n--- Test Failures Summary ---");
        for msg in &error_messages {
            println!("{}", msg);
        }
        return Err("Some specifications failed".to_string());
    }

    Ok(())
}
