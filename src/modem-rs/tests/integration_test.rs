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

use crate::{steps::*, world::World};

// Scenario: Two Modem End-to-End Flow (Call and SMS)
//   Given a modem "A"
//   And a modem "B" with number "12345"
//   When AT command "AT+CPIN?" is sent to "A"
//   Then response from "A" is "+CPIN: READY"
//   And response from "A" is "OK"
//   When AT command "AT+COPS?" is sent to "A"
//   Then response from "A" matches "+COPS: ..."
//   And response from "A" is "OK"
//   When AT command "ATD12345;" is sent to "A"
//   Then wait for connection (OK, RING, ATA, OK)
//   When AT command "AT+CLCC" is sent to "A"
//   Then response from "A" shows Active call
//   When AT command "ATH" is sent to "A"
//   Then response from "A" is "OK"
//   When AT command "AT+CLCC" is sent to "A"
//   Then response from "A" is "OK" (No calls)
//   When B sends SMS "hello" to A
//   Then A receives SMS
#[test]
fn test_two_modem_end_to_end_scenario() {
    let mut world = World::new();
    given_modem_with_number(&mut world, "A", "54321");
    given_modem_with_number(&mut world, "B", "12345");

    // Power On Checks
    when_at_command_sent(&mut world, "A", "AT");
    then_response_is(&mut world, "A", "OK");

    when_at_command_sent(&mut world, "A", "AT+INVALID");
    then_response_is(&mut world, "A", "ERROR");

    when_at_command_sent(&mut world, "A", "AT+CPIN?");
    then_response_is(&mut world, "A", "+CPIN: READY");
    then_response_is(&mut world, "A", "OK");

    when_at_command_sent(&mut world, "A", "AT+COPS?");
    then_response_is(&mut world, "A", "+COPS: 0,2,\"310260\"");
    then_response_is(&mut world, "A", "OK");

    // Make Call
    when_at_command_sent(&mut world, "A", "ATD12345;");
    then_response_is(&mut world, "A", "OK");

    then_wait_for_response_containing(&mut world, "B", "RING");
    when_at_command_sent(&mut world, "B", "ATA");
    then_wait_for_response_containing(&mut world, "A", "RING"); // Connected

    // Verify Active Call on A
    when_at_command_sent(&mut world, "A", "AT+CLCC");
    then_wait_for_response_containing(&mut world, "A", "+CLCC: 1,0,0,0,0,12345");
    then_wait_for_response_containing(&mut world, "A", "OK");

    // Hang Up
    when_at_command_sent(&mut world, "A", "ATH");
    then_response_is(&mut world, "A", "OK");

    // B receives hangup URC asynchronously
    then_wait_for_response_containing(&mut world, "B", "NO CARRIER");

    // Verify Idle
    when_at_command_sent(&mut world, "A", "AT+CLCC");
    then_response_is(&mut world, "A", "OK");

    // Send SMS B -> A in text mode
    when_at_command_sent(&mut world, "A", "AT+CMGF=1");
    then_response_is(&mut world, "A", "OK");

    when_at_command_sent(&mut world, "B", "AT+CMGF=1");
    then_response_is(&mut world, "B", "OK");

    when_at_command_sent(&mut world, "B", "AT+CMGS=\"54321\"");
    then_prompt_is(&mut world, "B", "> ");

    let hello_hex = "68656C6C6F1A";
    when_hex_bytes_sent(&mut world, "B", hello_hex);

    then_wait_for_response_containing(&mut world, "B", "+CMGS: ");
    then_response_is(&mut world, "B", "OK");

    then_wait_for_response_containing(&mut world, "A", "+CMT: \"12345\"");
    then_response_is(&mut world, "A", "hello");
}

