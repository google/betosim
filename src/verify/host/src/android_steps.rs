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
    collections::{HashMap, HashSet},
    process::{Child, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use futures::StreamExt;
use serde::{Deserialize, Serialize};
use tokio::io::AsyncWriteExt;
use tokio_util::codec::FramedRead;
use verify_macros::{step, step_module};

use crate::{
    orchestrator::TestContext,
    types::{ClientParams, LABEL_WIDTH, Throughput},
};

// Protocol markers for Guest-Host communication (legacy)

#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "type")]
enum RunnerMessage {
    ExecuteStep { id: i32, step: String },
    GetSteps { id: i32 },
    Quit { id: i32 },
    StartScenario { id: i32 },
    StopScenario { id: i32 },
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "type")]
pub enum GuestMessage {
    Response {
        id: i32,
        status: String,
        #[serde(default)]
        error_message: Option<String>,
        #[serde(default)]
        variables: HashMap<String, String>,
    },
    StepsResponse {
        id: i32,
        steps: Vec<String>,
    },
    Event {
        event_type: String,
        message: String,
    },
}

pub struct AndroidWorld {
    pub devices: HashMap<String, AndroidDevice>,
    pub avd_keys: Vec<String>,
}

impl AndroidWorld {
    pub fn new() -> Self {
        Self { devices: HashMap::new(), avd_keys: Vec::new() }
    }

    pub fn register(&mut self, agent: AndroidDevice) {
        let idx = self.avd_keys.len() + 1;
        let key = format!("@avd:{}", idx);
        self.devices.insert(key.clone(), agent);
        self.avd_keys.push(key);
    }

    pub fn get(&self, key: &str) -> Option<&AndroidDevice> {
        let k = self.resolve_key(key)?;
        self.devices.get(&k)
    }

    pub fn get_mut(&mut self, key: &str) -> Option<&mut AndroidDevice> {
        let k = self.resolve_key(key)?;
        self.devices.get_mut(&k)
    }

    pub fn get_all_serials(&self) -> HashSet<String> {
        self.devices.values().filter_map(|a| a.get_serial()).collect()
    }

    pub fn resolve_key(&self, actor: &str) -> Option<String> {
        if self.devices.contains_key(actor) {
            return Some(actor.to_string());
        }
        // Aliases
        if actor == "@avd" || actor == "@avd:1" || actor == "@android" || actor == "@android:1" {
            if let Some(key) = self.avd_keys.first() {
                return Some(key.clone());
            }
            return Some("@avd:1".to_string());
        }

        // Handle @avd:N
        if let Some(idx_str) = actor.strip_prefix("@avd:") {
            if let Ok(idx) = idx_str.parse::<usize>() {
                if idx > 0 && idx <= self.avd_keys.len() {
                    return Some(self.avd_keys[idx - 1].clone());
                }
                return Some(format!("@avd:{}", idx));
            }
        }

        // Handle @android:N
        if let Some(idx_str) = actor.strip_prefix("@android:") {
            if let Ok(idx) = idx_str.parse::<usize>() {
                if idx > 0 && idx <= self.avd_keys.len() {
                    return Some(self.avd_keys[idx - 1].clone());
                }
                return Some(format!("@avd:{}", idx));
            }
        }

        Some(actor.to_string())
    }
}

/// Executor for Android Guest devices using ADB.
pub struct AndroidDevice {
    pub adb_path: String,
    pub serial: Option<String>,
    pub agent_process: Option<Child>,
    pub netsim_process: Option<Child>,
    pub apk_path: Option<String>,
    pub avd_name: Option<String>,
    pub feedback_port: Option<u16>,
    pub feedback_sender: Arc<tokio::sync::Mutex<Option<tokio::net::tcp::OwnedWriteHalf>>>,
    pub feedback_receiver:
        Arc<tokio::sync::Mutex<Option<tokio::sync::mpsc::Receiver<GuestMessage>>>>,
    pub silent: Arc<AtomicBool>,
    pub next_msg_id: i32,
    pub steps: Vec<regex::Regex>,
    pub guest_instrumentation: String,
}

