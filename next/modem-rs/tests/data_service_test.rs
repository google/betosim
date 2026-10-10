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

use netsim_model::RegistrationStatus;

use crate::{common::constants::*, steps::*, world::World};

#[test]
fn test_set_and_query_quality_of_service() {
    let mut world = World::new();
    given_data_modem(&mut world, "A");

    world.send_and_expect_ok(
        "A",
        &format!("AT+CGDCONT={TEST_PDP_CID},\"{TEST_PDP_TYPE}\",\"{TEST_APN}\""),
    );

    for cmd in ["CGEQMIN", "CGEQREQ", "CGQMIN", "CGQREQ"] {
        world.send_and_expect_ok("A", &format!("AT+{cmd}={TEST_PDP_CID},{TEST_QOS_PARAMS_3G}"));
        world.send_and_expect(
            "A",
            &format!("AT+{cmd}?"),
            &[&format!("+{cmd}: {TEST_PDP_CID},{TEST_QOS_PARAMS_3G}"), "OK"],
        );
    }
}

// Scenario: Activate PDP Context
//   Given a modem "A"
//   When AT command 'AT+CGDCONT=1,"IP","test"' is sent to "A"
//   And AT command "AT+CGACT=1,1" is sent to "A"
//   Then response from "A" is "OK"
#[test]
fn test_set_pdp_context_activate() {
    let mut world = World::new();
    given_data_modem(&mut world, "A");

    when_at_command_sent(
        &mut world,
        "A",
        &format!("AT+CGDCONT={TEST_PDP_CID},\"{TEST_PDP_TYPE}\",\"{TEST_APN}\""),
    );
    then_wait_for_response_containing(&mut world, "A", "OK");

    when_at_command_sent(&mut world, "A", &format!("AT+CGACT=1,{TEST_PDP_CID}"));
    then_response_is(&mut world, "A", "OK");
}

// Scenario: Update Physical Channel Configs
//   Given a modem "A"
//   When physical channel configs are updated on "A"
//   Then response from "A" is "+CGEV: NW PDN DEACT 1"
#[test]
fn test_update_physical_channel_configs() {
    let mut world = World::new();
    given_data_modem(&mut world, "A");

    when_physical_channel_configs_updated(&mut world, "A");
    then_response_is(&mut world, "A", &format!("+CGEV: NW PDN DEACT {TEST_PDP_CID}"));
}

// Scenario: Read Dynamic Parameters
//   Given a modem "A"
//   When AT command 'AT+CGDCONT=1,"IP","test"' is sent to "A"
//   And AT command "ATD*99***1#" is sent to "A"
//   And AT command "AT+CGCONTRDP=1" is sent to "A"
//   Then response from "A" is '+CGCONTRDP:
// 1,5,"test","10.0.2.15/24","10.0.2.2","10.0.2.3"'   And response from "A" is
// "OK"
#[test]
fn test_read_dynamic_param() {
    let mut world = World::new();
    given_data_modem(&mut world, "A");

    when_at_command_sent(
        &mut world,
        "A",
        &format!("AT+CGDCONT={TEST_PDP_CID},\"{TEST_PDP_TYPE}\",\"{TEST_APN}\""),
    );
    then_wait_for_response_containing(&mut world, "A", "OK");

    when_at_command_sent(&mut world, "A", &format!("ATD{GPRS_DIAL_STRING_PREFIX}{TEST_PDP_CID}#"));
    then_response_is(&mut world, "A", "CONNECT");

    when_at_command_sent(&mut world, "A", &format!("AT+CGCONTRDP={TEST_PDP_CID}"));
    then_response_is(
        &mut world,
        "A",
        &format!(
            "+CGCONTRDP: {TEST_PDP_CID},{DEFAULT_BEARER_ID},\"{TEST_APN}\",\"{TEST_IP_ADDR}\",\"{TEST_GATEWAY}\",\"{TEST_DNS}\""
        ),
    );
    then_response_is(&mut world, "A", "OK");
}

