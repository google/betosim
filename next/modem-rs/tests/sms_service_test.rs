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

use std::time::Duration;

use jiff::{Timestamp, tz::TimeZone};
use modem_rs::PhoneNumber;
use netsim_model::RegistrationStatus;

use crate::{common::constants::*, steps::*, world::World};

// Scenario: Send SMS PDU Mode
//   Given a modem "A"
//   When AT command "AT+CMGS=14" is sent to "A"
//   Then response from "A" is "> "
//   When hex bytes are sent to "A"
//   Then response from "A" matches "+CMGS: "
//   And response from "A" is "OK"
#[test]
fn test_cmgs() {
    let mut world = World::new();
    given_modem(&mut world, "A");
    when_voice_registration_set(&mut world, "A", RegistrationStatus::RegisteredHome);

    when_at_command_sent(&mut world, "A", &format!("AT+CMGS={TEST_SMS_TPDU_LEN}"));
    then_prompt_is(&mut world, "A", "> ");

    when_hex_bytes_sent(&mut world, "A", TEST_SMS_PDU_WITH_CTRL_Z);
    then_response_contains(&mut world, "A", "+CMGS: ");
    then_response_is(&mut world, "A", "OK");
}

// Scenario: Store and Read SMS
//   Given a modem "A"
//   When AT command "AT+CMGW=16" is sent to "A"
//   Then response from "A" is "> "
//   When hex bytes are sent to "A"
//   Then response from "A" contains "+CMGW: 1"
//   And response from "A" is "OK"
//   When AT command "AT+CMGR=1" is sent to "A"
//   Then response from "A" starts with "+CMGR: 0,,16" and contains PDU
//   And response from "A" is "OK"
#[test]
fn test_store_and_read_sms() {
    let mut world = World::new();
    given_modem(&mut world, "A");

    // PDU Length = 16
    when_at_command_sent(&mut world, "A", &format!("AT+CMGW={TEST_SMS_PDU_LEN_CMGW}"));
    then_prompt_is(&mut world, "A", "> ");

    when_hex_bytes_sent(&mut world, "A", TEST_SMS_PDU_WITH_CTRL_Z);
    then_response_contains(&mut world, "A", "+CMGW: 1");
    then_response_is(&mut world, "A", "OK");

    when_at_command_sent(&mut world, "A", "AT+CMGR=1");
    then_response_contains(&mut world, "A", &format!("+CMGR: 2,,{TEST_SMS_TPDU_LEN}"));
    then_response_is(&mut world, "A", TEST_SMS_PDU);
    then_response_is(&mut world, "A", "OK");
}

#[test]
fn test_store_sms_with_stat() {
    let mut world = World::new();
    given_modem(&mut world, "A");

    // Test AT+CMGW=<length>,<stat> syntax with stat = 0 (REC UNREAD)
    when_at_command_sent(&mut world, "A", &format!("AT+CMGW={TEST_SMS_PDU_LEN_CMGW},0"));
    then_prompt_is(&mut world, "A", "> ");

    when_hex_bytes_sent(&mut world, "A", TEST_SMS_PDU_WITH_CTRL_Z);
    then_response_contains(&mut world, "A", "+CMGW: 1");
    then_response_is(&mut world, "A", "OK");

    when_at_command_sent(&mut world, "A", "AT+CMGR=1");
    then_response_contains(&mut world, "A", &format!("+CMGR: 0,,{TEST_SMS_TPDU_LEN}"));
    then_response_is(&mut world, "A", TEST_SMS_PDU);
    then_response_is(&mut world, "A", "OK");

    // Test AT+CMGW=<length>,<stat> syntax with stat = 1 (REC READ)
    when_at_command_sent(&mut world, "A", &format!("AT+CMGW={TEST_SMS_PDU_LEN_CMGW},1"));
    then_prompt_is(&mut world, "A", "> ");

    when_hex_bytes_sent(&mut world, "A", TEST_SMS_PDU_WITH_CTRL_Z);
    then_response_contains(&mut world, "A", "+CMGW: 2");
    then_response_is(&mut world, "A", "OK");

    when_at_command_sent(&mut world, "A", "AT+CMGR=2");
    then_response_contains(&mut world, "A", &format!("+CMGR: 1,,{TEST_SMS_TPDU_LEN}"));
    then_response_is(&mut world, "A", TEST_SMS_PDU);
    then_response_is(&mut world, "A", "OK");

    // Test AT+CMGW=<length>,<stat> syntax with stat = 3 (STO SENT)
    when_at_command_sent(&mut world, "A", &format!("AT+CMGW={TEST_SMS_PDU_LEN_CMGW},3"));
    then_prompt_is(&mut world, "A", "> ");

    when_hex_bytes_sent(&mut world, "A", TEST_SMS_PDU_WITH_CTRL_Z);
    then_response_contains(&mut world, "A", "+CMGW: 3");
    then_response_is(&mut world, "A", "OK");

    when_at_command_sent(&mut world, "A", "AT+CMGR=3");
    then_response_contains(&mut world, "A", &format!("+CMGR: 3,,{TEST_SMS_TPDU_LEN}"));
    then_response_is(&mut world, "A", TEST_SMS_PDU);
    then_response_is(&mut world, "A", "OK");

    // Test AT+CMGW=<length> syntax without stat defaults to stat = 2 (STO UNSENT)
    // per TS 27.005
    when_at_command_sent(&mut world, "A", &format!("AT+CMGW={TEST_SMS_PDU_LEN_CMGW}"));
    then_prompt_is(&mut world, "A", "> ");

    when_hex_bytes_sent(&mut world, "A", TEST_SMS_PDU_WITH_CTRL_Z);
    then_response_contains(&mut world, "A", "+CMGW: 4");
    then_response_is(&mut world, "A", "OK");

    when_at_command_sent(&mut world, "A", "AT+CMGR=4");
    then_response_contains(&mut world, "A", &format!("+CMGR: 2,,{TEST_SMS_TPDU_LEN}"));
    then_response_is(&mut world, "A", TEST_SMS_PDU);
    then_response_is(&mut world, "A", "OK");
}

