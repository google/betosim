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

use features::{DataTable, World};
use verify_macros::{step, step_module};

use crate::{
    orchestrator::TestContext,
    types::{ClientParams, Throughput},
};

pub struct NetsimWorld;

impl NetsimWorld {
    pub fn new() -> Self {
        Self
    }

    pub async fn run_client(
        &mut self,
        _params: ClientParams,
    ) -> Result<Option<Throughput>, String> {
        Ok(None)
    }

    pub fn get_label(&self) -> String {
        "netsim".to_string()
    }

    pub fn set_silent(&self, _s: bool) {}
}

#[step_module]
pub mod steps {
    use super::*;

    const DEFAULT_POSITION_DELTA: f32 = 0.001;

    fn evaluate_condition(key: &str, actual: &str, expected: &str) -> Result<(), String> {
        if expected == "*" {
            return Ok(());
        }
        if expected.starts_with(">=") {
            let exp_val =
                expected[2..].parse::<f64>().map_err(|e| format!("Parse expected value: {}", e))?;
            let act_val =
                actual.parse::<f64>().map_err(|e| format!("Parse actual value: {}", e))?;
            if !(act_val >= exp_val) {
                return Err(format!("Expected '{}' >= {}, but got {}", key, exp_val, act_val));
            }
        } else if expected.starts_with(">") {
            let exp_val =
                expected[1..].parse::<f64>().map_err(|e| format!("Parse expected value: {}", e))?;
            let act_val =
                actual.parse::<f64>().map_err(|e| format!("Parse actual value: {}", e))?;
            if !(act_val > exp_val) {
                return Err(format!("Expected '{}' > {}, but got {}", key, exp_val, act_val));
            }
        } else if expected.starts_with("<=") {
            let exp_val =
                expected[2..].parse::<f64>().map_err(|e| format!("Parse expected value: {}", e))?;
            let act_val =
                actual.parse::<f64>().map_err(|e| format!("Parse actual value: {}", e))?;
            if !(act_val <= exp_val) {
                return Err(format!("Expected '{}' <= {}, but got {}", key, exp_val, act_val));
            }
        } else if expected.starts_with("<") {
            let exp_val =
                expected[1..].parse::<f64>().map_err(|e| format!("Parse expected value: {}", e))?;
            let act_val =
                actual.parse::<f64>().map_err(|e| format!("Parse actual value: {}", e))?;
            if !(act_val < exp_val) {
                return Err(format!("Expected '{}' < {}, but got {}", key, exp_val, act_val));
            }
        } else {
            if actual.trim() != expected.trim() {
                return Err(format!(
                    "Expected '{}' to be '{}', but got '{}'",
                    key, expected, actual
                ));
            }
        }
        Ok(())
    }

    #[step("@netsim is running")]
    async fn netsim_running(w: &mut TestContext) -> Result<(), String> {
        w.log_step("@netsim", "GIVEN", "Is running");
        Ok(())
    }

    #[step(r#"@netsim moves @(\S+) to ([\d\.]+), ([\d\.]+), ([\d\.]+)"#)]
    async fn netsim_move(
        w: &mut TestContext,
        actor: String,
        x: f32,
        y: f32,
        z: f32,
    ) -> Result<(), String> {
        let resolved_actor = w.resolve_placeholders(&format!("@{}", actor));
        let netsim_device = w
            .map_actor_to_netsim(&resolved_actor)
            .map_err(|e| format!("got netsim device name: {}", e))?;

        w.log_step(
            "@netsim",
            "->",
            &format!("Moves {} ({}) to {}, {}, {}", resolved_actor, netsim_device, x, y, z),
        );
        if w.is_dry_run {
            return Ok(());
        }

        w.run_netsim_command(&[
            "move",
            &netsim_device,
            &x.to_string(),
            &y.to_string(),
            &z.to_string(),
        ])?;
        Ok(())
    }