#[test]
fn test_read_dynamic_param_goldfish_37() {
    let mut world = World::new();
    world.given_goldfish_37_data_modem("A");

    world.send_and_expect_ok(
        "A",
        &format!("AT+CGDCONT={TEST_PDP_CID},\"{TEST_PDP_TYPE}\",\"{TEST_APN}\""),
    );
    world.send_and_expect(
        "A",
        &format!("ATD{GPRS_DIAL_STRING_PREFIX}{TEST_PDP_CID}#"),
        &["CONNECT"],
    );
    world.send_and_expect(
        "A",
        &format!("AT+CGCONTRDP={TEST_PDP_CID}"),
        &[
            &format!(
                "+CGCONTRDP: {TEST_PDP_CID},{DEFAULT_BEARER_ID},\"{TEST_APN}\",{TEST_IP_ADDR},{TEST_GATEWAY},{TEST_DNS}"
            ),
            "OK",
        ],
    );
}

// Scenario: Query PDP Context
//   Given a modem "A"
//   When AT command "AT+CGDCONT?" is sent to "A"
//   Then response from "A" is "OK"
#[test]
fn test_query_pdp_context() {
    let mut world = World::new();
    given_data_modem(&mut world, "A");

    when_at_command_sent(&mut world, "A", "AT+CGDCONT?");
    then_response_is(&mut world, "A", "OK");
}

// Scenario: Set and Query QoS Requested
//   Given a modem "A"
//   When AT command 'AT+CGDCONT=1,"IP","test"' is sent to "A"

// Scenario: Set PS Attach
//   Given a modem "A"
//   When AT command "AT+CGATT=1" is sent to "A"
//   Then response from "A" is "OK"
#[test]
fn test_set_ps_attach() {
    let mut world = World::new();
    given_data_modem(&mut world, "A");

    when_at_command_sent(&mut world, "A", "AT+CGATT=1");
    then_response_is(&mut world, "A", "OK");
}

// Scenario: Modify PDP Context
//   Given a modem "A"
//   When AT command 'AT+CGDCONT=1,"IP","test"' is sent to "A"
//   And AT command "AT+CGCMOD=1" is sent to "A"
//   Then response from "A" is "OK"
#[test]
fn test_set_pdp_context_modify() {
    let mut world = World::new();
    given_data_modem(&mut world, "A");

    when_at_command_sent(
        &mut world,
        "A",
        &format!("AT+CGDCONT={TEST_PDP_CID},\"{TEST_PDP_TYPE}\",\"{TEST_APN}\""),
    );
    then_wait_for_response_containing(&mut world, "A", "OK");

    when_at_command_sent(&mut world, "A", &format!("AT+CGCMOD={TEST_PDP_CID}"));
    then_response_is(&mut world, "A", "OK");
}

// Scenario: Enter Data State
//   Given a modem "A"
//   When AT command 'AT+CGDCONT=1,"IP","test"' is sent to "A"
//   And AT command "AT+CGDATA=1" is sent to "A"
//   Then response from "A" is "CONNECT"
#[test]
fn test_enter_data_state() {
    let mut world = World::new();
    given_data_modem(&mut world, "A");

    when_at_command_sent(
        &mut world,
        "A",
        &format!("AT+CGDCONT={TEST_PDP_CID},\"{TEST_PDP_TYPE}\",\"{TEST_APN}\""),
    );
    then_wait_for_response_containing(&mut world, "A", "OK");

    // Explicit valid L2P "PPP"
    when_at_command_sent(&mut world, "A", &format!("AT+CGDATA=\"PPP\",{TEST_PDP_CID}"));
    then_response_is(&mut world, "A", "CONNECT");

    // Omitted L2P defaults to PPP
    when_at_command_sent(&mut world, "A", &format!("AT+CGDATA=,{TEST_PDP_CID}"));
    then_response_is(&mut world, "A", "CONNECT");

    // Explicit valid L2P "IP"
    when_at_command_sent(&mut world, "A", &format!("AT+CGDATA=\"IP\",{TEST_PDP_CID}"));
    then_response_is(&mut world, "A", "CONNECT");

    // Unsupported L2P returns error
    when_at_command_sent(&mut world, "A", &format!("AT+CGDATA=\"INVALID\",{TEST_PDP_CID}"));
    then_response_is(&mut world, "A", "ERROR");

    // Bare CID without L2P slot is rejected
    when_at_command_sent(&mut world, "A", &format!("AT+CGDATA={TEST_PDP_CID}"));
    then_response_is(&mut world, "A", "ERROR");
}