fn select_sim_storage(world: &mut World, modem: &str) {
    when_at_command_sent(world, modem, "AT+CPMS=\"SM\",\"SM\",\"SM\"");
    then_wait_for_response_containing(world, modem, "OK");
}

// Scenario: Store and Read SMS on SIM
//   Given a modem "A"
//   When AT command 'AT+CPMS="SM","SM","SM"' is sent to "A"
//   Then response from "A" is "OK"
#[test]
fn test_store_and_read_sms_on_sim() {
    let mut world = World::new();
    given_modem(&mut world, "A");

    select_sim_storage(&mut world, "A");

    when_at_command_sent(&mut world, "A", &format!("AT+CMGW={TEST_SMS_PDU_LEN_CMGW}"));
    then_prompt_is(&mut world, "A", "> ");

    when_hex_bytes_sent(&mut world, "A", TEST_SMS_PDU_WITH_CTRL_Z);
    then_response_contains(&mut world, "A", "+CMGW: 1");
    then_response_is(&mut world, "A", "OK");

    when_at_command_sent(&mut world, "A", "AT+CMGR=1");
    then_response_contains(&mut world, "A", &format!("+CMGR: 2,,{TEST_SMS_TPDU_LEN}"));
    then_response_is(&mut world, "A", TEST_SMS_PDU);
    then_response_is(&mut world, "A", "OK");
}

// Scenario: Delete SMS
//   Given a modem "A"
//   When AT command "AT+CMGW=16" is sent to "A"
//   Then response from "A" is "> "
//   When hex bytes are sent to "A"
//   Then response from "A" contains "+CMGW: 1"
//   And response from "A" is "OK"
//   When AT command "AT+CMGD=1" is sent to "A"
//   Then response from "A" is "OK"
//   When AT command "AT+CMGR=1" is sent to "A"
//   Then response from "A" is "ERROR"
#[test]
fn test_delete_sms() {
    let mut world = World::new();
    world.given_modem("A");

    world.when_at_command("A", &format!("AT+CMGW={TEST_SMS_PDU_LEN_CMGW}"));
    world.then_prompt("A");

    world.when_hex_bytes("A", TEST_SMS_PDU_WITH_CTRL_Z);
    world.then_response_contains("A", "+CMGW: 1");
    world.then_response_is("A", "OK");

    world.send_and_expect_ok("A", "AT+CMGD=1");
    world.send_and_expect_error("A", "AT+CMGR=1", "ERROR");
}

// Scenario: Delete SMS on SIM
//   (Same logic just ensure CPMS set correctly to SIM storage)
#[test]
fn test_delete_sms_on_sim() {
    let mut world = World::new();
    world.given_modem("A");

    world.select_sim_storage("A");

    world.when_at_command("A", &format!("AT+CMGW={TEST_SMS_PDU_LEN_CMGW}"));
    world.then_prompt("A");

    world.when_hex_bytes("A", TEST_SMS_PDU_WITH_CTRL_Z);
    world.then_response_contains("A", "+CMGW: 1");
    world.then_response_is("A", "OK");

    world.send_and_expect_ok("A", "AT+CMGD=1");
    world.send_and_expect_error("A", "AT+CMGR=1", "ERROR");
}

// Scenario: SMS New Message Acknowledgement
//   Given a modem "A"
//   When AT command "AT+CNMA" is sent to "A"
//   Then response from "A" is "OK"
#[test]
fn test_cnma() {
    let mut world = World::new();
    given_modem(&mut world, "A");
    when_at_command_sent(&mut world, "A", "AT+CNMA");
    then_response_is(&mut world, "A", "OK");
}

#[test]
fn test_cnma_with_val() {
    let mut world = World::new();
    given_modem(&mut world, "A");
    when_at_command_sent(&mut world, "A", "AT+CNMA=1");
    then_response_is(&mut world, "A", "OK");
}

// Scenario: Set SMS Message Format
//   Given a modem "A"
//   When AT command "AT+CMGF=1" is sent to "A"
//   Then response from "A" is "OK"
#[test]
fn test_cmgf() {
    let mut world = World::new();
    given_modem(&mut world, "A");
    when_at_command_sent(&mut world, "A", "AT+CMGF=1");
    then_response_is(&mut world, "A", "OK");
}

