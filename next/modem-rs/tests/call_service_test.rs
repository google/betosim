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

// Scenario: Emergency Call
//   Given a modem "A"
//   When AT command "ATD911@;" is sent to "A"
//   Then response from "A" is "OK"
#[test]
fn test_emergency_call() {
    let mut world = World::new();
    world.given_modem("A");
    world.send_and_expect_ok("A", &format!("ATD{}@;", TEST_EMERGENCY_NUMBER));
    world.then_response_is("A", "RING");
    world.assert_clcc("A", &[&format!("+CLCC: 1,0,0,0,0,{TEST_EMERGENCY_NUMBER},{TOA_NATIONAL}")]);
    world.send_and_expect_ok("A", "ATH");
    world.assert_idle("A");
}

// Scenario: Standard Call
//   Given a modem "A"
//   When AT command "ATD1234567;" is sent to "A"
//   Then response from "A" is "OK"
#[test]
fn test_standard_call() {
    let mut world = World::new();
    world.given_modem_with_number("A", TEST_PHONE_NUMBER_LONG_A);

    // Dial international number
    world.send_and_expect_ok("A", &format!("ATD+{TEST_PHONE_NUMBER_ALT};"));

    // Verify CLCC shows Dialing state (2) and International ToA (145)
    world.send_and_expect(
        "A",
        "AT+CLCC",
        &[&format!("+CLCC: 1,0,2,0,0,{TEST_PHONE_NUMBER_ALT},{TOA_INTERNATIONAL}"), "OK"],
    );
    world.then_no_response("A");
}

// Scenario: Query Current Calls
//   Given a modem "A"
//   And a modem "B" with number "111"
//   And a modem "C" with number "222"
//   When AT command "ATD111;" is sent to "A"
//   Then response from "A" is "OK"
//   When AT command "ATA" is sent to "B"
//   Then response from "B" is "OK"
//   When AT command "ATD222;" is sent to "A"
//   Then response from "A" is "OK"
//   When AT command "ATA" is sent to "C"
//   Then response from "C" is "OK"
//   When AT command "AT+CLCC" is sent to "A"
//   Then response from "A" contains "+CLCC: 1,0,1,0,0,111,129"
//   And response from "A" contains "+CLCC: 2,0,0,0,0,222,129"
//   And response from "A" contains "OK"
#[test]
fn test_query_current_calls() {
    let mut world = World::new();
    world.given_modem_with_number("A", TEST_PHONE_NUMBER_LONG_A);
    world.given_modem_with_number("B", CALL_PEER_B);
    world.given_modem_with_number("C", CALL_PEER_C);

    // A calls B, B answers; A calls C, B goes on hold, C answers
    world.connect_call("A", "B");
    world.connect_call("A", "C");

    // Query A's calls
    world.send_and_expect(
        "A",
        "AT+CLCC",
        &[
            &format!("+CLCC: 1,0,1,0,0,{CALL_PEER_B},{TOA_NATIONAL}"),
            &format!("+CLCC: 2,0,0,0,0,{CALL_PEER_C},{TOA_NATIONAL}"),
            "OK",
        ],
    );
}

// Scenario: Call Ring Timeout
//   Given a modem "A"
//   And a modem "B" with number "111"
//   And a modem "C" with number "222"
//   When AT command "ATD111;" is sent to "A"
//   And AT command "ATA" is sent to "B"
//   And AT command "ATD222;" is sent to "A"
//   Then check "C" is ringing (Incoming)
//   When time advances 30100 ms
//   Then check "C" is idle
#[test]
fn test_call_ring_timeout() {
    let mut world = World::new();
    world.given_modem_with_number("A", TEST_PHONE_NUMBER_LONG_A);
    world.given_modem_with_number("B", CALL_PEER_B);
    world.given_modem_with_number("C", CALL_PEER_C);

    // A calls B, B answers
    world.connect_call("A", "B");

    // A calls C
    world.dial_number("A", CALL_PEER_C, "C");

    // Check AT+CLCC on C
    let num_str = TEST_PHONE_NUMBER_LONG_A.strip_prefix('+').unwrap_or(TEST_PHONE_NUMBER_LONG_A);
    world.assert_clcc("C", &[&format!("+CLCC: 1,1,4,0,0,{num_str},{TOA_INTERNATIONAL}")]);

    // Advance time > 30s
    world.when_time_advances_ms(30100);

    world.then_response_is("A", "NO ANSWER");

    // Verify C is idle (AT+CLCC returns just OK)
    world.assert_idle("C");
}

// Scenario: Set Mute
//   Given a modem "A"
//   When AT command "AT+CMUT=1" is sent to "A"
//   Then response from "A" is "OK"
#[test]
fn test_set_mute() {
    let mut world = World::new();
    world.given_modem("A");
    world.send_and_expect_ok("A", "AT+CMUT=1");
}