// Scenario: Set Packet Event Reporting
//   Given a modem "A"
//   When AT command "AT+CGEREP=1,1" is sent to "A"
//   Then response from "A" is "OK"
#[test]
fn test_set_packet_event_reporting() {
    let mut world = World::new();
    given_data_modem(&mut world, "A");

    when_at_command_sent(&mut world, "A", "AT+CGEREP=1,1");
    then_response_is(&mut world, "A", "OK");
}

// Scenario: Show PDP Address
//   Given a modem "A"
//   When AT command 'AT+CGDCONT=1,"IP","test"' is sent to "A"
//   And AT command "AT+CGPADDR=1" is sent to "A"
//   Then response from "A" is '+CGPADDR: 1,"10.0.2.15"'
//   And response from "A" is "OK"
#[test]
fn test_show_pdp_address() {
    let mut world = World::new();
    given_data_modem(&mut world, "A");

    // 1. Define context (auto-activated by default)
    when_at_command_sent(
        &mut world,
        "A",
        &format!("AT+CGDCONT={TEST_PDP_CID},\"{TEST_PDP_TYPE}\",\"{TEST_APN}\""),
    );
    then_wait_for_response_containing(&mut world, "A", "OK");

    // 2. Query address (should be active IP)
    when_at_command_sent(&mut world, "A", &format!("AT+CGPADDR={TEST_PDP_CID}"));
    then_response_is(&mut world, "A", &format!("+CGPADDR: {TEST_PDP_CID},\"{TEST_IP_ADDR_ONLY}\""));
    then_response_is(&mut world, "A", "OK");

    // 3. Deactivate context (using Goldfish-style AT+CGACT=cid,state instead of
    //    standard state,cid)
    when_at_command_sent(&mut world, "A", &format!("AT+CGACT={TEST_PDP_CID},0"));
    then_response_is(&mut world, "A", "OK");

    // 4. Query address (should be ERROR because inactive)
    when_at_command_sent(&mut world, "A", &format!("AT+CGPADDR={TEST_PDP_CID}"));
    then_response_is(&mut world, "A", "ERROR");

    // 5. Reactivate context (Goldfish-style AT+CGACT=cid,state)
    when_at_command_sent(&mut world, "A", &format!("AT+CGACT={TEST_PDP_CID},1"));
    then_response_is(&mut world, "A", "OK");

    // 6. Query address (should be active IP again)
    when_at_command_sent(&mut world, "A", &format!("AT+CGPADDR={TEST_PDP_CID}"));
    then_response_is(&mut world, "A", &format!("+CGPADDR: {TEST_PDP_CID},\"{TEST_IP_ADDR_ONLY}\""));
    then_response_is(&mut world, "A", "OK");
}

#[test]
fn test_gprs_dialing_fallback() {
    let mut world = World::new();
    given_data_modem(&mut world, "A");

    // 1. Define the context first
    when_at_command_sent(
        &mut world,
        "A",
        &format!("AT+CGDCONT={TEST_PDP_CID},\"{TEST_PDP_TYPE}\",\"{TEST_APN}\""),
    );
    then_wait_for_response_containing(&mut world, "A", "OK");

    // 2. Dial GPRS fallback which should return CONNECT
    when_at_command_sent(&mut world, "A", &format!("ATD{GPRS_DIAL_STRING_PREFIX}{TEST_PDP_CID}#"));
    then_response_is(&mut world, "A", "CONNECT");

    // 3. Verify that the PDP context is indeed active now (returns active IP)
    when_at_command_sent(&mut world, "A", &format!("AT+CGPADDR={TEST_PDP_CID}"));
    then_response_is(&mut world, "A", &format!("+CGPADDR: {TEST_PDP_CID},\"{TEST_IP_ADDR_ONLY}\""));
    then_response_is(&mut world, "A", "OK");
}

#[test]
fn test_dial_non_existent_context() {
    let mut world = World::new();
    given_data_modem(&mut world, "A");

    // Dial GPRS context 2 which is undefined -> should return ERROR
    when_at_command_sent(
        &mut world,
        "A",
        &format!("ATD{GPRS_DIAL_STRING_PREFIX}{TEST_PDP_CID_ALT}#"),
    );
    then_response_is(&mut world, "A", "ERROR");
}

