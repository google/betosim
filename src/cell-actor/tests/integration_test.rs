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

use cell_actor::CellAction;
use netsim_model::{CallState, CellNetworkConfig, Quirks, RadioTechnology, RegistrationStatus};

use crate::world::{SLIRP_FIRST_V4, SLIRP_FIRST_V6, World};

/// Scenario: Creating a cellular chip initializes an active idle modem in the actor
#[tokio::test]
async fn test_create_chip() {
    // Given a cell actor with no chips
    let mut world = World::new().await;

    // When cellular chip 1 is created
    world.given_cell_chip(1).await;

    // Then reading and listing the cellular chip report an enabled idle modem
    world.then_cell_state_is(1, "idle").await;
    world.then_listed_cell_states_are(&[(1, true, "idle")]).await;
}

/// Scenario: Deleting a cellular chip removes it and notifies the device actor
#[tokio::test]
async fn test_delete_chip() {
    let mut world = World::new().await;

    // Given an active cellular chip 2
    world.given_cell_chip(2).await;

    // When chip 2 is deleted
    world.when_delete_chip(2).await;

    // Then the device actor is notified that chip 2 was removed
    world.then_device_notified_chip_removed(2).await;
}

/// Scenario: Closing the guest packet stream triggers automatic chip deletion
#[tokio::test]
async fn test_stream_error_triggers_delete() {
    let mut world = World::new().await;

    // Given an active cellular chip 3
    world.given_cell_chip(3).await;

    // When the guest closes its packet stream for chip 3
    world.when_guest_stream_closes(3);

    // Then the device actor is notified that chip 3 was removed
    world.then_device_notified_chip_removed(3).await;
}

/// Scenario: A host sink error event removes the cellular chip and notifies the device actor
#[tokio::test]
async fn test_sink_error_triggers_delete() {
    let mut world = World::new().await;

    // Given an active cellular chip 4 in the idle state
    world.given_cell_chip(4).await;
    world.then_cell_state_is(4, "idle").await;

    // When the modem simulator emits a sink error for chip 4
    world.when_host_emits_sink_error(4);

    // Then the device actor is notified and chip 4 is no longer found
    world.then_device_notified_chip_removed(4).await;
    world.then_chip_not_found(4).await;
}

/// Scenario: Sending an AT command over the guest packet stream returns the modem response on the guest sink
#[tokio::test]
async fn test_stream_to_controller_echo() {
    let mut world = World::new().await;

    // Given an active cellular chip 1
    world.given_cell_chip(1).await;

    // When the guest sends a basic AT command over its packet stream
    world.when_guest_sends_packet(1, b"AT\r").await;

    // Then the modem controller processes the command and replies with OK on the guest sink
    world.then_guest_receives_exact_packet(1, "OK\r\n").await;
}

/// Scenario: Reading an existing cellular chip returns its state while reading an unknown ID returns ChipNotFound
#[tokio::test]
async fn test_get_chip() {
    let mut world = World::new().await;

    // Given an active cellular chip 1
    world.given_cell_chip(1).await;

    // When chip 1 and non-existent chip 99 are read
    // Then reading chip 1 returns idle and reading chip 99 returns ChipNotFound
    world.then_cell_state_is(1, "idle").await;
    world.then_chip_not_found(99).await;
}

/// Scenario: Reading and listing a cellular chip reflect transitions across idle, ringing, and down states
#[tokio::test]
async fn test_get_and_list_chip_states_across_idle_ringing_and_down() {
    let mut world = World::new().await;

    // Given a voice-registered cellular chip 1 in the idle state
    world.given_cell_chip(1).await;
    world.given_voice_registered_home(1).await;
    world.then_cell_state_is(1, "idle").await;
    world.then_listed_cell_states_are(&[(1, true, "idle")]).await;

    // When an incoming call arrives on chip 1
    world.when_incoming_call(1, "123456").await;

    // Then both read and list report the ringing state
    world.then_cell_state_is(1, "ringing").await;
    world.then_listed_cell_states_are(&[(1, true, "ringing")]).await;

    // When chip 1 is disabled
    world.when_set_chip_enabled(1, false).await;

    // Then both read and list report the down state with radio disabled
    world.then_cell_state_is(1, "down").await;
    world.then_listed_cell_states_are(&[(1, false, "down")]).await;
}

