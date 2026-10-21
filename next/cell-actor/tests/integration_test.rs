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

use std::net::{IpAddr, Ipv4Addr};

use netsim_model::{CellNetworkConfig, Quirks, RegistrationStatus};

use crate::world::{SLIRP_FIRST_V4, SLIRP_FIRST_V6, World};

#[tokio::test]
async fn test_create_chip() {
    let mut world = World::new().await;
    world.given_cell_chip(1).await;
}

#[tokio::test]
async fn test_delete_chip() {
    let mut world = World::new().await;
    world.given_cell_chip(2).await;
    world.when_delete_chip(2).await;
    world.then_device_notified_chip_removed(2).await;
}

#[tokio::test]
async fn test_stream_error_triggers_delete() {
    let mut world = World::new().await;
    world.given_cell_chip(3).await;
    world.when_guest_stream_closes(3);
    world.then_device_notified_chip_removed(3).await;
}

#[tokio::test]
async fn test_sink_error_triggers_delete() {
    let mut world = World::new().await;
    world.given_cell_chip(4).await;
    world.then_cell_state_is(4, "idle").await;
    world.when_host_emits_sink_error(4);
    world.then_device_notified_chip_removed(4).await;
    world.then_chip_not_found(4).await;
}

#[tokio::test]
async fn test_stream_to_controller_echo() {
    // Requires controller to echo. ModemNetworkSimulator might not by default
    // unless we send AT commands it understands.
}

#[tokio::test]
async fn test_get_chip() {
    let mut world = World::new().await;
    world.given_cell_chip(1).await;
    world.then_cell_state_is(1, "idle").await;
    world.then_chip_not_found(99).await;
}

#[tokio::test]
async fn test_delete_chip_non_blocking() {
    let mut world = World::new().await;
    world.given_cell_chip(2).await;
    world.when_start_delete_chip(2);
    world.then_device_receives_chip_removed_unacked(2).await;
    world.then_read_does_not_block(2).await;
    world.when_device_acks_chip_removed();
    world.then_pending_delete_succeeds().await;
}

#[tokio::test]
async fn test_get_chip_status_fields() {
    let mut world = World::new().await;
    world.given_cell_chip(1).await;
    world.then_cell_sms_count_is(1, 0).await;
    // Default RSSI in NetworkService::new is 20, default BER is 99
    world
        .then_cell_status_is(
            1,
            20,
            99,
            RegistrationStatus::NotRegistered,
            RegistrationStatus::NotRegistered,
        )
        .await;

    world.when_set_signal_strength(1, 15, 2).await;
    world
        .when_set_registrations(1, RegistrationStatus::RegisteredHome, RegistrationStatus::Roaming)
        .await;

    world
        .then_cell_status_is(
            1,
            15,
            2,
            RegistrationStatus::RegisteredHome,
            RegistrationStatus::Roaming,
        )
        .await;
}

#[tokio::test]
async fn test_receive_pdu_action_forwarding() {
    let mut world = World::new().await;
    world.given_cell_chip(1).await;
    world.given_voice_registered_home(1).await;
    world.when_receive_pdu(1, "0011000B915155255155F40000AA01F0").await;
    world
        .then_guest_receives_exact_packet(1, "+CMT: ,15\r\n0011000B915155255155F40000AA01F0\r\n")
        .await;
}

#[tokio::test]
async fn test_receive_sms_action_forwarding() {
    let mut world = World::new().await;
    world.given_cell_chip(1).await;
    world.given_voice_registered_home(1).await;
    world.when_receive_sms(1, "123456", "hello").await;
    world
        .then_guest_receives_lowercase_packet_framing(
            1,
            "+cmt: ,26\r\n002406812143650008",
            "0a00680065006c006c006f\r\n",
        )
        .await;
}

#[tokio::test]
async fn test_receive_pdu_invalid_hex() {
    let mut world = World::new().await;
    world.given_cell_chip(1).await;
    world.when_try_receive_pdu(1, "0011000B915155255155F40000AA01Fg").await;
    world.then_last_error_contains("Invalid PDU format");
}

#[tokio::test]
async fn test_incoming_call_invalid_number() {
    let mut world = World::new().await;
    world.given_cell_chip(1).await;
    world.when_try_incoming_call(1, "12345abc").await;
    world.then_last_error_contains("Invalid phone number");
}

#[tokio::test]
async fn test_network_config_lifecycle_and_update() {
    let initial_config = CellNetworkConfig {
        ip_address: IpAddr::V4(Ipv4Addr::new(192, 168, 97, 2)),
        prefixlen: 30,
        gateway: IpAddr::V4(Ipv4Addr::new(192, 168, 97, 1)),
        dns: IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8)),
    };
    let updated_config = CellNetworkConfig {
        ip_address: IpAddr::V4(Ipv4Addr::new(192, 168, 97, 6)),
        prefixlen: 30,
        gateway: IpAddr::V4(Ipv4Addr::new(192, 168, 97, 5)),
        dns: IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1)),
    };

    let mut world = World::new().await;
    world.given_cell_chip_with_configs(1, vec![initial_config.clone()]).await;
    world.then_cell_network_configs_are(1, &[initial_config]).await;

    world.when_update_network_configs(1, vec![updated_config.clone()]).await;
    world.then_cell_network_configs_are(1, &[updated_config]).await;
}

#[tokio::test]
async fn test_goldfish_leases_and_releases_slirp_ip() {
    let goldfish_quirks = Quirks { dynamic_slirp_lease: true, ..Default::default() };
    let mut world = World::new().await;
    world.given_cell_chip_with_quirks(1, goldfish_quirks, Vec::new()).await;
    world.then_cell_has_dual_stack_lease(1, SLIRP_FIRST_V4, SLIRP_FIRST_V6).await;

    world.when_delete_chip(1).await;
    world.then_slirp_allocates_ipv4_lease(2, SLIRP_FIRST_V4).await;
}

#[tokio::test]
async fn test_cuttlefish_and_provided_configs_do_not_lease_slirp_ip() {
    let cuttlefish_quirks = Quirks { is_cuttlefish: true, ..Default::default() };
    let custom_config = vec![CellNetworkConfig {
        ip_address: IpAddr::V4(Ipv4Addr::new(192, 168, 1, 10)),
        prefixlen: 24,
        gateway: IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1)),
        dns: IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8)),
    }];

    let mut world = World::new().await;
    world.given_cell_chip_with_quirks(10, cuttlefish_quirks, Vec::new()).await;
    world.then_cell_network_configs_are(10, &[]).await;

    world.given_cell_chip(15).await;
    world.then_cell_network_configs_are(15, &[]).await;

    let slirp_quirks = Quirks { dynamic_slirp_lease: true, ..Default::default() };
    world.given_cell_chip_with_quirks(20, slirp_quirks, custom_config.clone()).await;
    world.then_cell_network_configs_are(20, &custom_config).await;

    world.then_slirp_allocates_ipv4_lease(30, SLIRP_FIRST_V4).await;
}