#[test]
fn test_gprs_dialing_default_cid() {
    let mut world = World::new();
    given_data_modem(&mut world, "A");

    // Define PDP context 1
    when_at_command_sent(
        &mut world,
        "A",
        &format!("AT+CGDCONT={TEST_PDP_CID},\"{TEST_PDP_TYPE}\",\"{TEST_APN}\""),
    );
    then_wait_for_response_containing(&mut world, "A", "OK");

    // Dialing *99# without specified CID should default to PDP context 1
    when_at_command_sent(&mut world, "A", "ATD*99#");
    then_response_is(&mut world, "A", "CONNECT");

    // Verify PDP context 1 is indeed active
    when_at_command_sent(&mut world, "A", &format!("AT+CGPADDR={TEST_PDP_CID}"));
    then_response_is(&mut world, "A", &format!("+CGPADDR: {TEST_PDP_CID},\"{TEST_IP_ADDR_ONLY}\""));
    then_response_is(&mut world, "A", "OK");
}

#[test]
fn test_multiple_concurrent_pdp_contexts() {
    let mut world = World::new();
    given_data_modem(&mut world, "A");

    // 1. Define and activate PDP context 1
    when_at_command_sent(
        &mut world,
        "A",
        &format!("AT+CGDCONT={TEST_PDP_CID},\"{TEST_PDP_TYPE}\",\"{TEST_APN1}\""),
    );
    then_wait_for_response_containing(&mut world, "A", "OK");
    when_at_command_sent(&mut world, "A", &format!("AT+CGACT=1,{TEST_PDP_CID}"));
    then_wait_for_response_containing(&mut world, "A", "OK");

    // 2. Define and activate PDP context 2
    when_at_command_sent(
        &mut world,
        "A",
        &format!("AT+CGDCONT={TEST_PDP_CID_ALT},\"{TEST_PDP_TYPE}\",\"{TEST_APN2}\""),
    );
    then_wait_for_response_containing(&mut world, "A", "OK");
    when_at_command_sent(&mut world, "A", &format!("AT+CGACT=1,{TEST_PDP_CID_ALT}")); // Activate CID 2
    then_wait_for_response_containing(&mut world, "A", "OK");

    when_at_command_sent(&mut world, "A", &format!("AT+CGPADDR={TEST_PDP_CID}"));
    then_response_is(&mut world, "A", &format!("+CGPADDR: {TEST_PDP_CID},\"{TEST_IP_ADDR_ONLY}\""));
    then_response_is(&mut world, "A", "OK");

    when_at_command_sent(&mut world, "A", &format!("AT+CGPADDR={TEST_PDP_CID_ALT}"));
    then_response_is(
        &mut world,
        "A",
        &format!("+CGPADDR: {TEST_PDP_CID_ALT},\"{TEST_IP_ADDR_ONLY}\""),
    );
    then_response_is(&mut world, "A", "OK");

    when_at_command_sent(&mut world, "A", &format!("AT+CGCONTRDP={TEST_PDP_CID}"));
    then_response_is(
        &mut world,
        "A",
        &format!(
            "+CGCONTRDP: {TEST_PDP_CID},{DEFAULT_BEARER_ID},\"{TEST_APN1}\",\"{TEST_IP_ADDR}\",\"{TEST_GATEWAY}\",\"{TEST_DNS}\""
        ),
    );
    then_response_is(&mut world, "A", "OK");

    when_at_command_sent(&mut world, "A", &format!("AT+CGCONTRDP={TEST_PDP_CID_ALT}"));
    then_response_is(
        &mut world,
        "A",
        &format!(
            "+CGCONTRDP: {TEST_PDP_CID_ALT},{DEFAULT_BEARER_ID},\"{TEST_APN2}\",\"{TEST_IP_ADDR}\",\"{TEST_GATEWAY}\",\"{TEST_DNS}\""
        ),
    );
    then_response_is(&mut world, "A", "OK");
}