/// Scenario: Deleting a chip spawns device notification asynchronously without blocking concurrent reads
#[tokio::test]
async fn test_delete_chip_non_blocking() {
    let mut world = World::new().await;

    // Given an active cellular chip 2
    world.given_cell_chip(2).await;

    // When chip deletion is initiated while the device actor has not yet acknowledged removal
    world.when_start_delete_chip(2);
    world.then_device_receives_chip_removed_unacked(2).await;

    // Then concurrent read requests on the cell actor do not block and deletion completes once acknowledged
    world.then_read_does_not_block(2).await;
    world.when_device_acks_chip_removed();
    world.then_pending_delete_succeeds().await;
}

/// Scenario: Updating signal strength and registration status is reflected in cellular chip status fields
#[tokio::test]
async fn test_get_chip_status_fields() {
    let mut world = World::new().await;

    // Given a newly created cellular chip 1 with default signal strength and unregistered state
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

    // When signal strength and voice/data registrations are updated
    world.when_set_signal_strength(1, 15, 2).await;
    world
        .when_set_registrations(1, RegistrationStatus::RegisteredHome, RegistrationStatus::Roaming)
        .await;

    // Then reading chip 1 reflects the updated RSSI, BER, and registration statuses
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

/// Scenario: Forwarding a valid hex SMS PDU delivers a +CMT URC to the guest sink
#[tokio::test]
async fn test_receive_pdu_action_forwarding() {
    let mut world = World::new().await;

    // Given a voice-registered cellular chip 1
    world.given_cell_chip(1).await;
    world.given_voice_registered_home(1).await;

    // When a valid hex PDU is injected via ReceivePdu
    world.when_receive_pdu(1, "0011000B915155255155F40000AA01F0").await;

    // Then the guest sink receives the formatted +CMT URC with the PDU payload
    world
        .then_guest_receives_exact_packet(1, "+CMT: ,15\r\n0011000B915155255155F40000AA01F0\r\n")
        .await;
}

/// Scenario: Forwarding an incoming text SMS encodes and delivers a UCS-2 PDU +CMT URC to the guest sink
#[tokio::test]
async fn test_receive_sms_action_forwarding() {
    let mut world = World::new().await;

    // Given a voice-registered cellular chip 1
    world.given_cell_chip(1).await;
    world.given_voice_registered_home(1).await;

    // When an incoming text SMS is injected via ReceiveSms
    world.when_receive_sms(1, "123456", "hello").await;

    // Then the guest sink receives a +CMT URC containing the sender and UCS-2 encoded message
    world
        .then_guest_receives_lowercase_packet_framing(
            1,
            "+cmt: ,26\r\n002406812143650008",
            "0a00680065006c006c006f\r\n",
        )
        .await;
}

/// Scenario: Rejecting a ReceivePdu action when the PDU contains non-hexadecimal characters
#[tokio::test]
async fn test_receive_pdu_invalid_hex() {
    let mut world = World::new().await;

    // Given an active cellular chip 1
    world.given_cell_chip(1).await;

    // When ReceivePdu is called with an invalid hex string
    world.when_try_receive_pdu(1, "0011000B915155255155F40000AA01Fg").await;

    // Then the action fails with an Invalid PDU format error
    world.then_last_error_contains("Invalid PDU format");
}

/// Scenario: Rejecting an IncomingCall action when the phone number contains invalid characters
#[tokio::test]
async fn test_incoming_call_invalid_number() {
    let mut world = World::new().await;

    // Given an active cellular chip 1
    world.given_cell_chip(1).await;

    // When IncomingCall is called with alphabetic characters in the phone number
    world.when_try_incoming_call(1, "12345abc").await;

    // Then the action fails with an Invalid phone number error
    world.then_last_error_contains("Invalid phone number");
}

/// Scenario: Creating a cellular chip with initial network configs and updating them via ChipVariantUpdate::Cell
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

    // Given a cellular chip 1 created with an initial IPv4 network config
    world.given_cell_chip_with_configs(1, vec![initial_config.clone()]).await;
    world.then_cell_network_configs_are(1, &[initial_config]).await;

    // When the chip's network configs are updated
    world.when_update_network_configs(1, vec![updated_config.clone()]).await;

    // Then reading chip 1 returns the updated IPv4 network config
    world.then_cell_network_configs_are(1, &[updated_config]).await;
}

