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

use crate::world::World;

#[tokio::test]
async fn test_rootcanal_handshake() {
    let mut world = World::new().await;
    world.given_running_daemon().await;

    world.when_client_connects().await;

    world.then_welcome_received().await;
}

#[tokio::test]
async fn test_rootcanal_list_devices() {
    let mut world = World::new().await;
    let id = world.given_device_with_bluetooth("guid_1", "11:22:33:44:55:66").await;
    world.given_running_daemon().await;
    world.given_connected_client().await;

    world.when_send_command("list", &[]).await;

    world.then_list_contains_device(id);
    world.then_list_phy_contains_device("0:LOW_ENERGY:", id);
    world.then_list_phy_contains_device("1:BR_EDR:", id);
}

#[tokio::test]
async fn test_rootcanal_phy_toggle() {
    let mut world = World::new().await;
    let id = world.given_device_with_bluetooth("guid_2", "11:22:33:44:55:77").await;
    world.given_running_daemon().await;
    world.given_connected_client().await;

    world.when_toggle_phy(id, "0", false).await;
    world.then_toggle_phy_response_is(id, "0", false);
    world.then_bluetooth_le_enabled_is(id, false).await;
    world.then_bluetooth_classic_enabled_is(id, true).await;

    world.when_toggle_phy(id, "1", false).await;
    world.then_toggle_phy_response_is(id, "1", false);
    world.then_bluetooth_le_enabled_is(id, false).await;
    world.then_bluetooth_classic_enabled_is(id, false).await;

    world.when_toggle_phy(id, "0", true).await;
    world.then_toggle_phy_response_is(id, "0", true);
    world.then_bluetooth_le_enabled_is(id, true).await;
    world.then_bluetooth_classic_enabled_is(id, false).await;
}

#[tokio::test]
async fn test_rootcanal_device_lifecycle() {
    let mut world = World::new().await;
    world.given_running_daemon().await;
    world.given_connected_client().await;

    let id = world.when_add_device("device", Some("11:22:33:44:55:88")).await;
    world.then_add_device_succeeded(id);
    world.then_bluetooth_le_enabled_is(id, true).await;
    world.then_bluetooth_classic_enabled_is(id, true).await;

    world.when_delete_device(id).await;
    world.then_delete_device_response_is(id);
    world.then_device_is_deleted(id).await;

    world.when_reset().await;
    world.then_response_is("OK");
}

#[tokio::test]
async fn test_rootcanal_argument_validation() {
    let mut world = World::new().await;
    world.given_running_daemon().await;
    world.given_connected_client().await;

    world.when_send_command("list", &["extra_arg"]).await;
    world.then_response_is("TestCommandHandler 'list' takes no arguments");

    world.when_send_command("add", &[]).await;
    world.then_response_is("TestCommandHandler 'add' takes an argument");

    world.when_send_command("del", &[]).await;
    world.then_response_is("TestCommandHandler 'del' takes an argument");

    world.when_send_command("del", &["abc"]).await;
    world.then_response_is("Invalid device_id");

    world.when_send_command("del", &["999"]).await;
    world.then_response_is(
        "Delete failed: Framework error: Service error: Device not found (explicit): 999",
    );
}

#[tokio::test]
async fn test_rootcanal_unknown_commands() {
    let mut world = World::new().await;
    world.given_running_daemon().await;
    world.given_connected_client().await;

    world.when_send_command("completely_unknown_command", &["arg1", "arg2"]).await;

    world.then_response_is("OK");
}

#[tokio::test]
async fn test_rootcanal_session_close() {
    let mut world = World::new().await;
    world.given_running_daemon().await;
    world.given_connected_client().await;

    world.when_send_close_test_channel().await;

    world.then_client_stream_is_closed().await;
}

#[tokio::test]
async fn test_rootcanal_abrupt_disconnect() {
    let mut world = World::new().await;
    world.given_running_daemon().await;

    world.when_client_abruptly_disconnects_mid_command().await;
    world.when_client_connects().await;

    world.then_welcome_received().await;
}

#[tokio::test]
async fn test_rootcanal_pipelining() {
    let mut world = World::new().await;
    world.given_running_daemon().await;
    world.given_connected_client().await;

    world.when_send_pipelined_commands(&[("list", &[]), ("list", &[])]).await;

    world.then_pipelined_responses_contain(2, "Devices:");
}

#[tokio::test]
async fn test_rootcanal_concurrency() {
    let mut world = World::new().await;
    world.given_running_daemon().await;
    world.given_concurrent_clients(3).await;

    world.when_concurrent_clients_send_command("list", &[]).await;

    world.then_all_concurrent_responses_contain("Devices:");
}

#[tokio::test]
async fn test_rootcanal_phy_all_toggle() {
    let mut world = World::new().await;
    let id = world.given_device_with_bluetooth("guid_phy_all", "11:22:33:44:55:aa").await;
    world.given_running_daemon().await;
    world.given_connected_client().await;

    world.when_toggle_phy(id, "2", false).await;
    world.then_toggle_phy_response_is(id, "2", false);
    world.then_bluetooth_chip_enabled_is(id, false).await;

    world.when_toggle_phy(id, "2", true).await;
    world.then_toggle_phy_response_is(id, "2", true);
    world.then_bluetooth_chip_enabled_is(id, true).await;
}

#[tokio::test]
async fn test_rootcanal_add_device_no_address() {
    let mut world = World::new().await;
    world.given_running_daemon().await;
    world.given_connected_client().await;

    let id = world.when_add_device("device", None).await;

    world.then_add_device_succeeded(id);
    world.then_bluetooth_le_enabled_is(id, true).await;
    world.then_bluetooth_classic_enabled_is(id, true).await;
}

#[tokio::test]
async fn test_rootcanal_set_device_configuration() {
    let mut world = World::new().await;
    world.given_running_daemon().await;
    world.given_connected_client().await;
    let id = world.given_added_device("device", Some("11:22:33:44:55:aa")).await;

    world.when_set_device_configuration(id, "csr_rck_pts_dongle").await;

    world.then_response_is(&format!("set_device_configuration {id} csr_rck_pts_dongle"));
    world.then_bluetooth_preset_is(id, Some("csr_rck_pts_dongle")).await;
}

#[tokio::test]
async fn test_rootcanal_set_device_configuration_invalid_preset() {
    let mut world = World::new().await;
    world.given_running_daemon().await;
    world.given_connected_client().await;
    let id = world.given_added_device("device", Some("11:22:33:44:55:aa")).await;

    world.when_set_device_configuration(id, "invalid_preset_name").await;

    world.then_response_starts_with("Failed to update device configuration");
    world.then_bluetooth_preset_is(id, None).await;
}