// Scenario: Query Mute
//   Given a modem "A"
//   When AT command "AT+CMUT=1" is sent to "A"
//   Then response from "A" is "OK"
//   When AT command "AT+CMUT?" is sent to "A"
//   Then response from "A" is "+CMUT: 1"
//   And response from "A" is "OK"
#[test]
fn test_query_mute() {
    let mut world = World::new();
    world.given_modem("A");
    world.send_and_expect_ok("A", "AT+CMUT=1");
    world.send_and_expect("A", "AT+CMUT?", &["+CMUT: 1", "OK"]);
}

// Scenario: Send DTMF
//   Given a modem "A"
//   When AT command "AT+VTS=1" is sent to "A"
//   Then response from "A" is "OK"
#[test]
fn test_send_dtmf() {
    let mut world = World::new();
    world.given_modem("A");
    world.send_and_expect_ok("A", "AT+VTS=1");
}

// Scenario: Set Emergency Mode
//   Given a modem "A"
//   When AT command "AT+WSOS=1" is sent to "A"
//   Then response from "A" is "OK"
//   When AT command "AT+WSOS=0" is sent to "A"
//   Then response from "A" is "OK"
#[test]
fn test_set_emergency_mode() {
    let mut world = World::new();
    world.given_modem("A");
    world.send_and_expect_ok("A", "AT+WSOS=1");
    world.send_and_expect_ok("A", "AT+WSOS=0");
}

// Scenario: Query Emergency Mode
//   Given a modem "A"
//   When AT command "AT+WSOS=1" is sent to "A"
//   Then response from "A" is "OK"
//   When AT command "AT+WSOS?" is sent to "A"
//   Then response from "A" is "+WSOS: 1"
//   And response from "A" is "OK"
//   When AT command "AT+WSOS=0" is sent to "A"
//   Then response from "A" is "OK"
//   When AT command "AT+WSOS?" is sent to "A"
//   Then response from "A" is "+WSOS: 0"
//   And response from "A" is "OK"
#[test]
fn test_query_emergency_mode() {
    let mut world = World::new();
    world.given_modem("A");
    world.send_and_expect_ok("A", "AT+WSOS=1");
    world.send_and_expect("A", "AT+WSOS?", &["+WSOS: 1", "OK"]);
    world.send_and_expect_ok("A", "AT+WSOS=0");
    world.send_and_expect("A", "AT+WSOS?", &["+WSOS: 0", "OK"]);
}

// Scenario: External Incoming Call (Console)
//   Given a modem "A"
//   When external call from "123456" matches "A"
//   Then response from "A" is "RING"
//   And response from "A" contains "+CLIP: \"123456\",129,,,,0"
//   When time, advances > 1s (call ring timeout)
//   Then check "A" is idle
#[test]
fn test_external_incoming_call() {
    let mut world = World::new();
    world.given_modem_with_number("A", TEST_PHONE_NUMBER_LONG_A);

    // Enable CLIP
    world.send_and_expect_ok("A", "AT+CLIP=1");

    // Inject call
    when_incoming_call_received(&mut world, "A", TEST_PHONE_NUMBER);

    // Expect RING
    world.then_response_is("A", "RING");
    // Expect +CLIP
    world.then_response_contains("A", &format!("+CLIP: \"{TEST_PHONE_NUMBER}\",{}", TOA_NATIONAL));

    // Verify call state (Incoming)
    // ID=1, Direction=1(Incoming), State=4(Incoming), Voice=0(Voice), Multiparty=0,
    // Number="123456", Type=129
    world.assert_clcc("A", &[&format!("+CLCC: 1,1,4,0,0,{TEST_PHONE_NUMBER},{}", TOA_NATIONAL)]);

    // Wait for timeout
    world.when_time_advances_ms(30100);

    // Call should be gone
    world.assert_idle("A");
}

// Scenario: External Call Control (Answer/Hangup)
//   Given a modem "A"
//   When AT command "ATD123;" is sent to "A"
//   Then response from "A" is "OK"
//   When external answer triggered for "A"
//   Then response from "A" is "OK" (representing CONNECT)
//   And check call state is Active
//   When external hangup triggered for "A"
//   Then response from "A" is "RING"
//   And check call state is Idle
#[test]
fn test_external_call_control() {
    let mut world = World::new();
    world.given_modem_with_number("A", TEST_PHONE_NUMBER_LONG_A);

    // Dial
    world.send_and_expect_ok("A", &format!("ATD{CALL_PEER_EXT};"));

    // Remote Answer
    when_external_call_answered(&mut world, "A");
    world.then_response_is("A", "OK");

    // Verify Active
    world.assert_clcc("A", &[&format!("+CLCC: 1,0,0,0,0,{CALL_PEER_EXT},{}", TOA_NATIONAL)]);

    // Remote Hangup
    when_external_call_hungup(&mut world, "A");
    world.then_response_is("A", "NO CARRIER");

    // Verify Idle
    world.assert_idle("A");
}