    /*
    #[step(r#"Netsim creates Wi-Fi Access Point "([^"]+)" with protocol "([^"]+)""#)]
    async fn host_creates_ap(
        w: &mut TestContext,
        ssid: String,
        protocol: String,
    ) -> Result<(), String> {
        w.log_step("@netsim", "->", &format!("Creates AP '{}' with protocol '{}'", ssid, protocol));

        if w.is_dry_run {
            return Ok(());
        }
        let client = w.get_or_create_ap_client().map_err(|e| format!("got ap client: {}", e))?;
        let mut ap = access_point::AccessPoint::new();
        ap.ssid = ssid;
        ap.hw_mode = protocol;
        let mut req = access_point::CreateAccessPointRequest::new();
        req.access_point = MessageField::some(ap);
        client.create(&req).map_err(|e| format!("created AP: {}", e))?;
        Ok(())
    }

    #[step(
        r#"Netsim creates Wi-Fi Access Point "([^"]+)" with protocol "([^"]+)" and password "([^"]+)""#
    )]
    async fn host_creates_secured_ap(
        w: &mut TestContext,
        ssid: String,
        protocol: String,
        password: String,
    ) -> Result<(), String> {
        w.log_step(
            "@netsim",
            "->",
            &format!("Creates Secured AP '{}' with protocol '{}'", ssid, protocol),
        );

        if w.is_dry_run {
            return Ok(());
        }
        let client = w.get_or_create_ap_client().map_err(|e| format!("got ap client: {}", e))?;
        let mut ap = access_point::AccessPoint::new();
        ap.ssid = ssid;
        ap.hw_mode = protocol;
        ap.wpa_passphrase = password;
        let mut req = access_point::CreateAccessPointRequest::new();
        req.access_point = MessageField::some(ap);
        client.create(&req).map_err(|e| format!("created secured AP: {}", e))?;
        Ok(())
    }

    #[step(r#"Wi-Fi Access Point "([^"]+)" in netsim has protocol "([^"]+)""#)]
    async fn verify_ap_protocol(
        w: &mut TestContext,
        ssid: String,
        protocol: String,
    ) -> Result<(), String> {
        w.log_step("@netsim", "THEN", &format!("AP '{}' has protocol '{}'", ssid, protocol));

        if w.is_dry_run {
            return Ok(());
        }

        let client = w.get_or_create_ap_client().map_err(|e| format!("got ap client: {}", e))?;
        let req = access_point::ListAccessPointsRequest::new();
        let resp = client.list(&req).map_err(|e| format!("listed APs: {}", e))?;

        let found = resp.access_points.iter().any(|ap| ap.ssid == ssid && ap.hw_mode == protocol);

        if !found {
            return Err(format!("AP with SSID '{}' and protocol '{}' not found", ssid, protocol));
        }
        Ok(())
    }

    #[step(r#"Netsim removes Wi-Fi Access Point "([^"]+)""#)]
    async fn host_removes_ap(w: &mut TestContext, ssid: String) -> Result<(), String> {
        w.log_step("@netsim", "->", &format!("Removes AP '{}'", ssid));

        if w.is_dry_run {
            return Ok(());
        }
        let client = w.get_or_create_ap_client().map_err(|e| format!("got ap client: {}", e))?;
        let req = access_point::ListAccessPointsRequest::new();
        let resp = client.list(&req).map_err(|e| format!("listed APs: {}", e))?;

        let ap = resp
            .access_points
            .iter()
            .find(|ap| ap.ssid == ssid)
            .ok_or_else(|| format!("AP with SSID '{}' not found", ssid))?;

        let mut del_req = access_point::DeleteAccessPointRequest::new();
        del_req.id = ap.id;
        client.delete(&del_req).map_err(|e| format!("deleted AP: {}", e))?;
        Ok(())
    }
    */