#[test]
fn test_pdu_mode_end_to_end_scenario() {
    let mut world = World::new();
    // Modem A (receiver): Matches DA in the hardcoded PDU
    given_modem_with_number(&mut world, "A", "18810189440");
    given_modem_with_number(&mut world, "B", "12345");

    when_at_command_sent(&mut world, "B", "AT+CPIN?");
    then_response_is(&mut world, "B", "+CPIN: READY");
    then_response_is(&mut world, "B", "OK");

    when_at_command_sent(&mut world, "B", "AT+CMGS=35");
    then_prompt_is(&mut world, "B", "> ");

    // Send PDU targeting "18810189440" containing 23 chars of 7-bit text.
    let pdu_hex = "0001000D91688118109844F0000017AFD7903AB55A9BBA69D639D4ADCBF99E3DCCAE97011A";
    when_hex_bytes_sent(&mut world, "B", pdu_hex);

    then_wait_for_response_containing(&mut world, "B", "+CMGS: ");
    then_response_is(&mut world, "B", "OK");

    // Verify A receives the converted SMS-DELIVER PDU.
    // SCTS timestamp is dynamic, so we match static prefix and suffix.
    then_wait_for_response_containing(&mut world, "A", "+CMT: ,37");
    let response = then_wait_for_response_containing(
        &mut world,
        "A",
        "17AFD7903AB55A9BBA69D639D4ADCBF99E3DCCAE9701",
    );
    assert!(response.contains("002405812143F50000"));
}

#[test]
fn test_sms_routing_failure_dropped() {
    let mut world = World::new();
    given_modem_with_number(&mut world, "B", "12345");
    given_modem_with_number(&mut world, "A", "54321");

    when_at_command_sent(&mut world, "B", "AT+CMGF=1");
    then_response_is(&mut world, "B", "OK");

    when_at_command_sent(&mut world, "B", "AT+CMGS=\"99999\"");
    then_prompt_is(&mut world, "B", "> ");

    let hello_hex = "68656C6C6F1A";
    when_hex_bytes_sent(&mut world, "B", hello_hex);

    // B should still get OK because the command was accepted and processed
    then_wait_for_response_containing(&mut world, "B", "+CMGS: ");
    then_response_is(&mut world, "B", "OK");

    then_no_response(&mut world, "A");
    then_no_response(&mut world, "B");
}

#[test]
fn test_read_sms_zero_index_does_not_panic() {
    let mut world = World::new();
    given_modem_with_number(&mut world, "A", "12345");

    // AT+CMGR=0 should fail gracefully and not panic the simulator
    when_at_command_sent(&mut world, "A", "AT+CMGR=0");
    then_wait_for_response_containing(&mut world, "A", "ERROR");
}

#[test]
fn test_text_mode_loopback() {
    let mut world = World::new();
    given_modem_with_number(&mut world, "A", "12345");

    when_at_command_sent(&mut world, "A", "AT+CMGF=1");
    then_response_is(&mut world, "A", "OK");

    when_at_command_sent(&mut world, "A", "AT+CMGS=\"12345\"");
    then_prompt_is(&mut world, "A", "> ");

    let hello_hex = "68656C6C6F1A";
    when_hex_bytes_sent(&mut world, "A", hello_hex);

    then_wait_for_response_containing(&mut world, "A", "+CMGS: ");
    then_response_is(&mut world, "A", "OK");

    then_wait_for_response_containing(&mut world, "A", "+CMT: \"12345\"");
    then_response_is(&mut world, "A", "hello");
}

#[test]
fn test_pdu_mode_loopback() {
    let mut world = World::new();
    given_modem_with_number(&mut world, "A", "18810189440");

    when_at_command_sent(&mut world, "A", "AT+CMGS=35");
    then_prompt_is(&mut world, "A", "> ");

    let pdu_hex = "0001000D91688118109844F0000017AFD7903AB55A9BBA69D639D4ADCBF99E3DCCAE97011A";
    when_hex_bytes_sent(&mut world, "A", pdu_hex);

    then_wait_for_response_containing(&mut world, "A", "+CMGS: ");
    then_response_is(&mut world, "A", "OK");

    then_wait_for_response_containing(&mut world, "A", "+CMT: ,40");
    let response = then_wait_for_response_containing(
        &mut world,
        "A",
        "17AFD7903AB55A9BBA69D639D4ADCBF99E3DCCAE9701",
    );
    assert!(response.contains("00240B918118109844F00000"));
}
