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

use modem_rs::{
    PhoneNumber,
    profiles::{PROFILE_CTS_XML, PROFILE_DEFAULT_XML, PROFILE_TEL_ALASKA_XML},
};

use crate::{common::constants::*, steps::*, world::World};

// Scenario: Query PIN Status
//   Given a modem "A"
//   When AT command "AT+CPIN?" is sent to "A"
//   Then response from "A" is "+CPIN: READY"
//   And response from "A" is "OK"
#[test]
fn test_cpin_query() {
    let mut world = World::new();
    given_modem(&mut world, "A");
    when_at_command_sent(&mut world, "A", "AT+CPIN?");
    then_response_is(&mut world, "A", "+CPIN: READY");
    then_response_is(&mut world, "A", "OK");
}

// Scenario: Enter PIN
//   Given a modem "A"
//   When AT command 'AT+CPIN="1234"' is sent to "A"
//   Then response from "A" is "OK"
#[test]
fn test_cpin_set() {
    let mut world = World::new();
    given_modem(&mut world, "A");
    when_at_command_sent(&mut world, "A", &format!("AT+CPIN=\"{TEST_PIN}\""));
    then_response_is(&mut world, "A", "OK");
}

// Scenario: CPIN in READY State
//   Given a modem "A"
//   When AT command 'AT+CPINR="SIM PIN"' is sent to "A"
//   Then response from "A" is "+CPINR: \"SIM PIN\",3,3"
//   And response from "A" is "OK"
//   When AT command 'AT+CPIN="0000"' is sent to "A"
//   Then response from "A" is "OK"
//   When AT command 'AT+CPINR="SIM PIN"' is sent to "A"
//   Then response from "A" is "+CPINR: \"SIM PIN\",3,3"
//   And response from "A" is "OK"
#[test]
fn test_cpin_in_ready_state_does_not_consume_retry() {
    let mut world = World::new();
    world.given_modem("A");

    world.send_and_expect_ok("A", "AT+CMEE=1");

    world.send_and_expect("A", "AT+CPINR=\"SIM PIN\"", &["+CPINR: \"SIM PIN\",3,3", "OK"]);
    world.send_and_expect("A", "AT+CPINR=\"SIM PIN2\"", &["+CPINR: \"SIM PIN2\",3,3", "OK"]);

    world.send_and_expect_error("A", &format!("AT+CPIN=\"{INVALID_PIN}\""), "+CME ERROR: 16");

    world.send_and_expect("A", "AT+CPINR=\"SIM PIN\"", &["+CPINR: \"SIM PIN\",3,3", "OK"]);
    world.send_and_expect("A", "AT+CPINR=\"SIM PIN2\"", &["+CPINR: \"SIM PIN2\",2,3", "OK"]);

    world.send_and_expect_ok("A", &format!("AT+CPIN=\"{TEST_PIN}\""));

    world.send_and_expect("A", "AT+CPINR=\"SIM PIN2\"", &["+CPINR: \"SIM PIN2\",3,3", "OK"]);

    world.send_and_expect_error("A", &format!("AT+CPIN=\"{INVALID_PIN}\""), "+CME ERROR: 16");
    world.send_and_expect_error("A", &format!("AT+CPIN=\"{INVALID_PIN}\""), "+CME ERROR: 16");
    world.send_and_expect_error("A", &format!("AT+CPIN=\"{INVALID_PIN}\""), "+CME ERROR: 18");

    world.send_and_expect("A", "AT+CPINR=\"SIM PIN2\"", &["+CPINR: \"SIM PIN2\",0,3", "OK"]);

    world.send_and_expect_error("A", &format!("AT+CPIN=\"{TEST_PIN}\""), "+CME ERROR: 18");
    world.send_and_expect_error("A", &format!("AT+CPIN=\"{INVALID_PIN}\""), "+CME ERROR: 18");

    world.send_and_expect_ok("A", &format!("AT+CPIN=\"{TEST_PUK}\",\"{TEST_PIN}\""));

    world.send_and_expect("A", "AT+CPINR=\"SIM PIN2\"", &["+CPINR: \"SIM PIN2\",3,3", "OK"]);

    world.send_and_expect_ok("A", &format!("AT+CPIN=\"{TEST_PIN}\""));
}

#[test]
fn test_cpin_set_no_quotes() {
    let mut world = World::new();
    given_modem(&mut world, "A");
    when_at_command_sent(&mut world, "A", &format!("AT+CPIN={TEST_PIN}"));
    then_response_is(&mut world, "A", "OK");
}

#[test]
fn test_cpin_set_no_quotes_cmee() {
    let mut world = World::new();
    given_modem(&mut world, "A");
    when_at_command_sent(&mut world, "A", "AT+CMEE=1");
    then_response_is(&mut world, "A", "OK");
    when_at_command_sent(&mut world, "A", &format!("AT+CPIN={TEST_PIN}"));
    then_response_is(&mut world, "A", "OK");
}

// Scenario: Request IMSI
//   Given a modem "A"
//   When AT command "AT+CIMI" is sent to "A"
//   Then response from "A" is "123456789012345"
//   And response from "A" is "OK"
#[test]
fn test_cimi() {
    let mut world = World::new();
    given_modem_with_sim_profile(&mut world, "A");
    when_at_command_sent(&mut world, "A", "AT+CIMI");
    then_response_is(&mut world, "A", TEST_IMSI);
    then_response_is(&mut world, "A", "OK");
}

// Scenario: Request ICCID
//   Given a modem "A"
//   When AT command "AT+CICCID" is sent to "A"
//   Then response from "A" is "89012345678901234567"
//   And response from "A" is "OK"
#[test]
fn test_cicc() {
    let mut world = World::new();
    given_modem_with_sim_profile(&mut world, "A");
    when_at_command_sent(&mut world, "A", "AT+CICCID");
    then_response_is(&mut world, "A", TEST_ICCID);
    then_response_is(&mut world, "A", "OK");
}

// Scenario: Verify PIN Retry Counter
//   Given a modem "A"
//   When AT command 'AT+CPIN="0000"' is sent to "A"
//   Then response from "A" is "ERROR"
//   When AT command 'AT+CPIN="0000"' is sent to "A"
//   Then response from "A" is "ERROR"
//   When AT command 'AT+CPINR="SIM PIN"' is sent to "A"
//   Then response from "A" is "+CPINR: \"SIM PIN\",1,3"
//   And response from "A" is "OK"
#[test]
fn test_pin_retry_counter() {
    let mut world = World::new();
    given_modem_with_locked_sim(&mut world, "A");
    when_at_command_sent(&mut world, "A", &format!("AT+CPIN=\"{INVALID_PIN}\""));
    then_response_is(&mut world, "A", "ERROR");
    when_at_command_sent(&mut world, "A", &format!("AT+CPIN=\"{INVALID_PIN}\""));
    then_response_is(&mut world, "A", "ERROR");
    when_at_command_sent(&mut world, "A", "AT+CPINR=\"SIM PIN\"");
    then_response_is(
        &mut world,
        "A",
        &format!("+CPINR: \"SIM PIN\",{},{}", DEFAULT_PIN_RETRIES - 2, DEFAULT_PIN_RETRIES),
    );
    then_response_is(&mut world, "A", "OK");
}

#[test]
fn test_pin_retry_counter_cpinr() {
    let mut world = World::new();
    given_modem_with_locked_sim(&mut world, "A");
    when_at_command_sent(&mut world, "A", "AT+CPINR=\"SIM PIN\"");
    then_response_is(
        &mut world,
        "A",
        &format!("+CPINR: \"SIM PIN\",{},{}", DEFAULT_PIN_RETRIES, DEFAULT_PIN_RETRIES),
    );
    then_response_is(&mut world, "A", "OK");

    when_at_command_sent(&mut world, "A", &format!("AT+CPIN=\"{INVALID_PIN}\""));
    then_response_is(&mut world, "A", "ERROR");

    when_at_command_sent(&mut world, "A", "AT+CPINR=\"SIM PIN\"");
    then_response_is(
        &mut world,
        "A",
        &format!("+CPINR: \"SIM PIN\",{},{}", DEFAULT_PIN_RETRIES - 1, DEFAULT_PIN_RETRIES),
    );
    then_response_is(&mut world, "A", "OK");

    when_at_command_sent(&mut world, "A", "AT+CPINR=\"SIM PUK\"");
    then_response_is(
        &mut world,
        "A",
        &format!("+CPINR: \"SIM PUK\",{},{}", DEFAULT_PUK_RETRIES, DEFAULT_PUK_RETRIES),
    );
    then_response_is(&mut world, "A", "OK");
}
// Scenario: Open Logical Channel
//   Given a modem "A"
//   When AT command 'AT+CCHO="1234"' is sent to "A"
//   Then response from "A" is "1"
//   And response from "A" is "OK"
#[test]
fn test_open_logical_channel() {
    let mut world = World::new();
    given_modem(&mut world, "A");
    when_at_command_sent(&mut world, "A", &format!("AT+CCHO=\"{TEST_AID}\""));
    then_response_is(&mut world, "A", "1");
    then_response_is(&mut world, "A", "OK");
}

// Scenario: Close Logical Channel
//   Given a modem "A"
//   When AT command 'AT+CCHO="{TEST_AID}"' is sent to "A"
//   Then response from "A" is "1"
//   And response from "A" is "OK"
//   When AT command "AT+CCHC=1" is sent to "A"
//   Then response from "A" is "+CCHC"
//   And response from "A" is "OK"
//   When AT command 'AT+CGLA=1,10,"00A40004022FE2"' is sent to "A"
//   Then response from "A" is "ERROR"
#[test]
fn test_close_logical_channel() {
    let mut world = World::new();
    given_modem(&mut world, "A");
    when_at_command_sent(&mut world, "A", &format!("AT+CCHO=\"{TEST_AID}\""));
    then_response_is(&mut world, "A", "1");
    then_response_is(&mut world, "A", "OK");

    when_at_command_sent(&mut world, "A", "AT+CCHC=1");
    then_response_is(&mut world, "A", "+CCHC");
    then_response_is(&mut world, "A", "OK");

    when_at_command_sent(&mut world, "A", &format!("AT+CGLA=1,10,\"{APDU_SELECT_EF_ICCID}\""));
    then_response_is(&mut world, "A", "ERROR");
}

// Scenario: Transmit Logical Channel
//   Given a modem "A"
//   When AT command 'AT+CCHO="{TEST_AID}"' is sent to "A"
//   Then response from "A" is "1"
//   And response from "A" is "OK"
//   When AT command 'AT+CGLA=1,10,"00A40004022FE2"' is sent to "A"
//   Then response from "A" is '+CGLA: 4,9000'
//   And response from "A" is "OK"
#[test]
fn test_transmit_logical_channel() {
    let mut world = World::new();
    given_modem(&mut world, "A");
    when_at_command_sent(&mut world, "A", &format!("AT+CCHO=\"{TEST_AID}\""));
    then_response_is(&mut world, "A", "1");
    then_response_is(&mut world, "A", "OK");

    when_at_command_sent(&mut world, "A", &format!("AT+CGLA=1,10,\"{APDU_SELECT_EF_ICCID}\""));
    then_response_is(&mut world, "A", &format!("+CGLA: {},{}", RESP_OK.len(), RESP_OK));
    then_response_is(&mut world, "A", "OK");
}

// Scenario: Change Password
//   Given a modem "A" with locked SIM
//   When AT command 'AT+CPWD="SC","1111","4321"' is sent to "A"
//   Then response from "A" is "OK"
//   When AT command 'AT+CPIN="1111"' is sent to "A"
//   Then response from "A" is "ERROR"
//   When AT command 'AT+CPIN="4321"' is sent to "A"
//   Then response from "A" is "OK"
#[test]
fn test_change_password() {
    let mut world = World::new();
    given_modem_with_locked_sim(&mut world, "A");

    when_at_command_sent(
        &mut world,
        "A",
        &format!("AT+CPWD=\"SC\",\"{LOCKED_PIN}\",\"{NEW_PIN}\""),
    );
    then_response_is(&mut world, "A", "OK");

    when_at_command_sent(&mut world, "A", &format!("AT+CPIN=\"{LOCKED_PIN}\""));
    then_response_is(&mut world, "A", "ERROR");

    when_at_command_sent(&mut world, "A", &format!("AT+CPIN=\"{NEW_PIN}\""));
    then_response_is(&mut world, "A", "OK");
}

// Scenario: Set CDMA Subscription Source
//   Given a modem "A"
//   When AT command "AT+CCSS=1" is sent to "A"
//   Then response from "A" is "OK"
//   When AT command "AT+CCSS?" is sent to "A"
//   Then response from "A" is "+CCSS: 1"
//   And response from "A" is "OK"
#[test]
fn test_set_cdma_subscription_source() {
    let mut world = World::new();
    given_modem(&mut world, "A");

    when_at_command_sent(&mut world, "A", "AT+CCSS=1");
    then_response_is(&mut world, "A", "OK");

    when_at_command_sent(&mut world, "A", "AT+CCSS?");
    then_response_is(&mut world, "A", "+CCSS: 1");
    then_response_is(&mut world, "A", "OK");
}