// Scenario: Broadcast Configuration
//   Given a modem "A"
//   When AT command "AT+CSCB?" is sent to "A"
//   Then response from "A" is '+CSCB: 0,"",""'
//   And response from "A" is "OK"
//   When AT command 'AT+CSCB=0,"1,2,3","4,5,6"' is sent to "A"
//   Then response from "A" is "OK"
//   When AT command "AT+CSCB?" is sent to "A"
//   Then response from "A" is '+CSCB: 0,"1,2,3","4,5,6"'
//   And response from "A" is "OK"
#[test]
fn test_broadcast_config() {
    let mut world = World::new();
    given_modem(&mut world, "A");

    // Default query
    when_at_command_sent(&mut world, "A", "AT+CSCB?");
    then_response_is(&mut world, "A", "+CSCB: 0,\"\",\"\"");
    then_response_is(&mut world, "A", "OK");

    // Set config
    when_at_command_sent(&mut world, "A", "AT+CSCB=0,\"1,2,3\",\"4,5,6\"");
    then_response_is(&mut world, "A", "OK");

    // Query back
    when_at_command_sent(&mut world, "A", "AT+CSCB?");
    then_response_is(&mut world, "A", "+CSCB: 0,\"1,2,3\",\"4,5,6\"");
    then_response_is(&mut world, "A", "OK");
}

// Scenario: Set SMSC Address
//   Given a modem "A"
//   When AT command 'AT+CSCA="+1234567890"' is sent to "A"
//   Then response from "A" is "OK"
//   When AT command "AT+CSCA?" is sent to "A"
//   Then response from "A" is '+CSCA: "+1234567890",145'
//   And response from "A" is "OK"
#[test]
fn test_smsc_address() {
    let mut world = World::new();
    given_modem(&mut world, "A");

    when_at_command_sent(&mut world, "A", &format!("AT+CSCA=\"{TEST_SMSC}\""));
    then_response_is(&mut world, "A", "OK");

    when_at_command_sent(&mut world, "A", "AT+CSCA?");
    then_response_is(&mut world, "A", &format!("+CSCA: \"{TEST_SMSC}\",{}", TOA_INTERNATIONAL));
    then_response_is(&mut world, "A", "OK");
}

#[test]
fn test_smsc_address_with_tosca() {
    let mut world = World::new();
    given_modem(&mut world, "A");

    // Test with explicit tosca 145
    when_at_command_sent(
        &mut world,
        "A",
        &format!("AT+CSCA=\"{}\",{}", TEST_SMSC, TOA_INTERNATIONAL),
    );
    then_response_is(&mut world, "A", "OK");

    when_at_command_sent(&mut world, "A", "AT+CSCA?");
    then_response_is(&mut world, "A", &format!("+CSCA: \"{TEST_SMSC}\",{}", TOA_INTERNATIONAL));
    then_response_is(&mut world, "A", "OK");

    // Test with explicit tosca 129
    when_at_command_sent(
        &mut world,
        "A",
        &format!("AT+CSCA=\"{}\",{}", TEST_SMSC_NO_PLUS, TOA_NATIONAL),
    );
    then_response_is(&mut world, "A", "OK");

    when_at_command_sent(&mut world, "A", "AT+CSCA?");
    then_response_is(&mut world, "A", &format!("+CSCA: \"{TEST_SMSC_NO_PLUS}\",{}", TOA_NATIONAL));
    then_response_is(&mut world, "A", "OK");

    // Test with empty address and tosca 0 (the failure case)
    when_at_command_sent(&mut world, "A", "AT+CSCA=\"\",0");
    then_response_is(&mut world, "A", "OK");

    when_at_command_sent(&mut world, "A", "AT+CSCA?");
    then_response_is(&mut world, "A", "+CSCA: \"\",0");
    then_response_is(&mut world, "A", "OK");

    // Test default tosca 129 (no +)
    when_at_command_sent(&mut world, "A", &format!("AT+CSCA=\"{TEST_SMSC_NO_PLUS}\""));
    then_response_is(&mut world, "A", "OK");

    when_at_command_sent(&mut world, "A", "AT+CSCA?");
    then_response_is(&mut world, "A", &format!("+CSCA: \"{TEST_SMSC_NO_PLUS}\",{}", TOA_NATIONAL));
    then_response_is(&mut world, "A", "OK");
}

// Scenario: Set Preferred Message Storage
//   Given a modem "A"
//   When AT command 'AT+CPMS="SM","SM","SM"' is sent to "A"
//   Then wait for OK
//   When AT command "AT+CPMS?" is sent to "A"
//   Then response from "A" matches "+CPMS: ..."
//   And response from "A" is "OK"
#[test]
fn test_set_preferred_message_storage() {
    let mut world = World::new();
    given_modem(&mut world, "A");

    when_at_command_sent(&mut world, "A", "AT+CPMS=\"SM\",\"SM\",\"SM\"");
    then_wait_for_response_containing(&mut world, "A", "OK");

    when_at_command_sent(&mut world, "A", "AT+CPMS?");
    then_response_contains(&mut world, "A", "+CPMS: \"SM\",0,255,\"SM\",0,255,\"SM\",0,255");
    then_response_is(&mut world, "A", "OK");
}

#[test]
fn test_set_preferred_message_storage_partial_and_invalid() {
    let mut world = World::new();
    given_modem(&mut world, "A");

    // Partial update: set only storage1 to "SM", storage2 and storage3 remain
    // default "ME"
    when_at_command_sent(&mut world, "A", "AT+CPMS=\"SM\"");
    then_response_is(&mut world, "A", "OK");

    when_at_command_sent(&mut world, "A", "AT+CPMS?");
    then_response_contains(&mut world, "A", "+CPMS: \"SM\",0,255,\"ME\",0,255,\"ME\",0,255");
    then_response_is(&mut world, "A", "OK");

    // Invalid storage string returns ERROR
    when_at_command_sent(&mut world, "A", "AT+CPMS=\"INVALID\"");
    then_response_is(&mut world, "A", "ERROR");
}