    #[step(r#"@netsim observes "([^"]+)" should be "([^"]+)""#)]
    async fn then_netsim_observes_is(
        w: &mut TestContext,
        key: String,
        expected_value: String,
    ) -> Result<(), String> {
        w.log_step(
            "@netsim",
            "THEN",
            &format!("observes \"{}\" should be \"{}\"", key, expected_value),
        );
        w.fetch_observables().await?;
        let actual_value =
            w.variables.get(&key).ok_or_else(|| format!("Observable '{}' not found", key))?;
        evaluate_condition(&key, actual_value, &expected_value)?;
        Ok(())
    }

    #[step(r#"@netsim observes:?"#)]
    async fn then_netsim_observes_table(
        w: &mut TestContext,
        table: DataTable,
    ) -> Result<(), String> {
        w.log_step("@netsim", "THEN", "observes multiple fields");
        w.fetch_observables().await?;
        let expected_fields = features::vertical_table_to_map(&table);

        for (key, expected_value) in expected_fields {
            w.log_step("@netsim", "INFO", &format!("  | {} | {} |", key, expected_value));
            let actual_value =
                w.variables.get(&key).ok_or_else(|| format!("Observable '{}' not found", key))?;
            evaluate_condition(&key, actual_value, &expected_value)?;
        }
        Ok(())
    }

    /*
    #[step(r#"Netsim creates BLE Beacon "([^"]+)" at ([\d\.]+), ([\d\.]+), ([\d\.]+)"#)]
    async fn host_creates_beacon(
        w: &mut TestContext,
        name: String,
        x: f32,
        y: f32,
        z: f32,
    ) -> Result<(), String> {
        host_creates_beacon_with_address(w, name, "".to_string(), x, y, z).await
    }

    #[step(r#"Netsim creates BLE Beacon "([^"]+)" with address "([^"]+)" at ([\d\.]+), ([\d\.]+), ([\d\.]+)"#)]
    async fn host_creates_beacon_with_address(
        w: &mut TestContext,
        name: String,
        address: String,
        x: f32,
        y: f32,
        z: f32,
    ) -> Result<(), String> {
        w.log_step(
            "@netsim",
            "->",
            &format!("Creates BLE Beacon '{}' at {}, {}, {}", name, x, y, z),
        );

        if w.is_dry_run {
            return Ok(());
        }

        let client =
            w.get_or_create_grpc_client().map_err(|e| format!("got grpc client: {}", e))?;

        let mut pos = netsim_proto::model::Position::new();
        pos.x = x;
        pos.y = y;
        pos.z = z;

        let mut beacon_create = netsim_proto::model::chip_create::BleBeaconCreate::new();
        beacon_create.address = address.clone();

        let mut chip_create = netsim_proto::model::ChipCreate::new();
        chip_create.kind = protobuf::EnumOrUnknown::new(netsim_proto::common::ChipKind::BLUETOOTH);
        chip_create.name = name.clone();
        chip_create.address = address;
        chip_create.set_ble_beacon(beacon_create);

        let mut dev_create = netsim_proto::model::DeviceCreate::new();
        dev_create.name = name;
        dev_create.position = MessageField::some(pos);
        dev_create.chips.push(chip_create);

        let mut req = frontend::CreateDeviceRequest::new();
        req.device = MessageField::some(dev_create);

        client.create_device(&req).map_err(|e| format!("created beacon device: {}", e))?;
        Ok(())
    }
    */