// Scenario: Set CDMA Roaming Preference
//   Given a modem "A"
//   When AT command "AT+WRMP=1" is sent to "A"
//   Then response from "A" is "OK"
//   When AT command "AT+WRMP?" is sent to "A"
//   Then response from "A" is "+WRMP: 1"
//   And response from "A" is "OK"
#[test]
fn test_set_cdma_roaming_preference() {
    let mut world = World::new();
    given_modem(&mut world, "A");

    when_at_command_sent(&mut world, "A", "AT+WRMP=1");
    then_response_is(&mut world, "A", "OK");

    when_at_command_sent(&mut world, "A", "AT+WRMP?");
    then_response_is(&mut world, "A", "+WRMP: 1");
    then_response_is(&mut world, "A", "OK");
}

// Scenario: SIM Authentication
//   Given a modem "A"
//   When AT command 'AT+MBAU="some_data"' is sent to "A"
//   Then response from "A" is "OK"
#[test]
fn test_sim_authentication() {
    let mut world = World::new();
    given_modem(&mut world, "A");
    when_at_command_sent(&mut world, "A", "AT+MBAU=\"some_data\"");
    then_response_is(&mut world, "A", "^MBAU: 0,0000000000000000,00000000");
    then_response_is(&mut world, "A", "OK");
}

#[test]
fn test_cmee_error_formatting_across_modes() {
    let mut world = World::new();
    given_modem(&mut world, "A");
    let (id, _) = world.get_modem("A");
    world.manager.set_sim_status(id, false);
    then_wait_for_response_containing(&mut world, "A", "+CPIN: ABSENT");

    // Mode 0 (Disable): Returns standard ERROR
    when_at_command_sent(&mut world, "A", "AT+CMEE=0");
    then_response_is(&mut world, "A", "OK");
    when_at_command_sent(&mut world, "A", "AT+CPIN?");
    then_response_is(&mut world, "A", "ERROR");

    // Mode 1 (Numeric): Returns "+CME ERROR: 10" (SIM not inserted)
    when_at_command_sent(&mut world, "A", "AT+CMEE=1");
    then_response_is(&mut world, "A", "OK");
    when_at_command_sent(&mut world, "A", "AT+CPIN?");
    then_response_is(&mut world, "A", CME_ERROR_SIM_NOT_INSERTED_NUMERIC);

    // Mode 2 (Verbose): Returns "+CME ERROR: SIM not inserted"
    when_at_command_sent(&mut world, "A", "AT+CMEE=2");
    then_response_is(&mut world, "A", "OK");
    when_at_command_sent(&mut world, "A", "AT+CPIN?");
    then_response_is(&mut world, "A", CME_ERROR_SIM_NOT_INSERTED_VERBOSE);
}
// Scenario: Restricted SIM Access (AT+CRSM) read and write EF files
#[test]
fn test_sim_crsm_read_write() {
    let mut world = World::new();
    given_modem_with_sim_profile(&mut world, "A");

    // Read ICCID (2FE2)
    when_at_command_sent(
        &mut world,
        "A",
        &format!("AT+CRSM={},{},0,0,10", CRSM_READ_BINARY, EF_ICCID_DEC),
    ); // 176=Read Binary, 12258=0x2FE2
    then_response_is(&mut world, "A", &format!("+CRSM: 144,0,{}", TEST_ICCID_SWAPPED));
    then_response_is(&mut world, "A", "OK");

    // Write new ICCID via UPDATE BINARY (214 / 0xD6)
    when_at_command_sent(
        &mut world,
        "A",
        &format!(
            "AT+CRSM={},{},0,0,10,\"{}\"",
            CRSM_UPDATE_BINARY, EF_ICCID_DEC, NEW_ICCID_SWAPPED
        ),
    );
    then_response_is(&mut world, "A", "+CRSM: 144,0");
    then_response_is(&mut world, "A", "OK");

    // Verify it updated the ICCID
    when_at_command_sent(&mut world, "A", "AT+CICCID");
    then_response_is(&mut world, "A", NEW_ICCID);
    then_response_is(&mut world, "A", "OK");

    // Read IMSI (6F07) via AT+CRSM
    when_at_command_sent(
        &mut world,
        "A",
        &format!("AT+CRSM={},{},0,0,9", CRSM_READ_BINARY, EF_IMSI_DEC),
    ); // 28423=0x6F07
    then_response_is(&mut world, "A", &format!("+CRSM: 144,0,{}", TEST_IMSI_ENCODED)); // 123456789012345 encoded
    then_response_is(&mut world, "A", "OK");

    // Write IMSI via UPDATE BINARY
    when_at_command_sent(
        &mut world,
        "A",
        &format!("AT+CRSM={},{},0,0,9,\"{}\"", CRSM_UPDATE_BINARY, EF_IMSI_DEC, NEW_IMSI_ENCODED),
    );
    then_response_is(&mut world, "A", "+CRSM: 144,0");
    then_response_is(&mut world, "A", "OK");

    // Verify IMSI is updated via AT+CIMI
    when_at_command_sent(&mut world, "A", "AT+CIMI");
    then_response_is(&mut world, "A", NEW_IMSI);
    then_response_is(&mut world, "A", "OK");
}

// Scenario: Update phone number and read MSISDN via AT+CRSM
#[test]
fn test_update_phone_number_and_read_msisdn() {
    let mut world = World::new();
    given_modem_with_sim_profile(&mut world, "A");

    let (id, _) = world.get_modem("A");
    world
        .manager
        .get_modem_mut(id)
        .unwrap()
        .set_phone_number(PhoneNumber::new_for_test(TEST_MSISDN));

    // Read EF_MSISDN (6F40) via AT+CRSM
    when_at_command_sent(
        &mut world,
        "A",
        &format!("AT+CRSM={},{},1,4,28", CRSM_READ_RECORD, EF_MSISDN_DEC),
    ); // 178=Read Record, 28480=0x6F40, record 1
    then_response_is(&mut world, "A", EXPECTED_MSISDN_CRSM_RESP);
    then_response_is(&mut world, "A", "OK");
}

// Scenario: Test Carrier Privileges via PKCS15 files on logical channels
#[test]
fn test_carrier_privileges_logical_channels() {
    let mut world = World::new();
    given_modem_with_sim_type(&mut world, "A", 2);

    // Open channel
    when_at_command_sent(&mut world, "A", &format!("AT+CCHO=\"{TEST_AID}\""));
    then_response_is(&mut world, "A", "1");
    then_response_is(&mut world, "A", "OK");

    // Select file 4300
    when_at_command_sent(&mut world, "A", &format!("AT+CGLA=1,14,\"{APDU_SELECT_EF_4300}\""));
    then_response_is(&mut world, "A", RESP_SELECT_EF_4300);
    then_response_is(&mut world, "A", "OK");

    // Read file 4300
    when_at_command_sent(&mut world, "A", "AT+CGLA=1,10,\"00B0000000\"");
    then_response_is(&mut world, "A", RESP_READ_EF_4300);
    then_response_is(&mut world, "A", "OK");

    // Select file 4318
    when_at_command_sent(&mut world, "A", &format!("AT+CGLA=1,14,\"{APDU_SELECT_EF_4318}\""));
    then_response_is(&mut world, "A", RESP_SELECT_EF_4318);
    then_response_is(&mut world, "A", "OK");

    // Read file 4318 (cert hashes)
    when_at_command_sent(&mut world, "A", "AT+CGLA=1,10,\"00B0000000\"");
    then_response_is(&mut world, "A", RESP_READ_EF_4318);
    then_response_is(&mut world, "A", "OK");
}

#[test]
fn test_cpin_empty() {
    let mut world = World::new();
    given_modem(&mut world, "A");
    when_at_command_sent(&mut world, "A", "AT+CPIN=\"\"");
    then_response_is(&mut world, "A", "ERROR");

    when_at_command_sent(&mut world, "A", "AT+CPINR=\"SIM PIN\"");
    then_response_is(
        &mut world,
        "A",
        &format!("+CPINR: \"SIM PIN\",{},{}", DEFAULT_PIN_RETRIES, DEFAULT_PIN_RETRIES),
    );
    then_response_is(&mut world, "A", "OK");
}

#[test]
fn test_close_basic_logical_channel() {
    let mut world = World::new();
    given_modem(&mut world, "A");
    // Closing channel 0 via MANAGE CHANNEL should return +CSIM status word
    // (6A86)
    when_at_command_sent(
        &mut world,
        "A",
        &format!("AT+CSIM=10,\"{}\"", APDU_MANAGE_CHANNEL_CLOSE_CH0_CLA_80),
    );
    then_response_is(
        &mut world,
        "A",
        &format!("+CSIM: {},{}", RESP_ERROR_INCORRECT_PARAMS.len(), RESP_ERROR_INCORRECT_PARAMS),
    );
    then_response_is(&mut world, "A", "OK");
}

#[test]
fn test_csim_unhandled_ins() {
    let mut world = World::new();
    given_modem(&mut world, "A");
    // Sending unhandled INS via CSIM should return status word 6A86 rather than
    // +CME ERROR
    when_at_command_sent(&mut world, "A", &format!("AT+CSIM=10,\"{}\"", APDU_UNHANDLED_INS));
    then_response_is(
        &mut world,
        "A",
        &format!("+CSIM: {},{}", RESP_ERROR_INCORRECT_PARAMS.len(), RESP_ERROR_INCORRECT_PARAMS),
    );
    then_response_is(&mut world, "A", "OK");
}

#[test]
fn test_cgla_wrong_length() {
    let mut world = World::new();
    given_modem(&mut world, "A");
    when_at_command_sent(&mut world, "A", &format!("AT+CCHO=\"{TEST_AID}\""));
    then_response_is(&mut world, "A", "1");
    then_response_is(&mut world, "A", "OK");

    // Lc says 10 (0x0A) but only 4 bytes of data are passed ("00A400040A2FE2")
    when_at_command_sent(&mut world, "A", "AT+CGLA=1,14,\"00A400040A2FE2\"");
    then_response_is(
        &mut world,
        "A",
        &format!("+CGLA: {},{}", RESP_ERROR_WRONG_LENGTH.len(), RESP_ERROR_WRONG_LENGTH),
    );
    then_response_is(&mut world, "A", "OK");
}

#[test]
fn test_puk_unlocking() {
    let mut world = World::new();
    given_modem_with_locked_sim(&mut world, "A");

    // Enable CMEE to get detailed errors
    when_at_command_sent(&mut world, "A", "AT+CMEE=1");
    then_response_is(&mut world, "A", "OK");

    // Initial state should be SIM PIN required
    when_at_command_sent(&mut world, "A", "AT+CPIN?");
    then_response_is(&mut world, "A", "+CPIN: SIM PIN");
    then_response_is(&mut world, "A", "OK");

    when_at_command_sent(&mut world, "A", "AT+CPINR=\"SIM PIN\"");
    then_response_is(&mut world, "A", "+CPINR: \"SIM PIN\",3,3");
    then_response_is(&mut world, "A", "OK");

    // Enter wrong PIN 1
    when_at_command_sent(&mut world, "A", "AT+CPIN=\"0000\"");
    then_response_is(&mut world, "A", "+CME ERROR: 16"); // Incorrect password

    when_at_command_sent(&mut world, "A", "AT+CPINR=\"SIM PIN\"");
    then_response_is(&mut world, "A", "+CPINR: \"SIM PIN\",2,3");
    then_response_is(&mut world, "A", "OK");

    // Enter wrong PIN 2
    when_at_command_sent(&mut world, "A", "AT+CPIN=\"0000\"");
    then_response_is(&mut world, "A", "+CME ERROR: 16");

    when_at_command_sent(&mut world, "A", "AT+CPINR=\"SIM PIN\"");
    then_response_is(&mut world, "A", "+CPINR: \"SIM PIN\",1,3");
    then_response_is(&mut world, "A", "OK");

    // Enter wrong PIN 3 -> Locks SIM, PUK required
    when_at_command_sent(&mut world, "A", "AT+CPIN=\"0000\"");
    then_response_is(&mut world, "A", "+CME ERROR: 16");

    // CPIN? should query SIM PUK
    when_at_command_sent(&mut world, "A", "AT+CPIN?");
    then_response_is(&mut world, "A", "+CPIN: SIM PUK");
    then_response_is(&mut world, "A", "OK");

    when_at_command_sent(&mut world, "A", "AT+CPINR=\"SIM PIN\"");
    then_response_is(&mut world, "A", "+CPINR: \"SIM PIN\",0,3");
    then_response_is(&mut world, "A", "OK");

    // CPINR should show PUK retries (default 10)
    when_at_command_sent(&mut world, "A", "AT+CPINR=\"SIM PUK\"");
    then_response_is(&mut world, "A", "+CPINR: \"SIM PUK\",10,10");
    then_response_is(&mut world, "A", "OK");

    // Enter wrong PUK (should consume retry)
    when_at_command_sent(&mut world, "A", &format!("AT+CPIN=\"{INVALID_PUK}\",\"{NEW_PIN}\""));
    then_response_is(&mut world, "A", "+CME ERROR: 16");

    when_at_command_sent(&mut world, "A", "AT+CPINR=\"SIM PUK\"");
    then_response_is(&mut world, "A", "+CPINR: \"SIM PUK\",9,10");
    then_response_is(&mut world, "A", "OK");

    // Enter correct PUK and set new PIN (puk1 is "12345678" in locked profile)
    when_at_command_sent(&mut world, "A", &format!("AT+CPIN=\"{TEST_PUK}\",\"{NEW_PIN}\""));
    then_response_is(&mut world, "A", "OK");

    // CPIN? should return READY
    when_at_command_sent(&mut world, "A", "AT+CPIN?");
    then_response_is(&mut world, "A", "+CPIN: READY");
    then_response_is(&mut world, "A", "OK");

    // PIN retries should be reset to 3
    when_at_command_sent(&mut world, "A", "AT+CPINR=\"SIM PIN\"");
    then_response_is(&mut world, "A", "+CPINR: \"SIM PIN\",3,3");
    then_response_is(&mut world, "A", "OK");
}