// Scenario: External Call Hold
//   Given a modem "A"
//   When AT command "ATD123;" is sent to "A"
//   Then response from "A" is "OK"
//   When external answer triggered
//   Then response from "A" is "OK"
//   When external hold (on) triggered
//   Then AT+CLCC indicates Held (State 1)
//   When external hold (off) triggered
//   Then AT+CLCC indicates Active (State 0)
#[test]
fn test_external_call_hold() {
    let mut world = World::new();
    world.given_modem_with_number("A", TEST_PHONE_NUMBER_LONG_A);

    // Dial and Answer to get Active call
    world.send_and_expect_ok("A", &format!("ATD{CALL_PEER_EXT};"));
    when_external_call_answered(&mut world, "A");
    world.then_response_is("A", "OK");

    // Hold ON
    when_external_call_held(&mut world, "A", true);

    // Check State (Held = 1)
    world.assert_clcc("A", &[&format!("+CLCC: 1,0,1,0,0,{CALL_PEER_EXT},{}", TOA_NATIONAL)]);

    // Hold OFF (Resume)
    when_external_call_held(&mut world, "A", false);

    // Check State (Active = 0)
    world.assert_clcc("A", &[&format!("+CLCC: 1,0,0,0,0,{CALL_PEER_EXT},{}", TOA_NATIONAL)]);
}

#[test]
fn test_dial_speed_dial_does_not_fallback() {
    let mut world = World::new();
    world.given_modem_with_number("A", TEST_PHONE_NUMBER_LONG_A);

    // Dial *999# which starts with *99 and ends with #, but is not standard GPRS
    // dial format. It should NOT fall back, but remain handled by CallService
    // as a normal voice dial attempt (returns OK). If it had fallen back, it
    // would have returned CONNECT!
    world.send_and_expect_ok("A", "ATD*999#");
}

#[test]
fn test_emergency_dial_syntax() {
    let mut world = World::new();
    world.given_modem_with_number("A", TEST_PHONE_NUMBER_LONG_A);

    // Emergency with category and CLIR
    world.send_and_expect_ok("A", &format!("ATD{}@1,#I;", TEST_EMERGENCY_NUMBER));
    world.then_response_is("A", "RING");

    // Verify it is in active calls in Active state (answered directly)
    world.assert_clcc("A", &[&format!("+CLCC: 1,0,0,0,0,{TEST_EMERGENCY_NUMBER},{TOA_NATIONAL}")]);

    // Hang up the emergency call
    world.send_and_expect_ok("A", "ATH");
    world.assert_idle("A");

    // Normal call to non-existent peer should be in Dialing state
    world.send_and_expect_ok("A", &format!("ATD{CALL_PEER_ALT_2};"));
    world.assert_clcc("A", &[&format!("+CLCC: 1,0,2,0,0,{CALL_PEER_ALT_2},{TOA_NATIONAL}")]);
}

#[test]
fn test_emergency_call_without_sim() {
    let mut world = World::new();
    world.given_modem("A");

    // Remove SIM (simulate SIM absent)
    world.when_sim_status("A", false);
    world.then_response_is("A", "+CPIN: ABSENT");

    // Enable verbose CME errors
    world.send_and_expect_ok("A", "AT+CMEE=1");

    // Normal calls fail when SIM is absent
    world.send_and_expect("A", &format!("ATD{CALL_PEER_ALT_2};"), &["+CME ERROR: 10"]);
    world.send_and_expect("A", &format!("ATD{TEST_EMERGENCY_NUMBER};"), &["+CME ERROR: 10"]);

    // Emergency calls succeed without SIM
    world.send_and_expect_ok("A", &format!("ATD{}@1,#I;", TEST_EMERGENCY_NUMBER));
    world.then_response_is("A", "RING");
    world.assert_clcc("A", &[&format!("+CLCC: 1,0,0,0,0,{TEST_EMERGENCY_NUMBER},{TOA_NATIONAL}")]);
    world.send_and_expect_ok("A", "ATH");
    world.assert_idle("A");

    world.send_and_expect_ok("A", "ATD999@1,#;");
    world.then_response_is("A", "RING");
    world.assert_clcc("A", &[&format!("+CLCC: 1,0,0,0,0,999,{TOA_NATIONAL}")]);
    world.send_and_expect_ok("A", "ATH");
    world.assert_idle("A");
}

#[test]
fn test_emergency_call_with_sim_pin_locked() {
    let mut world = World::new();
    world.given_modem_with_locked_sim("A");

    world.send_and_expect("A", "AT+CPIN?", &["+CPIN: SIM PIN", "OK"]);
    world.send_and_expect_ok("A", "AT+CMEE=1");

    // Normal calls fail when PIN locked
    world.send_and_expect("A", &format!("ATD{CALL_PEER_ALT_2};"), &["+CME ERROR: 11"]);
    world.send_and_expect("A", &format!("ATD{TEST_EMERGENCY_NUMBER};"), &["+CME ERROR: 11"]);

    // Emergency calls bypass PIN lock
    world.send_and_expect_ok("A", &format!("ATD{}@1,#I;", TEST_EMERGENCY_NUMBER));
    world.then_response_is("A", "RING");
    world.assert_clcc("A", &[&format!("+CLCC: 1,0,0,0,0,{TEST_EMERGENCY_NUMBER},{TOA_NATIONAL}")]);
    world.send_and_expect_ok("A", "ATH");
    world.assert_idle("A");

    world.send_and_expect_ok("A", "ATD999@1,#;");
    world.then_response_is("A", "RING");
    world.assert_clcc("A", &[&format!("+CLCC: 1,0,0,0,0,999,{TOA_NATIONAL}")]);
    world.send_and_expect_ok("A", "ATH");
    world.assert_idle("A");
}

