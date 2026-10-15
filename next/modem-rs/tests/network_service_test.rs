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

use netsim_model::RadioTechnology;

use crate::{common::constants::*, steps::*, world::World};

// Scenario: Query Operator Selection
//   Given a modem "A"
//   When AT command "AT+COPS?" is sent to "A"
//   Then response from "A" is "+COPS: 0,2,310260"
//   When AT command "AT+COPS=3,0" is sent to "A"
//   And AT command "AT+COPS?" is sent to "A"
//   Then response from "A" is '+COPS: 0,0,"Android Virtual Operator"'
#[test]
fn test_cops_query() {
    let mut world = World::new();
    given_modem(&mut world, "A");

    // Enable radio and attach
    when_at_command_sent(&mut world, "A", "AT+CFUN=1");
    then_response_is(&mut world, "A", "OK");
    when_time_advances_ms(&mut world, 10);
    let csq_response = RESP_CSQ_LTE_DEFAULT;
    then_response_is(&mut world, "A", csq_response);

    // 1. Default should be format 2 (numeric)
    when_at_command_sent(&mut world, "A", "AT+COPS?");
    then_response_is(&mut world, "A", &format!("+COPS: 0,2,\"{TEST_PLMN}\""));
    then_response_is(&mut world, "A", "OK");

    // 2. Set format to 0 (long alphanumeric)
    when_at_command_sent(&mut world, "A", "AT+COPS=3,0");
    then_response_is(&mut world, "A", "OK");

    when_at_command_sent(&mut world, "A", "AT+COPS?");
    then_response_is(&mut world, "A", &format!("+COPS: 0,0,\"{TEST_OPERATOR_LONG}\""));
    then_response_is(&mut world, "A", "OK");

    // 3. Set format to 1 (short alphanumeric)
    when_at_command_sent(&mut world, "A", "AT+COPS=3,1");
    then_response_is(&mut world, "A", "OK");

    when_at_command_sent(&mut world, "A", "AT+COPS?");
    then_response_is(&mut world, "A", &format!("+COPS: 0,1,\"{TEST_OPERATOR_SHORT}\""));
    then_response_is(&mut world, "A", "OK");
}

#[test]
fn test_cops_modes() {
    let mut world = World::new();
    given_modem(&mut world, "A");

    let csq_response = RESP_CSQ_LTE_DEFAULT;

    // Enable CREG URCs
    when_at_command_sent(&mut world, "A", "AT+CREG=1");
    then_response_is(&mut world, "A", "OK");

    // Initially we are NOT attached (startup)
    when_at_command_sent(&mut world, "A", "AT+CFUN=1");
    then_response_is(&mut world, "A", "OK");
    when_time_advances_ms(&mut world, 10);
    then_response_is(&mut world, "A", "+CREG: 1");
    then_response_is(&mut world, "A", csq_response);

    // 1. Test Deregister (AT+COPS=2)
    when_at_command_sent(&mut world, "A", "AT+COPS=2");
    then_response_is(&mut world, "A", "+CREG: 0");
    then_response_is(&mut world, "A", "OK");

    // Query should show mode 2
    when_at_command_sent(&mut world, "A", "AT+COPS?");
    then_response_is(&mut world, "A", "+COPS: 2");
    then_response_is(&mut world, "A", "OK");

    // 2. Test Auto Register (AT+COPS=0)
    when_at_command_sent(&mut world, "A", "AT+COPS=0");
    then_response_is(&mut world, "A", "+CREG: 1");
    then_response_is(&mut world, "A", csq_response);
    then_response_is(&mut world, "A", "OK");

    when_at_command_sent(&mut world, "A", "AT+COPS?");
    then_response_is(&mut world, "A", &format!("+COPS: 0,2,\"{TEST_PLMN}\""));
    then_response_is(&mut world, "A", "OK");

    // 3. Test Manual Register to wrong operator (AT+COPS=1,2,"123456")
    when_at_command_sent(&mut world, "A", &format!("AT+COPS=1,2,\"{INVALID_PLMN}\""));
    // Registration denied (3)
    then_response_is(&mut world, "A", "+CREG: 3");
    then_response_is(&mut world, "A", "ERROR");

    // Query should show mode 1 (retains Manual mode per 3GPP TS 27.007 § 7.3)
    when_at_command_sent(&mut world, "A", "AT+COPS?");
    then_response_is(&mut world, "A", "+COPS: 1");
    then_response_is(&mut world, "A", "OK");

    // 4. Test Manual Register to correct operator (AT+COPS=1,0,"Android Virtual
    //    Operator")
    when_at_command_sent(&mut world, "A", &format!("AT+COPS=1,0,\"{TEST_OPERATOR_LONG}\""));
    then_response_is(&mut world, "A", "+CREG: 1");
    then_response_is(&mut world, "A", csq_response);
    then_response_is(&mut world, "A", "OK");

    when_at_command_sent(&mut world, "A", "AT+COPS?");
    then_response_is(&mut world, "A", &format!("+COPS: 1,0,\"{TEST_OPERATOR_LONG}\""));
    then_response_is(&mut world, "A", "OK");
}