#[test]
fn test_apdu_msisdn_update() {
    let mut world = World::new();
    given_modem_with_msisdn_in_fs(&mut world, "A");

    // 1. Read initial MSISDN (should be default "15555211001" assigned by
    //    simulator)
    when_at_command_sent(&mut world, "A", "AT+CRSM=178,28480,1,4,28");
    then_response_is(
        &mut world,
        "A",
        "+CRSM: 144,0,FFFFFFFFFFFFFFFFFFFFFFFFFFFF07915155251100F1FFFFFFFFFFFF",
    );
    then_response_is(&mut world, "A", "OK");

    // 2. Update MSISDN to "15555215554" via APDU UPDATE RECORD (decimal 220)
    when_at_command_sent(
        &mut world,
        "A",
        "AT+CRSM=220,28480,1,4,28,\"FFFFFFFFFFFFFFFFFFFFFFFFFFFF07915155255155F4FFFFFFFFFFFF\"",
    );
    then_response_is(&mut world, "A", "+CRSM: 144,0");
    then_response_is(&mut world, "A", "OK");

    // 3. Read it back and verify it matches
    when_at_command_sent(&mut world, "A", "AT+CRSM=178,28480,1,4,28");
    then_response_is(
        &mut world,
        "A",
        "+CRSM: 144,0,FFFFFFFFFFFFFFFFFFFFFFFFFFFF07915155255155F4FFFFFFFFFFFF",
    );
    then_response_is(&mut world, "A", "OK");
}

#[test]
fn test_apdu_update_record_invalid_record_number() {
    let mut world = World::new();
    given_modem_with_msisdn_in_fs(&mut world, "A");

    // UPDATE RECORD with invalid record number (0)
    when_at_command_sent(
        &mut world,
        "A",
        "AT+CRSM=220,28480,0,4,28,\"FFFFFFFFFFFFFFFFFFFFFFFFFFFF07915155255155F4FFFFFFFFFFFF\"",
    );
    then_response_is(&mut world, "A", "+CRSM: 106,134"); // SW_INCORRECT_PARAMS (6A86)
    then_response_is(&mut world, "A", "OK");
}

#[test]
fn test_apdu_update_record_wrong_length() {
    let mut world = World::new();
    given_modem_with_msisdn_in_fs(&mut world, "A");

    // UPDATE RECORD with wrong length
    when_at_command_sent(&mut world, "A", "AT+CRSM=220,28480,1,4,28,\"00000000000000000000\"");
    then_response_is(&mut world, "A", "+CRSM: 103,0"); // SW_WRONG_LENGTH (6700)
    then_response_is(&mut world, "A", "OK");
}

#[test]
fn test_apdu_update_record_invalid_hex() {
    let mut world = World::new();
    given_modem_with_msisdn_in_fs(&mut world, "A");

    // UPDATE RECORD with invalid hex characters (e.g. 'G' at the end)
    // MSISDN record length is 28 bytes (56 hex characters)
    let invalid_hex = "FFFFFFFFFFFFFFFFFFFFFFFFFFFF07915155255155F4FFFFFFFFFFFG";
    when_at_command_sent(&mut world, "A", &format!("AT+CRSM=220,28480,1,4,28,\"{invalid_hex}\""));
    then_response_is(&mut world, "A", "+CRSM: 106,134"); // SW_INCORRECT_PARAMS (6A86)
    then_response_is(&mut world, "A", "OK");
}

#[test]
fn test_apdu_update_record_out_of_bounds() {
    let mut world = World::new();
    given_modem_with_msisdn_in_fs(&mut world, "A");

    // UPDATE RECORD for record out of bounds (record 5, only 1 exists)
    when_at_command_sent(
        &mut world,
        "A",
        "AT+CRSM=220,28480,5,4,28,\"FFFFFFFFFFFFFFFFFFFFFFFFFFFF07915155255155F4FFFFFFFFFFFF\"",
    );
    then_response_is(&mut world, "A", "+CRSM: 106,136"); // SW_REFERENCED_DATA_NOT_FOUND (6A88)
    then_response_is(&mut world, "A", "OK");
}

#[test]
fn test_transmit_logical_channel_errors() {
    let mut world = World::new();
    given_modem(&mut world, "A");

    // Open channel 1 to be able to transmit on it
    when_at_command_sent(&mut world, "A", "AT+CCHO=\"A000000063504B43532D3135\"");
    then_response_is(&mut world, "A", "1");
    then_response_is(&mut world, "A", "OK");

    // 1. SW_FILE_NOT_FOUND (6A82) when selecting non-existent file
    when_at_command_sent(&mut world, "A", "AT+CGLA=1,10,\"00A4000402FFFF\"");
    then_response_is(&mut world, "A", "+CGLA: 4,6A82");
    then_response_is(&mut world, "A", "OK");

    // 2. SW_INCORRECT_PARAMS (6A86) when manage channel has invalid p1
    when_at_command_sent(&mut world, "A", "AT+CGLA=1,10,\"0070010000\"");
    then_response_is(&mut world, "A", "+CGLA: 4,6A86");
    then_response_is(&mut world, "A", "OK");

    // 3. SW_INS_NOT_SUPPORTED (6D00) when unknown instruction
    when_at_command_sent(&mut world, "A", "AT+CGLA=1,10,\"00FF000000\"");
    then_response_is(&mut world, "A", "+CGLA: 4,6D00");
    then_response_is(&mut world, "A", "OK");
}

#[test]
fn test_sim_update_fplmn_and_mbdn() {
    let mut world = World::new();
    given_modem_with_fplmn_and_mbdn_in_fs(&mut world, "A");

    // 1. Test FPLMN (binary)
    // Update FPLMN
    when_at_command_sent(&mut world, "A", "AT+CRSM=214,28539,0,0,6,\"40F21040F220\"");
    then_response_is(&mut world, "A", "+CRSM: 144,0");
    then_response_is(&mut world, "A", "OK");

    // Read FPLMN and verify
    when_at_command_sent(&mut world, "A", "AT+CRSM=176,28539,0,0,12");
    then_response_is(&mut world, "A", "+CRSM: 144,0,40F21040F220FFFFFFFFFFFF");
    then_response_is(&mut world, "A", "OK");

    // 2. Test MBDN (record)
    let dummy_record = "A".repeat(76);
    // Update MBDN record 1
    when_at_command_sent(
        &mut world,
        "A",
        &format!("AT+CRSM=220,28615,1,4,38,\"{}\"", dummy_record),
    );
    then_response_is(&mut world, "A", "+CRSM: 144,0");
    then_response_is(&mut world, "A", "OK");

    // Read MBDN record 1 and verify
    when_at_command_sent(&mut world, "A", "AT+CRSM=178,28615,1,4,38");
    then_response_is(&mut world, "A", &format!("+CRSM: 144,0,{}", dummy_record));
    then_response_is(&mut world, "A", "OK");
}

#[test]
fn test_sim_status_change_action() {
    let mut world = World::new();
    given_modem(&mut world, "A");

    // Default status should be READY (SIM present)
    when_at_command_sent(&mut world, "A", "AT+CPIN?");
    then_response_is(&mut world, "A", "+CPIN: READY");
    then_response_is(&mut world, "A", "OK");

    // Remove SIM
    when_sim_status_set(&mut world, "A", false);
    then_wait_for_response_containing(&mut world, "A", "+CPIN: ABSENT");

    // CPIN should fail
    when_at_command_sent(&mut world, "A", "AT+CPIN?");
    then_response_is(&mut world, "A", "ERROR");

    // Re-insert SIM
    when_sim_status_set(&mut world, "A", true);
    then_wait_for_response_containing(&mut world, "A", "+CPIN: READY");

    // CPIN should work again
    when_at_command_sent(&mut world, "A", "AT+CPIN?");
    then_response_is(&mut world, "A", "+CPIN: READY");
    then_response_is(&mut world, "A", "OK");
}

#[test]
fn test_apdu_update_binary_resize() {
    let mut world = World::new();
    given_modem_with_fplmn_and_mbdn_in_fs(&mut world, "A");

    // FPLMN size is 12 bytes (24 hex chars).
    // Write 4 bytes at offset 10 (total 14 bytes, requires resize).
    // P1=0, P2=10 (0x0A) -> offset 10.
    when_at_command_sent(&mut world, "A", "AT+CRSM=214,28539,0,10,4,\"11223344\"");
    then_response_is(&mut world, "A", "+CRSM: 144,0");
    then_response_is(&mut world, "A", "OK");

    // Read FPLMN (should now be 14 bytes)
    when_at_command_sent(&mut world, "A", "AT+CRSM=176,28539,0,0,14");
    then_response_is(&mut world, "A", "+CRSM: 144,0,FFFFFFFFFFFFFFFFFFFF11223344");
    then_response_is(&mut world, "A", "OK");
}

#[test]
fn test_apdu_update_record_non_record_file() {
    let mut world = World::new();
    given_modem_with_fplmn_and_mbdn_in_fs(&mut world, "A");

    // FPLMN (28539) is transparent. Try UPDATE RECORD (220) on it - must fail
    // with SW_INCORRECT_PARAMS.
    when_at_command_sent(&mut world, "A", "AT+CRSM=220,28539,1,4,6,\"AABBCCDDEEFF\"");
    then_response_is(&mut world, "A", "+CRSM: 106,134");
    then_response_is(&mut world, "A", "OK");

    // Read it back (content should be unchanged)
    when_at_command_sent(&mut world, "A", "AT+CRSM=176,28539,0,0,12");
    then_response_is(&mut world, "A", "+CRSM: 144,0,FFFFFFFFFFFFFFFFFFFFFFFF");
    then_response_is(&mut world, "A", "OK");
}

#[test]
fn test_apdu_update_missing_data() {
    let mut world = World::new();
    given_modem(&mut world, "A");

    // Update FPLMN without data parameter.
    // If parser allows it, it should return +CRSM: 106,134.
    // If parser rejects it, it should return ERROR.
    // Let's test what it returns.
    when_at_command_sent(&mut world, "A", "AT+CRSM=214,28539,0,0,6");
    // We will assert +CRSM: 106,134 first. If it fails with parse error, we
    // will know.
    then_response_is(&mut world, "A", "+CRSM: 106,134");
    then_response_is(&mut world, "A", "OK");
}

#[test]
fn test_apdu_update_binary_invalid_hex() {
    let mut world = World::new();
    given_modem(&mut world, "A");

    // Update FPLMN with invalid hex (e.g. 'G' at the end)
    when_at_command_sent(&mut world, "A", "AT+CRSM=214,28539,0,0,6,\"40F21040F22G\"");
    then_response_is(&mut world, "A", "+CRSM: 106,134"); // SW_INCORRECT_PARAMS (6A86)
    then_response_is(&mut world, "A", "OK");
}

#[test]
fn test_apdu_update_file_not_found() {
    let mut world = World::new();
    given_modem(&mut world, "A");

    // Try to update a file that is not in FS and not supported fallback (e.g.
    // 0x9999 = 39321)
    when_at_command_sent(&mut world, "A", "AT+CRSM=214,39321,0,0,1,\"00\"");
    then_response_is(&mut world, "A", "+CRSM: 106,130"); // SW_FILE_NOT_FOUND (6A82)
    then_response_is(&mut world, "A", "OK");
}

#[test]
fn test_cgla_close_closed_channel() {
    let mut world = World::new();
    given_modem(&mut world, "A");
    when_at_command_sent(
        &mut world,
        "A",
        &format!("AT+CGLA=0,10,\"{APDU_MANAGE_CHANNEL_CLOSE_CH1}\""),
    );
    then_response_is(
        &mut world,
        "A",
        &format!("+CGLA: 4,{}", RESP_ERROR_REFERENCED_DATA_NOT_FOUND),
    );
    then_response_is(&mut world, "A", "OK");
}

#[test]
fn test_cgla_validation_errors() {
    let mut world = World::new();
    given_modem(&mut world, "A");

    // Too short APDU (less than 4 bytes)
    when_at_command_sent(&mut world, "A", "AT+CGLA=0,4,\"0070\"");
    then_response_is(&mut world, "A", "+CGLA: 4,6700");
    then_response_is(&mut world, "A", "OK");

    // Invalid hex data
    when_at_command_sent(&mut world, "A", "AT+CGLA=0,6,\"0070XX\"");
    then_response_is(&mut world, "A", "+CGLA: 4,6F00");
    then_response_is(&mut world, "A", "OK");
}