#[test]
fn test_emergency_call_with_sim_puk_locked() {
    let mut world = World::new();
    world.given_modem_with_locked_sim("A");

    world.send_and_expect_ok("A", "AT+CMEE=1");
    for _ in 0..3 {
        world.send_and_expect("A", "AT+CPIN=\"9999\"", &["+CME ERROR: 16"]);
    }
    world.send_and_expect("A", "AT+CPIN?", &["+CPIN: SIM PUK", "OK"]);

    // Normal calls fail when PUK locked
    world.send_and_expect("A", &format!("ATD{CALL_PEER_ALT_2};"), &["+CME ERROR: 12"]);
    world.send_and_expect("A", &format!("ATD{TEST_EMERGENCY_NUMBER};"), &["+CME ERROR: 12"]);

    // Emergency calls bypass PUK lock
    world.send_and_expect_ok("A", &format!("ATD{}@1,#I;", TEST_EMERGENCY_NUMBER));
    world.then_response_is("A", "RING");
    world.assert_clcc("A", &[&format!("+CLCC: 1,0,0,0,0,{TEST_EMERGENCY_NUMBER},{TOA_NATIONAL}")]);
    world.send_and_expect_ok("A", "ATH");
    world.assert_idle("A");

    world.send_and_expect_ok("A", "ATD999@1,#;");
    world.then_response_is("A", "RING");
    world.assert_clcc("A", &[&format!("+CLCC: 1,0,0,0,0,999,{TOA_NATIONAL}")]);
    world.send_and_expect_ok("A", "ATH");
    world.assert_idle("A");
}

#[test]
fn test_wsos_emergency_mode_full_lifecycle() {
    let mut world = World::new();
    world.given_modem_with_number("A", TEST_PHONE_NUMBER_LONG_A);

    world.send_and_expect("A", "AT+WSOS?", &["+WSOS: 0", "OK"]);
    world.send_and_expect_ok("A", "AT+WSOS=1");
    world.send_and_expect("A", "AT+WSOS?", &["+WSOS: 1", "OK"]);

    // Non-emergency dials are rejected while emergency mode is active
    world.send_and_expect_ok("A", "AT+CMEE=1");
    world.send_and_expect("A", &format!("ATD{CALL_PEER_ALT_2};"), &["+CME ERROR: 32"]);
    world.send_and_expect("A", &format!("ATD{TEST_EMERGENCY_NUMBER};"), &["+CME ERROR: 32"]);

    // Emergency dials are permitted
    world.send_and_expect_ok("A", &format!("ATD{}@;", TEST_EMERGENCY_NUMBER));
    world.then_response_is("A", "RING");
    world.assert_clcc("A", &[&format!("+CLCC: 1,0,0,0,0,{TEST_EMERGENCY_NUMBER},{TOA_NATIONAL}")]);
    world.send_and_expect_ok("A", "ATH");
    world.assert_idle("A");

    // Emergency mode persists until disabled
    world.send_and_expect("A", "AT+WSOS?", &["+WSOS: 1", "OK"]);
    world.send_and_expect_ok("A", "AT+WSOS=0");
    world.send_and_expect("A", "AT+WSOS?", &["+WSOS: 0", "OK"]);

    world.send_and_expect_ok("A", &format!("ATD{CALL_PEER_ALT_2};"));
    world.send_and_expect_ok("A", "ATH");
    world.assert_idle("A");
}

#[test]
fn test_emergency_dial_category_marker_syntax() {
    let mut world = World::new();
    world.given_modem("A");

    world.send_and_expect_ok("A", "AT+CMEE=1");
    world.send_and_expect_ok("A", "AT+WSOS=1");
    world.send_and_expect("A", &format!("ATD{CALL_PEER_ALT_2};"), &["+CME ERROR: 32"]);

    for dial in [
        format!("ATD{}@;", TEST_EMERGENCY_NUMBER),
        format!("ATD{}@0;", TEST_EMERGENCY_NUMBER),
        format!("ATD{}@1,#I;", TEST_EMERGENCY_NUMBER),
        format!("ATD{}@1,#i;", TEST_EMERGENCY_NUMBER),
        format!("ATD{}@1,#;", TEST_EMERGENCY_NUMBER),
        format!("ATD{}@,#I;", TEST_EMERGENCY_NUMBER),
        format!("ATD{}@,#;", TEST_EMERGENCY_NUMBER),
        "ATD999@1,#;".to_string(),
        "ATD110@;".to_string(),
    ] {
        world.send_and_expect_ok("A", &dial);
        world.then_response_is("A", "RING");
        world.send_and_expect_ok("A", "ATH");
        world.assert_idle("A");
    }
}