#[test]
fn test_cops_manual_with_act() {
    let mut world = World::new();
    world.given_modem("A");

    world.send_and_expect_ok("A", "AT+CFUN=1");

    // 1. Set manual with GSM (AcT = 0). Since not attached yet, attach emits GSM
    //    CSQ URC followed by OK.
    world.send_and_expect(
        "A",
        &format!("AT+COPS=1,2,\"{TEST_PLMN}\",0"),
        &[RESP_CSQ_GSM_DEFAULT, "OK"],
    );
    world.send_and_expect("A", "AT+CTEC?", &["+CTEC: 1,40", "OK"]);
    world.send_and_expect("A", "AT+CSQ", &[RESP_CSQ_GSM_DEFAULT, "OK"]);

    // 2. Set manual with LTE (AcT = 7). Tech changed while attached, emits LTE CSQ
    //    URC followed by OK.
    world.send_and_expect(
        "A",
        &format!("AT+COPS=1,2,\"{TEST_PLMN}\",7"),
        &[RESP_CSQ_LTE_DEFAULT, "OK"],
    );
    world.send_and_expect("A", "AT+CTEC?", &["+CTEC: 32,40", "OK"]);

    // 3. Set manual with NR (AcT = 11). Tech changed while attached, emits NR CSQ
    //    URC followed by OK.
    world.send_and_expect(
        "A",
        &format!("AT+COPS=1,2,\"{TEST_PLMN}\",11"),
        &[RESP_CSQ_NR_DEFAULT, "OK"],
    );
    world.send_and_expect("A", "AT+CTEC?", &["+CTEC: 64,40", "OK"]);

    world.send_and_expect(
        "A",
        &format!("AT+COPS=1,2,\"{TEST_PLMN}\",3"),
        &[RESP_CSQ_GSM_DEFAULT, "OK"],
    );
    world.send_and_expect("A", "AT+CTEC?", &["+CTEC: 1,40", "OK"]);

    world.send_and_expect(
        "A",
        &format!("AT+COPS=1,2,\"{TEST_PLMN}\",6"),
        &[RESP_CSQ_WCDMA_DEFAULT, "OK"],
    );
    world.send_and_expect("A", "AT+CTEC?", &["+CTEC: 2,40", "OK"]);

    world.send_and_expect_error("A", &format!("AT+COPS=1,2,\"{INVALID_PLMN}\",7"), "ERROR");
    world.send_and_expect("A", "AT+CTEC?", &["+CTEC: 2,40", "OK"]);
    world.send_and_expect("A", "AT+COPS?", &["+COPS: 1", "OK"]);

    world.send_and_expect_error("A", &format!("AT+COPS=1,2,\"{TEST_PLMN}\",99"), "ERROR");
    world.send_and_expect("A", "AT+CTEC?", &["+CTEC: 2,40", "OK"]);
}

#[test]
fn test_cops_unsupported_act_reports_cme_error() {
    let mut world = World::new();
    world.given_modem("A");
    world.send_and_expect_ok("A", "AT+CMEE=1");
    world.send_and_expect_ok("A", "AT+CFUN=1");

    world.send_and_expect_error("A", &format!("AT+COPS=1,2,\"{TEST_PLMN}\",99"), "+CME ERROR: 50");
}

#[test]
fn test_cops_rejected_command_does_not_change_tech() {
    let mut world = World::new();
    world.given_modem("A");
    world.send_and_expect_ok("A", "AT+CFUN=1");

    world.send_and_expect(
        "A",
        &format!("AT+COPS=1,2,\"{TEST_PLMN}\",0"),
        &[RESP_CSQ_GSM_DEFAULT, "OK"],
    );
    world.send_and_expect("A", "AT+CTEC?", &["+CTEC: 1,40", "OK"]);

    world.send_and_expect_error("A", &format!("AT+COPS=1,2,\"{INVALID_PLMN}\",7"), "ERROR");
    world.send_and_expect("A", "AT+CTEC?", &["+CTEC: 1,40", "OK"]);
}

#[test]
fn test_cops_manual_with_act_goldfish_37() {
    let mut world = World::new();
    world.given_goldfish_37_modem("A");

    world.send_and_expect_ok("A", "AT+CFUN=1");

    world.send_and_expect(
        "A",
        &format!("AT+COPS=1,2,\"{TEST_PLMN}\",1"),
        &[RESP_CSQ_GSM_DEFAULT, "OK"],
    );
    world.send_and_expect("A", "AT+CTEC?", &["+CTEC: 1,40", "OK"]);

    world.send_and_expect(
        "A",
        &format!("AT+COPS=1,2,\"{TEST_PLMN}\",2"),
        &[RESP_CSQ_WCDMA_DEFAULT, "OK"],
    );
    world.send_and_expect("A", "AT+CTEC?", &["+CTEC: 2,40", "OK"]);

    world.send_and_expect(
        "A",
        &format!("AT+COPS=1,2,\"{TEST_PLMN}\",3"),
        &[RESP_CSQ_LTE_DEFAULT, "OK"],
    );
    world.send_and_expect("A", "AT+CTEC?", &["+CTEC: 32,40", "OK"]);

    world.send_and_expect(
        "A",
        &format!("AT+COPS=1,2,\"{TEST_PLMN}\",6"),
        &[RESP_CSQ_NR_DEFAULT, "OK"],
    );
    world.send_and_expect("A", "AT+CTEC?", &["+CTEC: 64,40", "OK"]);

    world.send_and_expect_error("A", &format!("AT+COPS=1,2,\"{TEST_PLMN}\",99"), "ERROR");
    world.send_and_expect("A", "AT+CTEC?", &["+CTEC: 64,40", "OK"]);
}

// Scenario: Query Signal Quality
//   Given a modem "A"
//   When AT command "AT+CSQ" is sent to "A"
//   Then response from "A" is "+CSQ: 20,99"
//   And response from "A" is "OK"
#[test]
fn test_csq_query() {
    let mut world = World::new();
    given_modem(&mut world, "A");
    when_at_command_sent(&mut world, "A", "AT+CSQ");

    then_response_is(&mut world, "A", RESP_CSQ_LTE_DEFAULT);
    then_response_is(&mut world, "A", "OK");
}

