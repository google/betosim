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

use features::{DataTable, Features, assert_json_matches_table, table_to_struct};
use tracing_subscriber::fmt::try_init;
use verify_macros::{step, step_module};

/// # Example World
///
/// This is an example of a "World" struct that holds the state for Intentions
/// tests. It demonstrates how to use the `features_codegen` macro to
/// automatically generate glue code.
///
/// The `features_codegen` tool (invoked via `genrule` in BUILD) parses this
/// file, finds methods commented with `/// STEP:`, and generates
/// `features_world_glue.rs`.
struct TestWorld {
    /// The main display register (accumulator)
    display: i32,
    log: Vec<String>,
    users: std::collections::HashMap<String, i32>,
    tx_power: TxPower,
    enabled: bool,
    temperature: f64,
    /// The memory register (M+, M-, MR, MC)
    memory: i32,
    /// Mock network interface buffer (stores received packets)
    received_packets: Vec<Vec<u8>>,
}

#[derive(Debug, PartialEq, Clone, Copy)]
enum TxPower {
    High,
    Medium,
    Low,
    Ultralow,
}

impl std::str::FromStr for TxPower {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "High" => Ok(TxPower::High),
            "Medium" => Ok(TxPower::Medium),
            "Low" => Ok(TxPower::Low),
            "Ultralow" => Ok(TxPower::Ultralow),
            _ => Err(format!("Invalid TxPower: {}", s)),
        }
    }
}

// NOTE: Using String for numeric fields because Gherkin tables provide strings,
// and table_to_struct produces mixed types in JSON. Using String avoids the
// need for custom flexible deserializers in this test mock.
#[derive(serde::Deserialize, serde::Serialize, Debug, PartialEq)]
struct MockPacket {
    #[serde(rename = "udp.srcport")]
    src_port: String,
    #[serde(rename = "udp.dstport")]
    dst_port: String,
    #[serde(rename = "udp.length")]
    length: String,
    #[serde(rename = "udp.checksum")]
    checksum: String,
}

impl features::World for TestWorld {}

impl TestWorld {}

#[step_module]
pub mod steps {
    use super::*;

    #[step(r"I reset the counter")]
    async fn given_i_reset_the_counter(w: &mut TestWorld) {
        w.display = 0;
        w.log.push("reset".to_string());
    }

    #[step(r"I have a calculator with Memory (\d+)")]
    async fn given_i_have_a_calculator_with_memory(w: &mut TestWorld, memory: i32) {
        w.memory = memory;
        w.log.push(format!("reset_calculator memory {}", memory));
    }

    #[step(r"I have a calculator with No Memory")]
    async fn given_i_have_a_calculator_with_no_memory(w: &mut TestWorld) {
        w.memory = 0;
        w.log.push("reset_calculator memory 0".to_string());
    }

    #[step(r"I want to ensure a clean state before every scenario")]
    async fn given_clean_state(w: &mut TestWorld) {
        w.display = 0;
        w.log.push("reset_background".to_string());
    }

    #[step(r"I organize my scenarios with tags")]
    async fn given_tags(w: &mut TestWorld) {
        w.log.push("reset_tags".to_string());
    }

    #[step(r"I set TxPower to (High|Medium|Low|Ultralow)")]
    async fn when_i_set_tx_power(w: &mut TestWorld, level: TxPower) {
        w.tx_power = level;
        w.log.push(format!("tx_power {:?}", level));
    }

    #[step(r"I add (\d+)")]
    async fn when_i_add(w: &mut TestWorld, val: i32) {
        w.display += val;
        w.log.push(format!("add {}", val));
    }

    #[step(r"result is (\d+)")]
    async fn then_result_is(w: &mut TestWorld, val: i32) {
        assert_eq!(w.display, val, "Check failed: expected {}, got {}", val, w.display);
        w.log.push(format!("check {}", val));
    }

    #[step(r"system is (true|false)")]
    async fn given_system_is(w: &mut TestWorld, state: bool) {
        w.enabled = state;
        w.log.push(format!("system is {}", state));
    }

