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

//! # Netsim Test Orchestrator
//!
//! This module implements the central Control Plane for `verify`. It manages
//! the lifecycle of test actors (Guest Emulators and the Host), handles
//! ADB-based device discovery, and coordinates the execution of HDD-style
//! scenarios.

// Modules are now declared in main.rs (siblings)

pub use std::collections::{HashMap, HashSet};

use clap::Args;

use crate::{
    adb_steps::AdbWorld,
    android_steps::{AndroidDevice, AndroidWorld},
    host_steps::HostWorld,
    netsim_steps::NetsimWorld,
    scenarios,
    types::{ClientParams, LABEL_WIDTH, Throughput},
};

/// Standard arguments for running the test suite.
#[derive(Args, Debug, Clone)]
pub struct RunArgs {
    #[arg(long, help = "Path to the Android SDK root, platform-tools, or adb binary")]
    pub android_home: Option<String>,
    #[arg(long, help = "Path to the vbs APK")]
    pub apk_path: Option<String>,
    #[arg(long, help = "Path to netsimd binary")]
    pub netsim_path: Option<String>,
    #[arg(long, help = "Arguments to pass to netsim")]
    pub netsim_args: Option<String>,
    #[arg(long, help = "Path to netsim CLI binary")]
    pub netsim_cli_path: Option<String>,
    #[arg(long, help = "Gateway IP to connect to (defaults to 10.0.2.2)")]
    pub gateway_ip: Option<String>,
    #[arg(long, help = "Optional scenario filter (matches feature file name)")]
    pub filter: Option<String>,
    #[arg(
        long,
        help = "Package name of the guest agent",
        default_value = "com.android.verify.vbs"
    )]
    pub guest_package: String,
    #[arg(
        long,
        help = "Instrumentation class of the guest agent",
        default_value = "com.android.verify.vbs/com.android.verify.vbs.VbsInstrumentation"
    )]
    pub guest_instrumentation: String,
    #[arg(long, help = "Simulation mode (no-op for orchestrator logic verification)")]
    #[arg(default_value_t = false)]
    pub dry_run: bool,
    #[arg(long, short, help = "Enable verbose output")]
    pub verbose: bool,
    #[arg(long, help = "Directory containing feature files (specs)")]
    pub spec_dir: Option<String>,
    #[arg(long, help = "Continue running tests after a failure")]
    #[arg(default_value_t = false)]
    pub keep_going: bool,
    #[arg(
        long,
        help = "Comma-separated list of tags to ignore",
        default_value = "nyi,nyt,skip,ignore"
    )]
    pub ignore_tags: Option<String>,
}

pub async fn run(
    args: RunArgs,
    mut features: features::Features<TestContext>,
) -> Result<(), String> {
    let host = HostWorld::new(args.dry_run);
    let adb = AdbWorld::new(
        args.android_home,
        args.apk_path,
        args.netsim_path.clone(),
        args.netsim_args,
        args.guest_package.clone(),
        args.guest_instrumentation.clone(),
    );
    let netsim = NetsimWorld::new();

    let mut ctx = TestContext {
        android: AndroidWorld::new(),
        host,
        adb,
        netsim,
        target_ip: "10.0.2.2".to_string(),
        gateway_ip: args.gateway_ip.unwrap_or_else(|| "10.0.2.2".to_string()),
        filter: args.filter,
        is_dry_run: args.dry_run,
        is_verbose: args.verbose,
        keep_going: args.keep_going,
        variables: HashMap::new(),
        netsim_cli_path: args.netsim_cli_path,
        guest_package: args.guest_package,
        guest_instrumentation: args.guest_instrumentation,
    };
    if let Some(tags) = args.ignore_tags {
        features.ignore_tags_str(&tags);
    }
    scenarios::run_suite(&mut ctx, features, args.spec_dir).await.map_err(|e| e.to_string())?;
    Ok(())
}

pub async fn list_scenarios(
    mut features: features::Features<TestContext>,
    spec_dir: Option<String>,
    ignore_tags: Option<String>,
) -> Result<(), String> {
    if let Some(tags) = ignore_tags {
        features.ignore_tags_str(&tags);
    }
    let mut ctx = TestContext {
        android: AndroidWorld::new(),
        host: HostWorld::new(true),
        adb: AdbWorld::new(
            None,
            None,
            None,
            None,
            "com.android.verify.vbs".to_string(),
            "com.android.verify.vbs/com.android.verify.vbs.VbsInstrumentation".to_string(),
        ),
        netsim: NetsimWorld::new(),
        target_ip: "10.0.2.2".to_string(),
        gateway_ip: "10.0.2.2".to_string(),
        filter: None,
        is_dry_run: true,
        is_verbose: false,
        keep_going: false,
        variables: HashMap::new(),
        netsim_cli_path: None,
        guest_package: "com.android.verify.vbs".to_string(),
        guest_instrumentation: "com.android.verify.vbs/com.android.verify.vbs.VbsInstrumentation"
            .to_string(),
    };
    scenarios::run_suite(&mut ctx, features, spec_dir).await.map_err(|e| e.to_string())?;
    Ok(())
}