#[test]
fn test_cops_query_available_operators() {
    let mut world = World::new();
    given_modem(&mut world, "A");
    when_at_command_sent(&mut world, "A", "AT+COPS=?");
    then_response_is(
        &mut world,
        "A",
        "+COPS: (1,\"Android Virtual Operator\",\"Android\",\"310260\",7),,(0,1,2,3,4),(0,1,2)",
    );
    then_response_is(&mut world, "A", "OK");
}

// Scenario: Network Registration on Radio ON
//   Given a modem "A"
//   When AT command "AT+CFUN=0" is sent to "A"
//   And AT command "AT+CREG=1" is sent to "A"
//   And AT command "AT+CGREG=1" is sent to "A"
//   And AT command "AT+CEREG=1" is sent to "A"
//   And AT command "AT+CFUN=1" is sent to "A"
//   Then response from "A" is "+CREG: 1"
//   And response from "A" is "+CGREG: 1"
//   And response from "A" is "+CEREG: 1"
//   And response from "A" is "OK"
#[test]
fn test_network_registration() {
    let mut world = World::new();
    given_modem(&mut world, "A");

    // Turn radio OFF first to simulate clean boot sequence
    when_at_command_sent(&mut world, "A", "AT+CFUN=0");
    then_response_is(&mut world, "A", "OK");

    // Enable unsolicited reports
    when_at_command_sent(&mut world, "A", "AT+CREG=1");
    then_response_is(&mut world, "A", "OK");
    when_at_command_sent(&mut world, "A", "AT+CGREG=1");
    then_response_is(&mut world, "A", "OK");
    when_at_command_sent(&mut world, "A", "AT+CEREG=1");
    then_response_is(&mut world, "A", "OK");

    // Turn radio ON which returns only OK synchronously
    when_at_command_sent(&mut world, "A", "AT+CFUN=1");
    then_response_is(&mut world, "A", "OK");

    // Advance time by 10ms to trigger the AttachNetwork event and send URCs!
    when_time_advances_ms(&mut world, 10);

    // Verify unsolicited reports arrive in correct order
    then_response_is(&mut world, "A", "+CREG: 1");
    then_response_is(&mut world, "A", "+CGREG: 1");
    then_response_is(&mut world, "A", "+CEREG: 1");
    then_response_is(&mut world, "A", RESP_CSQ_LTE_DEFAULT);
}

// Scenario: Set dynamic registration status
//   Given a modem "A"
//   When voice registration is set to Roaming (5)
//   Then unsolicited response from "A" is "+CREG: 5"
//   When data registration is set to Denied (3)
//   Then unsolicited response from "A" is "+CGREG: 3"
#[test]
fn test_set_registration_status() {
    use modem_rs::RegistrationStatus;
    let mut world = World::new();
    given_modem(&mut world, "A");

    // Enable unsolicited reports first to make it spec-compliant
    when_at_command_sent(&mut world, "A", "AT+CREG=1");
    then_response_is(&mut world, "A", "OK");
    when_at_command_sent(&mut world, "A", "AT+CGREG=1");
    then_response_is(&mut world, "A", "OK");

    // Set Voice
    when_voice_registration_set(&mut world, "A", RegistrationStatus::Roaming);
    then_response_is(&mut world, "A", "+CREG: 5");

    // Set Data
    when_data_registration_set(&mut world, "A", RegistrationStatus::Denied);
    then_response_is(&mut world, "A", "+CGREG: 3");
}

// Scenario: Query Extended Signal Quality
//   Given a modem "A"
//   When AT command "AT+CESQ" is sent to "A"
//   Then response from "A" is "OK"
#[test]
fn test_query_extended_signal_quality() {
    let mut world = World::new();
    given_modem(&mut world, "A");
    when_at_command_sent(&mut world, "A", "AT+CESQ");

    then_response_is(&mut world, "A", "OK");
}

// Scenario: Set dynamic signal strength
//   Given a modem "A"
//   When signal strength is set to 25, 0
//   Then response from "A" to "AT+CSQ" is "+CSQ: 25,0"
#[test]
fn test_set_signal_strength() {
    let mut world = World::new();
    given_modem(&mut world, "A");

    // Change technology to GSM first, so GSM signal strength is active and
    // testable!
    when_at_command_sent(&mut world, "A", "AT+CTEC=1,\"1\"");
    then_response_is(&mut world, "A", "+CTEC: DONE");
    then_response_is(&mut world, "A", "OK");

    // Check default (which should now be 20,99 because we are on GSM)
    when_at_command_sent(&mut world, "A", "AT+CSQ");
    then_response_is(&mut world, "A", RESP_CSQ_GSM_DEFAULT);
    then_response_is(&mut world, "A", "OK");

    // Change value
    let rssi = 25;
    let ber = 0;
    when_signal_strength_set(&mut world, "A", rssi, ber);
    then_response_is(&mut world, "A", &format!("+CSQ: {rssi},{ber},{CSQ_MAX_20}"));

    // Check new value
    when_at_command_sent(&mut world, "A", "AT+CSQ");
    then_response_is(&mut world, "A", &format!("+CSQ: {rssi},{ber},{CSQ_MAX_20}"));
    then_response_is(&mut world, "A", "OK");
}