/// Scenario: Goldfish dynamic SLIRP lease allocates a dual-stack IP on create and releases it on delete
#[tokio::test]
async fn test_goldfish_leases_and_releases_slirp_ip() {
    let goldfish_quirks = Quirks { dynamic_slirp_lease: true, ..Default::default() };
    let mut world = World::new().await;

    // Given a cellular chip 1 with dynamic_slirp_lease enabled and no pre-provisioned configs
    world.given_cell_chip_with_quirks(1, goldfish_quirks, Vec::new()).await;

    // Then chip 1 receives the first dual-stack SLIRP lease
    world.then_cell_has_dual_stack_lease(1, SLIRP_FIRST_V4, SLIRP_FIRST_V6).await;

    // When chip 1 is deleted
    world.when_delete_chip(1).await;

    // Then the released SLIRP IPv4 lease is recycled to the next requester
    world.then_slirp_allocates_ipv4_lease(2, SLIRP_FIRST_V4).await;
}

/// Scenario: Cuttlefish modems and modems with explicit network configs do not consume SLIRP leases
#[tokio::test]
async fn test_cuttlefish_and_provided_configs_do_not_lease_slirp_ip() {
    // Given a cell actor and a custom IPv4 network configuration
    let cuttlefish_quirks = Quirks { is_cuttlefish: true, ..Default::default() };
    let custom_config = vec![CellNetworkConfig {
        ip_address: IpAddr::V4(Ipv4Addr::new(192, 168, 1, 10)),
        prefixlen: 24,
        gateway: IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1)),
        dns: IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8)),
    }];

    let mut world = World::new().await;

    // When chips 10, 15, and 20 are created with Cuttlefish quirks, default quirks, and explicit network configs
    world.given_cell_chip_with_quirks(10, cuttlefish_quirks, Vec::new()).await;
    world.then_cell_network_configs_are(10, &[]).await;

    world.given_cell_chip(15).await;
    world.then_cell_network_configs_are(15, &[]).await;

    let slirp_quirks = Quirks { dynamic_slirp_lease: true, ..Default::default() };
    world.given_cell_chip_with_quirks(20, slirp_quirks, custom_config.clone()).await;
    world.then_cell_network_configs_are(20, &custom_config).await;

    // Then the first SLIRP IPv4 lease remains unallocated and available for chip 30
    world.then_slirp_allocates_ipv4_lease(30, SLIRP_FIRST_V4).await;
}