#[test]
fn test_goldfish_ril_compat_incorrect_cgact() {
    let mut world = World::new();
    given_data_modem(&mut world, "A");

    // 1. Define and activate PDP context 1
    when_at_command_sent(
        &mut world,
        "A",
        &format!("AT+CGDCONT={TEST_PDP_CID},\"{TEST_PDP_TYPE}\",\"{TEST_APN}\""),
    );
    then_wait_for_response_containing(&mut world, "A", "OK");
    when_at_command_sent(&mut world, "A", &format!("AT+CGACT=1,{TEST_PDP_CID}"));
    then_wait_for_response_containing(&mut world, "A", "OK");

    // Verify it is active
    when_at_command_sent(&mut world, "A", &format!("AT+CGPADDR={TEST_PDP_CID}"));
    then_response_is(&mut world, "A", &format!("+CGPADDR: {TEST_PDP_CID},\"{TEST_IP_ADDR_ONLY}\""));
    then_response_is(&mut world, "A", "OK");

    // 2. Send the goldfish RIL "deactivate CID 1" command: AT+CGACT=1,0
    // (Standard would be AT+CGACT=0,1)
    when_at_command_sent(&mut world, "A", &format!("AT+CGACT={TEST_PDP_CID},0"));
    then_wait_for_response_containing(&mut world, "A", "OK");

    // 3. Verify it is now inactive (should return ERROR)
    when_at_command_sent(&mut world, "A", &format!("AT+CGPADDR={TEST_PDP_CID}"));
    then_response_is(&mut world, "A", "ERROR");

    // 4. Define CID 2
    when_at_command_sent(
        &mut world,
        "A",
        &format!("AT+CGDCONT={TEST_PDP_CID_ALT},\"{TEST_PDP_TYPE}\",\"test2\""),
    );
    then_wait_for_response_containing(&mut world, "A", "OK");

    // 5. Activate CID 2 using legacy format: AT+CGACT=2,1 (Standard is
    //    AT+CGACT=1,2)
    when_at_command_sent(&mut world, "A", &format!("AT+CGACT={TEST_PDP_CID_ALT},1"));
    then_wait_for_response_containing(&mut world, "A", "OK");

    // Verify CID 2 is active
    when_at_command_sent(&mut world, "A", &format!("AT+CGPADDR={TEST_PDP_CID_ALT}"));
    then_response_is(
        &mut world,
        "A",
        &format!("+CGPADDR: {TEST_PDP_CID_ALT},\"{TEST_IP_ADDR_ONLY}\""),
    );
    then_response_is(&mut world, "A", "OK");

    // 6. Deactivate CID 2 using legacy format: AT+CGACT=2,0 (Standard is
    //    AT+CGACT=0,2)
    when_at_command_sent(&mut world, "A", &format!("AT+CGACT={TEST_PDP_CID_ALT},0"));
    then_wait_for_response_containing(&mut world, "A", "OK");

    // Verify CID 2 is inactive (should return ERROR)
    when_at_command_sent(&mut world, "A", &format!("AT+CGPADDR={TEST_PDP_CID_ALT}"));
    then_response_is(&mut world, "A", "ERROR");
}

#[test]
fn test_query_pdp_context_activate() {
    let mut world = World::new();
    given_data_modem(&mut world, "A");

    // 1. Define context 1 and 2
    when_at_command_sent(
        &mut world,
        "A",
        &format!("AT+CGDCONT={TEST_PDP_CID},\"{TEST_PDP_TYPE}\",\"{TEST_APN1}\""),
    );
    then_wait_for_response_containing(&mut world, "A", "OK");
    when_at_command_sent(
        &mut world,
        "A",
        &format!("AT+CGDCONT={TEST_PDP_CID_ALT},\"{TEST_PDP_TYPE}\",\"{TEST_APN2}\""),
    );
    then_wait_for_response_containing(&mut world, "A", "OK");

    // By default, contexts are auto-activated in Goldfish mode.
    when_at_command_sent(&mut world, "A", "AT+CGACT?");
    then_response_is(&mut world, "A", &format!("+CGACT: {TEST_PDP_CID},1"));
    then_response_is(&mut world, "A", &format!("+CGACT: {TEST_PDP_CID_ALT},1"));
    then_response_is(&mut world, "A", "OK");

    // Deactivate context 1
    when_at_command_sent(&mut world, "A", &format!("AT+CGACT={TEST_PDP_CID},0"));
    then_wait_for_response_containing(&mut world, "A", "OK");

    // Query again
    when_at_command_sent(&mut world, "A", "AT+CGACT?");
    then_response_is(&mut world, "A", &format!("+CGACT: {TEST_PDP_CID},0"));
    then_response_is(&mut world, "A", &format!("+CGACT: {TEST_PDP_CID_ALT},1"));
    then_response_is(&mut world, "A", "OK");
}