// Scenario: Network Registration after Radio Cycle (ON -> OFF -> ON)
//   Given a modem "A"
//   When AT command "AT+CREG=1" is sent to "A"
//   And AT command "AT+CFUN=1" is sent to "A"
//   And time advances 10 ms
//   Then response from "A" is "+CREG: 1"
//   And response from "A" is "+CSQ: 20,99"
//   When AT command "AT+CFUN=0" is sent to "A"
//   Then response from "A" is "+CREG: 0"
//   And response from "A" is "OK"
//   When AT command "AT+CFUN=1" is sent to "A"
//   And time advances 10 ms
//   Then response from "A" is "+CREG: 1"
//   And response from "A" is "+CSQ: 20,99"
#[test]
fn test_network_registration_radio_cycle() {
    let mut world = World::new();
    given_modem(&mut world, "A");

    // 1. Turn radio ON and check reactive URCs
    when_at_command_sent(&mut world, "A", "AT+CREG=1");
    then_response_is(&mut world, "A", "OK");
    when_at_command_sent(&mut world, "A", "AT+CFUN=1");
    then_response_is(&mut world, "A", "OK");

    when_time_advances_ms(&mut world, 10);
    then_response_is(&mut world, "A", "+CREG: 1");
    then_response_is(&mut world, "A", RESP_CSQ_LTE_DEFAULT);

    // 2. Turn radio OFF (drops registration and sends URC synchronously)
    when_at_command_sent(&mut world, "A", "AT+CFUN=0");
    then_response_is(&mut world, "A", "+CREG: 0");
    then_response_is(&mut world, "A", "OK");

    // 3. Turn radio ON again and verify reactive URC triggers again!
    when_at_command_sent(&mut world, "A", "AT+CFUN=1");
    then_response_is(&mut world, "A", "OK");

    when_time_advances_ms(&mut world, 10);
    then_response_is(&mut world, "A", "+CREG: 1");
    then_response_is(&mut world, "A", RESP_CSQ_LTE_DEFAULT);
}

// Scenario: Query Operator in All Formats (Compound Query)
//   Given a modem "A"
//   When AT command "AT+COPS=3,0;+COPS?;+COPS=3,1;+COPS?;+COPS=3,2;+COPS?" is
// sent to "A"   Then response from "A" is "+COPS: 0,0,\"Android Virtual
// Operator\""   And response from "A" is "+COPS: 0,1,\"Android\""
//   And response from "A" is "+COPS: 0,2,310260"
//   And response from "A" is "OK"
#[test]
fn test_query_operator_all_formats() {
    let mut world = World::new();
    given_modem(&mut world, "A");

    // Enable radio and attach
    when_at_command_sent(&mut world, "A", "AT+CFUN=1");
    then_response_is(&mut world, "A", "OK");
    when_time_advances_ms(&mut world, 10);
    let csq_response = RESP_CSQ_LTE_DEFAULT;
    then_response_is(&mut world, "A", csq_response);

    when_at_command_sent(&mut world, "A", "AT+COPS=3,0;+COPS?;+COPS=3,1;+COPS?;+COPS=3,2;+COPS?");
    then_response_is(&mut world, "A", &format!("+COPS: 0,0,\"{TEST_OPERATOR_LONG}\""));
    then_response_is(&mut world, "A", &format!("+COPS: 0,1,\"{TEST_OPERATOR_SHORT}\""));
    then_response_is(&mut world, "A", &format!("+COPS: 0,2,\"{TEST_PLMN}\""));
    then_response_is(&mut world, "A", "OK");
}

// Scenario: Query Current Network Technology Mode (AT+CTEC?)
//   Given a modem "A"
//   When AT command "AT+CTEC?" is sent to "A"
//   Then response from "A" is "+CTEC: 32,40"
//   And response from "A" is "OK"
#[test]
fn test_query_current_ctec() {
    let mut world = World::new();
    given_modem(&mut world, "A");
    when_at_command_sent(&mut world, "A", "AT+CTEC?");
    then_response_is(&mut world, "A", "+CTEC: 32,40");
    then_response_is(&mut world, "A", "OK");
}

// Scenario: Query Supported Network Technology Modes (AT+CTEC=?)
//   Given a modem "A"
//   When AT command "AT+CTEC=?" is sent to "A"
//   Then response from "A" is "+CTEC: 0,1,5"
//   And response from "A" is "OK"
#[test]
fn test_query_supported_ctec() {
    let mut world = World::new();
    given_modem(&mut world, "A");
    when_at_command_sent(&mut world, "A", "AT+CTEC=?");
    then_response_is(&mut world, "A", "+CTEC: 0,1,5");
    then_response_is(&mut world, "A", "OK");
}

// Scenario: Set Network Technology Mode (AT+CTEC=current,preferred)
//   Given a modem "A"
//   When AT command "AT+CTEC=1,"21"" is sent to "A"
//   Then response from "A" is "+CTEC: DONE"
//   And response from "A" is "OK"
#[test]
fn test_set_ctec() {
    let mut world = World::new();
    given_modem(&mut world, "A");
    when_at_command_sent(&mut world, "A", "AT+CTEC=1,\"21\"");
    then_response_is(&mut world, "A", "+CTEC: DONE");
    then_response_is(&mut world, "A", "OK");

    // Verify that values are updated and queried back correctly
    when_at_command_sent(&mut world, "A", "AT+CTEC?");
    then_response_is(&mut world, "A", "+CTEC: 1,21");
    then_response_is(&mut world, "A", "OK");
}