/// Global context for the test suite, holding actors and shared
/// configuration.
pub struct TestContext {
    pub android: AndroidWorld,
    pub host: HostWorld,
    pub adb: AdbWorld,
    pub netsim: NetsimWorld,

    pub target_ip: String,
    pub gateway_ip: String,
    pub filter: Option<String>,
    pub is_dry_run: bool,
    pub is_verbose: bool,
    pub keep_going: bool,
    pub variables: HashMap<String, String>,
    pub netsim_cli_path: Option<String>,
    pub guest_package: String,
    pub guest_instrumentation: String,
}

impl features::World for TestContext {
    fn reset(&mut self) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + '_>> {
        Box::pin(async move {
            // 1. Reset all host-side actors and the simulation environment
            let _ = self.reset_actors(false).await;

            // 2. Explicitly trigger Kotlin agent reset for each device
            let keys: Vec<String> = self.android.devices.keys().cloned().collect();
            for key in keys {
                if let Some(agent) = self.android.devices.get_mut(&key) {
                    // "resets world" is defined in LifecycleSteps.kt
                    if let Err(e) = agent.execute_step("resets world", 60).await {
                        eprintln!("WARN: Failed to reset world on {}: {}", key, e);
                    }
                }
            }
        })
    }

    fn fetch_observables(
        &mut self,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), String>> + Send + '_>> {
        Box::pin(async move {
            // 1. Fetch from Kotlin agents (Android side)
            let keys: Vec<String> = self.android.devices.keys().cloned().collect();
            for key in keys {
                if self.is_dry_run {
                    self.variables.insert("mock-feature".to_string(), "42".to_string());
                } else {
                    let android = self
                        .get_android_actor_mut(&key)
                        .ok_or_else(|| format!("Android actor '{}' not found", key))?;
                    let vars = android
                        .execute_step("fetch feature observables", 60)
                        .await
                        .map_err(|e| e.to_string())?;
                    self.variables.extend(vars);
                }
            }

            // 2. Fetch from Netsim (Host side)
            if self.is_dry_run {
                self.variables.insert("connected-devices".to_string(), "1".to_string());
                self.variables.insert("netsim-version".to_string(), "0.0.1-mock".to_string());
            } else {
                // Fetch devices
                let devices_output = self.run_netsim_command(&["devices", "--json"])?;
                let devices_json: serde_json::Value =
                    serde_json::from_slice(&devices_output.stdout)
                        .map_err(|e| format!("Failed to parse netsim devices JSON: {}", e))?;
                let devices_count =
                    devices_json["devices"].as_array().map(|a| a.len()).unwrap_or(0);
                self.variables.insert("connected-devices".to_string(), devices_count.to_string());

                // Fetch version
                let version_output = self.run_netsim_command(&["version"])?;
                let version_str = String::from_utf8_lossy(&version_output.stdout);
                let version = version_str.trim_start_matches("Netsim version: ").trim().to_string();
                self.variables.insert("netsim-version".to_string(), version);
            }

            Ok(())
        })
    }
}