#[test]
fn test_ps_attach_detach() {
    let mut world = World::new();
    given_data_modem(&mut world, "A");

    // 1. Verify initially attached
    when_at_command_sent(&mut world, "A", "AT+CGATT?");
    then_response_is(&mut world, "A", "+CGATT: 1");
    then_response_is(&mut world, "A", "OK");

    // 2. Define a context
    when_at_command_sent(
        &mut world,
        "A",
        &format!("AT+CGDCONT={TEST_PDP_CID},\"{TEST_PDP_TYPE}\",\"{TEST_APN}\""),
    );
    then_wait_for_response_containing(&mut world, "A", "OK");

    // 3. Detach PS
    when_at_command_sent(&mut world, "A", "AT+CGATT=0");
    then_wait_for_response_containing(&mut world, "A", "OK");

    // 4. Verify detached
    when_at_command_sent(&mut world, "A", "AT+CGATT?");
    then_response_is(&mut world, "A", "+CGATT: 0");
    then_response_is(&mut world, "A", "OK");

    // 5. Verify context is deactivated
    when_at_command_sent(&mut world, "A", "AT+CGACT?");
    then_response_is(&mut world, "A", &format!("+CGACT: {TEST_PDP_CID},0"));
    then_response_is(&mut world, "A", "OK");
}

#[test]
fn test_qos_queries_when_empty() {
    let mut world = World::new();
    given_data_modem(&mut world, "A");

    // No contexts defined, queries should return OK (no data lines)
    when_at_command_sent(&mut world, "A", "AT+CGEQMIN?");
    then_response_is(&mut world, "A", "OK");

    when_at_command_sent(&mut world, "A", "AT+CGEQREQ?");
    then_response_is(&mut world, "A", "OK");

    when_at_command_sent(&mut world, "A", "AT+CGQMIN?");
    then_response_is(&mut world, "A", "OK");

    when_at_command_sent(&mut world, "A", "AT+CGQREQ?");
    then_response_is(&mut world, "A", "OK");
}

#[test]
fn test_data_commands_invalid_cid() {
    let mut world = World::new();
    given_data_modem(&mut world, "A");

    // CID 2 does not exist (no contexts defined at all, or we only define CID
    // 1)
    when_at_command_sent(
        &mut world,
        "A",
        &format!("AT+CGDCONT={TEST_PDP_CID},\"{TEST_PDP_TYPE}\",\"{TEST_APN}\""),
    );
    then_wait_for_response_containing(&mut world, "A", "OK");

    // 1. QoS settings on non-existent CID 2
    when_at_command_sent(
        &mut world,
        "A",
        &format!("AT+CGEQMIN={INVALID_PDP_CID},{TEST_QOS_PARAMS_GPRS}"),
    );
    then_response_is(&mut world, "A", "ERROR");

    when_at_command_sent(
        &mut world,
        "A",
        &format!("AT+CGEQREQ={INVALID_PDP_CID},{TEST_QOS_PARAMS_GPRS}"),
    );
    then_response_is(&mut world, "A", "ERROR");

    when_at_command_sent(
        &mut world,
        "A",
        &format!("AT+CGQMIN={INVALID_PDP_CID},{TEST_QOS_PARAMS_GPRS}"),
    );
    then_response_is(&mut world, "A", "ERROR");

    when_at_command_sent(
        &mut world,
        "A",
        &format!("AT+CGQREQ={INVALID_PDP_CID},{TEST_QOS_PARAMS_GPRS}"),
    );
    then_response_is(&mut world, "A", "ERROR");

    // 2. Data commands on non-existent CID 2
    when_at_command_sent(&mut world, "A", &format!("AT+CGACT=1,{INVALID_PDP_CID}")); // Activate CID 2 (state=1, cid=2)
    then_response_is(&mut world, "A", "ERROR");

    when_at_command_sent(&mut world, "A", &format!("AT+CGCMOD={INVALID_PDP_CID}")); // Modify CID 2
    then_response_is(&mut world, "A", "ERROR");

    when_at_command_sent(&mut world, "A", &format!("AT+CGPADDR={INVALID_PDP_CID}")); // Show address for CID 2
    then_response_is(&mut world, "A", "ERROR");

    when_at_command_sent(&mut world, "A", &format!("AT+CGCONTRDP={INVALID_PDP_CID}")); // Read dynamic param for CID 2
    then_response_is(&mut world, "A", "ERROR");

    when_at_command_sent(&mut world, "A", &format!("AT+CGDATA={INVALID_PDP_CID}")); // Enter data state for CID 2
    then_response_is(&mut world, "A", "ERROR");
}