#[test]
fn test_set_ctec_invalid() {
    let mut world = World::new();
    world.given_modem("A");
    world.send_and_expect_ok("A", "AT+CMEE=1");

    world.send_and_expect("A", "AT+CTEC=32,\"21\"", &["+CTEC: DONE", "OK"]);
    world.send_and_expect("A", "AT+CTEC?", &["+CTEC: 32,21", "OK"]);
    world.send_and_expect_error("A", "AT+CTEC=5,\"21\"", "+CME ERROR: 50");
    world.send_and_expect_error("A", "AT+CTEC=99,\"21\"", "+CME ERROR: 50");
    world.send_and_expect_error("A", "AT+CTEC=1,\"80\"", "+CME ERROR: 50");
    world.send_and_expect_error("A", "AT+CTEC=4,\"21\"", "+CME ERROR: 50");
    world.send_and_expect_error("A", "AT+CTEC=8,\"21\"", "+CME ERROR: 50");
    world.send_and_expect_error("A", "AT+CTEC=16,\"21\"", "+CME ERROR: 50");
    world.send_and_expect("A", "AT+CTEC=1,\"4\"", &["+CTEC: DONE", "OK"]);

    world.given_goldfish_37_modem("GF");
    world.send_and_expect_ok("GF", "AT+CMEE=1");
    world.send_and_expect("GF", "AT+CTEC=1,\"21\"", &["+CTEC: DONE", "OK"]);
    world.send_and_expect("GF", "AT+CTEC?", &["+CTEC: 1,21", "OK"]);
    world.send_and_expect_error("GF", "AT+CTEC=5,\"21\"", "+CME ERROR: 50");
    world.send_and_expect("GF", "AT+CTEC=32,\"21\"", &["+CTEC: DONE", "OK"]);
    world.send_and_expect("GF", "AT+CTEC?", &["+CTEC: 32,21", "OK"]);
}

#[test]
fn test_set_ctec_multi_byte_priority() {
    let mut world = World::new();
    given_modem(&mut world, "A");

    // Multi-byte priority masks (e.g. 0x201 = 1st priority GSM (1), 2nd
    // priority WCDMA (2))
    when_at_command_sent(&mut world, "A", "AT+CTEC=1,\"201\"");
    then_response_is(&mut world, "A", "+CTEC: DONE");
    then_response_is(&mut world, "A", "OK");

    // Multi-byte priority mask with LTE (0x20 in byte 2 -> 0x200000)
    when_at_command_sent(&mut world, "A", "AT+CTEC=1,\"200000\"");
    then_response_is(&mut world, "A", "+CTEC: DONE");
    then_response_is(&mut world, "A", "OK");
}

#[test]
fn test_set_ctec_preferred_mask_zero() {
    let mut world = World::new();
    given_modem(&mut world, "A");

    // Setting preferred_mask = 0 represents "automatic" or "no preference"
    when_at_command_sent(&mut world, "A", "AT+CTEC=1,\"0\"");
    then_response_is(&mut world, "A", "+CTEC: DONE");
    then_response_is(&mut world, "A", "OK");

    when_at_command_sent(&mut world, "A", "AT+CTEC?");
    then_response_is(&mut world, "A", "+CTEC: 1,0");
    then_response_is(&mut world, "A", "OK");
}

#[test]
fn test_set_ctec_wcdma_and_urc() {
    let mut world = World::new();
    given_modem(&mut world, "A");

    // 1. Attach to network and enable unsolicited reports with format 2 (includes
    //    AcT!)
    when_at_command_sent(&mut world, "A", "AT+CREG=2");
    then_response_is(&mut world, "A", "OK");
    when_at_command_sent(&mut world, "A", "AT+CGREG=2");
    then_response_is(&mut world, "A", "OK");
    when_at_command_sent(&mut world, "A", "AT+CEREG=2");
    then_response_is(&mut world, "A", "OK");

    when_at_command_sent(&mut world, "A", "AT+CFUN=1");
    then_response_is(&mut world, "A", "OK");
    when_time_advances_ms(&mut world, 10);

    // Consume the initial attachment URCs (which default to LTE act = 7!)
    then_response_is(&mut world, "A", &format!("+CREG: 1,\"{TEST_LAC}\",\"{TEST_CID}\",7"));
    then_response_is(&mut world, "A", &format!("+CGREG: 1,\"{TEST_LAC}\",\"{TEST_CID}\",7"));
    then_response_is(&mut world, "A", &format!("+CEREG: 1,\"{TEST_LAC}\",\"{TEST_CID}\",7"));
    then_response_is(&mut world, "A", RESP_CSQ_LTE_DEFAULT);

    // 2. Switch technology to WCDMA (current mode = 2, preferred mask = 0x21)
    when_at_command_sent(&mut world, "A", "AT+CTEC=2,\"21\"");

    // Verify CTEC response and the IMMEDIATE unsolicited URCs showing WCDMA act
    // = 2!
    then_response_is(&mut world, "A", "+CTEC: DONE");
    then_response_is(&mut world, "A", &format!("+CREG: 1,\"{TEST_LAC}\",\"{TEST_CID}\",2"));
    then_response_is(&mut world, "A", &format!("+CGREG: 1,\"{TEST_LAC}\",\"{TEST_CID}\",2"));
    then_response_is(&mut world, "A", &format!("+CEREG: 1,\"{TEST_LAC}\",\"{TEST_CID}\",2"));
    then_response_is(&mut world, "A", RESP_CSQ_WCDMA_DEFAULT);
    then_response_is(&mut world, "A", "OK");

    // Verify that tech is queried back correctly as WCDMA (current = 2!)
    when_at_command_sent(&mut world, "A", "AT+CTEC?");
    then_response_is(&mut world, "A", "+CTEC: 2,21");
    then_response_is(&mut world, "A", "OK");
}