// Scenario: Send SMS Text Mode
//   Given a modem "A"
//   And a modem "B" with number "12345"
//   When AT command "AT+CMGF=1" is sent to "A"
//   Then response from "A" is "OK"
//   When AT command 'AT+CMGS="12345"' is sent to "A"
//   Then response from "A" is "> "
//   When hex bytes "48656C6C6F1A" (Hello^Z) are sent to "A"
//   Then response from "A" matches "+CMGS: "
//   And response from "A" is "OK"
//   Then response from "B" starts with "+CMT: "12345""
//   And response from "B" ends with "Hello"
#[test]
fn test_send_sms_text_mode() {
    let mut world = World::new();
    given_modem_with_number(&mut world, "A", SMS_SENDER);
    given_modem_with_number(&mut world, "B", SMS_RECEIVER);

    when_at_command_sent(&mut world, "A", "AT+CMGF=1");
    then_response_is(&mut world, "A", "OK");

    when_at_command_sent(&mut world, "B", "AT+CMGF=1");
    then_response_is(&mut world, "B", "OK");

    when_at_command_sent(&mut world, "A", &format!("AT+CMGS=\"{SMS_RECEIVER}\""));
    then_prompt_is(&mut world, "A", "> ");

    // Hello + Ctrl-Z
    when_hex_bytes_sent(&mut world, "A", TEXT_SMS_BODY_HEX);

    then_response_contains(&mut world, "A", "+CMGS: ");
    then_response_is(&mut world, "A", "OK");

    // Verify reception on B
    // CMT unsolicited
    then_wait_for_response_containing(&mut world, "B", &format!("+CMT: \"{SMS_SENDER}\""));
    then_response_is(&mut world, "B", TEXT_SMS_BODY);
}

// Scenario: Incoming SMS (Text and PDU)
//   Given a modem "A"
//   When external SMS "Hello" from "123456" is sent to "A"
//   Then A receives +CMT: "123456",,"..."\r\nHello
//   When external PDU "0011..." is sent to "A"
//   Then A receives +CMT: ,<len>\r\nPDU...
#[test]
fn test_incoming_sms() {
    let mut world = World::new();
    given_modem(&mut world, "A");
    when_voice_registration_set(&mut world, "A", RegistrationStatus::RegisteredHome);

    // 1. Text Mode
    when_at_command_sent(&mut world, "A", "AT+CMGF=1");
    then_response_is(&mut world, "A", "OK");

    when_incoming_sms_received(&mut world, "A", TEST_PHONE_NUMBER, TEST_SMS_TEXT);

    // Expect +CMT response
    then_wait_for_response_containing(&mut world, "A", &format!("+CMT: \"{TEST_PHONE_NUMBER}\""));
    then_response_is(&mut world, "A", TEST_SMS_TEXT);

    // 2. PDU Mode
    // Construct PDU for "Hello World"
    // PDU_HEX = "0011000B915155255155F40000AA01F0"
    // TPDU Len = 15.

    when_incoming_pdu_received(&mut world, "A", TEST_SMS_PDU);

    then_wait_for_response_containing(&mut world, "A", &format!("+CMT: ,{TEST_SMS_TPDU_LEN}"));
    then_response_is(&mut world, "A", TEST_SMS_PDU);
}

#[test]
fn test_incoming_sms_pdu_mode_plain_text() {
    let mut world = World::new();
    given_modem(&mut world, "A");
    when_voice_registration_set(&mut world, "A", RegistrationStatus::RegisteredHome);

    // Set PDU Mode
    when_at_command_sent(&mut world, "A", "AT+CMGF=0");
    then_response_is(&mut world, "A", "OK");

    // Trigger incoming SMS with plain text
    when_incoming_sms_received(&mut world, "A", TEST_PHONE_NUMBER, TEST_SMS_TEXT);

    // Expect +CMT response header with TPDU length 38
    then_wait_for_response_containing(&mut world, "A", &format!("+CMT: ,{TEST_SMS_UCS2_TPDU_LEN}"));

    // Expect the PDU line containing the UCS-2 encoded "Hello World"
    then_wait_for_response_containing(&mut world, "A", TEST_SMS_TEXT_UCS2_HEX);
}

#[test]
fn test_sms_send_abort() {
    let mut world = World::new();
    given_modem(&mut world, "A");
    when_voice_registration_set(&mut world, "A", RegistrationStatus::RegisteredHome);

    // Set PDU Mode
    when_at_command_sent(&mut world, "A", "AT+CMGF=0");
    then_response_is(&mut world, "A", "OK");

    // Start SMS send (PDU mode)
    when_at_command_sent(&mut world, "A", &format!("AT+CMGS={TEST_SMS_TPDU_LEN}"));
    then_prompt_is(&mut world, "A", "> ");

    // Send ESC (1B) to abort
    when_hex_bytes_sent(&mut world, "A", "1B");

    // It should return OK
    then_response_is(&mut world, "A", "OK");

    // Verify we are back to normal command mode
    when_at_command_sent(&mut world, "A", "AT");
    then_response_is(&mut world, "A", "OK");
}