#[test]
fn test_csim_manage_channel_limits() {
    let mut world = World::new();
    given_modem(&mut world, "A");

    // Open channel 1
    when_at_command_sent(&mut world, "A", "AT+CSIM=10,\"0070000000\"");
    then_response_is(&mut world, "A", "+CSIM: 6,019000");
    then_response_is(&mut world, "A", "OK");

    // Close channel 1 (verifies successful close path)
    when_at_command_sent(&mut world, "A", "AT+CSIM=10,\"0070800100\"");
    then_response_is(&mut world, "A", "+CSIM: 4,9000");
    then_response_is(&mut world, "A", "OK");

    // Open channel 1 again (should reuse index 1)
    when_at_command_sent(&mut world, "A", "AT+CSIM=10,\"0070000000\"");
    then_response_is(&mut world, "A", "+CSIM: 6,019000");
    then_response_is(&mut world, "A", "OK");

    // Open channel 2
    when_at_command_sent(&mut world, "A", "AT+CSIM=10,\"0070000000\"");
    then_response_is(&mut world, "A", "+CSIM: 6,029000");
    then_response_is(&mut world, "A", "OK");

    // Open channel 3
    when_at_command_sent(&mut world, "A", "AT+CSIM=10,\"0070000000\"");
    then_response_is(&mut world, "A", "+CSIM: 6,039000");
    then_response_is(&mut world, "A", "OK");

    // Try to open 5th channel (index 4, but limit is 4 channels (0-3))
    when_at_command_sent(&mut world, "A", "AT+CSIM=10,\"0070000000\"");
    then_response_is(&mut world, "A", "+CSIM: 4,6A81");
    then_response_is(&mut world, "A", "OK");
}

#[test]
fn test_csim_close_closed_channel_and_invalid_action() {
    let mut world = World::new();
    given_modem(&mut world, "A");

    // Close closed channel 1
    when_at_command_sent(
        &mut world,
        "A",
        &format!("AT+CSIM=10,\"{APDU_MANAGE_CHANNEL_CLOSE_CH1}\""),
    );
    then_response_is(
        &mut world,
        "A",
        &format!("+CSIM: 4,{}", RESP_ERROR_REFERENCED_DATA_NOT_FOUND),
    );
    then_response_is(&mut world, "A", "OK");

    // Close channel 0 (invalid operation)
    when_at_command_sent(
        &mut world,
        "A",
        &format!("AT+CSIM=10,\"{APDU_MANAGE_CHANNEL_CLOSE_CH0}\""),
    );
    then_response_is(&mut world, "A", &format!("+CSIM: 4,{}", RESP_ERROR_INCORRECT_PARAMS));
    then_response_is(&mut world, "A", "OK");

    // Close channel 4 (out of bounds)
    when_at_command_sent(
        &mut world,
        "A",
        &format!("AT+CSIM=10,\"{APDU_MANAGE_CHANNEL_CLOSE_CH4}\""),
    );
    then_response_is(&mut world, "A", &format!("+CSIM: 4,{}", RESP_ERROR_OUT_OF_BOUNDS));
    then_response_is(&mut world, "A", "OK");

    // Invalid action (P1=01)
    when_at_command_sent(
        &mut world,
        "A",
        &format!("AT+CSIM=10,\"{APDU_MANAGE_CHANNEL_INVALID_P1}\""),
    );
    then_response_is(&mut world, "A", &format!("+CSIM: 4,{}", RESP_ERROR_INCORRECT_PARAMS));
    then_response_is(&mut world, "A", "OK");
}

#[test]
fn test_cgla_manage_channel_open_and_close() {
    let mut world = World::new();
    given_modem(&mut world, "A");

    // 1. Open channel via CGLA on channel 0
    when_at_command_sent(&mut world, "A", &format!("AT+CGLA=0,10,\"{APDU_MANAGE_CHANNEL_OPEN}\""));
    // Should return channel ID "01" and SW_SUCCESS "9000" -> combined "019000",
    // length is 6 chars.
    then_response_is(&mut world, "A", "+CGLA: 6,019000");
    then_response_is(&mut world, "A", "OK");

    // 2. Verify we can transmit on the new channel 1 (e.g. SELECT EF_ICCID)
    when_at_command_sent(&mut world, "A", &format!("AT+CGLA=1,14,\"{APDU_SELECT_EF_ICCID}\""));
    then_response_is(&mut world, "A", "+CGLA: 4,9000");
    then_response_is(&mut world, "A", "OK");

    // 3. Close channel 1 via CGLA on channel 0
    when_at_command_sent(
        &mut world,
        "A",
        &format!("AT+CGLA=0,10,\"{APDU_MANAGE_CHANNEL_CLOSE_CH1}\""),
    );
    then_response_is(&mut world, "A", "+CGLA: 4,9000");
    then_response_is(&mut world, "A", "OK");

    // 4. Verify channel 1 is now closed (transmitting on it should fail)
    when_at_command_sent(&mut world, "A", &format!("AT+CGLA=1,14,\"{APDU_SELECT_EF_ICCID}\""));
    then_response_is(&mut world, "A", "ERROR");
}

#[test]
fn test_cgla_close_invalid_channels() {
    let mut world = World::new();
    given_modem(&mut world, "A");

    // Close channel 0 -> 6A86 (Incorrect params)
    when_at_command_sent(
        &mut world,
        "A",
        &format!("AT+CGLA=0,10,\"{APDU_MANAGE_CHANNEL_CLOSE_CH0}\""),
    );
    then_response_is(&mut world, "A", &format!("+CGLA: 4,{}", RESP_ERROR_INCORRECT_PARAMS));
    then_response_is(&mut world, "A", "OK");

    // Close channel 4 -> 6A88 (Out of bounds)
    when_at_command_sent(
        &mut world,
        "A",
        &format!("AT+CGLA=0,10,\"{APDU_MANAGE_CHANNEL_CLOSE_CH4}\""),
    );
    then_response_is(&mut world, "A", &format!("+CGLA: 4,{}", RESP_ERROR_OUT_OF_BOUNDS));
    then_response_is(&mut world, "A", "OK");
}

#[test]
fn test_default_sim_profile_reads() {
    let mut world = World::new();
    // sim_type = 1 triggers the loading of the default production profile:
    // PROFILE_DEFAULT_XML
    given_modem_with_sim_type(&mut world, "A", 1);

    // 1. Verify CPIN status is READY (default profile does not lock by default)
    when_at_command_sent(&mut world, "A", "AT+CPIN?");
    then_response_is(&mut world, "A", "+CPIN: READY");
    then_response_is(&mut world, "A", "OK");

    // 2. Query IMSI (AT+CIMI) -> parsed from USIM ADF
    when_at_command_sent(&mut world, "A", "AT+CIMI");
    then_response_is(&mut world, "A", "310260000000000");
    then_response_is(&mut world, "A", "OK");

    // 3. Query ICCID (AT+CICCID) -> parsed from CCID tag in EF_ICCID
    when_at_command_sent(&mut world, "A", "AT+CICCID");
    then_response_is(&mut world, "A", "89860318640220133897");
    then_response_is(&mut world, "A", "OK");

    // 4. Query ICCID via Restricted SIM Access (AT+CRSM) on EF_ICCID (12258 /
    //    0x2FE2)
    when_at_command_sent(&mut world, "A", "AT+CRSM=176,12258,0,0,10");
    then_response_is(&mut world, "A", "+CRSM: 144,0,98683081462002318379");
    then_response_is(&mut world, "A", "OK");

    // 5. Query Preferred Languages via Restricted SIM Access on EF_PL (12037 /
    //    0x2F05)
    when_at_command_sent(&mut world, "A", "AT+CRSM=176,12037,0,0,4");
    then_response_is(&mut world, "A", "+CRSM: 144,0,FFFFFFFF");
    then_response_is(&mut world, "A", "OK");
}

#[test]
fn test_cuttlefish_default_tel_alaska_sim_profile_reads() {
    let mut world = World::new();
    // Cuttlefish modems default to the TelAlaska profile
    // (PROFILE_TEL_ALASKA_XML)
    given_cuttlefish_modem(&mut world, "A");

    // Verify CPIN status is READY
    when_at_command_sent(&mut world, "A", "AT+CPIN?");
    then_response_is(&mut world, "A", "+CPIN: READY");
    then_response_is(&mut world, "A", "OK");

    // Query IMSI (AT+CIMI) -> parsed from USIM ADF (TelAlaska 311740)
    when_at_command_sent(&mut world, "A", "AT+CIMI");
    then_response_is(&mut world, "A", "311740123456789");
    then_response_is(&mut world, "A", "OK");

    // Enable verbose error reporting
    when_at_command_sent(&mut world, "A", "AT+CMEE=1");
    then_response_is(&mut world, "A", "OK");

    // Query EID -> should return NotFound CME ERROR (22) because CardProfile is
    // missing
    when_at_command_sent(&mut world, "A", "AT+CEID");
    then_response_is(&mut world, "A", "+CME ERROR: 22");

    // Query ATR -> should return NotFound CME ERROR (22) because CardProfile is
    // missing
    when_at_command_sent(&mut world, "A", "AT+CATR");
    then_response_is(&mut world, "A", "+CME ERROR: 22");

    // Enable radio and wait for registration
    when_at_command_sent(&mut world, "A", "AT+CFUN=1");
    then_response_is(&mut world, "A", "OK");
    when_time_advances_ms(&mut world, 10);
    then_response_is(&mut world, "A", RESP_CSQ_LTE_DEFAULT);

    // Query COPS -> returns TelAlaska PLMN (311740) derived from SIM profile
    when_at_command_sent(&mut world, "A", "AT+COPS?");
    then_response_is(&mut world, "A", "+COPS: 0,2,\"311740\"");
    then_response_is(&mut world, "A", "OK");
}

#[test]
fn test_cts_profile_csim_and_status_reads() {
    let mut world = World::new();
    // sim_type = 2 triggers the loading of the CTS profile: PROFILE_CTS_XML
    given_modem_with_sim_type(&mut world, "A", 2);

    // 1. Verify AT+CSIM STATUS command query (80f2000000)
    // This command is explicitly mapped in the CSIM block of the CTS XML
    // profile. It returns the long Master File FCP template.
    when_at_command_sent(&mut world, "A", "AT+CSIM=10,\"80f2000000\"");
    then_response_is(
        &mut world,
        "A",
        "+CSIM: 110,62338202782183023F00A50C80016187010183040007DBF08A01058B062F0601020002C60C90016083010183010A83010D8102FFFF9000",
    );
    then_response_is(&mut world, "A", "OK");

    // 2. Verify AT+CSIM wrong length STATUS command query (80F20000)
    // This maps to the wrong length error response (6C35) configured in the
    // XML.
    when_at_command_sent(&mut world, "A", "AT+CSIM=8,\"80F20000\"");
    then_response_is(&mut world, "A", "+CSIM: 4,6C35");
    then_response_is(&mut world, "A", "OK");

    // 3. Verify AT+CRSM STATUS command query (command 242 / 0xF2)
    // The simulator has a hardcoded override for APDU_STATUS (0xF2) to return
    // the MF FCP template:
    // "62338202782183023F00A50C80016187010183040007DBF08A01058B062F0601020002C60C90016083010183010A83010D8102FFFF"
    when_at_command_sent(&mut world, "A", "AT+CRSM=242,0,0,0,0");
    then_response_is(
        &mut world,
        "A",
        "+CRSM: 144,0,62338202782183023F00A50C80016187010183040007DBF08A01058B062F0601020002C60C90016083010183010A83010D8102FFFF",
    );
    then_response_is(&mut world, "A", "OK");
}

#[test]
fn test_cgla_apdu_parsing_cases() {
    let mut world = World::new();
    // sim_type = 1 triggers the loading of the default production profile:
    // PROFILE_DEFAULT_XML
    given_modem_with_sim_type(&mut world, "A", 1);

    // 1. Open Logical Channel 1 (Case 2 APDU: Le=00)
    when_at_command_sent(&mut world, "A", &format!("AT+CGLA=0,10,\"{APDU_MANAGE_CHANNEL_OPEN}\""));
    then_response_is(&mut world, "A", "+CGLA: 6,019000");
    then_response_is(&mut world, "A", "OK");

    // 2. Select EF_ICCID with FCP request (Case 4 APDU: Lc=02, Data=2FE2, Le=0C)
    // Send standard select to EF_ICCID: CLA=00 INS=A4 P1=00 P2=04 Lc=02
    // Data=2FE2 Le=0C
    when_at_command_sent(&mut world, "A", "AT+CGLA=1,16,\"00A40004022FE20C\"");
    then_response_is(&mut world, "A", "+CGLA: 4,9000");
    then_response_is(&mut world, "A", "OK");

    // 3. Read Binary Part (Case 2 APDU, Le > 0)
    // Read 5 bytes of selected EF_ICCID: CLA=00 INS=B0 P1=00 P2=00 Le=05
    when_at_command_sent(&mut world, "A", "AT+CGLA=1,10,\"00B0000005\"");
    then_response_is(&mut world, "A", "+CGLA: 14,98683081469000");
    then_response_is(&mut world, "A", "OK");

    // 4. Read Binary Full (Case 2 APDU, Le = 0)
    // Read all bytes of selected EF_ICCID: CLA=00 INS=B0 P1=00 P2=00 Le=00
    when_at_command_sent(&mut world, "A", "AT+CGLA=1,10,\"00B0000000\"");
    then_response_is(&mut world, "A", "+CGLA: 24,986830814620023183799000");
    then_response_is(&mut world, "A", "OK");

    // 5. Invalid Length APDU (6 bytes, but Lc=2 which expects 7 bytes)
    when_at_command_sent(&mut world, "A", "AT+CGLA=1,12,\"00B000000201\"");
    then_response_is(&mut world, "A", "+CGLA: 4,6700");
    then_response_is(&mut world, "A", "OK");

    // 6. Close Channel 1 (Case 2 APDU: Le=00)
    when_at_command_sent(
        &mut world,
        "A",
        &format!("AT+CGLA=0,10,\"{APDU_MANAGE_CHANNEL_CLOSE_CH1}\""),
    );
    then_response_is(&mut world, "A", "+CGLA: 4,9000");
    then_response_is(&mut world, "A", "OK");
}