#[test]
fn test_cops_mode_4() {
    let mut world = World::new();
    given_modem(&mut world, "A");

    // Enable radio
    when_at_command_sent(&mut world, "A", "AT+CFUN=1");
    then_response_is(&mut world, "A", "OK");
    when_time_advances_ms(&mut world, 10);
    // Consume CSQ report
    let csq_response = RESP_CSQ_LTE_DEFAULT;
    then_response_is(&mut world, "A", csq_response);

    // 1. Missing oper -> ERROR
    when_at_command_sent(&mut world, "A", "AT+COPS=4");
    then_response_is(&mut world, "A", "ERROR");

    when_at_command_sent(&mut world, "A", "AT+COPS=4,0");
    then_response_is(&mut world, "A", "ERROR");

    // 2. Correct operator -> Success (cops_mode = 4)
    when_at_command_sent(&mut world, "A", &format!("AT+COPS=4,0,\"{TEST_OPERATOR_LONG}\""));
    then_response_is(&mut world, "A", "OK");

    when_at_command_sent(&mut world, "A", "AT+COPS?");
    then_response_is(&mut world, "A", &format!("+COPS: 4,0,\"{TEST_OPERATOR_LONG}\""));
    then_response_is(&mut world, "A", "OK");

    // 3. Wrong operator -> Fallback to automatic (cops_mode = 0)
    when_at_command_sent(&mut world, "A", "AT+COPS=4,0,\"Wrong Operator\"");
    then_response_is(&mut world, "A", "OK");

    when_at_command_sent(&mut world, "A", "AT+COPS?");
    then_response_is(&mut world, "A", &format!("+COPS: 0,0,\"{TEST_OPERATOR_LONG}\""));
    then_response_is(&mut world, "A", "OK");
}

#[test]
fn test_csq_nr_technology() {
    let mut world = World::new();
    given_modem(&mut world, "A");

    // 1. Register and check standard LTE CSQ first
    when_at_command_sent(&mut world, "A", "AT+CREG=2");
    then_response_is(&mut world, "A", "OK");
    when_at_command_sent(&mut world, "A", "AT+CFUN=1");
    then_response_is(&mut world, "A", "OK");
    when_time_advances_ms(&mut world, 10);
    then_response_is(&mut world, "A", &format!("+CREG: 1,\"{TEST_LAC}\",\"{TEST_CID}\",7"));
    then_response_is(&mut world, "A", RESP_CSQ_LTE_DEFAULT);

    // 2. Change tech to NR (64 = bitmask for NR)
    when_at_command_sent(&mut world, "A", "AT+CTEC=64,\"21\"");
    then_response_is(&mut world, "A", "+CTEC: DONE");

    // Verify it triggers URC with CREG = 1 and technology clamped to LTE (7)
    then_response_is(&mut world, "A", &format!("+CREG: 1,\"{TEST_LAC}\",\"{TEST_CID}\",7"));
    then_response_is(&mut world, "A", RESP_CSQ_LTE_DEFAULT);
    then_response_is(&mut world, "A", "OK");

    // 3. Query CSQ, verify LTE fields are reported
    when_at_command_sent(&mut world, "A", "AT+CSQ");
    then_response_is(&mut world, "A", RESP_CSQ_LTE_DEFAULT);
    then_response_is(&mut world, "A", "OK");
}

#[test]
fn test_network_registration_radio_cycle_cfun_4() {
    let mut world = World::new();
    given_modem(&mut world, "A");

    // 1. Turn radio ON and check reactive URCs
    when_at_command_sent(&mut world, "A", "AT+CREG=1");
    then_response_is(&mut world, "A", "OK");
    when_at_command_sent(&mut world, "A", "AT+CFUN=1");
    then_response_is(&mut world, "A", "OK");

    when_time_advances_ms(&mut world, 10);
    then_response_is(&mut world, "A", "+CREG: 1");
    then_response_is(&mut world, "A", RESP_CSQ_LTE_DEFAULT);

    // 2. Turn radio to Minimum Functionality (4) (drops registration and sends URC
    //    synchronously)
    when_at_command_sent(&mut world, "A", "AT+CFUN=4");
    then_response_is(&mut world, "A", "+CREG: 0");
    then_response_is(&mut world, "A", "OK");

    // Verify QueryRadioPower returns 4
    when_at_command_sent(&mut world, "A", "AT+CFUN?");
    then_response_is(&mut world, "A", "+CFUN: 4");
    then_response_is(&mut world, "A", "OK");

    // 3. Turn radio ON again and verify reactive URC triggers again!
    when_at_command_sent(&mut world, "A", "AT+CFUN=1");
    then_response_is(&mut world, "A", "OK");

    when_time_advances_ms(&mut world, 10);
    then_response_is(&mut world, "A", "+CREG: 1");
    then_response_is(&mut world, "A", RESP_CSQ_LTE_DEFAULT);
}

#[test]
fn test_radio_power_with_reset() {
    let mut world = World::new();
    world.given_modem("A");

    // Initialize modem and attach to network
    when_at_command_sent(&mut world, "A", "AT+CREG=1");
    then_response_is(&mut world, "A", "OK");

    when_at_command_sent(&mut world, "A", "AT+CFUN=1");
    then_response_is(&mut world, "A", "OK");

    when_time_advances_ms(&mut world, 10);
    then_response_is(&mut world, "A", "+CREG: 1");
    then_response_is(&mut world, "A", RESP_CSQ_LTE_DEFAULT);

    // Send AT+CFUN=1,1 (reset while staying at full power)
    when_at_command_sent(&mut world, "A", "AT+CFUN=1,1");
    then_response_is(&mut world, "A", "+CREG: 0");
    then_response_is(&mut world, "A", "OK");

    // After 10ms delay, modem re-attaches
    when_time_advances_ms(&mut world, 10);
    then_response_is(&mut world, "A", "+CREG: 1");
    then_response_is(&mut world, "A", RESP_CSQ_LTE_DEFAULT);
}