#[test]
fn test_sms_pdu_error_when_sim_absent() {
    let mut world = World::new();
    given_modem(&mut world, "A");

    // Set SMS storage to SIM
    when_at_command_sent(&mut world, "A", "AT+CPMS=\"SM\",\"SM\",\"SM\"");
    then_wait_for_response_containing(&mut world, "A", "OK");

    // Remove SIM
    when_sim_status_set(&mut world, "A", false);
    then_wait_for_response_containing(&mut world, "A", "+CPIN: ABSENT");

    // Set PDU Mode
    when_at_command_sent(&mut world, "A", "AT+CMGF=0");
    then_response_is(&mut world, "A", "OK");

    // Start SMS store (PDU mode)
    // AT+CMGW=<length>
    when_at_command_sent(&mut world, "A", &format!("AT+CMGW={TEST_SMS_TPDU_LEN}"));
    then_prompt_is(&mut world, "A", "> ");

    // Send PDU data + Ctrl+Z (0x1A)
    // Since SIM is absent, this should return ERROR (translated from
    // SmsError::Generic)
    when_hex_bytes_sent(&mut world, "A", INVALID_SMS_PDU_HEX);
    then_response_is(&mut world, "A", "ERROR");
}

#[test]
fn test_legacy_sms() {
    let mut world = World::new();
    given_goldfish_37_modem(&mut world, "A");
    given_goldfish_37_modem(&mut world, "B");
    let id_a = world.modems.get("A").unwrap().0;
    let id_b = world.modems.get("B").unwrap().0;

    // Set phone numbers
    if let Some(modem) = world.manager.get_modem_mut(id_a) {
        modem.set_phone_number(PhoneNumber::new_for_test("98765"));
        modem.set_voice_registration(RegistrationStatus::RegisteredHome);
    }
    if let Some(modem) = world.manager.get_modem_mut(id_b) {
        modem.set_phone_number(PhoneNumber::new_for_test("12345"));
        modem.set_voice_registration(RegistrationStatus::RegisteredHome);
    }

    // 1. Text Mode SMS Reception (B receives from A)
    when_at_command_sent(&mut world, "B", "AT+CMGF=1");
    then_response_is(&mut world, "B", "OK");

    when_incoming_sms_received(&mut world, "B", "98765", "Hello");
    // Should receive with \r delimiters and NO \n (handled by
    // then_response_is/normalize)
    then_wait_for_response_containing(&mut world, "B", "+CMT: \"98765\"");
    then_response_is(&mut world, "B", "Hello");

    // 2. PDU Mode SMS Reception (B receives from A, via incoming PDU injection)
    // Switch B back to PDU mode
    when_at_command_sent(&mut world, "B", "AT+CMGF=0");
    then_response_is(&mut world, "B", "OK");

    // TEST_SMS_PDU = "0011000B915155255155F40000AA01F0" (tpdu_len = 15)
    when_incoming_pdu_received(&mut world, "B", TEST_SMS_PDU);

    // Expect +CMT: 15 (no comma) and PDU
    then_wait_for_response_containing(&mut world, "B", "+CMT: 15");
    then_response_is(&mut world, "B", TEST_SMS_PDU);
}

#[test]
fn test_sms_status_report() {
    let mut world = World::new();
    world.clock.set_zoned(
        Timestamp::from_second(1_781_551_845).expect("valid timestamp").to_zoned(TimeZone::UTC),
    );
    given_modem_with_number(&mut world, "A", TEST_PHONE_NUMBER_LONG_B);
    given_modem_with_number(&mut world, "B", TEST_PHONE_NUMBER_LONG_A);

    // Set A and B to PDU mode
    when_at_command_sent(&mut world, "A", "AT+CMGF=0");
    then_response_is(&mut world, "A", "OK");
    when_at_command_sent(&mut world, "B", "AT+CMGF=0");
    then_response_is(&mut world, "B", "OK");

    // Send SMS with SRR from A to B
    when_at_command_sent(&mut world, "A", "AT+CMGS=15");
    then_prompt_is(&mut world, "A", "> ");

    // PDU with SRR (0x31)
    let pdu_with_srr_ctrl_z = "0031000B915155255155F40000AA01F01A";
    when_hex_bytes_sent(&mut world, "A", pdu_with_srr_ctrl_z);

    // A should get CMGS success response
    then_response_contains(&mut world, "A", "+CMGS: ");
    then_response_is(&mut world, "A", "OK");

    // B should receive the CMT unsolicited response
    then_wait_for_response_containing(&mut world, "B", "+CMT: ,20");
    then_wait_for_response_containing(&mut world, "B", "00240B915155255155F6000062605191035400");

    world.clock.advance(Duration::from_secs(5));

    // B acknowledges delivery via AT+CNMA=1 per 3GPP TS 27.005
    when_at_command_sent(&mut world, "B", "AT+CNMA=1");
    then_response_is(&mut world, "B", "OK");

    // A should receive the CDS unsolicited response (Status Report)
    then_wait_for_response_containing(&mut world, "A", "+CDS: 25");
    then_response_contains(&mut world, "A", "0002010B915155255155F4626051910354006260519103050000");
}