#[test]
fn test_invalid_cid_reports_cme_error() {
    let mut world = World::new();
    given_data_modem(&mut world, "A");

    world.send_and_expect_ok("A", "AT+CMEE=1");

    when_at_command_sent(
        &mut world,
        "A",
        &format!("AT+CGDCONT={TEST_PDP_CID},\"{TEST_PDP_TYPE}\",\"{TEST_APN}\""),
    );
    then_wait_for_response_containing(&mut world, "A", "OK");

    when_at_command_sent(&mut world, "A", &format!("ATD{GPRS_DIAL_STRING_PREFIX}{TEST_PDP_CID}#"));
    then_response_is(&mut world, "A", "CONNECT");

    when_at_command_sent(&mut world, "A", &format!("AT+CGPADDR={INVALID_PDP_CID}"));
    then_response_is(&mut world, "A", "+CME ERROR: 21");

    when_at_command_sent(&mut world, "A", &format!("AT+CGCONTRDP={INVALID_PDP_CID}"));
    then_response_is(&mut world, "A", "+CME ERROR: 21");
}

#[test]
fn test_pdp_type_mismatch_fails() {
    let mut world = World::new();
    // Default data modem has only IPv4 configured
    given_data_modem(&mut world, "A");

    // CMEE=0 (default): generic ERROR.
    when_at_command_sent(&mut world, "A", "AT+CGDCONT=1,\"IPV6\",\"test.apn\"");
    then_wait_for_response_containing(&mut world, "A", "OK");

    when_at_command_sent(&mut world, "A", "ATD*99***1#");
    then_response_is(&mut world, "A", "CONNECT");

    // Address query should fail because no IPv6 config exists
    when_at_command_sent(&mut world, "A", "AT+CGPADDR=1");
    then_response_is(&mut world, "A", "ERROR");

    // Dynamic param query should fail because no IPv6 config exists
    when_at_command_sent(&mut world, "A", "AT+CGCONTRDP=1");
    then_response_is(&mut world, "A", "ERROR");

    // Goldfish/Cuttlefish RIL sets CMEE=1 at init. Emit CME error to avoid 3s
    // HAL timeout.
    world.send_and_expect_ok("A", "AT+CMEE=1");

    when_at_command_sent(&mut world, "A", "AT+CGPADDR=1");
    then_response_is(&mut world, "A", "+CME ERROR: 30");

    when_at_command_sent(&mut world, "A", "AT+CGCONTRDP=1");
    then_response_is(&mut world, "A", "+CME ERROR: 30");
}