#[test]
fn test_network_registration_queries() {
    for cmd in ["CREG", "CGREG", "CEREG"] {
        let mut world = World::new();
        world.given_modem("A");

        // 1. Query initial status (default unsol mode 0, not registered 0)
        world.send_and_expect("A", &format!("AT+{cmd}?"), &[&format!("+{cmd}: 0,0"), "OK"]);

        // 2. Set unsol mode to 1
        world.send_and_expect_ok("A", &format!("AT+{cmd}=1"));
        world.send_and_expect("A", &format!("AT+{cmd}?"), &[&format!("+{cmd}: 1,0"), "OK"]);

        // 3. Turn radio ON (attaches and registers)
        world.send_and_expect_ok("A", "AT+CFUN=1");
        world.when_time_advances_ms(10);

        // Consume URC (others are disabled by default)
        world.then_response_is("A", &format!("+{cmd}: 1"));
        world.then_response_is("A", RESP_CSQ_LTE_DEFAULT);

        // Query status (should show registered 1,1)
        world.send_and_expect("A", &format!("AT+{cmd}?"), &[&format!("+{cmd}: 1,1"), "OK"]);

        // 4. Set unsol mode to 2 (should return immediate location info URC)
        world.send_and_expect(
            "A",
            &format!("AT+{cmd}=2"),
            &[&format!("+{cmd}: 1,\"{TEST_LAC}\",\"{TEST_CID}\",7"), "OK"],
        );

        // Query status (should show mode 2, status 1, LAC "2142", CID
        // "0000B804", act 7 (LTE))
        world.send_and_expect(
            "A",
            &format!("AT+{cmd}?"),
            &[&format!("+{cmd}: 2,1,\"{TEST_LAC}\",\"{TEST_CID}\",7"), "OK"],
        );
    }
}

#[test]
fn test_network_technology_changes() {
    let mut world = World::new();
    given_modem(&mut world, "A");

    // Enable URCs for CREG, CGREG, CEREG (mode 2 to get location info)
    when_at_command_sent(&mut world, "A", "AT+CREG=2");
    then_response_is(&mut world, "A", "OK");
    when_at_command_sent(&mut world, "A", "AT+CGREG=2");
    then_response_is(&mut world, "A", "OK");
    when_at_command_sent(&mut world, "A", "AT+CEREG=2");
    then_response_is(&mut world, "A", "OK");

    // Turn radio ON
    when_at_command_sent(&mut world, "A", "AT+CFUN=1");
    then_response_is(&mut world, "A", "OK");
    when_time_advances_ms(&mut world, 10);
    // Consume initial registration URCs (default is LTE = 7)
    then_response_is(&mut world, "A", &format!("+CREG: 1,\"{TEST_LAC}\",\"{TEST_CID}\",7"));
    then_response_is(&mut world, "A", &format!("+CGREG: 1,\"{TEST_LAC}\",\"{TEST_CID}\",7"));
    then_response_is(&mut world, "A", &format!("+CEREG: 1,\"{TEST_LAC}\",\"{TEST_CID}\",7"));
    let csq_response = RESP_CSQ_LTE_DEFAULT;
    then_response_is(&mut world, "A", csq_response);

    // 1. Change technology to GSM (act 0)
    when_network_technology_changes(&mut world, "A", RadioTechnology::Gsm);
    when_time_advances_ms(&mut world, 10);
    // Expect URCs with act 0
    then_response_is(&mut world, "A", &format!("+CREG: 1,\"{TEST_LAC}\",\"{TEST_CID}\",0"));
    then_response_is(&mut world, "A", &format!("+CGREG: 1,\"{TEST_LAC}\",\"{TEST_CID}\",0"));
    then_response_is(&mut world, "A", &format!("+CEREG: 1,\"{TEST_LAC}\",\"{TEST_CID}\",0"));
    let gsm_csq_response = RESP_CSQ_GSM_DEFAULT;
    then_response_is(&mut world, "A", gsm_csq_response);

    // 2. Change technology to NR (act 11)
    when_network_technology_changes(&mut world, "A", RadioTechnology::Nr);
    when_time_advances_ms(&mut world, 10);
    // Expect URCs with act 11
    then_response_is(&mut world, "A", &format!("+CREG: 1,\"{TEST_LAC}\",\"{TEST_CID}\",11"));
    then_response_is(&mut world, "A", &format!("+CGREG: 1,\"{TEST_LAC}\",\"{TEST_CID}\",11"));
    then_response_is(&mut world, "A", &format!("+CEREG: 1,\"{TEST_LAC}\",\"{TEST_CID}\",11"));
    let nr_csq_response = RESP_CSQ_NR_DEFAULT;
    then_response_is(&mut world, "A", nr_csq_response);
}

#[test]
fn test_legacy_creg_format() {
    let mut world = World::new();
    given_goldfish_37_modem(&mut world, "A");

    // 1. Query CREG with default unsol_mode (0)
    when_at_command_sent(&mut world, "A", "AT+CREG?");
    then_response_is(&mut world, "A", "+CREG: 0,0");
    then_response_is(&mut world, "A", "OK");

    // 2. Enable unsolicited reports (mode 1)
    when_at_command_sent(&mut world, "A", "AT+CREG=1");
    then_response_is(&mut world, "A", "OK");
    when_at_command_sent(&mut world, "A", "AT+CGREG=1");
    then_response_is(&mut world, "A", "OK");
    when_at_command_sent(&mut world, "A", "AT+CEREG=1");
    then_response_is(&mut world, "A", "OK");

    // 3. Turn radio ON and wait for attachment
    when_at_command_sent(&mut world, "A", "AT+CFUN=1");
    then_response_is(&mut world, "A", "OK");
    when_time_advances_ms(&mut world, 10);

    // URC should be full format even though unsol_mode=1
    then_response_is(&mut world, "A", "+CREG: 1,\"2142\",\"0000B804\",7");
    then_response_is(&mut world, "A", "+CGREG: 1,\"2142\",\"0000B804\",7");
    then_response_is(&mut world, "A", "+CEREG: 1,\"2142\",\"0000B804\",7");

    // Consume CSQ report
    then_response_is(&mut world, "A", RESP_CSQ_LTE_DEFAULT);

    // 4. Query CREG again (now registered, unsol_mode=1)
    when_at_command_sent(&mut world, "A", "AT+CREG?");
    // Should return full format with unsol_mode=1
    then_response_is(&mut world, "A", "+CREG: 1,1,\"2142\",\"0000B804\",7");
    then_response_is(&mut world, "A", "OK");

    world.send_and_expect_ok("A", "AT+CREG=0");
    world.send_and_expect("A", "AT+CREG?", &["+CREG: 0,1,\"2142\",\"0000B804\",7", "OK"]);

    world.send_and_expect("A", "AT+CFUN=0", &["+CGREG: 0", "+CEREG: 0", "OK"]);
    world.send_and_expect("A", "AT+CREG?", &["+CREG: 0,0", "OK"]);
}