impl TestContext {
    pub fn run_netsim_command(&self, args: &[&str]) -> Result<std::process::Output, String> {
        let mut cmd = if let Some(path) = &self.netsim_cli_path {
            std::process::Command::new(path)
        } else {
            std::process::Command::new("netsim")
        };
        cmd.args(args);
        let output = cmd.output().map_err(|e| format!("Failed to run netsim command: {}", e))?;
        if !output.status.success() {
            return Err(format!(
                "netsim command failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }
        Ok(output)
    }
    /// Maps an orchestrator actor label (e.g., @avd:1 or @Beacon1) to a netsim
    /// device name.
    ///
    /// For Android devices, it queries the AVD name via ADB.
    /// For built-in devices like beacons, it queries the Netsim device list via
    /// gRPC to find a matching name.
    pub fn map_actor_to_netsim(&mut self, actor: &str) -> Result<String, String> {
        // 1. Check if it's an Android VBS
        if let Some(android) = self.android.get(actor) {
            let adb_path = &android.adb_path;
            let serial = android.serial.as_ref().ok_or_else(|| {
                "Android device must have a serial for netsim mapping".to_string()
            })?;

            let mut cmd = std::process::Command::new(adb_path);
            cmd.arg("-s").arg(serial).arg("shell").arg("getprop").arg("ro.boot.qemu.avd_name");

            let output = cmd.output().map_err(|e| e.to_string())?;
            if output.status.success() {
                let avd_name = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if !avd_name.is_empty() {
                    // Netsim replaces underscores with spaces in AVD names
                    return Ok(avd_name.replace('_', " "));
                }
            }
        }

        // 2. For non-Android actors, fallback to returning the name directly when gRPC
        //    is disabled.
        // TODO(b/508335216): Refactor to use CLI state queries when supported.
        // This assumes the actor name matches the netsim device name.
        let stripped_actor = actor.strip_prefix('@').unwrap_or(actor);
        Ok(stripped_actor.to_string())
    }

    // Helper to resolve generic actor lookups for the engine
    // Since we removed TestActor enum, we need to dispatch manually if needed
    // But for now, most steps are specific to an actor type.

    // Legacy support for dry-run resolution (mostly used for label resolving)

    pub fn get_android_actor(&self, key: &str) -> Option<&AndroidDevice> {
        self.android.get(key)
    }

    pub fn get_android_actor_mut(&mut self, key: &str) -> Option<&mut AndroidDevice> {
        self.android.get_mut(key)
    }

    pub fn get_all_serials(&self) -> HashSet<String> {
        self.android.get_all_serials()
    }

    pub fn register_android_actor(&mut self, agent: AndroidDevice) {
        self.android.register(agent);
    }

    pub fn set_variable(&mut self, key: &str, value: String) {
        self.variables.insert(key.to_string(), value);
    }

    /// Execute a network client task on a specific actor.
    pub async fn run_client_check(
        &mut self,
        actor: &str,
        mut params: ClientParams,
    ) -> Result<Option<Throughput>, String> {
        if self.is_dry_run {
            return Ok(None);
        }
        params.target = self.resolve_placeholders(&params.target);

        if actor == "@host" {
            return self.host.run_client(params).await.map_err(|e| e.to_string());
        } else if actor == "@adb" {
            return self.adb.run_client(params).await.map_err(|e| e.to_string());
        } else if actor == "@netsim" {
            return self.netsim.run_client(params).await.map_err(|e| e.to_string());
        }

        if let Some(agent) = self.get_android_actor_mut(actor) {
            Ok(agent.run_client(params).await.map_err(|e| e.to_string())?)
        } else {
            Err(format!("Unknown actor: {}", actor))
        }
    }

    /// Run a benchmark with multiple samples and log iperf3-style output.
    pub async fn run_benchmark(
        &mut self,
        actor: &str,
        mut params: ClientParams,
        samples: usize,
    ) -> Result<(), String> {
        // Resolve target before cloning
        params.target = self.resolve_placeholders(&params.target);
        if self.is_dry_run {
            return Ok(());
        }

        self.log_info(actor, "[ ID] Interval           Transfer     Bitrate");
        let mut total_bytes = 0;
        let mut total_duration = std::time::Duration::from_secs(0);

        // Scope for mutable borrow of actor
        {
            {
                if actor == "@host" {
                    self.host.set_silent(true);
                } else if actor == "@adb" {
                    self.adb.set_silent(true);
                } else if actor == "@netsim" {
                    self.netsim.set_silent(true);
                } else {
                    let agent = self
                        .get_android_actor_mut(actor)
                        .ok_or_else(|| format!("Unknown actor: {}", actor))?;
                    agent.set_silent(true);
                }
            }
        }

        for i in 0..samples {
            let start_time = total_duration.as_secs_f64();
            let params_clone = ClientParams {
                payload_size: params.payload_size,
                proto: params.proto.clone(),
                target: params.target.clone(),
                expect_eof: params.expect_eof,
                timeout_ms: params.timeout_ms,
            };

            // run_client_check handles borrow internally
            if let Some(tp) = self.run_client_check(actor, params_clone).await? {
                total_bytes += tp.bytes;
                total_duration += tp.duration;
                self.log_iperf_line(actor, tp, start_time);
            }
            if i < samples - 1 {
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
        }

        {
            {
                if actor == "@host" {
                    self.host.set_silent(false);
                } else if actor == "@adb" {
                    self.adb.set_silent(false);
                } else if actor == "@netsim" {
                    self.netsim.set_silent(false);
                } else {
                    let agent = self
                        .get_android_actor_mut(actor)
                        .ok_or_else(|| format!("Unknown actor: {}", actor))?;
                    agent.set_silent(false);
                }
            }
        }

        // Summary Line
        let tag = self.actor_tag(actor);
        eprintln!("    {:<6} {} - - - - - - - - - - - - - - - - - - - - - - - - -", "INFO", tag);
        self.log_iperf_line(
            actor,
            Throughput { bytes: total_bytes, duration: total_duration },
            0.0,
        );
        Ok(())
    }

    pub async fn reset_actors(&mut self, hard: bool) -> Result<(), String> {
        if self.is_dry_run {
            return Ok(());
        }

        self.host.reset_actor().await.map_err(|e| e.to_string())?;
        self.adb.reset_actor().await.map_err(|e| e.to_string())?;

        // Reset netsim via CLI
        self.run_netsim_command(&["reset"])?;

        for agent in self.android.devices.values_mut() {
            agent.reset_actor(hard).await.map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    /// Centralized logging for HDD steps and telemetry.
    pub fn log_step(&self, actor: &str, verb: &str, msg: &str) {
        if self.is_verbose {
            let tag = self.actor_tag(actor);
            let prefix = if verb == "INFO" { "INFO" } else { "->" };
            let resolved = self.resolve_placeholders(msg);
            let capitalized = if let Some(first) = resolved.chars().next() {
                format!("{}{}", first.to_uppercase(), &resolved[first.len_utf8()..])
            } else {
                resolved
            };
            eprintln!("    {:<6} {} {}", prefix, tag, capitalized);
        }
    }

    /// Logs observational runtime information (telemetry, progress).
    pub fn log_info(&self, actor: &str, msg: &str) {
        if self.is_verbose {
            let tag = self.actor_tag(actor);
            eprintln!("INFO   {:<20} {}", tag, msg);
        }
    }

    /// Resolves the human-readable tag for an actor (e.g., "@emulator-5554").
    fn actor_tag(&self, actor: &str) -> String {
        let mut label = if actor == "@host" {
            self.host.get_label()
        } else if actor == "@adb" {
            self.adb.get_label()
        } else if actor == "@netsim" {
            self.netsim.get_label()
        } else if let Some(agent) = self.get_android_actor(actor) {
            agent.get_label()
        } else {
            // In dry run, actor might be "@avd:1"
            // If we mocked devices, get_android_actor would return Some.
            // If we didn't, we fall back here.
            actor.to_string()
        };
        if !label.starts_with('@') {
            label = format!("@{}", label);
        }
        format!("{:width$}", label, width = LABEL_WIDTH)
    }

    fn log_iperf_line(&self, actor: &str, tp: Throughput, start: f64) {
        let end = start + tp.duration.as_secs_f64();
        let bits_sec = (tp.bytes as f64 * 8.0) / tp.duration.as_secs_f64();

        let (trans, t_unit) = if tp.bytes >= 1024 * 1024 {
            (tp.bytes as f64 / 1048576.0, "MBytes")
        } else if tp.bytes >= 1024 {
            (tp.bytes as f64 / 1024.0, "KBytes")
        } else {
            (tp.bytes as f64, "Bytes")
        };

        let (rate, r_unit) = if bits_sec >= 1_000_000.0 {
            (bits_sec / 1_000_000.0, "Mbits/sec")
        } else if bits_sec >= 1_000.0 {
            (bits_sec / 1_000.0, "Kbits/sec")
        } else {
            (bits_sec, "bits/sec")
        };

        println!(
            "    {:<6} {} [  5] {:5.2}-{:5.2} sec  {:6.2} {:6}  {:6.2} {}",
            "INFO",
            self.actor_tag(actor),
            start,
            end,
            trans,
            t_unit,
            rate,
            r_unit
        );
    }

    /// Resolves HDD placeholders (e.g., {port}, {target}) to runtime values.
    pub fn resolve_placeholders(&self, s: &str) -> String {
        let mut res = s.to_string();

        // Resolve variables from HashMap
        for (key, val) in &self.variables {
            res = res.replace(&format!("{{{}}}", key), val);
        }

        // Helpers
        if let Some(port) = self.variables.get("port") {
            let target = if port.contains(':') {
                port.clone()
            } else {
                format!("{}:{}", self.target_ip, port)
            };
            res = res.replace("{target}", &target);
        }

        let target = format!("{}:0", self.target_ip); // Default target if port unknown
        res = res.replace("{target}", &target);

        // Resolve @avd:N placeholders
        for (i, key) in self.android.avd_keys.iter().enumerate() {
            let n = i + 1;
            let tag = format!("@avd:{}", n);
            let label = self.android.devices.get(key).map(|e| e.get_label()).unwrap_or(key.clone());
            res = res.replace(&tag, &format!("@{}", label));
        }

        // Fallback for @avd
        if !self.android.avd_keys.is_empty() {
            let label = self
                .android
                .devices
                .get(&self.android.avd_keys[0])
                .map(|e| e.get_label())
                .unwrap_or("@avd:1".to_string());
            res = res.replace("@avd", &format!("@{}", label));
        }

        res
    }
}