#[test]
fn test_emergency_mode_concurrent_dial_rejected_while_emergency_active() {
    let mut world = World::new();
    world.given_modem("A");

    world.send_and_expect_ok("A", "AT+CMEE=1");
    world.send_and_expect_ok("A", "AT+WSOS=1");

    world.send_and_expect_ok("A", &format!("ATD{}@;", TEST_EMERGENCY_NUMBER));
    world.then_response_is("A", "RING");
    world.assert_clcc("A", &[&format!("+CLCC: 1,0,0,0,0,{TEST_EMERGENCY_NUMBER},{TOA_NATIONAL}")]);

    // Concurrent normal dial rejected while emergency call is active
    world.send_and_expect("A", &format!("ATD{CALL_PEER_ALT_2};"), &["+CME ERROR: 32"]);
    world.assert_clcc("A", &[&format!("+CLCC: 1,0,0,0,0,{TEST_EMERGENCY_NUMBER},{TOA_NATIONAL}")]);

    world.send_and_expect_ok("A", "ATH");
    world.assert_idle("A");
}

#[test]
fn test_dial_clir_semicolon() {
    let mut world = World::new();
    world.given_modem_with_number("A", TEST_PHONE_NUMBER_LONG_A);

    // Dial with CLIR 'i' and semicolon
    world.send_and_expect_ok("A", &format!("ATD{CALL_PEER_ALT_2}i;"));

    // Verify it dialed "12345" (clean number)
    world.assert_clcc("A", &[&format!("+CLCC: 1,0,2,0,0,{CALL_PEER_ALT_2},{}", TOA_NATIONAL)]);
}

#[test]
fn test_invalid_dial_syntax() {
    let mut world = World::new();
    world.given_modem_with_number("A", TEST_PHONE_NUMBER_LONG_A);

    // Plus in the middle should be rejected
    world.send_and_expect_error("A", "ATD123+456;", "ERROR");

    // Multiple pluses should be rejected
    world.send_and_expect_error("A", "ATD++123;", "ERROR");

    // Plus at the beginning should be accepted
    world.send_and_expect_ok("A", &format!("ATD+{CALL_PEER_ALT_2};"));
}

#[test]
fn test_dtmf_validation() {
    let mut world = World::new();
    world.given_modem("A");

    // Valid DTMFs
    world.send_and_expect_ok("A", "AT+VTS=1");
    world.send_and_expect_ok("A", "AT+VTS=*");
    world.send_and_expect_ok("A", "AT+VTS=A,10");

    // Invalid DTMFs
    world.send_and_expect_error("A", "AT+VTS=X", "ERROR");
    world.send_and_expect_error("A", "AT+VTS=12", "ERROR");
    world.send_and_expect_error("A", "AT+VTS=1,A", "ERROR");
}

#[test]
fn test_standard_call_with_leading_plus_routing() {
    let mut world = World::new();
    world.given_modem_with_number("A", TEST_PHONE_NUMBER_LONG_A);
    world.given_modem_with_number("B", TEST_PHONE_NUMBER_LONG_B);

    // A dials B's number with leading '+'
    world.dial_number("A", &format!("+{TEST_PHONE_NUMBER_LONG_B}"), "B");
}

#[test]
fn test_clir_dial_suffixes() {
    let mut world = World::new();
    world.given_modem_with_number("A", TEST_PHONE_NUMBER_LONG_A);
    world.given_modem_with_number("B", TEST_PHONE_NUMBER_LONG_B);

    // Enable CLIP on B
    world.send_and_expect_ok("B", "AT+CLIP=1");

    // Scenario 1: Dial with CLIR suppression 'i' (allow presentation)
    world.send_and_expect_ok("A", &format!("ATD{TEST_PHONE_NUMBER_LONG_B}i;"));

    // B should receive RING and +CLIP presenting A's number
    world.then_response_contains("B", "RING");
    world.then_response_contains("B", &format!("+CLIP: \"{TEST_PHONE_NUMBER_LONG_A}\",145,,,,0"));

    // Hang up
    world.hangup("A");

    // Scenario 2: Dial with CLIR invocation 'I' (restrict presentation)
    world.send_and_expect_ok("A", &format!("ATD{TEST_PHONE_NUMBER_LONG_B}I;"));

    // B should receive RING and +CLIP with restricted caller ID
    world.then_response_contains("B", "RING");
    world.then_response_contains("B", "+CLIP: \"\",129,,,,1");
}