#[test]
fn test_cgla_invalid_channel_index() {
    let mut world = World::new();
    given_modem(&mut world, "A");

    // Enable verbose errors
    when_at_command_sent(&mut world, "A", "AT+CMEE=2");
    then_response_is(&mut world, "A", "OK");

    // Transmit on invalid channel index 4
    when_at_command_sent(&mut world, "A", "AT+CGLA=4,10,\"00A40004022FE2\"");
    then_response_is(&mut world, "A", "+CME ERROR: invalid index");
}

#[test]
fn test_apdu_read_binary_offset_out_of_bounds() {
    let mut world = World::new();
    given_modem_with_sim_profile(&mut world, "A"); // ICCID size is 10

    // Read at offset 11
    when_at_command_sent(&mut world, "A", "AT+CRSM=176,12258,0,11,1");
    then_response_is(&mut world, "A", "+CRSM: 106,134");
    then_response_is(&mut world, "A", "OK");
}

#[test]
fn test_apdu_read_binary_range_out_of_bounds() {
    let mut world = World::new();
    given_modem_with_sim_profile(&mut world, "A"); // ICCID size is 10

    // Read 5 bytes at offset 8 (ends at 13, past size 10)
    when_at_command_sent(&mut world, "A", "AT+CRSM=176,12258,0,8,5");
    then_response_is(&mut world, "A", "+CRSM: 103,0");
    then_response_is(&mut world, "A", "OK");
}

#[test]
fn test_apdu_read_binary_at_end_of_file() {
    let mut world = World::new();
    given_modem_with_sim_profile(&mut world, "A"); // ICCID size is 10

    // Read 0 bytes at offset 10 (exactly at end of file)
    when_at_command_sent(&mut world, "A", "AT+CRSM=176,12258,0,10,0");
    then_response_is(&mut world, "A", "+CRSM: 144,0,");
    then_response_is(&mut world, "A", "OK");
}

#[test]
fn test_apdu_read_record_file_not_found() {
    let mut world = World::new();
    given_modem_with_sim_profile(&mut world, "A");

    // Read record 1 of non-existent file 9999
    when_at_command_sent(&mut world, "A", "AT+CRSM=178,9999,1,4,28");
    then_response_is(&mut world, "A", "+CRSM: 106,130");
    then_response_is(&mut world, "A", "OK");
}

#[test]
fn test_apdu_read_record_transparent_file() {
    let mut world = World::new();
    given_modem_with_sim_profile(&mut world, "A"); // ICCID is transparent

    // Try to read record 1 of ICCID
    when_at_command_sent(&mut world, "A", "AT+CRSM=178,12258,1,4,10");
    then_response_is(&mut world, "A", "+CRSM: 106,136");
    then_response_is(&mut world, "A", "OK");
}

#[test]
fn test_apdu_read_record_index_zero() {
    let mut world = World::new();
    given_modem_with_msisdn_in_fs(&mut world, "A"); // MSISDN is record file

    // Read record 0 of MSISDN
    when_at_command_sent(&mut world, "A", "AT+CRSM=178,28480,0,4,28");
    then_response_is(&mut world, "A", "+CRSM: 106,136");
    then_response_is(&mut world, "A", "OK");
}

#[test]
fn test_apdu_read_record_out_of_bounds() {
    let mut world = World::new();
    given_modem_with_msisdn_in_fs(&mut world, "A");

    // Read record 5 of MSISDN (only 1 exists)
    when_at_command_sent(&mut world, "A", "AT+CRSM=178,28480,5,4,28");
    then_response_is(&mut world, "A", "+CRSM: 106,136");
    then_response_is(&mut world, "A", "OK");
}

#[test]
fn test_even_length_imsi_processing() {
    let mut world = World::new();
    let xml = r#"<IccProfile>
        <MF>
            <ADF aid="A0000000871002FF86FF0389FFFFFFFF">
                <EF id="6F07" structure="transparent">
                    <CIMI>31174012345678</CIMI>
                </EF>
            </ADF>
        </MF>
    </IccProfile>"#;
    given_modem_with_xml_profile(&mut world, "A", xml);

    // 1. Check IMSI readout via AT+CIMI
    when_at_command_sent(&mut world, "A", "AT+CIMI");
    then_response_is(&mut world, "A", "31174012345678");
    then_response_is(&mut world, "A", "OK");

    // 2. Check low-level BCD encoding and padding in EF_IMSI file
    when_at_command_sent(&mut world, "A", "AT+CRSM=176,28423,0,0,9");
    then_response_is(&mut world, "A", "+CRSM: 144,0,0831114710325476F8");
    then_response_is(&mut world, "A", "OK");
}

#[test]
fn test_cgla_apdu_write_cases() {
    let mut world = World::new();
    given_modem_with_sim_profile(&mut world, "A"); // ICCID is transparent, MSISDN is linear fixed

    // 1. Open Logical Channel 1
    when_at_command_sent(&mut world, "A", &format!("AT+CGLA=0,10,\"{APDU_MANAGE_CHANNEL_OPEN}\""));
    then_response_is(&mut world, "A", "+CGLA: 6,019000");
    then_response_is(&mut world, "A", "OK");

    // 2. Select EF_ICCID
    when_at_command_sent(&mut world, "A", "AT+CGLA=1,16,\"00A40004022FE20C\"");
    then_response_is(&mut world, "A", "+CGLA: 4,9000");
    then_response_is(&mut world, "A", "OK");

    // 3. Write Binary (UPDATE BINARY Case 3: CLA=00 INS=D6 P1=00 P2=05 Lc=05
    //    Data=1122334455)
    when_at_command_sent(&mut world, "A", "AT+CGLA=1,20,\"00D60005051122334455\"");
    then_response_is(&mut world, "A", "+CGLA: 4,9000");
    then_response_is(&mut world, "A", "OK");

    // 4. Read Binary Back to verify state (Le=05)
    when_at_command_sent(&mut world, "A", "AT+CGLA=1,10,\"00B0000505\"");
    then_response_is(&mut world, "A", "+CGLA: 14,11223344559000");
    then_response_is(&mut world, "A", "OK");

    // 5. Select EF_MSISDN (28480 = 0x6F40)
    when_at_command_sent(&mut world, "A", "AT+CGLA=1,16,\"00A40004026F400C\"");
    then_response_is(&mut world, "A", "+CGLA: 4,9000");
    then_response_is(&mut world, "A", "OK");

    // 6. Write Record (UPDATE RECORD Case 3: CLA=00 INS=DC P1=01 P2=04 Lc=1C
    //    Data=28-bytes)
    // MSISDN record length is 28 bytes (56 hex characters) in the default mock
    // layout
    let new_record_hex = "0000000000000000000000000000079181F2FF00000FFFFFFFFFFFFF";
    when_at_command_sent(&mut world, "A", &format!("AT+CGLA=1,66,\"00DC01041C{new_record_hex}\""));
    then_response_is(&mut world, "A", "+CGLA: 4,9000");
    then_response_is(&mut world, "A", "OK");

    // 7. Read Record Back to verify state (Le=1C)
    when_at_command_sent(&mut world, "A", "AT+CGLA=1,10,\"00B201041C\"");
    then_response_is(&mut world, "A", &format!("+CGLA: 60,{new_record_hex}9000"));
    then_response_is(&mut world, "A", "OK");

    // 8. Close Channel 1
    when_at_command_sent(
        &mut world,
        "A",
        &format!("AT+CGLA=0,10,\"{APDU_MANAGE_CHANNEL_CLOSE_CH1}\""),
    );
    then_response_is(&mut world, "A", "+CGLA: 4,9000");
    then_response_is(&mut world, "A", "OK");
}

#[test]
fn test_cgla_invalid_file_and_channel_handling() {
    let mut world = World::new();
    given_modem_with_sim_profile(&mut world, "A");

    // Try to select a non-existent file ID (e.g. 0xFFFF)
    when_at_command_sent(&mut world, "A", "AT+CGLA=0,16,\"00A4000402FFFF0C\"");
    then_response_is(&mut world, "A", "+CGLA: 4,6A82");
    then_response_is(&mut world, "A", "OK");
}

#[test]
fn test_csim_select_and_unhandled_ins() {
    let mut world = World::new();
    given_modem(&mut world, "A");

    // 1. SELECT command via CSIM returns success
    when_at_command_sent(&mut world, "A", "AT+CSIM=14,\"00A40004022FE2\"");
    then_response_is(&mut world, "A", "+CSIM: 4,9000");
    then_response_is(&mut world, "A", "OK");

    // 2. Unhandled INS via CSIM returns SW_INCORRECT_PARAMS (6A86) as baseline
    when_at_command_sent(&mut world, "A", "AT+CSIM=10,\"00FE000000\"");
    then_response_is(&mut world, "A", "+CSIM: 4,6A86");
    then_response_is(&mut world, "A", "OK");

    // 3. APDU with mismatched Lc length via CSIM returns SW_WRONG_LENGTH (6700)
    when_at_command_sent(&mut world, "A", "AT+CSIM=14,\"00A400040A1122\"");
    then_response_is(&mut world, "A", "+CSIM: 4,6700");
    then_response_is(&mut world, "A", "OK");
}

#[test]
fn test_csim_invalid_channel_rejection() {
    let mut world = World::new();
    given_modem_with_sim_type(&mut world, "A", 1);

    // 1. Send command with CLA 40 (Format 2, channel 4) -> out of bounds (max 3
    //    supported)
    // Expect SW_CLASS_NOT_SUPPORTED (6E00)
    when_at_command_sent(&mut world, "A", "AT+CSIM=10,\"40B0000005\"");
    then_response_is(&mut world, "A", "+CSIM: 4,6E00");
    then_response_is(&mut world, "A", "OK");

    // 2. Send command with CLA 01 (Format 1, channel 1) which is closed
    // Expect SW_CLASS_NOT_SUPPORTED (6E00)
    when_at_command_sent(&mut world, "A", "AT+CSIM=10,\"01B0000005\"");
    then_response_is(&mut world, "A", "+CSIM: 4,6E00");
    then_response_is(&mut world, "A", "OK");
}

#[test]
fn test_lenient_cla_rejection() {
    let mut world = World::new();
    given_modem_with_sim_type(&mut world, "A", 1);

    // 1. Open channel 1
    when_at_command_sent(&mut world, "A", "AT+CGLA=0,10,\"0070000001\"");
    then_response_is(&mut world, "A", "+CGLA: 6,019000");
    then_response_is(&mut world, "A", "OK");

    // 2. Open channel 2
    when_at_command_sent(&mut world, "A", "AT+CGLA=0,10,\"0070000001\"");
    then_response_is(&mut world, "A", "+CGLA: 6,029000");
    then_response_is(&mut world, "A", "OK");

    // 3. Transmit on channel 1 with APDU specifying channel 2 (CLA 02)
    when_at_command_sent(&mut world, "A", "AT+CGLA=1,10,\"02F2000000\"");
    then_response_is(&mut world, "A", "+CGLA: 4,6E00");
    then_response_is(&mut world, "A", "OK");
}

#[test]
fn test_get_eid_and_atr() {
    let mut world = World::new();
    given_modem(&mut world, "A");

    // EID from default profile (iccprofile_for_sim0.xml)
    // <EID>89049032000001000000000254806852</EID>
    when_at_command_sent(&mut world, "A", "AT+CEID");
    then_response_is(&mut world, "A", "+CEID: 89049032000001000000000254806852");
    then_response_is(&mut world, "A", "OK");

    // ATR from default profile (iccprofile_for_sim0.xml)
    // <ATR>3F979580BFFE8210428031A073BE211797</ATR>
    when_at_command_sent(&mut world, "A", "AT+CATR");
    then_response_is(&mut world, "A", "+CATR: 3F979580BFFE8210428031A073BE211797");
    then_response_is(&mut world, "A", "OK");
}