impl AndroidDevice {
    /// Find adb binary in standard locations or provided path.
    pub fn find_adb(provided_path: Option<&str>) -> String {
        if let Some(path) = provided_path {
            let p = std::path::Path::new(path);
            if p.is_file() {
                return path.to_string();
            }
            // Check provided path directly
            let adb = p.join("adb");
            if adb.exists() {
                return adb.to_string_lossy().to_string();
            }
            // Check platform-tools subdirectory (if it's ANDROID_HOME)
            let adb = p.join("platform-tools").join("adb");
            if adb.exists() {
                return adb.to_string_lossy().to_string();
            }
        }
        if let Ok(path) = which::which("adb") {
            return path.to_string_lossy().to_string();
        }
        for var in &["ANDROID_HOME", "ANDROID_SDK_ROOT", "ANDROID_SDK_HOME"] {
            if let Ok(val) = std::env::var(var) {
                let p = std::path::Path::new(&val).join("platform-tools").join("adb");
                if p.exists() {
                    return p.to_string_lossy().to_string();
                }
            }
        }
        "adb".to_string()
    }

    pub fn new(
        serial: Option<String>,
        android_home: String,
        apk_path: Option<String>,
        guest_instrumentation: String,
    ) -> Result<Self, String> {
        Ok(Self {
            adb_path: android_home,
            apk_path,
            serial,
            agent_process: None,
            netsim_process: None,
            avd_name: None,
            feedback_port: None,
            feedback_sender: Arc::new(tokio::sync::Mutex::new(None)),
            feedback_receiver: Arc::new(tokio::sync::Mutex::new(None)),
            silent: Arc::new(AtomicBool::new(false)),
            next_msg_id: 1,
            steps: Vec::new(),
            guest_instrumentation,
        })
    }

    pub fn adb_command(&self) -> std::process::Command {
        let mut cmd = std::process::Command::new(&self.adb_path);
        if let Some(s) = &self.serial {
            cmd.arg("-s").arg(s);
        }
        cmd
    }

    pub fn launch_netsim(
        &mut self,
        bin: &Option<String>,
        args: &Option<String>,
    ) -> Result<(), String> {
        if let Some(bin_path) = bin {
            let mut cmd = std::process::Command::new(bin_path);
            if let Some(a) = args {
                for arg in a.split_whitespace() {
                    cmd.arg(arg);
                }
            }
            self.netsim_process = Some(
                cmd.stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()
                    .map_err(|e| e.to_string())?,
            );
        }
        Ok(())
    }

    pub fn install_apk(&mut self) -> Result<(), String> {
        if let Some(apk) = &self.apk_path {
            let status = self
                .adb_command()
                .arg("install")
                .arg("-r")
                .arg("-g")
                .arg(apk)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .map_err(|e| e.to_string())?;
            if !status.success() {
                return Err(format!("Failed to install APK: {}", apk));
            }
        }
        Ok(())
    }

    pub fn uninstall_apk(&mut self, package: &str) -> Result<(), String> {
        let _ = self
            .adb_command()
            .arg("uninstall")
            .arg(package)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        Ok(())
    }