#[test]
fn test_sms_status_report_rejected() {
    let mut world = World::new();
    given_modem_with_number(&mut world, "A", TEST_PHONE_NUMBER_LONG_B);
    given_modem_with_number(&mut world, "B", TEST_PHONE_NUMBER_LONG_A);

    when_at_command_sent(&mut world, "A", "AT+CMGF=0");
    then_response_is(&mut world, "A", "OK");
    when_at_command_sent(&mut world, "B", "AT+CMGF=0");
    then_response_is(&mut world, "B", "OK");

    when_at_command_sent(&mut world, "A", "AT+CMGS=15");
    then_prompt_is(&mut world, "A", "> ");

    let pdu_with_srr_ctrl_z = "0031000B915155255155F40000AA01F01A";
    when_hex_bytes_sent(&mut world, "A", pdu_with_srr_ctrl_z);

    then_response_contains(&mut world, "A", "+CMGS: ");
    then_response_is(&mut world, "A", "OK");

    then_wait_for_response_containing(&mut world, "B", "+CMT: ,20");
    then_wait_for_response_containing(&mut world, "B", "00240B915155255155");

    // B rejects the SMS via AT+CNMA=2 (negative ACK)
    when_at_command_sent(&mut world, "B", "AT+CNMA=2");
    then_response_is(&mut world, "B", "OK");

    // A should NOT receive any CDS response
    then_no_response(&mut world, "A");
}

#[test]
fn test_sms_stop_and_wait_fifo_delivery() {
    let mut world = World::new();
    given_modem_with_number(&mut world, "A", TEST_PHONE_NUMBER_LONG_B);
    given_modem_with_number(&mut world, "B", TEST_PHONE_NUMBER_LONG_A);
    given_modem_with_number(&mut world, "C", "15555215556");

    when_at_command_sent(&mut world, "A", "AT+CMGF=0");
    then_response_is(&mut world, "A", "OK");
    when_at_command_sent(&mut world, "B", "AT+CMGF=0");
    then_response_is(&mut world, "B", "OK");
    when_at_command_sent(&mut world, "C", "AT+CMGF=0");
    then_response_is(&mut world, "C", "OK");

    // 1. A sends SMS 1 with SRR to B
    when_at_command_sent(&mut world, "A", "AT+CMGS=15");
    then_prompt_is(&mut world, "A", "> ");

    let pdu1 = "0031000B915155255155F40000AA01F01A";
    when_hex_bytes_sent(&mut world, "A", pdu1);

    then_response_contains(&mut world, "A", "+CMGS: ");
    then_response_is(&mut world, "A", "OK");

    // B receives +CMT: for SMS 1
    then_wait_for_response_containing(&mut world, "B", "+CMT: ,20");
    then_wait_for_response_containing(&mut world, "B", "00240B915155255155");

    // 2. C sends SMS 2 with SRR to B (while B has NOT acknowledged SMS 1 yet)
    when_at_command_sent(&mut world, "C", "AT+CMGS=15");
    then_prompt_is(&mut world, "C", "> ");

    let pdu2 = "0031000B915155255155F40000AA01F11A";
    when_hex_bytes_sent(&mut world, "C", pdu2);

    then_response_contains(&mut world, "C", "+CMGS: ");
    then_response_is(&mut world, "C", "OK");

    // Stop-and-wait: B must NOT receive +CMT: for SMS 2 yet!
    then_no_response(&mut world, "B");
    then_no_response(&mut world, "A");
    then_no_response(&mut world, "C");

    // 3. B acknowledges SMS 1 via AT+CNMA=1
    when_at_command_sent(&mut world, "B", "AT+CNMA=1");
    then_response_is(&mut world, "B", "OK");

    // A receives status report for SMS 1
    then_wait_for_response_containing(&mut world, "A", "+CDS: 25");
    then_response_contains(&mut world, "A", "0002010B915155255155F4");

    // B now receives +CMT: for queued SMS 2!
    then_wait_for_response_containing(&mut world, "B", "+CMT: ,20");
    then_wait_for_response_containing(&mut world, "B", "00240B915155255155");

    // 4. B acknowledges SMS 2 via AT+CNMA=1
    when_at_command_sent(&mut world, "B", "AT+CNMA=1");
    then_response_is(&mut world, "B", "OK");

    // C receives status report for SMS 2
    then_wait_for_response_containing(&mut world, "C", "+CDS: 25");
}

#[test]
fn test_cnma_invalid_val() {
    let mut world = World::new();
    given_modem(&mut world, "A");
    when_at_command_sent(&mut world, "A", "AT+CNMA=3");
    then_response_is(&mut world, "A", "ERROR");
}

#[test]
fn test_cms_errors() {
    for (cmee, invalid_idx_err, invalid_pdu_err, invalid_txt_err) in [
        (1, "+CMS ERROR: 321", "+CMS ERROR: 304", "+CMS ERROR: 305"),
        (
            2,
            "+CMS ERROR: invalid memory index",
            "+CMS ERROR: invalid PDU mode parameter",
            "+CMS ERROR: invalid text mode parameter",
        ),
    ] {
        let mut world = World::new();
        world.given_modem("A");
        when_voice_registration_set(&mut world, "A", RegistrationStatus::RegisteredHome);

        world.send_and_expect_ok("A", &format!("AT+CMEE={cmee}"));

        // Reading invalid index
        world.send_and_expect_error("A", "AT+CMGR=99", invalid_idx_err);

        // Reading index 0
        world.send_and_expect_error("A", "AT+CMGR=0", invalid_idx_err);

        // Deleting invalid index
        world.send_and_expect_error("A", "AT+CMGD=99", invalid_idx_err);

        // Sending invalid PDU via AT+CMGS
        world.when_at_command("A", "AT+CMGS=15");
        world.then_prompt("A");
        // Send invalid hex string followed by Ctrl-Z
        world.when_hex_bytes("A", "0001001A");
        world.then_response_is("A", invalid_pdu_err);

        // Writing invalid PDU via AT+CMGW
        world.when_at_command("A", "AT+CMGW=15");
        world.then_prompt("A");
        world.when_hex_bytes("A", "1A");
        world.then_response_is("A", invalid_pdu_err);

        // Sending invalid text mode destination via AT+CMGS
        world.send_and_expect_ok("A", "AT+CMGF=1");
        world.send_and_expect_error("A", "AT+CMGS=\"invalid#phone\"", invalid_txt_err);
    }
}