#[test]
fn test_cgla_select_by_aid() {
    let mut world = World::new();
    given_modem_with_sim_type(&mut world, "A", 2); // CTS profile with USIM AID

    // Open channel 1
    when_at_command_sent(&mut world, "A", "AT+CGLA=0,10,\"0070000001\"");
    then_response_is(&mut world, "A", "+CGLA: 6,019000");
    then_response_is(&mut world, "A", "OK");

    // Select USIM AID (A0000000871002FF86FF0389FFFFFFFF) on channel 1
    when_at_command_sent(
        &mut world,
        "A",
        "AT+CGLA=1,42,\"01A4040010A0000000871002FF86FF0389FFFFFFFF\"",
    );
    then_response_is(&mut world, "A", "+CGLA: 4,6147");
    then_response_is(&mut world, "A", "OK");
}

#[test]
fn test_cgla_close_other_channel_success() {
    let mut world = World::new();
    given_modem(&mut world, "A");

    // 1. Open channel 1
    when_at_command_sent(&mut world, "A", "AT+CGLA=0,10,\"0070000001\"");
    then_response_is(&mut world, "A", "+CGLA: 6,019000");
    then_response_is(&mut world, "A", "OK");

    // 2. Open channel 2
    when_at_command_sent(&mut world, "A", "AT+CGLA=0,10,\"0070000002\"");
    then_response_is(&mut world, "A", "+CGLA: 6,029000");
    then_response_is(&mut world, "A", "OK");

    // 3. Close channel 2 from channel 0 -> should succeed
    when_at_command_sent(&mut world, "A", "AT+CGLA=0,10,\"0070800200\"");
    then_response_is(&mut world, "A", "+CGLA: 4,9000"); // SW_SUCCESS
    then_response_is(&mut world, "A", "OK");

    // 4. Verify channel 2 is closed by trying to close it again -> should fail with
    //    6A88
    when_at_command_sent(&mut world, "A", "AT+CGLA=0,10,\"0070800200\"");
    then_response_is(&mut world, "A", "+CGLA: 4,6A88"); // SW_REFERENCED_DATA_NOT_FOUND
    then_response_is(&mut world, "A", "OK");
}

#[test]
fn test_get_eid_and_atr_not_found() {
    let mut world = World::new();
    given_modem_with_sim_profile(&mut world, "A");

    // Default CMEE is 0 (disable), so it should return generic "ERROR"
    when_at_command_sent(&mut world, "A", "AT+CEID");
    then_response_is(&mut world, "A", "ERROR");

    when_at_command_sent(&mut world, "A", "AT+CATR");
    then_response_is(&mut world, "A", "ERROR");

    // Enable CMEE=1 (numeric error), should return "+CME ERROR: 22" (NotFound)
    when_at_command_sent(&mut world, "A", "AT+CMEE=1");
    then_response_is(&mut world, "A", "OK");

    when_at_command_sent(&mut world, "A", "AT+CEID");
    then_response_is(&mut world, "A", "+CME ERROR: 22");

    when_at_command_sent(&mut world, "A", "AT+CATR");
    then_response_is(&mut world, "A", "+CME ERROR: 22");
}

#[test]
fn test_cgla_select_profile_failure_rollback() {
    let mut world = World::new();
    given_modem_with_sim_type(&mut world, "A", 2); // CTS profile

    // 1. Select EF_ICCID (0x2FE2) on channel 0 -> success
    when_at_command_sent(&mut world, "A", "AT+CGLA=0,14,\"00A4000C022FE2\"");
    then_response_is(&mut world, "A", "+CGLA: 4,9000");
    then_response_is(&mut world, "A", "OK");

    // 2. Attempt to select invalid file FFFF -> fails with 6A82 (defined in CTS
    //    profile)
    when_at_command_sent(&mut world, "A", "AT+CGLA=0,14,\"00A4000C02FFFF\"");
    then_response_is(&mut world, "A", "+CGLA: 4,6A82");
    then_response_is(&mut world, "A", "OK");
}

#[test]
fn test_cgla_manage_channel_profile_sync() {
    let mut world = World::new();
    given_modem_with_sim_type(&mut world, "A", 2); // CTS profile

    when_at_command_sent(&mut world, "A", "AT+CSIM=10,\"0070000001\"");
    then_response_is(&mut world, "A", "+CSIM: 6,019000");
    then_response_is(&mut world, "A", "OK");

    when_at_command_sent(&mut world, "A", "AT+CGLA=1,14,\"01A4000C022FE2\"");
    then_response_is(&mut world, "A", "+CGLA: 4,9000");
    then_response_is(&mut world, "A", "OK");
}

#[test]
fn test_cgla_close_channel_self_explicit() {
    let mut world = World::new();
    given_modem_with_sim_type(&mut world, "A", 1);

    when_at_command_sent(&mut world, "A", "AT+CMEE=2");
    then_response_is(&mut world, "A", "OK");

    when_at_command_sent(&mut world, "A", "AT+CGLA=0,10,\"0070000001\"");
    then_response_is(&mut world, "A", "+CGLA: 6,019000");
    then_response_is(&mut world, "A", "OK");

    when_at_command_sent(&mut world, "A", "AT+CGLA=1,10,\"0170800100\"");
    then_response_is(&mut world, "A", "+CGLA: 4,9000");
    then_response_is(&mut world, "A", "OK");

    when_at_command_sent(&mut world, "A", "AT+CGLA=1,10,\"01F2000000\"");
    then_response_is(&mut world, "A", "+CME ERROR: not found");
}

#[test]
fn test_cgla_close_channel_cross() {
    let mut world = World::new();
    given_modem_with_sim_type(&mut world, "A", 1);

    when_at_command_sent(&mut world, "A", "AT+CMEE=2");
    then_response_is(&mut world, "A", "OK");

    when_at_command_sent(&mut world, "A", "AT+CGLA=0,10,\"0070000001\"");
    then_response_is(&mut world, "A", "+CGLA: 6,019000");
    then_response_is(&mut world, "A", "OK");

    when_at_command_sent(&mut world, "A", "AT+CGLA=0,10,\"0070000001\"");
    then_response_is(&mut world, "A", "+CGLA: 6,029000");
    then_response_is(&mut world, "A", "OK");

    when_at_command_sent(&mut world, "A", "AT+CGLA=1,10,\"0170800200\"");
    then_response_is(&mut world, "A", "+CGLA: 4,9000");
    then_response_is(&mut world, "A", "OK");

    when_at_command_sent(&mut world, "A", "AT+CGLA=2,10,\"02F2000000\"");
    then_response_is(&mut world, "A", "+CME ERROR: not found");

    when_at_command_sent(&mut world, "A", "AT+CGLA=1,10,\"01F2000000\"");
    then_response_is(
        &mut world,
        "A",
        "+CGLA: 110,62338202782183023F00A50C80016187010183040007DBF08A01058B062F0601020002C60C90016083010183010A83010D8102FFFF9000",
    );
    then_response_is(&mut world, "A", "OK");
}

#[test]
fn test_aid_reset_on_select_outside() {
    let mut world = World::new();
    given_modem_with_sim_type(&mut world, "A", 2); // CTS profile with USIM AID

    when_at_command_sent(
        &mut world,
        "A",
        "AT+CSIM=42,\"00A4040010A0000000871002FF86FF0389FFFFFFFF\"",
    );
    then_response_is(&mut world, "A", "+CSIM: 4,6147");
    then_response_is(&mut world, "A", "OK");

    when_at_command_sent(&mut world, "A", "AT+CSIM=14,\"00A40004027F10\"");
    then_response_is(&mut world, "A", "+CSIM: 4,9000");
    then_response_is(&mut world, "A", "OK");

    when_at_command_sent(&mut world, "A", "AT+CSIM=10,\"00F2000000\"");
    then_response_is(
        &mut world,
        "A",
        "+CSIM: 110,62338202782183023F00A50C80016187010183040007DBF08A01058B062F0601020002C60C90016083010183010A83010D8102FFFF9000",
    );
    then_response_is(&mut world, "A", "OK");
}

#[test]
fn test_select_invalid_p2() {
    let mut world = World::new();
    given_modem_with_sim_type(&mut world, "A", 1);

    // P2 = 08 (ReturnProprietary) is unsupported, should return
    // SW_INCORRECT_PARAMS (6A86)
    when_at_command_sent(&mut world, "A", "AT+CGLA=0,14,\"00A40008027F10\"");
    then_response_is(&mut world, "A", "+CGLA: 4,6A86");
    then_response_is(&mut world, "A", "OK");

    // Invalid P2 (e.g. 01) should return SW_INCORRECT_PARAMS (6A86)
    when_at_command_sent(&mut world, "A", "AT+CGLA=0,14,\"00A40001027F10\"");
    then_response_is(&mut world, "A", "+CGLA: 4,6A86");
    then_response_is(&mut world, "A", "OK");
}

#[test]
fn test_read_record_invalid_p2() {
    let mut world = World::new();
    given_modem_with_sim_type(&mut world, "A", 1);

    // Select EF_MSISDN (linear fixed)
    when_at_command_sent(&mut world, "A", "AT+CGLA=0,14,\"00A40004026F40\"");
    then_response_is(&mut world, "A", "+CGLA: 4,9000");
    then_response_is(&mut world, "A", "OK");

    // Read record 1 with P2 = 02 (NextRecord - unsupported), should return 6A86
    when_at_command_sent(&mut world, "A", "AT+CGLA=0,10,\"00B2010200\"");
    then_response_is(&mut world, "A", "+CGLA: 4,6A86");
    then_response_is(&mut world, "A", "OK");
}

#[test]
fn test_crsm_read_record_invalid_p2() {
    let mut world = World::new();
    given_modem_with_msisdn_in_fs(&mut world, "A");

    // Read record 1 of EF_MSISDN (6F40) via CRSM with P2 = 02 (NextRecord -
    // unsupported), should return 6A86 (sw 106,134)
    when_at_command_sent(&mut world, "A", "AT+CRSM=178,28480,1,2,0");
    then_response_is(&mut world, "A", "+CRSM: 106,134");
    then_response_is(&mut world, "A", "OK");

    // With P2 = 04 (AbsoluteMode - supported)
    when_at_command_sent(&mut world, "A", "AT+CRSM=178,28480,1,4,28");
    then_response_is(
        &mut world,
        "A",
        "+CRSM: 144,0,FFFFFFFFFFFFFFFFFFFFFFFFFFFF07915155251100F1FFFFFFFFFFFF",
    );
    then_response_is(&mut world, "A", "OK");
}

#[test]
fn test_select_aid_does_not_match_file_id_mock() {
    let mut world = World::new();
    let xml = r#"<IccProfile>
        <MF>
            <ADF aid="A0000000871002FF86FF0389FFFFFFFF">
                <File id="A000">
                    <CGLA cmd="00A4000402A000">4,6F00</CGLA>
                </File>
                <CGLA cmd="00A4040010A0000000871002FF86FF0389FFFFFFFF">4,9000</CGLA>
            </ADF>
        </MF>
    </IccProfile>"#;
    given_modem_with_xml_profile(&mut world, "A", xml);

    when_at_command_sent(
        &mut world,
        "A",
        "AT+CGLA=0,42,\"00A4040010A0000000871002FF86FF0389FFFFFFFF\"",
    );
    then_response_is(&mut world, "A", "+CGLA: 4,9000");
    then_response_is(&mut world, "A", "OK");
}

#[test]
fn test_cgla_get_response_retrieves_buffered_data() {
    let mut world = World::new();
    given_modem_with_sim_type(&mut world, "A", 1);

    // 1. Send SELECT command for MF (3F00) with P2=00 (ReturnFcp) on channel 0
    when_at_command_sent(&mut world, "A", "AT+CGLA=0,14,\"00A40000023F00\"");
    then_response_is(&mut world, "A", "+CGLA: 4,6135");
    then_response_is(&mut world, "A", "OK");

    // 2. Issuing GET_RESPONSE (0xC0) retrieves the buffered FCP template (53 bytes
    //    = 0x35)
    when_at_command_sent(&mut world, "A", "AT+CGLA=0,10,\"00C0000035\"");
    then_response_is(
        &mut world,
        "A",
        "+CGLA: 110,62338202782183023F00A50C80016187010183040007DBF08A01058B062F0601020002C60C90016083010183010A83010D8102FFFF9000",
    );
    then_response_is(&mut world, "A", "OK");
}

#[test]
fn test_cgla_get_response_partial_reads() {
    let mut world = World::new();
    given_modem_with_sim_type(&mut world, "A", 1);

    when_at_command_sent(&mut world, "A", "AT+CGLA=0,14,\"00A40000023F00\"");
    then_response_is(&mut world, "A", "+CGLA: 4,6135");
    then_response_is(&mut world, "A", "OK");

    // Read first 10 (0x0A) bytes -> 43 (0x2B) bytes remaining, status word 612B
    when_at_command_sent(&mut world, "A", "AT+CGLA=0,10,\"00C000000A\"");
    then_response_is(&mut world, "A", "+CGLA: 24,62338202782183023F00612B");
    then_response_is(&mut world, "A", "OK");

    // Read remaining 43 (0x2B) bytes -> status word 9000
    when_at_command_sent(&mut world, "A", "AT+CGLA=0,10,\"00C000002B\"");
    then_response_is(
        &mut world,
        "A",
        "+CGLA: 90,A50C80016187010183040007DBF08A01058B062F0601020002C60C90016083010183010A83010D8102FFFF9000",
    );
    then_response_is(&mut world, "A", "OK");
}