    #[step(r"I set temperature to (\d+\.\d+)")]
    async fn when_i_set_temperature(w: &mut TestWorld, temp: f64) {
        w.temperature = temp;
        w.log.push(format!("temperature set to {}", temp));
    }

    #[step(r"system should be (true|false)")]
    async fn then_system_should_be(w: &mut TestWorld, state: bool) {
        assert_eq!(w.enabled, state);
        w.log.push(format!("check system is {}", state));
    }

    #[step(r"temperature should be (\d+\.\d+)")]
    async fn then_temperature_should_be(w: &mut TestWorld, temp: f64) {
        assert!((w.temperature - temp).abs() < 0.001);
        w.log.push(format!("check temperature is {}", temp));
    }

    pub async fn before_hook(w: &mut TestWorld) {
        w.display = 0;
        w.log.push("before".to_string());
    }

    pub async fn after_hook(w: &mut TestWorld) {
        w.log.push("after".to_string());
    }

    #[step(r"the following users:")]
    async fn given_users(w: &mut TestWorld, table: DataTable) {
        for row in table.iter().skip(1) {
            let name = row[0].clone();
            let age: i32 = row[1].parse().expect("Age must be a number");
            w.users.insert(name.clone(), age);
            w.log.push(format!("user {} age {}", name, age));
        }
    }

    #[step(r"lookup (\w+) is (\d+)")]
    async fn then_lookup(w: &mut TestWorld, name: String, age: i32) {
        let actual = w.users.get(&name).expect("User not found");
        assert_eq!(*actual, age, "Age mismatch for {}", name);
        w.log.push(format!("lookup {} is {}", name, age));
    }

    #[step(r"I have a UDP Echo Server on Port (\d+)")]
    async fn given_udp_echo_server(w: &mut TestWorld, _port: u16) {
        w.received_packets.clear();
        w.log.push("udp_echo_server_init".to_string());
    }

    #[step(r"I send a UDP packet with:")]
    async fn when_send_packet(w: &mut TestWorld, table: DataTable) -> Result<(), String> {
        let packet: MockPacket =
            table_to_struct(&table).map_err(|e| format!("Failed to parse MockPacket: {}", e))?;
        w.received_packets.push(serde_json::to_vec(&packet).map_err(|e| e.to_string())?);
        w.log.push("send_udp".to_string());
        Ok(())
    }

    #[step(r"I receive a UDP packet matching:")]
    async fn then_receive_packet(w: &mut TestWorld, table: DataTable) -> Result<(), String> {
        if w.received_packets.is_empty() {
            return Err("No packets received".to_string());
        }
        let last_packet = w.received_packets.last().unwrap();
        let packet: MockPacket = serde_json::from_slice(last_packet).map_err(|e| e.to_string())?;
        assert_json_matches_table(&packet, &table);
        w.log.push("check_udp".to_string());
        Ok(())
    }

    #[step(r"I setup with default users:")]
    async fn given_setup_default_users(w: &mut TestWorld, table: DataTable) {
        given_users(w, table).await;
    }

    #[step(r"I check if additive table works:")]
    async fn check_additive_table(w: &mut TestWorld, table: DataTable) {
        use features::horizontal_table_to_structs;
        use serde::Deserialize;

        #[derive(Deserialize)]
        struct KeyVal {
            key: String,
            value: String,
        }

        let rows: Vec<KeyVal> = horizontal_table_to_structs(&table).expect("Failed to parse table");

        for row in rows {
            if row.key == "add" {
                let val: i32 = row.value.parse().expect("Value must be a number");
                w.display += val;
                w.log.push(format!("add {}", val));
            }
        }
    }

    #[step(r"I fail with an error")]
    async fn when_i_fail_with_an_error(_w: &mut TestWorld) -> Result<(), String> {
        Err("Intentional failure".to_string())
    }
}

fn before_helper<'a>(
    w: &'a mut TestWorld,
    _args: Vec<String>,
    _ctx: features::StepContext,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), String>> + Send + 'a>> {
    Box::pin(async move {
        steps::before_hook(w).await;
        Ok(())
    })
}