#[test]
fn test_cms_sim_absent_error() {
    let mut world = World::new();
    world.given_modem("A");

    world.send_and_expect_ok("A", "AT+CMEE=1");

    // Remove SIM card
    world.when_sim_status("A", false);
    world.then_response_is("A", "+CPIN: ABSENT");

    world.select_sim_storage("A");

    // Reading from SIM when absent returns CMS ERROR 310 (SIM not inserted)
    world.send_and_expect_error("A", "AT+CMGR=1", "+CMS ERROR: 310");

    // Deleting from SIM when absent returns CMS ERROR 310
    world.send_and_expect_error("A", "AT+CMGD=1", "+CMS ERROR: 310");

    // Writing to SIM when absent returns CMS ERROR 310
    world.when_at_command("A", &format!("AT+CMGW={TEST_SMS_PDU_LEN_CMGW}"));
    world.then_prompt("A");
    world.when_hex_bytes("A", TEST_SMS_PDU_WITH_CTRL_Z);
    world.then_response_is("A", "+CMS ERROR: 310");
}

#[test]
fn test_cms_sim_locked_error() {
    let mut world = World::new();
    world.given_modem_with_locked_sim("A");
    world.send_and_expect_ok("A", "AT+CMEE=1");
    world.select_sim_storage("A");
    world.send_and_expect_error("A", "AT+CMGR=1", "+CMS ERROR: 311");
    world.send_and_expect_error("A", "AT+CMGD=1", "+CMS ERROR: 311");
}

#[test]
fn test_cmgw_with_status_parameter() {
    let mut world = World::new();
    world.given_modem("A");

    world.send_and_expect_ok("A", "AT+CMEE=1");
    world.select_sim_storage("A");

    // Status: 2 (STO UNSENT), 1 (REC READ), 3 (STO SENT)
    for (status, index) in [(2, 1), (1, 2), (3, 3)] {
        world.when_at_command("A", &format!("AT+CMGW={TEST_SMS_PDU_LEN_CMGW},{status}"));
        world.then_prompt("A");
        world.when_hex_bytes("A", TEST_SMS_PDU_WITH_CTRL_Z);
        world.then_response_contains("A", &format!("+CMGW: {index}"));
        world.then_response_is("A", "OK");

        world.when_at_command("A", &format!("AT+CMGR={index}"));
        world.then_response_contains("A", &format!("+CMGR: {status},,{TEST_SMS_TPDU_LEN}"));
        world.then_response_is("A", TEST_SMS_PDU);
        world.then_response_is("A", "OK");
    }
}

#[test]
fn test_sms_large_index_bounds() {
    let mut world = World::new();
    world.given_modem("A");

    world.send_and_expect_ok("A", "AT+CMEE=1");
    world.select_sim_storage("A");

    world.send_and_expect_error("A", "AT+CMGR=300", "+CMS ERROR: 321");
    world.send_and_expect_error("A", "AT+CMGD=300", "+CMS ERROR: 321");
}

#[test]
fn test_cpms_invalid_storage_rejected() {
    let mut world = World::new();
    world.given_modem("A");

    // Invalid storage parameter should be rejected at the parser boundary
    world.send_and_expect_error("A", "AT+CPMS=\"INVALID\",\"SM\",\"SM\"", "ERROR");
}

#[test]
fn test_sms_slot_recycling() {
    let mut world = World::new();
    world.given_modem("A");

    world.select_sim_storage("A");

    // 1. Store 3 messages: slots 1, 2, 3
    for expected_idx in 1..=3 {
        world.when_at_command("A", &format!("AT+CMGW={TEST_SMS_PDU_LEN_CMGW}"));
        world.then_prompt("A");
        world.when_hex_bytes("A", TEST_SMS_PDU_WITH_CTRL_Z);
        world.then_response_contains("A", &format!("+CMGW: {expected_idx}"));
        world.then_response_is("A", "OK");
    }

    // 2. Delete slot 2
    world.send_and_expect_ok("A", "AT+CMGD=2");

    // 3. Storing next message must recycle slot 2 (lowest available)
    world.when_at_command("A", &format!("AT+CMGW={TEST_SMS_PDU_LEN_CMGW}"));
    world.then_prompt("A");
    world.when_hex_bytes("A", TEST_SMS_PDU_WITH_CTRL_Z);
    world.then_response_contains("A", "+CMGW: 2");
    world.then_response_is("A", "OK");

    // 4. Storing subsequent message must take slot 4
    world.when_at_command("A", &format!("AT+CMGW={TEST_SMS_PDU_LEN_CMGW}"));
    world.then_prompt("A");
    world.when_hex_bytes("A", TEST_SMS_PDU_WITH_CTRL_Z);
    world.then_response_contains("A", "+CMGW: 4");
    world.then_response_is("A", "OK");
}