#[test]
fn test_response_buffer_cleared_on_other_command() {
    let mut world = World::new();
    given_modem_with_sim_type(&mut world, "A", 1);

    when_at_command_sent(&mut world, "A", "AT+CGLA=0,14,\"00A40000023F00\"");
    then_response_is(&mut world, "A", "+CGLA: 4,6135");
    then_response_is(&mut world, "A", "OK");

    // Send another command (e.g. STATUS 0xF2) -> clears response buffer
    when_at_command_sent(&mut world, "A", "AT+CGLA=0,10,\"00F2000000\"");
    then_response_is(
        &mut world,
        "A",
        "+CGLA: 110,62338202782183023F00A50C80016187010183040007DBF08A01058B062F0601020002C60C90016083010183010A83010D8102FFFF9000",
    );
    then_response_is(&mut world, "A", "OK");

    // GET_RESPONSE now returns 6A88 because buffer was cleared
    when_at_command_sent(&mut world, "A", "AT+CGLA=0,10,\"00C0000035\"");
    then_response_is(&mut world, "A", "+CGLA: 4,6A88");
    then_response_is(&mut world, "A", "OK");
}

#[test]
fn test_response_buffer_cleared_on_channel_close() {
    let mut world = World::new();
    given_modem_with_sim_type(&mut world, "A", 1);

    // 1. Open channel 1
    when_at_command_sent(&mut world, "A", "AT+CGLA=0,10,\"0070000001\"");
    then_response_is(&mut world, "A", "+CGLA: 6,019000");
    then_response_is(&mut world, "A", "OK");

    // 2. Select MF on channel 1 (buffers FCP)
    when_at_command_sent(&mut world, "A", "AT+CGLA=1,14,\"01A40000023F00\"");
    then_response_is(&mut world, "A", "+CGLA: 4,6135");
    then_response_is(&mut world, "A", "OK");

    // 3. Close channel 1
    when_at_command_sent(&mut world, "A", "AT+CGLA=0,10,\"0070800101\"");
    then_response_is(&mut world, "A", "+CGLA: 4,9000");
    then_response_is(&mut world, "A", "OK");

    // 4. Open channel 1 again
    when_at_command_sent(&mut world, "A", "AT+CGLA=0,10,\"0070000001\"");
    then_response_is(&mut world, "A", "+CGLA: 6,019000");
    then_response_is(&mut world, "A", "OK");

    // 5. GET_RESPONSE on channel 1 returns 6A88 because buffer was cleared on close
    when_at_command_sent(&mut world, "A", "AT+CGLA=1,10,\"01C0000035\"");
    then_response_is(&mut world, "A", "+CGLA: 4,6A88");
    then_response_is(&mut world, "A", "OK");
}

#[test]
fn test_crsm_get_response_ef_msisdn() {
    let mut world = World::new();
    world.given_modem_with_msisdn_in_fs("A");

    // GET_RESPONSE for EF_MSISDN (6F40 / 28480) with P3 = 15
    // Header format (15 bytes): 00 00 00 1C 6F 40 04 00 00 00 00 00 02 01 1C
    world.send_and_expect(
        "A",
        "AT+CRSM=192,28480,0,0,15",
        &["+CRSM: 144,0,0000001C6F4004000000000002011C", "OK"],
    );
}

#[test]
fn test_crsm_get_response_df_telecom() {
    let mut world = World::new();
    world.given_modem_with_msisdn_in_fs("A");

    // GET_RESPONSE for DF_TELECOM (7F10 / 32528) with P3 = 22
    // Header format: File ID at bytes 4-5 is 7F10, File Type at byte 6 is 02
    // (DF), num_ef_children at byte 15 is 03
    world.send_and_expect(
        "A",
        "AT+CRSM=192,32528,0,0,22",
        &["+CRSM: 144,0,000000007F1002000000000000000300000000000000", "OK"],
    );
}

#[test]
fn test_crsm_get_response_p3_larger_than_header() {
    let mut world = World::new();
    world.given_modem_with_msisdn_in_fs("A");

    // GET_RESPONSE for EF_MSISDN (6F40 / 28480) with P3 = 50 > 15 byte header
    // Per ISO/IEC 7816-4 and 3GPP TS 27.007 §8.18, returns available 15 bytes
    world.send_and_expect(
        "A",
        "AT+CRSM=192,28480,0,0,50",
        &["+CRSM: 144,0,0000001C6F4004000000000002011C", "OK"],
    );
}

#[test]
fn test_crsm_get_response_transparent_ef_p3_15() {
    let mut world = World::new();
    world.given_modem("A");

    // GET_RESPONSE for EF_FPLMN (6F7B / 28539) with P3 = 15 returns 14-byte
    // header Byte 13 is 01 (length of following data), Byte 14 is 00
    // (transparent)
    world.send_and_expect(
        "A",
        "AT+CRSM=192,28539,0,0,15",
        &["+CRSM: 144,0,0000001E6F7B0400000000000100", "OK"],
    );
}

#[test]
fn test_crsm_read_record_wrong_length() {
    let mut world = World::new();
    given_modem_with_msisdn_in_fs(&mut world, "A");

    // READ_RECORD for EF_MSISDN (6F40 / 28480) with P3 = 50 > 28 byte record
    when_at_command_sent(&mut world, "A", "AT+CRSM=178,28480,1,4,50");
    then_response_is(&mut world, "A", "+CRSM: 103,0");
    then_response_is(&mut world, "A", "OK");
}

#[test]
fn test_crsm_read_record_p3_truncation() {
    let mut world = World::new();
    given_modem_with_msisdn_in_fs(&mut world, "A");

    // READ_RECORD for EF_MSISDN (6F40 / 28480) with P3 = 14 (alpha identifier
    // only)
    when_at_command_sent(&mut world, "A", "AT+CRSM=178,28480,1,4,14");
    then_response_is(&mut world, "A", "+CRSM: 144,0,FFFFFFFFFFFFFFFFFFFFFFFFFFFF");
    then_response_is(&mut world, "A", "OK");
}

#[test]
fn test_crsm_update_binary_wrong_length() {
    let mut world = World::new();
    given_modem_with_msisdn_in_fs(&mut world, "A");

    // UPDATE_BINARY with mismatched P3 (data is 6 bytes = 12 hex chars, P3 is
    // 10)
    when_at_command_sent(&mut world, "A", "AT+CRSM=214,28539,0,0,10,\"40F21040F220\"");
    then_response_is(&mut world, "A", "+CRSM: 103,0");
    then_response_is(&mut world, "A", "OK");
}

#[test]
fn test_crsm_update_record_wrong_length() {
    let mut world = World::new();
    given_modem_with_msisdn_in_fs(&mut world, "A");

    // UPDATE_RECORD with mismatched P3 (data is 28 bytes, P3 is 20)
    when_at_command_sent(
        &mut world,
        "A",
        "AT+CRSM=220,28480,1,4,20,\"FFFFFFFFFFFFFFFFFFFFFFFFFFFF07915155255155F4FFFFFFFFFFFF\"",
    );
    then_response_is(&mut world, "A", "+CRSM: 103,0");
    then_response_is(&mut world, "A", "OK");
}

#[test]
fn test_msisdn_profile_reads() {
    let test_cases = [
        (PROFILE_DEFAULT_XML, "FFFFFFFFFFFFFFFFFFFFFFFFFFFF07915155251100F1FFFFFFFFFFFF"),
        (PROFILE_TEL_ALASKA_XML, "FFFFFFFFFFFFFFFFFFFFFFFFFFFF07915155251100F1FFFFFFFFFFFF"),
        (PROFILE_CTS_XML, "00000000000000000000000000000891688118109844F0FFFFFFFFFF"),
    ];

    for (profile_xml, expected_record_hex) in test_cases {
        let mut world = World::new();
        given_modem_with_xml_profile(&mut world, "A", profile_xml);
        world.send_and_expect(
            "A",
            "AT+CRSM=178,28480,1,4,28",
            &[&format!("+CRSM: 144,0,{expected_record_hex}"), "OK"],
        );
    }
}

#[test]
fn test_carrier_api_cts_profile_mbdn_update_and_read() {
    let mut world = World::new();
    given_modem_with_xml_profile(&mut world, "A", PROFILE_CTS_XML);

    let tag_a_payload =
        "74616741FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF06812143658709FFFFFFFFFFFFFF";
    let tag_b_payload =
        "74616742FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF06819078563412FFFFFFFFFFFFFF";

    // 1. Open logical channel 1
    when_at_command_sent(&mut world, "A", "AT+CSIM=10,\"0070000000\"");
    then_response_is(&mut world, "A", "+CSIM: 6,019000");
    then_response_is(&mut world, "A", "OK");

    // 2. Select EF_MBDN (0x6FC7) on channel 1
    when_at_command_sent(&mut world, "A", "AT+CGLA=1,14,\"01A40004026FC7\"");
    then_response_is(&mut world, "A", "+CGLA: 4,9000");
    then_response_is(&mut world, "A", "OK");

    // 3. Update record 1 with tagA payload via UPDATE RECORD (0xDC)
    when_at_command_sent(&mut world, "A", &format!("AT+CGLA=1,86,\"01DC010426{tag_a_payload}\""));
    then_response_is(&mut world, "A", "+CGLA: 4,9000");
    then_response_is(&mut world, "A", "OK");

    // 4. Read record 1 via READ RECORD (0xB2)
    when_at_command_sent(&mut world, "A", "AT+CGLA=1,10,\"01B2010426\"");
    then_response_is(&mut world, "A", &format!("+CGLA: 80,{tag_a_payload}9000"));
    then_response_is(&mut world, "A", "OK");

    // 5. Update record 2 with tagB payload
    when_at_command_sent(&mut world, "A", &format!("AT+CGLA=1,86,\"01DC020426{tag_b_payload}\""));
    then_response_is(&mut world, "A", "+CGLA: 4,9000");
    then_response_is(&mut world, "A", "OK");

    // 6. Read record 2
    when_at_command_sent(&mut world, "A", "AT+CGLA=1,10,\"01B2020426\"");
    then_response_is(&mut world, "A", &format!("+CGLA: 80,{tag_b_payload}9000"));
    then_response_is(&mut world, "A", "OK");

    // 7. Verify record 1 is still tagA
    when_at_command_sent(&mut world, "A", "AT+CGLA=1,10,\"01B2010426\"");
    then_response_is(&mut world, "A", &format!("+CGLA: 80,{tag_a_payload}9000"));
    then_response_is(&mut world, "A", "OK");
}

#[test]
fn test_dual_df_synchronization_mbdn_and_msisdn() {
    let mut world = World::new();
    given_modem_with_xml_profile(&mut world, "A", PROFILE_CTS_XML);

    let mbdn_payload =
        "74616741FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF06812143658709FFFFFFFFFFFFFF";

    // 1. Update EF_MBDN via AT+CRSM (targeting DF_TELECOM path)
    when_at_command_sent(&mut world, "A", &format!("AT+CRSM=220,28615,1,4,38,\"{mbdn_payload}\""));
    then_response_is(&mut world, "A", "+CRSM: 144,0");
    then_response_is(&mut world, "A", "OK");

    // 2. Open logical channel 1 (ADF_USIM path)
    when_at_command_sent(&mut world, "A", "AT+CSIM=10,\"0070000000\"");
    then_response_is(&mut world, "A", "+CSIM: 6,019000");
    then_response_is(&mut world, "A", "OK");

    // 3. Select EF_MBDN on channel 1 and read record 1 -> should reflect the CRSM
    //    update!
    when_at_command_sent(&mut world, "A", "AT+CGLA=1,14,\"01A40004026FC7\"");
    then_response_is(&mut world, "A", "+CGLA: 4,9000");
    then_response_is(&mut world, "A", "OK");

    when_at_command_sent(&mut world, "A", "AT+CGLA=1,10,\"01B2010426\"");
    then_response_is(&mut world, "A", &format!("+CGLA: 80,{mbdn_payload}9000"));
    then_response_is(&mut world, "A", "OK");

    // 4. Update EF_MSISDN via CGLA (ADF_USIM path)
    let msisdn_payload = "74616741FFFFFFFFFFFFFFFFFFFF06812143658709FFFFFFFFFFFFFF";
    when_at_command_sent(&mut world, "A", "AT+CGLA=1,14,\"01A40004026F40\"");
    then_response_is(&mut world, "A", "+CGLA: 4,9000");
    then_response_is(&mut world, "A", "OK");

    when_at_command_sent(&mut world, "A", &format!("AT+CGLA=1,66,\"01DC01041C{msisdn_payload}\""));
    then_response_is(&mut world, "A", "+CGLA: 4,9000");
    then_response_is(&mut world, "A", "OK");

    // 5. Read EF_MSISDN via AT+CRSM -> should reflect the CGLA update!
    when_at_command_sent(&mut world, "A", "AT+CRSM=178,28480,1,4,28");
    then_response_is(&mut world, "A", &format!("+CRSM: 144,0,{msisdn_payload}"));
    then_response_is(&mut world, "A", "OK");
}