fn after_helper<'a>(
    w: &'a mut TestWorld,
    _args: Vec<String>,
    _ctx: features::StepContext,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), String>> + Send + 'a>> {
    Box::pin(async move {
        steps::after_hook(w).await;
        Ok(())
    })
}

fn setup_features_world() -> (Features<TestWorld>, TestWorld) {
    let mut features = Features::<TestWorld>::new();
    steps::register_steps(&mut features);
    features.before(before_helper);
    features.after(after_helper);
    let world = TestWorld {
        display: 0,
        log: Vec::new(),
        users: std::collections::HashMap::new(),
        tx_power: TxPower::Low,
        enabled: false,
        temperature: 0.0,
        memory: 0,
        received_packets: Vec::new(),
    };
    (features, world)
}

#[tokio::test]
async fn test_features_methods_success() {
    let (features, mut world) = setup_features_world();
    world.display = 999;

    features.run("I reset the counter", &mut world).await.unwrap();
    features.run("I add 10", &mut world).await.unwrap();
    features.run("result is 10", &mut world).await.unwrap();

    assert_eq!(world.log, vec!["reset", "add 10", "check 10"]);
}

#[tokio::test]
async fn test_features_background() {
    let _ = try_init();
    let (features, mut world) = setup_features_world();

    features
        .execute_from_memory(include_str!("features/background.feat"), &mut world)
        .await
        .unwrap();

    // Feature: Background Support
    // Background: reset_background
    // Scenario: Incremental Add (add 5, check 5)
    //
    // Sequence:
    // Before -> Background(reset_background) -> Steps(add 5, check 5) -> After

    assert_eq!(world.log, vec!["before", "reset_background", "add 5", "check 5", "after"]);
}

#[tokio::test]
async fn test_features_outline() {
    let _ = try_init();
    let (features, mut world) = setup_features_world();

    features.execute_from_memory(include_str!("features/outline.feat"), &mut world).await.unwrap();

    // Feature: Scenario Outline Support
    // Background: reset_outline
    // Scenario Outline: Add multiple numbers
    //   Ex 1: 10, 20 -> 30
    //   Ex 2: 1, 2 -> 3
    //
    // Sequence:
    // Before -> Background(reset_outline) -> Steps(add 10, add 20, check 30) ->
    // After Before -> Background(reset_outline) -> Steps(add 1, add 2, check 3)
    // -> After

    assert_eq!(
        world.log,
        vec![
            "before",
            "reset_calculator memory 0",
            "add 10",
            "add 20",
            "check 30",
            "after",
            "before",
            "reset_calculator memory 0",
            "add 1",
            "add 2",
            "check 3",
            "after"
        ]
    );
}

#[tokio::test]
#[should_panic(expected = "Check failed: expected 9999, got 10")]
async fn test_features_assertion_failure() {
    let (features, mut world) = setup_features_world();

    features.run("I reset the counter", &mut world).await.unwrap();
    features.run("I add 10", &mut world).await.unwrap();
    // This should panic
    features.run("result is 9999", &mut world).await.unwrap();
}

#[tokio::test]
async fn test_features_missing_step() {
    let (features, mut world) = setup_features_world();
    let err = features.run("I do not exist", &mut world).await.unwrap_err();
    assert_eq!(err.to_string(), "No step definition found for: I do not exist");
}

#[tokio::test]
async fn test_features_tags_filtering() {
    let _ = try_init();
    let (mut features, mut world) = setup_features_world();

    // Set filter to @wip
    features.filter("@wip");

    features.execute_from_memory(include_str!("features/tags.feat"), &mut world).await.unwrap();

    // Filtered execution (only Tagged scenario)
    // Before -> Background(reset_tags) -> Steps(add 100, check 100) -> After

    assert_eq!(world.log, vec!["before", "reset_tags", "add 100", "check 100", "after"]);
}