#[test]
fn test_sms_prompt_with_goldfish_quirk() {
    let mut world = World::new();
    world.given_goldfish_37_modem("A");
    when_voice_registration_set(&mut world, "A", RegistrationStatus::RegisteredHome);

    world.when_at_command("A", "AT+CMGS=15");
    world.then_prompt_is("A", "> \r");
}

// TS 27.005 § 3.5.1: the CMS error for a bad AT+CMGS destination depends on the
// active <mode>, because the quoted form parses identically in both.
#[test]
fn test_cmgs_invalid_destination_error_follows_message_format() {
    let mut world = World::new();
    world.given_modem("A");
    when_voice_registration_set(&mut world, "A", RegistrationStatus::RegisteredHome);
    world.send_and_expect_ok("A", "AT+CMEE=1");

    world.send_and_expect_ok("A", "AT+CMGF=1");
    world.send_and_expect_error("A", "AT+CMGS=\"invalid#phone\"", "+CMS ERROR: 305");
    world.send_and_expect_ok("A", "AT+CMGF=0");
    world.send_and_expect_error("A", "AT+CMGS=\"invalid#phone\"", "+CMS ERROR: 304");
}

// TS 27.005 § 3.5.1: the argument shape must match the active <mode>.
#[test]
fn test_cmgs_syntax_mismatch_rejected() {
    let mut world = World::new();
    world.given_modem("A");
    when_voice_registration_set(&mut world, "A", RegistrationStatus::RegisteredHome);
    world.send_and_expect_ok("A", "AT+CMEE=1");

    // In text mode, PDU syntax (AT+CMGS=<length>) is rejected with CMS 305.
    world.send_and_expect_ok("A", "AT+CMGF=1");
    world.send_and_expect_error("A", "AT+CMGS=15", "+CMS ERROR: 305");

    // In PDU mode, text syntax (AT+CMGS="<da>") is rejected with CMS 304.
    world.send_and_expect_ok("A", "AT+CMGF=0");
    world.send_and_expect_error("A", "AT+CMGS=\"12345\"", "+CMS ERROR: 304");
}

// TS 27.005 § 3.5.1 reports <mr> only on success, so a rejected message must
// not consume one. Exercised in PDU mode because that is the rejection path
// that exists today; the text mode capacity rejection arrives with GSM 7-bit
// encoding.
#[test]
fn test_rejected_sms_does_not_consume_message_reference() {
    let mut world = World::new();
    world.given_modem("A");
    when_voice_registration_set(&mut world, "A", RegistrationStatus::RegisteredHome);
    world.send_and_expect_ok("A", "AT+CMEE=1");

    world.when_at_command("A", &format!("AT+CMGS={TEST_SMS_TPDU_LEN}"));
    world.then_prompt("A");
    world.when_hex_bytes("A", INVALID_SMS_PDU_HEX);
    world.then_response_is("A", "+CMS ERROR: 304");

    // The rejected attempt did not burn a reference, so the first accepted
    // message still reports 1.
    world.when_at_command("A", &format!("AT+CMGS={TEST_SMS_TPDU_LEN}"));
    world.then_prompt("A");
    world.when_hex_bytes("A", TEST_SMS_PDU_WITH_CTRL_Z);
    world.then_response_contains("A", "+CMGS: 1");
}

#[test]
fn test_cmgs_fails_when_unregistered_or_radio_off() {
    let mut world = World::new();
    world.given_modem("A");
    world.send_and_expect_ok("A", "AT+CMEE=1");

    world.send_and_expect_error("A", "AT+CMGS=15", "+CMS ERROR: 331");

    when_voice_registration_set(&mut world, "A", RegistrationStatus::RegisteredHome);
    world.send_and_expect_ok("A", "AT+CFUN=0");
    world.send_and_expect_error("A", "AT+CMGS=15", "+CMS ERROR: 331");
}

#[test]
fn test_cmgs_goldfish_37_quirk_admission_error() {
    let mut world = World::new();
    world.given_goldfish_37_modem("A");
    world.send_and_expect_ok("A", "AT+CMEE=1");

    world.when_at_command("A", "AT+CMGS=15");
    world.then_prompt_is("A", "> \r");

    world.when_hex_bytes("A", TEST_SMS_PDU_WITH_CTRL_Z);
    world.then_response_is("A", "+CME ERROR: 30");
}

#[test]
fn test_receive_sms_suppressed_when_unregistered_or_radio_off() {
    let mut world = World::new();
    world.given_modem("A");

    when_incoming_sms_received(&mut world, "A", TEST_PHONE_NUMBER, TEST_SMS_TEXT);
    world.then_no_response("A");

    when_incoming_pdu_received(&mut world, "A", TEST_SMS_PDU);
    world.then_no_response("A");

    when_voice_registration_set(&mut world, "A", RegistrationStatus::RegisteredHome);
    world.send_and_expect_ok("A", "AT+CFUN=0");

    when_incoming_sms_received(&mut world, "A", TEST_PHONE_NUMBER, TEST_SMS_TEXT);
    world.then_no_response("A");

    when_incoming_pdu_received(&mut world, "A", TEST_SMS_PDU);
    world.then_no_response("A");
}