#[test]
fn test_fdn_dial_restriction() {
    let mut world = World::new();
    world.given_modem_with_fdn_sim_profile("A");
    world.given_modem_with_number("B", TEST_PHONE_NUMBER_LONG_B);

    // Enable verbose CME errors
    world.send_and_expect_ok("A", "AT+CMEE=1");

    // By default FDN is disabled, so dialing B's number should succeed
    world.dial_number("A", TEST_PHONE_NUMBER_LONG_B, "B");

    // Hang up B
    world.hangup("B");
    world.then_response_is("A", "BUSY");

    // Enable FDN lock using PIN2 "5678"
    world.send_and_expect_ok("A", "AT+CLCK=\"FD\",1,\"5678\"");

    // Query FDN status -> should be 1 (enabled)
    world.send_and_expect("A", "AT+CLCK=\"FD\",2", &["+CLCK: 1", "OK"]);

    // Try to dial B's number (not in FDN list) -> should fail with CME ERROR 56
    world.send_and_expect_error("A", &format!("ATD{TEST_PHONE_NUMBER_LONG_B};"), "+CME ERROR: 56");

    // Dial number in FDN list (12345) -> should succeed
    world.send_and_expect_ok("A", "ATD12345;");

    // Hang up A
    world.hangup("A");

    // Dial number starting with FDN list entry (1234567) -> should succeed (prefix
    // match)
    world.send_and_expect_ok("A", "ATD1234567;");

    // Hang up A
    world.hangup("A");

    // Dial number that is prefix of FDN entry but shorter (1234) -> should fail
    world.send_and_expect_error("A", "ATD1234;", "+CME ERROR: 56");

    // Disable FDN lock
    world.send_and_expect_ok("A", "AT+CLCK=\"FD\",0,\"5678\"");

    // Dial B's number again -> should succeed now
    world.dial_number("A", TEST_PHONE_NUMBER_LONG_B, "B");
}

#[test]
fn test_fdn_emergency_call_bypass() {
    let mut world = World::new();
    world.given_modem_with_fdn_sim_profile("A");

    // Enable FDN lock using PIN2 "5678"
    world.send_and_expect_ok("A", "AT+CLCK=\"FD\",1,\"5678\"");

    // Emergency calls bypass FDN lock
    world.send_and_expect_ok("A", "ATD911@;");
    world.then_response_is("A", "RING");
    world.send_and_expect_ok("A", "ATH");
    world.assert_idle("A");

    world.send_and_expect_ok("A", "ATD999@1,#;");
    world.then_response_is("A", "RING");
    world.send_and_expect_ok("A", "ATH");
    world.assert_idle("A");
}

#[test]
fn test_emergency_callback_mode_urc() {
    let mut world = World::new();
    world.given_modem("A");

    world.send_and_expect("A", "AT+WSOS?", &["+WSOS: 0", "OK"]);

    world.trigger_emergency_callback_mode("A", true);
    world.then_response_is("A", "+WSOS: 1");
    world.send_and_expect("A", "AT+WSOS?", &["+WSOS: 1", "OK"]);

    world.send_and_expect_ok("A", "AT+WSOS=0");
    world.send_and_expect("A", "AT+WSOS?", &["+WSOS: 0", "OK"]);
}

#[test]
fn test_fdn_pin2_toggle_failure() {
    let mut world = World::new();
    world.given_modem_with_fdn_sim_profile("A");

    // Try to enable FDN lock with INCORRECT PIN2 "0000" -> should fail with ERROR
    world.send_and_expect_error("A", "AT+CLCK=\"FD\",1,\"0000\"", "ERROR");

    // Verify FDN lock status remains 0 (disabled)
    world.send_and_expect("A", "AT+CLCK=\"FD\",2", &["+CLCK: 0", "OK"]);
}

#[test]
fn test_clir_clcc_number_hiding() {
    let mut world = World::new();
    world.given_modem_with_number("A", TEST_PHONE_NUMBER_LONG_A);
    world.given_modem_with_number("B", TEST_PHONE_NUMBER_LONG_B);

    // Enable CLIP on B
    world.send_and_expect_ok("B", "AT+CLIP=1");

    // Dial B from A with CLIR invocation 'I' (restrict presentation)
    world.send_and_expect_ok("A", &format!("ATD{TEST_PHONE_NUMBER_LONG_B}I;"));

    // B should receive RING and +CLIP with restricted caller ID
    world.then_response_contains("B", "RING");
    world.then_response_contains("B", "+CLIP: \"\",129,,,,1");

    // B queries current calls list -> should show incoming call with hidden caller
    // number
    world.assert_clcc("B", &["+CLCC: 1,1,4,0,0,,129"]);
}

#[test]
fn test_fdn_international_matching() {
    let mut world = World::new();
    world.given_modem_with_fdn_sim_profile("A");

    // Enable FDN lock using PIN2 "5678"
    world.send_and_expect_ok("A", "AT+CLCK=\"FD\",1,\"5678\"");

    // Dial international FDN number with '+' prefix -> should succeed
    world.send_and_expect_ok("A", "ATD+16505550100;");

    // Hang up A
    world.hangup("A");

    // Dial same international number WITHOUT '+' prefix -> should also succeed due
    // to normalization
    world.send_and_expect_ok("A", "ATD16505550100;");
}