    /// Set up TCP reverse tunneling for feedback from the Kotlin agent.
    pub async fn setup_feedback(&mut self) -> Result<(), String> {
        let listener =
            tokio::net::TcpListener::bind("0.0.0.0:0").await.map_err(|e| e.to_string())?;
        let port = listener.local_addr().map_err(|e| e.to_string())?.port();
        self.feedback_port = Some(port);
        self.adb_command()
            .arg("reverse")
            .arg(format!("tcp:{}", port))
            .arg(format!("tcp:{}", port))
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map_err(|e| e.to_string())?;

        let name = self.get_label();
        let feedback_sender = self.feedback_sender.clone();
        let silent = self.silent.clone();
        let (tx, rx) = tokio::sync::mpsc::channel(100);
        {
            let mut guard = self.feedback_receiver.lock().await;
            *guard = Some(rx);
        }

        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, _)) => {
                        let name_clone = name.clone();
                        let silent_clone = silent.clone();
                        let tx_clone = tx.clone();
                        let feedback_sender_clone = feedback_sender.clone();

                        tokio::spawn(async move {
                            let (read_half, write_half) = stream.into_split();

                            {
                                let mut guard = feedback_sender_clone.lock().await;
                                *guard = Some(write_half);
                            }

                            let mut framed_read = FramedRead::new(
                                read_half,
                                tokio_util::codec::LengthDelimitedCodec::builder()
                                    .length_field_length(2)
                                    .length_field_type::<u16>()
                                    .new_codec(),
                            );

                            while let Some(frame) = framed_read.next().await {
                                match frame {
                                    Ok(bytes) => {
                                        let json_str = match String::from_utf8(bytes.to_vec()) {
                                            Ok(s) => s,
                                            Err(e) => {
                                                eprintln!("!! ERR: Invalid UTF-8: {}", e);
                                                continue;
                                            }
                                        };

                                        if let Ok(msg) =
                                            serde_json::from_str::<GuestMessage>(&json_str)
                                        {
                                            match msg {
                                                GuestMessage::Event { event_type, message } => {
                                                    if event_type == "Log" {
                                                        if !silent_clone.load(Ordering::SeqCst) {
                                                            Self::print_hdd_line(
                                                                &name_clone,
                                                                &message,
                                                            );
                                                        }
                                                    }
                                                }
                                                GuestMessage::Response { .. }
                                                | GuestMessage::StepsResponse { .. } => {
                                                    let _ = tx_clone.send(msg).await;
                                                }
                                            }
                                        } else {
                                            eprintln!("!! ERR: Failed to parse JSON: {}", json_str);
                                        }
                                    }
                                    Err(e) => {
                                        eprintln!("!! ERR: Failed to read frame: {}", e);
                                        break;
                                    }
                                }
                            }
                        });
                    }
                    Err(e) => {
                        eprintln!("!! ERR: Failed to accept feedback connection: {}", e);
                    }
                }
            }
        });
        Ok(())
    }

    fn print_hdd_line(name: &str, line: &str) {
        let line = line.trim();
        let actor_tag = format!("@{:width$}", name, width = LABEL_WIDTH);
        if line.starts_with("GIVEN")
            || line.starts_with("WHEN")
            || line.starts_with("AND")
            || line.starts_with("THEN")
            || line.starts_with("INFO")
        {
            let mut parts = line.splitn(2, ' ');
            let _verb = parts.next().unwrap_or("");
            let msg = parts.next().unwrap_or("");
            println!("    {:<6} {} {}", "->", actor_tag, msg);
        }
    }

    pub fn launch_agent(&mut self) -> Result<(), String> {
        let name = self.get_label();
        let port_arg =
            self.feedback_port.ok_or_else(|| "Feedback port not set".to_string())?.to_string();
        self.agent_process = Some(
            self.adb_command()
                .arg("shell")
                .arg("am")
                .arg("instrument")
                .arg("-w")
                .arg("-e")
                .arg("wait")
                .arg("true")
                .arg("-e")
                .arg("device_name")
                .arg(&name)
                .arg("-e")
                .arg("control_port")
                .arg(&port_arg)
                .arg(&self.guest_instrumentation)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .map_err(|e| e.to_string())?,
        );
        Ok(())
    }

    pub async fn wait_for_feedback(&self) -> Result<(), String> {
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(60) {
            if self.feedback_sender.lock().await.is_some() {
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        Err("Feedback timeout waiting for VBS to connect (60s)".to_string())
    }

    async fn wait_for_response(
        &self,
        expected_id: i32,
        timeout_secs: u64,
    ) -> Result<GuestMessage, String> {
        let receiver = self.feedback_receiver.clone();
        let mut rx_guard = receiver.lock().await;
        let rx =
            rx_guard.as_mut().ok_or_else(|| "Feedback receiver not initialized".to_string())?;

        let start = Instant::now();
        while start.elapsed().as_secs() < timeout_secs {
            match tokio::time::timeout(Duration::from_millis(100), rx.recv()).await {
                Ok(Some(msg)) => match &msg {
                    GuestMessage::Response { id, .. } if *id == expected_id => return Ok(msg),
                    GuestMessage::StepsResponse { id, .. } if *id == expected_id => return Ok(msg),
                    _ => {}
                },
                Ok(None) => {
                    return Err("Feedback channel closed".to_string());
                }
                Err(_) => {
                    // Timeout, continue loop
                }
            }
        }
        Err(format!("Timeout waiting for response ID: {}", expected_id))
    }
}

impl AndroidDevice {
    async fn send_message(&self, msg: &RunnerMessage) -> Result<(), String> {
        let json_str = serde_json::to_string(msg).map_err(|e| e.to_string())?;
        let bytes = json_str.as_bytes();
        let len = bytes.len() as u16;

        let mut guard = self.feedback_sender.lock().await;
        let stream = guard.as_mut().ok_or_else(|| "Feedback channel disconnected".to_string())?;
        stream.write_all(&len.to_be_bytes()).await.map_err(|e| e.to_string())?;
        stream.write_all(bytes).await.map_err(|e| e.to_string())?;
        stream.flush().await.map_err(|e| e.to_string())?;
        Ok(())
    }

    pub async fn get_steps(&mut self) -> Result<(), String> {
        let id = self.next_msg_id;
        self.next_msg_id += 1;
        self.send_message(&RunnerMessage::GetSteps { id }).await?;
        let response = self.wait_for_response(id, 10).await?;
        if let GuestMessage::StepsResponse { steps, .. } = response {
            self.steps = steps
                .into_iter()
                .filter_map(|s| match regex::Regex::new(&s) {
                    Ok(re) => Some(re),
                    Err(e) => {
                        eprintln!("Invalid regex from guest: '{}' - {}", s, e);
                        None
                    }
                })
                .collect();
            return Ok(());
        }
        Err("Unexpected response to GetSteps".to_string())
    }

    pub async fn execute_step(
        &mut self,
        step: &str,
        timeout_secs: u64,
    ) -> Result<HashMap<String, String>, String> {
        let id = self.next_msg_id;
        self.next_msg_id += 1;
        let cmd = RunnerMessage::ExecuteStep { id, step: step.to_string() };

        self.send_message(&cmd).await?;

        // Wait for the completion signal
        let response = self.wait_for_response(id, timeout_secs).await?;

        if let GuestMessage::Response { status, error_message, variables, .. } = response {
            if status == "Failure" {
                let msg = error_message.unwrap_or_else(|| "Unknown actor-side error".to_string());
                return Err(format!("Step failed on actor: {}", msg));
            }
            return Ok(variables);
        }
        Err("Internal error: did not receive Response message".to_string())
    }

    pub async fn start_scenario(&mut self) -> Result<(), String> {
        let id = self.next_msg_id;
        self.next_msg_id += 1;
        let cmd = RunnerMessage::StartScenario { id };
        self.send_message(&cmd).await?;
        self.wait_for_response(id, 10).await?;
        Ok(())
    }

    pub async fn stop_scenario(&mut self) -> Result<(), String> {
        let id = self.next_msg_id;
        self.next_msg_id += 1;
        let cmd = RunnerMessage::StopScenario { id };
        self.send_message(&cmd).await?;
        self.wait_for_response(id, 10).await?;
        Ok(())
    }
}

impl AndroidDevice {
    pub async fn run_client(&mut self, params: ClientParams) -> Result<Option<Throughput>, String> {
        let step = format!(
            "When Android sends {} bytes of {} data to {}",
            params.payload_size,
            params.proto.to_uppercase(),
            params.target
        );

        let start = Instant::now();
        let timeout_secs = std::cmp::max(60, (params.timeout_ms / 1000) as u64);
        self.execute_step(&step, timeout_secs).await?;
        let duration = start.elapsed();

        Ok(Some(Throughput { bytes: params.payload_size, duration }))
    }

    pub async fn hard_reset(&mut self) -> Result<(), String> {
        let id = self.next_msg_id;
        self.next_msg_id += 1;
        let cmd = RunnerMessage::Quit { id };

        if let Err(e) = self.send_message(&cmd).await {
            eprintln!("WARN Failed to send Quit message during reset: {}", e);
        }

        {
            let mut guard = self.feedback_sender.lock().await;
            *guard = None;
        }

        // Wait for response with timeout 2s (best effort)
        let _ = self.wait_for_response(id, 2).await;

        if let Some(mut child) = self.agent_process.take() {
            let _ = child.wait();
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
        self.launch_agent()?;
        self.wait_for_feedback().await
    }

    pub async fn reset_actor(&mut self, hard: bool) -> Result<(), String> {
        if hard {
            self.hard_reset().await?;
        } else if let Err(e) = self.stop_scenario().await {
            eprintln!("WARN Failed to stop scenario during reset: {}", e);
            // Fallback to hard reset if soft reset fails
            self.hard_reset().await?;
        }
        self.start_scenario().await
    }

    pub fn get_label(&self) -> String {
        self.avd_name.clone().or(self.serial.clone()).unwrap_or_else(|| "Android".to_string())
    }

    pub fn get_serial(&self) -> Option<String> {
        self.serial.clone()
    }

    pub fn set_silent(&self, silent: bool) {
        self.silent.store(silent, Ordering::SeqCst);
    }
}

#[step_module]
pub mod steps {
    use super::*;

    // Helper function
    async fn generic_execution(
        w: &mut TestContext,
        actor: String,
        step: String,
        timeout_secs: u64,
    ) -> Result<(), String> {
        let step = w.resolve_placeholders(&step);
        let vars = {
            let android = w
                .get_android_actor_mut(&actor)
                .ok_or_else(|| "Actor not found or is not an Android Agent".to_string())?;

            if android.steps.is_empty() {
                android
                    .get_steps()
                    .await
                    .map_err(|e| format!("Failed to fetch steps from Android: {}", e))?;
            }

            if !android.steps.iter().any(|re| {
                if let Some(m) = re.find(&step) {
                    m.start() == 0 && m.end() == step.len()
                } else {
                    false
                }
            }) {
                return Err(format!(
                    "Step '{}' does not match any registered step on guest {}",
                    step, actor
                ));
            }

            android
                .execute_step(&step, timeout_secs)
                .await
                .map_err(|e| format!("Android step execution failed: {}", e))?
        };
        for (k, v) in vars {
            let prefixed_key = format!("{}:{}", actor, k);
            w.set_variable(&prefixed_key, v);
        }
        Ok(())
    }

    #[step(r#"(?:@avd|@android)(?::(\S+))? measures performance with (\d+) samples of (\d+)(KB|MB|B) (TCP|UDP) to (.*)"#)]
    async fn performance_benchmark(
        w: &mut TestContext,
        label: String,
        samples: usize,
        size_val: usize,
        unit: String,
        proto: String,
        target: String,
    ) -> Result<(), String> {
        let proto = proto.to_lowercase();
        let payload_size = match unit.as_str() {
            "KB" => size_val * 1024,
            "MB" => size_val * 1024 * 1024,
            "B" => size_val,
            _ => size_val,
        };

        let actor = if label.is_empty() { "@avd:1".to_string() } else { format!("@avd:{}", label) };
        w.run_benchmark(
            &actor,
            ClientParams { proto, target, payload_size, expect_eof: false, timeout_ms: 60000 },
            samples,
        )
        .await
        .map_err(|e| format!("Benchmark failed: {}", e))?;
        Ok(())
    }

    #[step(r#"Android connects to Wi-Fi SSID "([^"]+)""#)]
    async fn android_connects_to_wifi(w: &mut TestContext, ssid: String) -> Result<(), String> {
        let step = format!("When Android connects to Wi-Fi SSID \"{}\"", ssid);
        generic_execution(w, "@avd:1".to_string(), step, 120).await
    }

    #[step(r#"Android connects to Wi-Fi SSID "([^"]+)" with password "([^"]+)""#)]
    async fn android_connects_to_secured_wifi(
        w: &mut TestContext,
        ssid: String,
        password: String,
    ) -> Result<(), String> {
        let step = format!(
            "When Android connects to Wi-Fi SSID \"{}\" with password \"{}\"",
            ssid, password
        );
        generic_execution(w, "@avd:1".to_string(), step, 120).await
    }

    #[step(r#"Android is connected to Wi-Fi SSID "([^"]+)""#)]
    async fn android_is_connected_to_wifi(w: &mut TestContext, ssid: String) -> Result<(), String> {
        let step = format!("Then Android is connected to Wi-Fi SSID \"{}\"", ssid);
        generic_execution(w, "@avd:1".to_string(), step, 60).await
    }

    #[step(r#"Wi-Fi device info shows SSID "([^"]+)""#)]
    async fn wifi_device_info_shows_ssid(w: &mut TestContext, ssid: String) -> Result<(), String> {
        let step = format!("Then Wi-Fi device info shows SSID \"{}\"", ssid);
        generic_execution(w, "@avd:1".to_string(), step, 60).await
    }

    #[step("Android releases Wi-Fi connection")]
    async fn android_releases_wifi_connection(w: &mut TestContext) -> Result<(), String> {
        let step = "When Android releases Wi-Fi connection".to_string();
        generic_execution(w, "@avd:1".to_string(), step, 60).await
    }

    #[step(r#"((?:@avd|@android)(?::\S+)?)\s*observes "([^"]+)" should be "([^"]+)""#)]
    async fn then_observed_feature_should_be(
        w: &mut TestContext,
        actor: String,
        feature: String,
        expected_value: String,
    ) -> Result<(), String> {
        let actor = if actor.starts_with("@avd") && !actor.contains(":") {
            format!("{}:1", actor)
        } else {
            actor
        };
        w.log_step(
            &actor,
            "THEN",
            &format!("observes \"{}\" should be \"{}\"", feature, expected_value),
        );
        let prefixed_key = format!("{}:{}", actor, feature);
        let actual_value = w.variables.get(&prefixed_key).ok_or_else(|| {
            let keys: Vec<&String> = w.variables.keys().collect();
            format!("Feature observable '{}' not found. Available observables: {:?}", feature, keys)
        })?;
        if actual_value != &expected_value {
            return Err(format!(
                "Expected feature '{}' to be '{}', but got '{}'",
                feature, expected_value, actual_value
            ));
        }
        Ok(())
    }

    #[step(r#"((?:@avd|@android)(?::\S+)?)\s*observes "([^"]+)" should be greater than (\d+)"#)]
    async fn then_observed_feature_should_be_greater_than(
        w: &mut TestContext,
        actor: String,
        feature: String,
        expected_min: i64,
    ) -> Result<(), String> {
        let actor = if actor.starts_with("@avd") && !actor.contains(":") {
            format!("{}:1", actor)
        } else {
            actor
        };
        w.log_step(
            &actor,
            "THEN",
            &format!("observes \"{}\" should be greater than {}", feature, expected_min),
        );
        let prefixed_key = format!("{}:{}", actor, feature);
        let actual_value_str = w.variables.get(&prefixed_key).ok_or_else(|| {
            let keys: Vec<&String> = w.variables.keys().collect();
            format!("Feature observable '{}' not found. Available observables: {:?}", feature, keys)
        })?;
        let actual_value = actual_value_str.parse::<i64>().map_err(|e| {
            format!(
                "Failed to parse feature '{}' value '{}' as integer: {}",
                feature, actual_value_str, e
            )
        })?;
        if actual_value <= expected_min {
            return Err(format!(
                "Expected feature '{}' to be greater than {}, but got {}",
                feature, expected_min, actual_value
            ));
        }
        Ok(())
    }

    #[step(r#"(?:@avd|@android)(?::(\S+))? sets Wi-Fi to (enabled|disabled) via UI"#)]
    async fn android_sets_wifi_state(
        w: &mut TestContext,
        label: String,
        state: String,
    ) -> Result<(), String> {
        let actor = if label.is_empty() { "@avd:1".to_string() } else { format!("@avd:{}", label) };
        let step = format!("sets Wi-Fi to {} via UI", state);
        generic_execution(w, actor, step, 60).await
    }

    #[step(r#"(?:@avd|@android)(?::(\S+))? Android Wi-Fi is disabled"#)]
    async fn android_wifi_is_disabled(w: &mut TestContext, label: String) -> Result<(), String> {
        let actor = if label.is_empty() { "@avd:1".to_string() } else { format!("@avd:{}", label) };
        generic_execution(w, actor, "Android Wi-Fi is disabled".to_string(), 60).await
    }

    #[step(r#"(?:@avd|@android)(?::(\S+))? Android Wi-Fi is enabled"#)]
    async fn android_wifi_is_enabled(w: &mut TestContext, label: String) -> Result<(), String> {
        let actor = if label.is_empty() { "@avd:1".to_string() } else { format!("@avd:{}", label) };
        generic_execution(w, actor, "Android Wi-Fi is enabled".to_string(), 60).await
    }

    #[step(r#"(?:@avd|@android)(?::(\S+))? fetch feature observables"#)]
    async fn fetch_observables_step(w: &mut TestContext, label: String) -> Result<(), String> {
        let actor = if label.is_empty() { "@avd:1".to_string() } else { format!("@avd:{}", label) };
        w.log_step(&actor, "->", "fetch feature observables");

        if !w.is_dry_run {
            generic_execution(w, actor, "fetch feature observables".to_string(), 60).await?;
        }

        Ok(())
    }
}

pub use steps::register_steps;