#[test]
fn test_unregistered_suppresses_location_tokens() {
    let mut world = World::new();
    world.given_modem("A");

    world.send_and_expect_ok("A", "AT+CREG=2");
    world.send_and_expect("A", "AT+CREG?", &["+CREG: 2,0", "OK"]);

    world.send_and_expect_ok("A", "AT+CFUN=1");
    world.when_time_advances_ms(10);
    world.then_response_is("A", "+CREG: 1,\"2142\",\"0000B804\",7");
    world.then_response_is("A", RESP_CSQ_LTE_DEFAULT);

    world.send_and_expect("A", "AT+CREG?", &["+CREG: 2,1,\"2142\",\"0000B804\",7", "OK"]);

    world.send_and_expect("A", "AT+CFUN=0", &["+CREG: 0", "OK"]);
    world.send_and_expect("A", "AT+CREG?", &["+CREG: 2,0", "OK"]);
}

#[test]
fn test_cts_network_selection_mode_flow_goldfish_37() {
    let mut world = World::new();
    world.given_goldfish_37_modem("A");

    world.send_and_expect_ok("A", "AT+CREG=1");
    world.send_and_expect_ok("A", "AT+CGREG=1");
    world.send_and_expect_ok("A", "AT+CEREG=1");

    world.send_and_expect_ok("A", "AT+CFUN=1");
    world.when_time_advances_ms(10);

    world.then_response_is("A", "+CREG: 1,\"2142\",\"0000B804\",7");
    world.then_response_is("A", "+CGREG: 1,\"2142\",\"0000B804\",7");
    world.then_response_is("A", "+CEREG: 1,\"2142\",\"0000B804\",7");
    world.then_response_is("A", RESP_CSQ_LTE_DEFAULT);

    world.send_and_expect("A", "AT+COPS?", &[&format!("+COPS: 0,2,\"{TEST_PLMN}\""), "OK"]);

    world.send_and_expect(
        "A",
        &format!("AT+COPS=1,2,\"{INVALID_PLMN}\""),
        &["+CREG: 3", "+CGREG: 3", "+CEREG: 3", "OK"],
    );

    world.send_and_expect("A", "AT+COPS?", &["+COPS: 1,2,\"\"", "OK"]);

    world.send_and_expect(
        "A",
        "AT+COPS=0",
        &[
            "+CREG: 1,\"2142\",\"0000B804\",7",
            "+CGREG: 1,\"2142\",\"0000B804\",7",
            "+CEREG: 1,\"2142\",\"0000B804\",7",
            RESP_CSQ_LTE_DEFAULT,
            "OK",
        ],
    );

    world.send_and_expect("A", "AT+COPS?", &[&format!("+COPS: 0,2,\"{TEST_PLMN}\""), "OK"]);
}

#[test]
fn test_cts_network_selection_mode_flow_standard() {
    let mut world = World::new();
    world.given_modem("A");

    world.send_and_expect_ok("A", "AT+CMEE=1");
    world.send_and_expect_ok("A", "AT+CREG=1");
    world.send_and_expect_ok("A", "AT+CGREG=1");
    world.send_and_expect_ok("A", "AT+CEREG=1");

    world.send_and_expect_ok("A", "AT+CFUN=1");
    world.when_time_advances_ms(10);

    world.then_response_is("A", "+CREG: 1");
    world.then_response_is("A", "+CGREG: 1");
    world.then_response_is("A", "+CEREG: 1");
    world.then_response_is("A", RESP_CSQ_LTE_DEFAULT);

    world.send_and_expect("A", "AT+COPS?", &[&format!("+COPS: 0,2,\"{TEST_PLMN}\""), "OK"]);

    world.send_and_expect(
        "A",
        &format!("AT+COPS=1,2,\"{INVALID_PLMN}\""),
        &["+CREG: 3", "+CGREG: 3", "+CEREG: 3", "+CME ERROR: 30"],
    );

    world.send_and_expect("A", "AT+COPS?", &["+COPS: 1", "OK"]);

    world.send_and_expect(
        "A",
        "AT+COPS=0",
        &["+CREG: 1", "+CGREG: 1", "+CEREG: 1", RESP_CSQ_LTE_DEFAULT, "OK"],
    );

    world.send_and_expect("A", "AT+COPS?", &[&format!("+COPS: 0,2,\"{TEST_PLMN}\""), "OK"]);
}

#[test]
fn test_cops_auto_register_without_sim_remains_unregistered() {
    let mut world = World::new();
    world.given_modem("A");

    world.when_sim_status("A", false);
    world.then_response_contains("A", "+CPIN: ABSENT");
    world.send_and_expect_ok("A", "AT+CREG=1");
    world.send_and_expect_ok("A", "AT+CGREG=1");
    world.send_and_expect_ok("A", "AT+CFUN=1");

    world.send_and_expect_ok("A", "AT+COPS=0");
    world.send_and_expect("A", "AT+CREG?", &["+CREG: 1,0", "OK"]);
    world.send_and_expect("A", "AT+CGREG?", &["+CGREG: 1,0", "OK"]);
}