#[test]
fn test_dial_with_pause_modifier() {
    let mut world = World::new();
    world.given_modem_with_number("A", "987654");
    world.given_modem_with_number("B", "123456");

    // Enable CLIP on B to receive caller ID
    world.send_and_expect_ok("B", "AT+CLIP=1");

    // Dial B's number (123456) from A with a pause modifier and DTMF suffix
    world.send_and_expect_ok("A", "ATD123456,1234;");

    // Verify B receives the incoming RING and +CLIP with A's number
    world.then_response_contains("B", "RING");
    world.then_response_contains("B", "+CLIP: \"987654\",129,,,,0");

    // A queries current calls list -> should show dialing/active call with B's
    // clean number
    world.assert_clcc("A", &["+CLCC: 1,0,2,0,0,123456,129"]);
}

#[test]
fn test_external_incoming_call_answered_ata() {
    let mut world = World::new();
    world.given_modem_with_number("A", TEST_PHONE_NUMBER_LONG_A);

    // Enable CLIP
    world.send_and_expect_ok("A", "AT+CLIP=1");

    // Inject external incoming call
    when_incoming_call_received(&mut world, "A", TEST_PHONE_NUMBER);
    world.then_response_is("A", "RING");
    world.then_response_contains("A", &format!("+CLIP: \"{TEST_PHONE_NUMBER}\",{}", TOA_NATIONAL));

    // Answer incoming call with ATA
    world.send_and_expect_ok("A", "ATA");

    // Verify call is now Active (State 0)
    world.assert_clcc("A", &[&format!("+CLCC: 1,1,0,0,0,{TEST_PHONE_NUMBER},{}", TOA_NATIONAL)]);

    // Hang up active call
    world.hangup("A");

    // Verify Idle
    world.assert_idle("A");
}

#[test]
fn test_single_incoming_call_declined_udub_chld0() {
    let mut world = World::new();
    world.given_modem_with_number("A", "333");
    world.given_modem_with_number("B", "111");

    // B calls A
    world.dial("B", "A");

    // Callee A rejects the incoming ringing call with AT+CHLD=0 (UDUB)
    world.release_held_calls("A");

    // Calling peer B receives remote hangup URC
    then_wait_for_response_containing(&mut world, "B", "BUSY");

    // Verify both modems are now idle
    world.assert_idle("A");
    world.assert_idle("B");
}

#[test]
fn test_dial_while_already_dialing_returns_error() {
    let mut world = World::new();
    world.given_modem_with_number("A", "333");
    world.given_modem_with_number("B", "111");

    // Enable verbose CME errors
    world.send_and_expect_ok("A", "AT+CMEE=1");

    // A initiates call to B (remains in Dialing state until B answers)
    world.dial("A", "B");

    // Attempt to dial another number while dialing call is pending -> CME ERROR 3
    // (Operation not allowed)
    world.send_and_expect_error("A", "ATD222;", "+CME ERROR: 3");

    // Disable verbose CME errors and verify generic ERROR
    world.send_and_expect_ok("A", "AT+CMEE=0");
    world.send_and_expect_error("A", "ATD222;", "ERROR");
}

#[test]
fn test_empty_call_list_teardown_idempotency() {
    let mut world = World::new();
    world.given_modem("A");

    // When call list is empty, release actions must succeed idempotently with OK
    world.send_and_expect_ok("A", "AT+CHLD=0");
    world.send_and_expect_ok("A", "AT+CHLD=1");
    world.send_and_expect_ok("A", "AT+CHLD=11");

    // Conference on empty call list should return ERROR
    world.send_and_expect_error("A", "AT+CHLD=3", "ERROR");

    // ATH on idle modem succeeds idempotently with OK (3GPP TS 22.030 §4.5.5.1)
    world.send_and_expect_ok("A", "ATH");
}

#[test]
fn test_voice_call_registration_admission_matrix() {
    let mut world = World::new();
    world.given_modem("A");
    world.send_and_expect_ok("A", "AT+CMEE=1");

    let (id, _) = world.get_modem("A");

    // Radio Off (CFUN=0)
    world.send_and_expect_ok("A", "AT+CFUN=0");
    world.send_and_expect_error("A", "ATD123456;", "+CME ERROR: 30");
    world.send_and_expect_error("A", "ATD911@;", "+CME ERROR: 30");

    // Airplane mode / DisableRf (CFUN=4)
    world.send_and_expect_ok("A", "AT+CFUN=4");
    world.send_and_expect_error("A", "ATD123456;", "+CME ERROR: 30");
    world.send_and_expect_error("A", "ATD911@;", "+CME ERROR: 30");

    world.send_and_expect_ok("A", "AT+CFUN=1");

    // Unregistered (NotRegistered, Searching, Denied)
    for status in [
        RegistrationStatus::NotRegistered,
        RegistrationStatus::Searching,
        RegistrationStatus::Denied,
    ] {
        world.manager.get_modem_mut(id).unwrap().set_voice_registration(status);
        world.send_and_expect_error("A", "ATD123456;", "+CME ERROR: 30");
        // Emergency calls bypass registration per 3GPP TS 22.101 §10.1
        world.send_and_expect_ok("A", "ATD911@;");
        world.then_response_is("A", "RING");
        world.hangup("A");
    }

    // Emergency Only (Status 8)
    world.manager.get_modem_mut(id).unwrap().set_voice_registration(RegistrationStatus::Emergency);
    world.send_and_expect_error("A", "ATD123456;", "+CME ERROR: 32");
    world.send_and_expect_ok("A", "ATD911@;");
    world.then_response_is("A", "RING");
    world.hangup("A");

    world
        .manager
        .get_modem_mut(id)
        .unwrap()
        .set_voice_registration(RegistrationStatus::RegisteredHome);
    world.send_and_expect_ok("A", "ATD123456;");
    world.hangup("A");
    world.send_and_expect_ok("A", "ATD911@;");
    world.then_response_is("A", "RING");
    world.hangup("A");

    // Registered Roaming (Status 5)
    world.manager.get_modem_mut(id).unwrap().set_voice_registration(RegistrationStatus::Roaming);
    world.send_and_expect_ok("A", "ATD123456;");
    world.hangup("A");
    world.send_and_expect_ok("A", "ATD911@;");
    world.then_response_is("A", "RING");
    world.hangup("A");
}