#[tokio::test]
async fn test_features_tags_ignoring() {
    let _ = try_init();
    let (mut features, mut world) = setup_features_world();

    // Set ignore tags to @wip
    features.ignore_tags(&["wip"]);

    features.execute_from_memory(include_str!("features/tags.feat"), &mut world).await.unwrap();

    // Ignored execution (only Untagged scenario)
    // Before -> Background(reset_tags) -> Steps(add 1, check 1) -> After

    assert_eq!(world.log, vec!["before", "reset_tags", "add 1", "check 1", "after"]);
}

#[tokio::test]
async fn test_features_data_table() {
    let _ = try_init();
    let (features, mut world) = setup_features_world();

    let feature = include_str!("features/data_table.feat");

    features.execute_from_memory(feature, &mut world).await.unwrap();

    assert_eq!(
        world.log,
        vec![
            "before",
            "user Alice age 30",
            "user Bob age 25",
            "lookup Alice is 30",
            "lookup Bob is 25",
            "after"
        ]
    );
}

#[tokio::test]
async fn test_features_types() {
    let _ = try_init();
    let (features, mut world) = setup_features_world();

    let feature = include_str!("features/types.feat");

    features.execute_from_memory(feature, &mut world).await.unwrap();

    // Feature: Type Support
    // Scenario: Boolean and Float types
    //   Given system is true
    //   When I set temperature to 98.6
    //   Then system should be true
    //   And temperature should be 98.6
    //
    // Scenario: Disable system
    //   Given system is false
    //   Then system should be false

    assert_eq!(
        world.log,
        vec![
            "before",
            "system is true",
            "temperature set to 98.6",
            "check system is true",
            "check temperature is 98.6",
            "after",
            "before",
            "system is false",
            "check system is false",
            "after"
        ]
    );
}

#[tokio::test]
async fn test_features_enum() {
    let _ = try_init();
    let (mut features, mut world) = setup_features_world();

    let feature_content = include_str!("features/enum.feat");

    features.execute_from_memory(feature_content, &mut world).await.unwrap();

    assert_eq!(world.log, vec!["before", "tx_power High", "tx_power Ultralow", "after"]);
    assert_eq!(world.tx_power, TxPower::Ultralow);
}

#[tokio::test]
async fn test_features_network() {
    let _ = try_init();
    let (features, mut world) = setup_features_world();
    features.execute_from_memory(include_str!("features/network.feat"), &mut world).await.unwrap();

    assert_eq!(
        world.log,
        vec!["before", "udp_echo_server_init", "send_udp", "check_udp", "after",]
    );
}

#[tokio::test]
async fn test_features_background_table() {
    let _ = try_init();
    let (features, mut world) = setup_features_world();

    features
        .execute_from_memory(include_str!("features/background_table.feature"), &mut world)
        .await
        .unwrap();

    assert_eq!(
        world.log,
        vec![
            "before",
            "user Admin age 99",
            "user Guest age 10",
            "lookup Admin is 99",
            "lookup Guest is 10",
            "after"
        ]
    );
}

#[tokio::test]
async fn test_features_methods_success_file() {
    let _ = try_init();
    let (features, mut world) = setup_features_world();

    features
        .execute_from_memory(include_str!("features/methods_success.feature"), &mut world)
        .await
        .unwrap();

    assert_eq!(world.log, vec!["before", "reset", "add 10", "check 10", "after"]);
}

#[tokio::test]
async fn test_features_outline_table() {
    let _ = try_init();
    let (features, mut world) = setup_features_world();

    features
        .execute_from_memory(include_str!("features/outline_table.feature"), &mut world)
        .await
        .unwrap();

    assert_eq!(
        world.log,
        vec![
            // First example: 10, 20 -> 30
            "before", "reset", "add 10", "add 20", // via table
            "check 30", "after", // Second example: 5, 5 -> 10
            "before", "reset", "add 5", "add 5", // via table
            "check 10", "after"
        ]
    );
}

#[tokio::test]
async fn test_features_result_error_propagation() {
    let (features, mut world) = setup_features_world();
    let result = features.run("I fail with an error", &mut world).await;
    assert!(result.is_err());
}