    async fn verify_device_position_impl(
        w: &mut TestContext,
        actor: String,
        x: f32,
        y: f32,
        z: f32,
        delta: f32,
    ) -> Result<(), String> {
        let resolved_actor = w.resolve_placeholders(&format!("@{}", actor));
        let netsim_device = w
            .map_actor_to_netsim(&resolved_actor)
            .map_err(|e| format!("got netsim device name: {}", e))?;

        w.log_step(
            "@netsim",
            "THEN",
            &format!("Device '{}' ({}) is at {}, {}, {}", resolved_actor, netsim_device, x, y, z),
        );

        if w.is_dry_run {
            return Ok(());
        }

        let output = w.run_netsim_command(&["devices", "--json"])?;
        let json: serde_json::Value = serde_json::from_slice(&output.stdout)
            .map_err(|e| format!("Failed to parse netsim devices JSON: {}", e))?;

        let devices = json["devices"].as_array().ok_or("Invalid JSON: missing devices array")?;
        let device = devices
            .iter()
            .find(|d| d["name"].as_str() == Some(&netsim_device))
            .ok_or_else(|| format!("Device '{}' not found in netsim devices", netsim_device))?;

        let pos = device.get("pose").and_then(|p| p.get("position")).unwrap_or(&device["position"]);
        let act_x = pos["x"].as_f64().unwrap_or(0.0) as f32;
        let act_y = pos["y"].as_f64().unwrap_or(0.0) as f32;
        let act_z = pos["z"].as_f64().unwrap_or(0.0) as f32;

        if (act_x - x).abs() > delta || (act_y - y).abs() > delta || (act_z - z).abs() > delta {
            return Err(format!(
                "Device '{}' position mismatch. Expected: {}, {}, {}. Actual: {}, {}, {}",
                netsim_device, x, y, z, act_x, act_y, act_z
            ));
        }
        Ok(())
    }

    #[step(r#"Device "([^"]+)" in netsim is at ([\d\.]+), ([\d\.]+), ([\d\.]+)"#)]
    async fn verify_device_position(
        w: &mut TestContext,
        actor: String,
        x: f32,
        y: f32,
        z: f32,
    ) -> Result<(), String> {
        verify_device_position_impl(w, actor, x, y, z, DEFAULT_POSITION_DELTA).await
    }