#[test]
fn test_mt_incoming_call_delivery_matrix() {
    let mut world = World::new();
    world.given_modem_with_number("A", TEST_PHONE_NUMBER_LONG_A);
    world.given_modem_with_number("B", TEST_PHONE_NUMBER_LONG_B);

    // Callee Radio Off (AT+CFUN=0): caller dials, callee gets NO RING / NO CLIP
    world.send_and_expect_ok("B", "AT+CFUN=0");
    world.send_and_expect_ok("A", &format!("ATD{TEST_PHONE_NUMBER_LONG_B};"));
    world.then_no_response("B");
    world.then_response_is("A", "RING");
    world.assert_clcc("A", &[]);

    let (id_b, _) = world.get_modem("B");

    // Callee Radio On (AT+CFUN=1) but Unregistered: caller dials, callee gets NO
    // RING
    world.send_and_expect_ok("B", "AT+CFUN=1");
    world
        .manager
        .get_modem_mut(id_b)
        .unwrap()
        .set_voice_registration(RegistrationStatus::NotRegistered);
    world.send_and_expect_ok("A", &format!("ATD{TEST_PHONE_NUMBER_LONG_B};"));
    world.then_no_response("B");
    world.then_response_is("A", "RING");
    world.assert_clcc("A", &[]);

    // Callee Radio On (AT+CFUN=1) but Emergency Registration: caller dials, callee
    // gets NO RING
    world
        .manager
        .get_modem_mut(id_b)
        .unwrap()
        .set_voice_registration(RegistrationStatus::Emergency);
    world.send_and_expect_ok("A", &format!("ATD{TEST_PHONE_NUMBER_LONG_B};"));
    world.then_no_response("B");
    world.then_response_is("A", "RING");
    world.assert_clcc("A", &[]);

    // Callee Radio On (AT+CFUN=1) + Registered: caller dials, callee gets RING /
    // CLIP
    world
        .manager
        .get_modem_mut(id_b)
        .unwrap()
        .set_voice_registration(RegistrationStatus::RegisteredHome);
    world.send_and_expect_ok("B", "AT+CLIP=1");
    world.when_time_advances_ms(15); // Wait for attach (RegisteredHome)

    world.send_and_expect_ok("A", &format!("ATD{TEST_PHONE_NUMBER_LONG_B};"));
    world.then_response_contains("B", "RING");
    let num_str = TEST_PHONE_NUMBER_LONG_A.strip_prefix('+').unwrap_or(TEST_PHONE_NUMBER_LONG_A);
    world.then_response_contains("B", &format!("+CLIP: \"{num_str}\""));

    world.send_and_expect_ok("B", "ATA");
    world.then_response_is("A", "RING"); // Caller receives connection indication
    world.assert_clcc("B", &[&format!("+CLCC: 1,1,0,0,0,{num_str},145")]);

    world.hangup("A");
    world.then_response_is("B", "NO CARRIER"); // remote hangup indication
    world.assert_idle("A");
    world.assert_idle("B");
}

#[test]
fn test_goldfish_37_udub_and_timeout_preserves_ring() {
    let mut world = World::new();
    world.given_goldfish_37_modem("A");
    world.given_goldfish_37_modem("B");
    let (id_a, _) = world.get_modem("A");
    let (id_b, _) = world.get_modem("B");
    world
        .manager
        .get_modem_mut(id_a)
        .unwrap()
        .set_voice_registration(RegistrationStatus::RegisteredHome);
    world
        .manager
        .get_modem_mut(id_b)
        .unwrap()
        .set_voice_registration(RegistrationStatus::RegisteredHome);

    world.dial("B", "A");

    // Callee A rejects incoming call with AT+CHLD=0 (UDUB)
    world.release_held_calls("A");

    world.then_response_is("B", "RING");
    world.assert_idle("A");
    world.assert_idle("B");

    world.dial("B", "A");
    world.when_time_advances_ms(30100);
    world.then_response_is("B", "RING");
    world.assert_idle("A");
    world.assert_idle("B");

    world.connect_call("B", "A");
    world.hangup("A");
    world.then_response_is("B", "RING");
    world.assert_idle("A");
    world.assert_idle("B");
}