/// Scenario: Listing and counting multiple cellular modems reflect state updates, radio toggles, and deletions
#[tokio::test]
async fn test_list_and_count_multiple_modems_and_reflect_deletion() {
    let mut world = World::new().await;

    // Given three active cellular modems 10, 20, and 30
    world.given_cell_chip(10).await;
    world.given_cell_chip(20).await;
    world.given_cell_chip(30).await;

    // Then list, read_count_for_testing, and reset report all three modems as enabled and idle
    world.then_chip_count_is(3).await;
    world
        .then_listed_cell_states_are(&[(10, true, "idle"), (20, true, "idle"), (30, true, "idle")])
        .await;
    world.then_reset_chip_returns_state(10, "idle").await;

    // When modem 20 is set to "down" via CellUpdate.state, modem 30 radio is toggled off and back on, and modem 10 is deleted
    world.when_set_cell_state(20, "down").await;
    world.when_set_cell_radio_state(30, false).await;
    world.then_cell_state_is(30, "down").await;
    world.when_set_cell_radio_state(30, true).await;
    world.when_delete_chip(10).await;
    world.then_device_notified_chip_removed(10).await;

    // Then read_count_for_testing and list reflect only modems 20 (down) and 30 (idle)
    world.then_chip_count_is(2).await;
    world.then_listed_cell_states_are(&[(20, false, "down"), (30, true, "idle")]).await;
}

/// Scenario: Full voice call lifecycle across IncomingCall, AT answer, RemoteHold, UpdateCall, RemoteAnswer, and EndCall
#[tokio::test]
async fn test_call_lifecycle_actions_incoming_answer_hold_update_and_end() {
    let mut world = World::new().await;

    // Given a voice-registered cellular chip 1
    world.given_cell_chip(1).await;
    world.given_voice_registered_home(1).await;

    // When an incoming call from +15551234567 arrives
    world.when_incoming_call(1, "+15551234567").await;

    // Then the modem enters the ringing state with an Incoming MobileTerminated call and emits RING
    world.then_cell_state_is(1, "ringing").await;
    world.then_mt_call_state_is(1, "+15551234567", CallState::Incoming).await;
    world.then_guest_receives_exact_packet(1, "RING\r\n").await;

    // When the guest answers with ATA, the remote party places the call on hold and resumes it, and UpdateCall and EndCall are invoked
    world.when_guest_sends_packet(1, b"ATA\r").await;
    world.then_guest_receives_exact_packet(1, "OK\r\n").await;
    world.then_cell_state_is(1, "idle").await;
    world.then_mt_call_state_is(1, "+15551234567", CallState::Active).await;

    world.when_remote_hold(1, true).await;
    world.then_mt_call_state_is(1, "+15551234567", CallState::Holding).await;

    world.when_remote_hold(1, false).await;
    world.then_mt_call_state_is(1, "+15551234567", CallState::Active).await;

    // When a PDP context is activated via AT+CGDCONT
    world.when_guest_sends_packet(1, b"AT+CGDCONT=1,\"IP\",\"internet\"\r").await;
    world.then_guest_receives_exact_packet(1, "OK\r\n").await;

    world.when_update_call(1).await;
    world.then_guest_receives_exact_packet(1, "%CGFPCCFG: 1,5000,5,0,1\r\n").await;
    world.then_guest_receives_exact_packet(1, "%CGFPCCFG: 2,5000,5,0,1\r\n").await;

    world.when_end_call(1).await;

    // Then the guest receives NO CARRIER and active_calls becomes empty
    world.then_guest_receives_exact_packet(1, "NO CARRIER\r\n").await;
    world.then_active_calls_are(1, &[]).await;
    world.then_cell_state_is(1, "idle").await;

    // When the guest dials an outbound call and RemoteAnswer followed by EndCall are invoked
    world.when_guest_sends_packet(1, b"ATD+15557654321;\r").await;
    world.then_guest_receives_exact_packet(1, "OK\r\n").await;
    world.then_mo_call_state_is(1, "+15557654321", CallState::Dialing).await;

    world.when_remote_answer(1).await;
    world.then_guest_receives_exact_packet(1, "OK\r\n").await;
    world.then_mo_call_state_is(1, "+15557654321", CallState::Active).await;

    world.when_end_call(1).await;

    // Then the outbound call is terminated and active_calls is empty
    world.then_guest_receives_exact_packet(1, "NO CARRIER\r\n").await;
    world.then_active_calls_are(1, &[]).await;
}