#[test]
fn test_multi_record_msisdn_preservation() {
    let mut world = World::new();
    given_modem_with_multi_record_msisdn_in_fs(&mut world, "A");

    // Read record 1 via AT+CRSM
    when_at_command_sent(&mut world, "A", "AT+CRSM=178,28480,1,4,28");
    then_response_is(
        &mut world,
        "A",
        "+CRSM: 144,0,4C696E6531FFFFFFFFFFFFFFFFFF07915155111111F1FFFFFFFFFFFF",
    );
    then_response_is(&mut world, "A", "OK");

    // Read record 2 via AT+CRSM
    when_at_command_sent(&mut world, "A", "AT+CRSM=178,28480,2,4,28");
    then_response_is(
        &mut world,
        "A",
        "+CRSM: 144,0,4C696E6532FFFFFFFFFFFFFFFFFF07915155222222F2FFFFFFFFFFFF",
    );
    then_response_is(&mut world, "A", "OK");

    // Update Line 1 programmatically (as done via Netsim gRPC)
    when_phone_number_set(&mut world, "A", "15553333333");

    // Verify record 1 reflects update
    when_at_command_sent(&mut world, "A", "AT+CRSM=178,28480,1,4,28");
    then_response_is(
        &mut world,
        "A",
        "+CRSM: 144,0,FFFFFFFFFFFFFFFFFFFFFFFFFFFF07915155333333F3FFFFFFFFFFFF",
    );
    then_response_is(&mut world, "A", "OK");

    // Crucially verify record 2 was NOT truncated or destroyed
    when_at_command_sent(&mut world, "A", "AT+CRSM=178,28480,2,4,28");
    then_response_is(
        &mut world,
        "A",
        "+CRSM: 144,0,4C696E6532FFFFFFFFFFFFFFFFFF07915155222222F2FFFFFFFFFFFF",
    );
    then_response_is(&mut world, "A", "OK");
}

#[test]
fn test_custom_record_len_msisdn_update() {
    let mut world = World::new();
    // Custom record length = 34 bytes (20 bytes alpha tag + 14 bytes footer)
    given_modem_with_custom_record_len_msisdn_in_fs(&mut world, "A", 34);

    // Update phone number programmatically (as done via Netsim gRPC)
    when_phone_number_set(&mut world, "A", "15554444444");

    // Read record 1 via AT+CRSM (length 34 bytes)
    // 20 bytes of 0xFF alpha tag (40 hex chars) + 14 bytes footer (28 hex
    // chars)
    when_at_command_sent(&mut world, "A", "AT+CRSM=178,28480,1,4,34");
    then_response_is(
        &mut world,
        "A",
        "+CRSM: 144,0,FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF07915155444444F4FFFFFFFFFFFF",
    );
    then_response_is(&mut world, "A", "OK");
}

#[test]
fn test_2g_sim_profile_preserves_type_without_adf() {
    let mut world = World::new();
    given_modem_with_2g_sim_profile(&mut world, "A");

    // 1. MSISDN is initialized in DF_TELECOM (0x7F10)
    when_at_command_sent(&mut world, "A", "AT+CRSM=178,28480,1,4,28");
    then_response_is(
        &mut world,
        "A",
        "+CRSM: 144,0,FFFFFFFFFFFFFFFFFFFFFFFFFFFF07915155251100F1FFFFFFFFFFFF",
    );
    then_response_is(&mut world, "A", "OK");

    // 2. Selecting 0x7FFF (AdfDefault) fails with SW_FILE_NOT_FOUND (6A82),
    // proving ADF_USIM was not synthesized on a 2G SIM.
    when_at_command_sent(&mut world, "A", "AT+CSIM=14,\"00A40004027FFF\"");
    then_response_is(&mut world, "A", "+CSIM: 4,6A82");
    then_response_is(&mut world, "A", "OK");
}

#[test]
fn test_cphs_mwi_not_found_in_default_sim0_profile() {
    let mut world = World::new();
    given_modem_with_xml_profile(&mut world, "A", modem_rs::profiles::PROFILE_DEFAULT_XML);

    use modem_rs::{apdu::Instruction, constants::UiccFileId};

    // ReadBinary on EF_VOICE_MAIL_INDICATOR_CPHS should return
    // SW_FILE_NOT_FOUND
    when_at_command_sent(
        &mut world,
        "A",
        &format!(
            "AT+CRSM={},{},0,0,1",
            Instruction::ReadBinary,
            UiccFileId::VoiceMailIndicatorCphs
        ),
    );
    then_response_is(&mut world, "A", RESP_CRSM_FILE_NOT_FOUND);
    then_response_is(&mut world, "A", "OK");

    // GetResponse on EF_VOICE_MAIL_INDICATOR_CPHS should also return
    // SW_FILE_NOT_FOUND
    when_at_command_sent(
        &mut world,
        "A",
        &format!(
            "AT+CRSM={},{},0,0,15",
            Instruction::GetResponse,
            UiccFileId::VoiceMailIndicatorCphs
        ),
    );
    then_response_is(&mut world, "A", RESP_CRSM_FILE_NOT_FOUND);
    then_response_is(&mut world, "A", "OK");
}

fn assert_pin_retries(world: &mut World, modem: &str, pin_type: &str, retries: u32, default: u32) {
    world.send_and_expect(
        modem,
        &format!("AT+CPINR=\"{pin_type}\""),
        &[&format!("+CPINR: \"{pin_type}\",{retries},{default}"), "OK"],
    );
}

#[test]
fn test_pin1_pin2_isolation_and_facility_locks() {
    let mut world = World::new();
    world.given_modem("A");

    world.send_and_expect_ok("A", "AT+CMEE=1");

    // 1. AT+CPWD="SC" changes PIN1
    world.send_and_expect_ok("A", "AT+CPWD=\"SC\",\"1234\",\"9999\"");

    // 2. AT+CPWD="P2" changes PIN2 without affecting PIN1
    world.send_and_expect_ok("A", "AT+CPWD=\"P2\",\"1234\",\"7777\"");

    // Changing with wrong old PIN2 fails with CME ERROR: 16
    world.send_and_expect_error("A", "AT+CPWD=\"P2\",\"1234\",\"8888\"", "+CME ERROR: 16");

    // Unsupported facility in AT+CPWD returns CME ERROR: 4
    world.send_and_expect_error("A", "AT+CPWD=\"XX\",\"1234\",\"5678\"", "+CME ERROR: 4");

    // 3. AT+CPIN in READY state validates old PIN1 before update
    world.send_and_expect_ok("A", "AT+CPIN=\"9999\",\"1111\"");
    world.send_and_expect_error("A", "AT+CPIN=\"0000\",\"2222\"", "+CME ERROR: 16");

    // 4. AT+CLCK for call barring facilities (AO, OI, etc.)
    world.send_and_expect("A", "AT+CLCK=\"AO\",2", &["+CLCK: 0", "OK"]);

    // Missing password returns CME ERROR: 16 (IncorrectPassword)
    world.send_and_expect_error("A", "AT+CLCK=\"AO\",1", "+CME ERROR: 16");
    world.send_and_expect_error("A", "AT+CLCK=\"AO\",0", "+CME ERROR: 16");
    world.send_and_expect("A", "AT+CLCK=\"AO\",2", &["+CLCK: 0", "OK"]);

    // Lock with correct default barring password "0000" succeeds
    world.send_and_expect_ok("A", "AT+CLCK=\"AO\",1,\"0000\"");
    world.send_and_expect("A", "AT+CLCK=\"AO\",2", &["+CLCK: 1", "OK"]);

    // Lock with incorrect barring password fails with CME ERROR: 16
    world.send_and_expect_error("A", "AT+CLCK=\"AO\",0,\"wrong\"", "+CME ERROR: 16");
    world.send_and_expect("A", "AT+CLCK=\"AO\",2", &["+CLCK: 1", "OK"]);

    // Unlock with correct barring password "0000" succeeds
    world.send_and_expect_ok("A", "AT+CLCK=\"AO\",0,\"0000\"");
    world.send_and_expect("A", "AT+CLCK=\"AO\",2", &["+CLCK: 0", "OK"]);

    // Change barring password using AT+CPWD="AB" from "0000" to "4321"
    world.send_and_expect_ok("A", "AT+CPWD=\"AB\",\"0000\",\"4321\"");

    // Old password fails now
    world.send_and_expect_error("A", "AT+CLCK=\"AO\",0,\"0000\"", "+CME ERROR: 16");

    // New password succeeds
    world.send_and_expect_ok("A", "AT+CLCK=\"AO\",0,\"4321\"");

    // P2 is only valid in AT+CPWD, not AT+CLCK; AT+CLCK="P2",1 returns CME
    // ERROR: 4
    world.send_and_expect_error("A", "AT+CLCK=\"P2\",1", "+CME ERROR: 4");

    // Unsupported facility returns CME ERROR: 4
    world.send_and_expect_error("A", "AT+CLCK=\"ZZ\",2", "+CME ERROR: 4");
}

#[test]
fn test_pin2_exhaustion_and_puk2_unlock() {
    let mut world = World::new();
    world.given_modem("A");

    world.send_and_expect_ok("A", "AT+CMEE=1");

    // Attempt invalid PIN2 in CPWD until retries reach 0 (2 attempts return CME
    // ERROR: 16)
    for _ in 0..2 {
        world.send_and_expect_error("A", "AT+CPWD=\"P2\",\"0000\",\"1111\"", "+CME ERROR: 16");
    }

    // Third failure exhausts retries -> CME ERROR: 18 (SimPuk2Required)
    world.send_and_expect_error("A", "AT+CPWD=\"P2\",\"0000\",\"1111\"", "+CME ERROR: 18");

    // Subsequent CPWD attempts return SimPuk2Required
    world.send_and_expect_error("A", "AT+CPWD=\"P2\",\"1234\",\"1111\"", "+CME ERROR: 18");

    // Query PIN retries confirms PIN2 is 0
    assert_pin_retries(&mut world, "A", "SIM PIN2", 0, 3);

    // Unlock PIN2 via PUK2 using AT+CPIN="<puk2>","<new_pin2>"
    world.send_and_expect_ok("A", "AT+CPIN=\"12345678\",\"5678\"");

    // PIN2 retries restored to 3
    assert_pin_retries(&mut world, "A", "SIM PIN2", 3, 3);
}

#[test]
fn test_pin1_exhaustion_in_cpwd_and_puk_unlock() {
    let mut world = World::new();
    world.given_modem("A");

    world.send_and_expect_ok("A", "AT+CMEE=1");

    for _ in 0..2 {
        world.send_and_expect_error("A", "AT+CPWD=\"SC\",\"0000\",\"1111\"", "+CME ERROR: 16");
    }

    world.send_and_expect_error("A", "AT+CPWD=\"SC\",\"0000\",\"1111\"", "+CME ERROR: 12");
    world.send_and_expect_error("A", "AT+CPWD=\"SC\",\"1234\",\"1111\"", "+CME ERROR: 12");

    assert_pin_retries(&mut world, "A", "SIM PIN", 0, 3);

    world.send_and_expect_ok("A", "AT+CPIN=\"12345678\",\"5678\"");

    assert_pin_retries(&mut world, "A", "SIM PIN", 3, 3);
}

#[test]
fn test_call_barring_lockout_after_failed_attempts() {
    let mut world = World::new();
    world.given_modem("A");

    world.send_and_expect_ok("A", "AT+CMEE=1");

    // 3 failed attempts return CME ERROR: 16
    for _ in 0..3 {
        world.send_and_expect_error("A", "AT+CLCK=\"AO\",1,\"wrong\"", "+CME ERROR: 16");
    }

    // Subsequent lock attempt is blocked -> CME ERROR: 3 (OperationNotAllowed),
    // even with correct password
    world.send_and_expect_error("A", "AT+CLCK=\"AO\",1,\"0000\"", "+CME ERROR: 3");

    // Subsequent password change attempt is also blocked -> CME ERROR: 3
    world.send_and_expect_error("A", "AT+CPWD=\"AB\",\"0000\",\"1234\"", "+CME ERROR: 3");
}

#[test]
fn test_pin1_retries_decremented_when_pin2_blocked() {
    let mut world = World::new();
    world.given_modem("A");

    world.send_and_expect_ok("A", "AT+CMEE=1");

    // Exhaust PIN2 retries to 0
    for _ in 0..2 {
        world.send_and_expect_error("A", "AT+CPWD=\"P2\",\"0000\",\"1111\"", "+CME ERROR: 16");
    }
    world.send_and_expect_error("A", "AT+CPWD=\"P2\",\"0000\",\"1111\"", "+CME ERROR: 18");
    assert_pin_retries(&mut world, "A", "SIM PIN2", 0, 3);
    assert_pin_retries(&mut world, "A", "SIM PIN", 3, 3);

    // Attempting wrong 4-digit PIN1 in AT+CPIN must decrement PIN1 retries, not
    // PUK2
    world.send_and_expect_error("A", "AT+CPIN=\"0000\",\"9999\"", "+CME ERROR: 16");
    assert_pin_retries(&mut world, "A", "SIM PIN", 2, 3);
    assert_pin_retries(&mut world, "A", "SIM PUK2", 10, 10);
}
