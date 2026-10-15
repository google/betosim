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

//! Command-line tool for running Netsim integration tests.

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "runner")]
#[command(about = "Network Tester for Netsim orchestrator", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Run e2e tests
    Run(verify_host_lib::orchestrator::RunArgs),
    /// List available test scenarios
    Scenarios {
        #[arg(long, help = "Directory containing feature files (specs)")]
        spec_dir: Option<String>,
        #[arg(
            long,
            help = "Comma-separated list of tags to ignore",
            default_value = "nyi,nyt,skip,ignore"
        )]
        ignore_tags: String,
    },
}

use verify_host_lib::{
    adb_steps, android_steps,
    features::Features,
    host_steps, netsim_steps,
    orchestrator::{self, TestContext},
};

#[tokio::main]
async fn main() -> Result<(), String> {
    tracing_subscriber::fmt::init();
    let cli = Cli::parse();

    match cli.command {
        Commands::Run(mut args) => {
            let resolved_android_home = match args
                .android_home
                .clone()
                .or_else(|| std::env::var("ANDROID_HOME").ok())
            {
                Some(path) => path,
                None => {
                    return Err("ANDROID_HOME environment variable is not set. Please set it to your Android SDK path or pass it via --android-home.".to_string());
                }
            };
            let p = std::path::Path::new(&resolved_android_home);
            if !p.is_dir() {
                return Err(format!(
                    "ANDROID_HOME path is not a directory: {}",
                    resolved_android_home
                ));
            }
            if !p.join("platform-tools").join("adb").exists() && !p.join("adb").exists() {
                return Err(format!(
                    "ANDROID_HOME does not point to a valid Android SDK (missing adb). Path: {}",
                    resolved_android_home
                ));
            }
            args.android_home = Some(resolved_android_home);

            let mut features = Features::<TestContext>::new();
            adb_steps::register_steps(&mut features);
            android_steps::register_steps(&mut features);
            host_steps::register_steps(&mut features);
            netsim_steps::register_steps(&mut features);

            orchestrator::run(args, features).await?;
        }
        Commands::Scenarios { spec_dir, ignore_tags } => {
            let mut features = Features::<TestContext>::new();
            adb_steps::register_steps(&mut features);
            android_steps::register_steps(&mut features);
            host_steps::register_steps(&mut features);
            netsim_steps::register_steps(&mut features);
            orchestrator::list_scenarios(features, spec_dir, Some(ignore_tags)).await?;
        }
    }

    Ok(())
}