/// Scenario: Injecting Cell Broadcasts and updating operator, network technology, and SIM status via CellAction
#[tokio::test]
async fn test_cell_broadcast_sim_status_network_tech_and_operator_actions() {
    let mut world = World::new().await;

    // Given a registered cellular chip 1
    world.given_cell_chip(1).await;
    world
        .when_set_registrations(
            1,
            RegistrationStatus::RegisteredHome,
            RegistrationStatus::RegisteredHome,
        )
        .await;

    // When a Cell Broadcast alert is injected on chip 1
    world.when_inject_cell_broadcast(1, 0x4000, 4370, 0x01, "Test Alert").await;

    // Then the guest sink receives the encoded 88-octet +CBM URC frame
    world.then_guest_receives_cell_broadcast(1, 0x4000, 4370, 0x01, "Test Alert").await;

    // When the operator is manually set to "310260" and network technology is switched to NR (5G)
    world.when_set_operator(1, "310260").await;
    world.then_guest_receives_packet_starting_with(1, "+CSQ:").await;
    world.when_guest_sends_packet(1, b"AT+COPS?\r").await;
    world
        .then_guest_receives_exact_packet(1, "+COPS: 1,0,\"Android Virtual Operator\"\r\nOK\r\n")
        .await;

    world.when_set_network_technology(1, RadioTechnology::Nr).await;
    world.then_guest_receives_packet_starting_with(1, "+CSQ:").await;
    world.when_guest_sends_packet(1, b"AT+CTEC?\r").await;
    world.then_guest_receives_exact_packet(1, "+CTEC: 64,40\r\nOK\r\n").await;

    // When the SIM card is ejected via SetSimStatus(false) and re-inserted via SetSimStatus(true)
    world.when_set_sim_status(1, false).await;
    world.then_guest_receives_exact_packet(1, "+CPIN: ABSENT\r\n").await;
    world.when_guest_sends_packet(1, b"AT+CPIN?\r").await;
    world.then_guest_receives_exact_packet(1, "ERROR\r\n").await;

    world.when_set_sim_status(1, true).await;
    world.then_guest_receives_exact_packet(1, "+CPIN: READY\r\n").await;
    world.when_guest_sends_packet(1, b"AT+CPIN?\r").await;

    // Then querying AT+CPIN? confirms the SIM card is READY again
    world.then_guest_receives_exact_packet(1, "+CPIN: READY\r\nOK\r\n").await;
}

/// Scenario: Rejecting duplicate chip creation, missing packet streams/sinks, unknown chip IDs, and out-of-range action arguments
#[tokio::test]
async fn test_negative_create_duplicate_missing_streams_and_invalid_actions() {
    let mut world = World::new().await;

    // Given an active cellular chip 1
    world.given_cell_chip(1).await;

    // When creating a duplicate chip 1 or creating chips without a stream or sink
    world.when_try_create_chip(1, true, true).await;
    world.then_last_error_contains("already exists");

    world.when_try_create_chip(2, false, true).await;
    world.then_last_error_contains("Missing packet stream or sink");
    world.then_chip_not_found(2).await;

    world.when_try_create_chip(3, true, false).await;
    world.then_last_error_contains("Missing packet stream or sink");
    world.then_chip_not_found(3).await;

    // When performing an action or update on a non-existent chip 999
    world.when_try_action_on_chip(999, CellAction::EndCall).await;
    world.then_last_error_is_chip_not_found(999);

    world.when_try_update_chip_enabled(999, false).await;
    world.then_last_error_is_chip_not_found(999);

    // When invoking ReceiveSms with an invalid sender or SetSignalStrength with out-of-range rssi/ber
    world.when_try_receive_sms(1, "bad@sender", "hi").await;
    world.then_last_error_contains("Invalid SMS sender");

    world.when_try_set_signal_strength(1, 300, 0).await;
    world.then_last_error_contains("rssi out of range");

    world.when_try_set_signal_strength(1, 20, 300).await;

    // Then each invalid request returns the expected validation error
    world.then_last_error_contains("ber out of range");
}