    #[step(
        r#"Device "([^"]+)" in netsim is at ([\d\.]+), ([\d\.]+), ([\d\.]+) with delta ([\d\.]+)"#
    )]
    async fn verify_device_position_with_delta(
        w: &mut TestContext,
        actor: String,
        x: f32,
        y: f32,
        z: f32,
        delta: f32,
    ) -> Result<(), String> {
        verify_device_position_impl(w, actor, x, y, z, delta).await
    }

    /*
    #[step(r#"Wi-Fi Access Point "([^"]+)" exists in netsim"#)]
    async fn verify_ap_exists(w: &mut TestContext, ssid: String) -> Result<(), String> {
        w.log_step("@netsim", "THEN", &format!("AP '{}' exists", ssid));

        if w.is_dry_run {
            return Ok(());
        }

        let client = w.get_or_create_ap_client().map_err(|e| format!("got ap client: {}", e))?;
        let req = access_point::ListAccessPointsRequest::new();
        let resp = client.list(&req).map_err(|e| format!("listed APs: {}", e))?;

        let found = resp.access_points.iter().any(|ap| ap.ssid == ssid);

        if !found {
            return Err(format!("AP with SSID '{}' not found", ssid));
        }
        Ok(())
    }

    #[step(r#"Netsim creates BLE Beacon "([^"]+)" with Tx Power "([^"]+)" at ([\d\.]+), ([\d\.]+), ([\d\.]+)"#)]
    async fn host_creates_beacon_with_tx_power(
        w: &mut TestContext,
        name: String,
        tx_power: String,
        x: f32,
        y: f32,
        z: f32,
    ) -> Result<(), String> {
        w.log_step(
            "@netsim",
            "->",
            &format!(
                "Creates BLE Beacon '{}' with Tx Power '{}' at {}, {}, {}",
                name, tx_power, x, y, z
            ),
        );

        if w.is_dry_run {
            return Ok(());
        }

        let client =
            w.get_or_create_grpc_client().map_err(|e| format!("got grpc client: {}", e))?;

        let mut pos = netsim_proto::model::Position::new();
        pos.x = x;
        pos.y = y;
        pos.z = z;

        let power_level = match tx_power.to_lowercase().as_str() {
            "ultra_low" | "ultralow" => netsim_proto::model::chip::ble_beacon::advertise_settings::AdvertiseTxPower::ULTRA_LOW,
            "low" => netsim_proto::model::chip::ble_beacon::advertise_settings::AdvertiseTxPower::LOW,
            "medium" => netsim_proto::model::chip::ble_beacon::advertise_settings::AdvertiseTxPower::MEDIUM,
            "high" => netsim_proto::model::chip::ble_beacon::advertise_settings::AdvertiseTxPower::HIGH,
            _ => return Err(format!("Unknown Tx Power level: {}", tx_power)),
        };

        let mut settings = netsim_proto::model::chip::ble_beacon::AdvertiseSettings::new();
        settings.set_tx_power_level(power_level);

        let mut beacon_create = netsim_proto::model::chip_create::BleBeaconCreate::new();
        beacon_create.settings = MessageField::some(settings);

        let mut chip_create = netsim_proto::model::ChipCreate::new();
        chip_create.kind = protobuf::EnumOrUnknown::new(netsim_proto::common::ChipKind::BLUETOOTH);
        chip_create.name = name.clone();
        chip_create.set_ble_beacon(beacon_create);

        let mut dev_create = netsim_proto::model::DeviceCreate::new();
        dev_create.name = name;
        dev_create.position = MessageField::some(pos);
        dev_create.chips.push(chip_create);

        let mut req = frontend::CreateDeviceRequest::new();
        req.device = MessageField::some(dev_create);

        client.create_device(&req).map_err(|e| format!("created beacon device: {}", e))?;
        Ok(())
    }
    */

    #[step(r#"Netsim version is "([^"]+)""#)]
    async fn verify_netsim_version(
        w: &mut TestContext,
        expected_version: String,
    ) -> Result<(), String> {
        w.log_step("@netsim", "THEN", &format!("Version is '{}'", expected_version));

        if w.is_dry_run {
            return Ok(());
        }

        let output = w.run_netsim_command(&["version"])?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        let version = stdout.trim_start_matches("Netsim version: ").trim();

        if version != expected_version {
            return Err(format!(
                "Version mismatch. Expected: {}. Actual: {}",
                expected_version, version
            ));
        }
        Ok(())
    }

    #[step(r#"Netsim has at least (\d+) devices"#)]
    async fn verify_netsim_device_count(
        w: &mut TestContext,
        expected_count: usize,
    ) -> Result<(), String> {
        w.log_step("@netsim", "THEN", &format!("Has at least {} devices", expected_count));

        if w.is_dry_run {
            return Ok(());
        }

        let output = w.run_netsim_command(&["devices", "--json"])?;
        let json: serde_json::Value = serde_json::from_slice(&output.stdout)
            .map_err(|e| format!("Failed to parse netsim devices JSON: {}", e))?;

        let devices = json["devices"].as_array().map(|a| a.len()).unwrap_or(0);

        if devices < expected_count {
            return Err(format!(
                "Device count too low. Expected at least: {}. Actual: {}",
                expected_count, devices
            ));
        }
        Ok(())
    }

    /*
    #[step(r#"Wi-Fi Access Point "([^"]+)" does not exist in netsim"#)]
    async fn verify_ap_does_not_exist(w: &mut TestContext, ssid: String) -> Result<(), String> {
        w.log_step("@netsim", "THEN", &format!("AP '{}' does not exist", ssid));

        if w.is_dry_run {
            return Ok(());
        }

        let client = w.get_or_create_ap_client().map_err(|e| format!("got ap client: {}", e))?;
        let req = access_point::ListAccessPointsRequest::new();
        let resp = client.list(&req).map_err(|e| format!("listed APs: {}", e))?;

        let found = resp.access_points.iter().any(|ap| ap.ssid == ssid);

        if found {
            return Err(format!("AP with SSID '{}' found, but expected not to exist", ssid));
        }
        Ok(())
    }

    // TODO: Add step `Device "{name}" in netsim has Tx Power "{tx_power}"`
    // once the Netsim backend populates `ble_beacon` field in `ListDevice`
    // response.
    */
}

pub use steps::register_steps;