#[test]
fn test_ipv6_pdp_context_success() {
    let mut world = World::new();
    let v6_config = netsim_model::CellNetworkConfig {
        ip_address: "fec0::15".parse().unwrap(),
        prefixlen: 64,
        gateway: "fec0::2".parse().unwrap(),
        dns: "fec0::3".parse().unwrap(),
    };
    world.given_modem_with_network_config("A", v6_config);

    // Define IPv6 context on an IPv6-configured modem
    when_at_command_sent(&mut world, "A", "AT+CGDCONT=1,\"IPV6\",\"test.apn\"");
    then_wait_for_response_containing(&mut world, "A", "OK");

    when_at_command_sent(&mut world, "A", "ATD*99***1#");
    then_response_is(&mut world, "A", "CONNECT");

    when_at_command_sent(&mut world, "A", "AT+CGPADDR=1");
    then_response_is(&mut world, "A", "+CGPADDR: 1,\"fec0::15\"");
    then_response_is(&mut world, "A", "OK");

    when_at_command_sent(&mut world, "A", "AT+CGCONTRDP=1");
    then_response_is(
        &mut world,
        "A",
        "+CGCONTRDP: 1,5,\"test.apn\",\"fec0::15/64\",\"fec0::2\",\"fec0::3\"",
    );
    then_response_is(&mut world, "A", "OK");

    // IPv4 context on an IPv6-only modem should fail address query
    when_at_command_sent(&mut world, "A", "AT+CGDCONT=2,\"IP\",\"test2.apn\"");
    then_wait_for_response_containing(&mut world, "A", "OK");

    when_at_command_sent(&mut world, "A", "ATD*99***2#");
    then_response_is(&mut world, "A", "CONNECT");

    when_at_command_sent(&mut world, "A", "AT+CGPADDR=2");
    then_response_is(&mut world, "A", "ERROR");
}

#[test]
fn test_packet_data_gating_matrix() {
    let mut world = World::new();
    given_data_modem(&mut world, "A");

    world.send_and_expect_ok("A", "AT+CMEE=1");
    world.send_and_expect_ok("A", "AT+CGDCONT=1,\"IP\",\"test\"");

    let (id, _) = world.get_modem("A");

    // Radio Off (AT+CFUN=0)
    world.send_and_expect_ok("A", "AT+CFUN=0");
    world.send_and_expect_error("A", "ATD*99***1#", "+CME ERROR: 30");
    world.send_and_expect_error("A", "AT+CGACT=1,1", "+CME ERROR: 30");
    world.send_and_expect_error("A", "AT+CGDATA=\"PPP\",1", "+CME ERROR: 30");
    // Context deactivation allowed even when radio is off
    world.send_and_expect_ok("A", "AT+CGACT=0,1");

    world.send_and_expect_ok("A", "AT+CFUN=1");

    // Radio On + Data Unregistered (CGREG: 0)
    world
        .manager
        .get_modem_mut(id)
        .unwrap()
        .set_data_registration(RegistrationStatus::NotRegistered);
    world.send_and_expect_error("A", "ATD*99***1#", "+CME ERROR: 30");
    world.send_and_expect_error("A", "AT+CGACT=1,1", "+CME ERROR: 30");
    world.send_and_expect_error("A", "AT+CGDATA=\"PPP\",1", "+CME ERROR: 30");
    world.send_and_expect_ok("A", "AT+CGACT=0,1");

    // Radio On + Data Emergency Only (CGREG: 8)
    world.manager.get_modem_mut(id).unwrap().set_data_registration(RegistrationStatus::Emergency);
    world.send_and_expect_error("A", "ATD*99***1#", "+CME ERROR: 32");
    world.send_and_expect_error("A", "AT+CGACT=1,1", "+CME ERROR: 32");
    world.send_and_expect_error("A", "AT+CGDATA=\"PPP\",1", "+CME ERROR: 32");
    world.send_and_expect_ok("A", "AT+CGACT=0,1");

    // Radio On + Data Registered + PS Detached (AT+CGATT=0)
    world
        .manager
        .get_modem_mut(id)
        .unwrap()
        .set_data_registration(RegistrationStatus::RegisteredHome);
    world.send_and_expect_ok("A", "AT+CGATT=0");
    world.send_and_expect_error("A", "ATD*99***1#", "+CME ERROR: 53");
    world.send_and_expect_error("A", "AT+CGACT=1,1", "+CME ERROR: 53");
    world.send_and_expect_error("A", "AT+CGDATA=\"PPP\",1", "+CME ERROR: 53");

    // Radio On + Data Registered + PS Attached (AT+CGATT=1)
    world.send_and_expect_ok("A", "AT+CGATT=1");
    world.send_and_expect("A", "ATD*99***1#", &["CONNECT"]);
    world.send_and_expect_ok("A", "AT+CGACT=0,1");
    world.send_and_expect_ok("A", "AT+CGACT=1,1");
    world.send_and_expect("A", "AT+CGDATA=\"PPP\",1", &["CONNECT"]);
}
