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

use crate::{common::constants::*, steps::*, world::World};

// Scenario: Set and Query CLIP (Calling Line Identification Presentation)
//   Given a modem "A"
//   When AT command "AT+CLIP?" is sent to "A"
//   Then response from "A" is "+CLIP: 0,1"
//   And response from "A" is "OK"
//   When AT command "AT+CLIP=1" is sent to "A"
//   Then response from "A" is "OK"
//   When AT command "AT+CLIP?" is sent to "A"
//   Then response from "A" is "+CLIP: 1,1"
//   And response from "A" is "OK"
//   When AT command "AT+CLIP=0" is sent to "A"
//   Then response from "A" is "OK"
//   When AT command "AT+CLIP?" is sent to "A"
//   Then response from "A" is "+CLIP: 0,1"
//   And response from "A" is "OK"
#[test]
fn test_set_and_query_clip() {
    let mut world = World::new();
    given_modem(&mut world, "A");

    // Default query
    when_at_command_sent(&mut world, "A", "AT+CLIP?");
    then_response_is(&mut world, "A", "+CLIP: 0,1");
    then_response_is(&mut world, "A", "OK");

    // Enable CLIP
    when_at_command_sent(&mut world, "A", "AT+CLIP=1");
    then_response_is(&mut world, "A", "OK");

    // Query enabled state
    when_at_command_sent(&mut world, "A", "AT+CLIP?");
    then_response_is(&mut world, "A", "+CLIP: 1,1");
    then_response_is(&mut world, "A", "OK");

    // Disable CLIP
    when_at_command_sent(&mut world, "A", "AT+CLIP=0");
    then_response_is(&mut world, "A", "OK");

    // Query disabled state
    when_at_command_sent(&mut world, "A", "AT+CLIP?");
    then_response_is(&mut world, "A", "+CLIP: 0,1");
    then_response_is(&mut world, "A", "OK");
}

// Scenario: Set Call Waiting
//   Given a modem "A"
//   When AT command "AT+CCWA=1,1" is sent to "A"
//   Then response from "A" is "OK"
#[test]
fn test_set_call_waiting() {
    let mut world = World::new();
    given_modem(&mut world, "A");
    when_at_command_sent(&mut world, "A", "AT+CCWA=1,1");
    then_response_is(&mut world, "A", "OK");
}

// Scenario: Send USSD
//   Given a modem "A"
//   When AT command 'AT+CUSD=1,"*123#"' is sent to "A"
//   Then response from "A" is '+CUSD: 0,"OK",15'
//   And response from "A" is "OK"
#[test]
fn test_send_ussd() {
    let mut world = World::new();
    given_modem(&mut world, "A");
    when_at_command_sent(&mut world, "A", &format!("AT+CUSD=1,\"{TEST_USSD}\""));
    then_response_is(&mut world, "A", r#"+CUSD: 0,"OK",15"#);
    then_response_is(&mut world, "A", "OK");
}

// Scenario: Cancel USSD
//   Given a modem "A"
//   When AT command "AT+CUSD=2" is sent to "A"
//   Then response from "A" is "OK"
#[test]
fn test_cancel_ussd() {
    let mut world = World::new();
    given_modem(&mut world, "A");
    when_at_command_sent(&mut world, "A", "AT+CUSD=2");
    then_response_is(&mut world, "A", "OK");
}

// Scenario: Call Forwarding
//   Given a modem "A"
//   When AT command 'AT+CCFC=1,1,"+1234567890",145,20' is sent to "A"
//   Then response from "A" is "OK"
#[test]
fn test_call_forwarding() {
    let mut world = World::new();
    world.given_modem("A");
    world.send_and_expect_ok("A", &format!("AT+CCFC=1,1,\"{TEST_SMSC}\",{TOA_INTERNATIONAL},20"));
}

// Scenario: Query CLIR
//   Given a modem "A"
//   When AT command "AT+CLIR?" is sent to "A"
//   Then response from "A" is "+CLIR: 0,0"
//   And response from "A" is "OK"
#[test]
fn test_query_clir() {
    let mut world = World::new();
    given_modem(&mut world, "A");
    when_at_command_sent(&mut world, "A", "AT+CLIR?");
    then_response_is(&mut world, "A", "+CLIR: 0,1");
    then_response_is(&mut world, "A", "OK");
}

// Scenario: Supplementary Service Notification
//   Given a modem "A"
//   When AT command "AT+CSSN=1,1" is sent to "A"
//   Then response from "A" is "OK"
#[test]
fn test_supp_service_notification() {
    let mut world = World::new();
    given_modem(&mut world, "A");
    when_at_command_sent(&mut world, "A", "AT+CSSN=1,1");
    then_response_is(&mut world, "A", "OK");
}

// Scenario: Set Facility Lock
//   Given a modem "A"
//   When AT command 'AT+CLCK="SC",1,"1234"' is sent to "A"
//   Then response from "A" is "OK"
#[test]
fn test_set_facility_lock() {
    let mut world = World::new();
    given_modem(&mut world, "A");
    when_at_command_sent(&mut world, "A", &format!("AT+CLCK=\"SC\",1,\"{TEST_PIN}\""));
    then_response_is(&mut world, "A", "OK");
}

#[test]
fn test_query_facility_lock() {
    let mut world = World::new();
    given_modem(&mut world, "A");
    when_at_command_sent(&mut world, "A", r#"AT+CLCK="FD",2"#);
    then_response_is(&mut world, "A", "+CLCK: 0");
    then_response_is(&mut world, "A", "OK");
}

#[test]
fn test_call_forward_utility_registration_and_query() {
    let mut world = World::new();
    world.given_modem("A");

    world.send_and_expect_ok("A", &format!("AT+CCFCU=0,3,2,2,\"{CALL_PEER_ALT}\",1,\"\",\"\",,1"));
    world.send_and_expect(
        "A",
        "AT+CCFCU=0,2,2,2,\"\",1",
        &[&format!("+CCFCU: 1,1,2,2,\"{CALL_PEER_ALT}\""), "OK"],
    );
}

#[test]
fn test_call_forward_utility_cfnr_time() {
    let mut world = World::new();
    world.given_modem("A");

    world.send_and_expect_ok("A", &format!("AT+CCFCU=2,3,2,1,\"{CALL_PEER_ALT}\",1,\"\",\"\",,20"));
    world.send_and_expect(
        "A",
        "AT+CCFCU=2,2,2,1,\"\",1",
        &[&format!("+CCFCU: 1,1,2,1,\"{CALL_PEER_ALT}\",,,20"), "OK"],
    );
}

#[test]
fn test_call_forward_utility_enable_disable_erasure() {
    let mut world = World::new();
    world.given_modem("A");

    world.send_and_expect_ok("A", &format!("AT+CCFCU=1,3,2,2,\"{CALL_PEER_ALT}\",1"));
    world.send_and_expect_ok("A", "AT+CCFCU=1,0");
    world.send_and_expect(
        "A",
        "AT+CCFCU=1,2,2,2,\"\",1",
        &[&format!("+CCFCU: 0,1,2,2,\"{CALL_PEER_ALT}\""), "OK"],
    );
    world.send_and_expect_ok("A", "AT+CCFCU=1,1");
    world.send_and_expect(
        "A",
        "AT+CCFCU=1,2,2,2,\"\",1",
        &[&format!("+CCFCU: 1,1,2,2,\"{CALL_PEER_ALT}\""), "OK"],
    );
    world.send_and_expect_ok("A", "AT+CCFCU=1,4");
    world.send_and_expect("A", "AT+CCFCU=1,2,2,2,\"\",1", &["+CCFCU: 0,1,2,129,\"\"", "OK"]);
}

#[test]
fn test_call_forward_utility_all_and_conditional_queries() {
    let mut world = World::new();
    world.given_modem("A");

    world.send_and_expect_ok("A", &format!("AT+CCFCU=5,3,2,1,\"{CALL_PEER_ALT}\",1,\"\",\"\",,15"));
    world.send_and_expect(
        "A",
        "AT+CCFCU=5,2,2,1,\"\",1",
        &[
            &format!("+CCFCU: 1,1,2,1,\"{CALL_PEER_ALT}\""),
            &format!("+CCFCU: 1,1,2,1,\"{CALL_PEER_ALT}\",,,15"),
            &format!("+CCFCU: 1,1,2,1,\"{CALL_PEER_ALT}\""),
            "OK",
        ],
    );
    world.send_and_expect(
        "A",
        "AT+CCFCU=4,2,2,2,\"\",1",
        &[
            "+CCFCU: 0,1,2,129,\"\"",
            &format!("+CCFCU: 1,1,2,1,\"{CALL_PEER_ALT}\""),
            &format!("+CCFCU: 1,1,2,1,\"{CALL_PEER_ALT}\",,,15"),
            &format!("+CCFCU: 1,1,2,1,\"{CALL_PEER_ALT}\""),
            "OK",
        ],
    );
}

#[test]
fn test_call_forwarding_registration_query_toa_inference() {
    let mut world = World::new();
    world.given_modem("A");

    world.send_and_expect_ok("A", "AT+CCFC=0,3,\"+15551234\"");
    world.send_and_expect("A", "AT+CCFC=0,2", &["+CCFC: 1,1,\"+15551234\",145", "OK"]);
}

#[test]
fn test_call_forwarding_cfnr_time() {
    let mut world = World::new();
    world.given_modem("A");

    world.send_and_expect("A", "AT+CCFC=2,2", &["+CCFC: 0,1,\"\",129,,,20", "OK"]);
    world.send_and_expect_ok("A", "AT+CCFC=2,3,\"12345\",129,1,,,20");
    world.send_and_expect("A", "AT+CCFC=2,2", &["+CCFC: 1,1,\"12345\",129,,,20", "OK"]);
}

#[test]
fn test_call_forward_utility_untouched_query() {
    let mut world = World::new();
    world.given_modem("A");

    world.send_and_expect("A", "AT+CCFCU=0,2", &["+CCFCU: 0,1,2,129,\"\"", "OK"]);
    world.send_and_expect("A", "AT+CCFCU=2,2", &["+CCFCU: 0,1,2,129,\"\",,,20", "OK"]);
}

#[test]
fn test_call_forward_utility_class_echo() {
    let mut world = World::new();
    world.given_modem("A");

    world.send_and_expect("A", "AT+CCFCU=0,2,2,2,\"\",2", &["+CCFCU: 0,2,2,129,\"\"", "OK"]);
}

#[test]
fn test_call_forward_utility_erasure_all_conditional() {
    let mut world = World::new();
    world.given_modem("A");

    world.send_and_expect_ok("A", &format!("AT+CCFCU=5,3,2,1,\"{CALL_PEER_ALT}\",1,\"\",\"\",,15"));
    world.send_and_expect_ok("A", "AT+CCFCU=5,4");
    world.send_and_expect(
        "A",
        "AT+CCFCU=5,2",
        &["+CCFCU: 0,1,2,129,\"\"", "+CCFCU: 0,1,2,129,\"\",,,20", "+CCFCU: 0,1,2,129,\"\"", "OK"],
    );
}

#[test]
fn test_call_forward_utility_ims_tel_uri_unknown_toa() {
    let mut world = World::new();
    world.given_modem("A");

    let uri = "tel:1234;phone-context=ims.mnc001.mcc001.3gppnetwork.org";
    world.send_and_expect_ok("A", &format!("AT+CCFCU=0,3,1,0,\"{uri}\",1"));
    world.send_and_expect("A", "AT+CCFCU=0,2", &[&format!("+CCFCU: 1,1,1,0,\"{uri}\""), "OK"]);
}

#[test]
fn test_call_forwarding_empty_number_registration_keeps_address() {
    let mut world = World::new();
    world.given_modem("A");

    world.send_and_expect_ok("A", "AT+CCFC=0,3,\"+15551234\",145");
    world.send_and_expect_ok("A", "AT+CCFC=0,3");
    world.send_and_expect("A", "AT+CCFC=0,2", &["+CCFC: 1,1,\"+15551234\",145", "OK"]);
}

#[test]
fn test_call_forward_utility_empty_number_registration_keeps_address() {
    let mut world = World::new();
    world.given_modem("A");

    let uri = "tel:1234;phone-context=ims.mnc001.mcc001.3gppnetwork.org";
    world.send_and_expect_ok("A", &format!("AT+CCFCU=0,3,1,0,\"{uri}\",1"));
    world.send_and_expect_ok("A", "AT+CCFCU=0,3");
    world.send_and_expect("A", "AT+CCFCU=0,2", &[&format!("+CCFCU: 1,1,1,0,\"{uri}\""), "OK"]);
}

#[test]
fn test_disable_facility_lock() {
    let mut world = World::new();
    given_modem(&mut world, "A");

    // Enable lock first
    when_at_command_sent(&mut world, "A", &format!("AT+CLCK=\"SC\",1,\"{TEST_PIN}\""));
    then_response_is(&mut world, "A", "OK");

    // Disable lock with correct password
    when_at_command_sent(&mut world, "A", &format!("AT+CLCK=\"SC\",0,\"{TEST_PIN}\""));
    then_response_is(&mut world, "A", "OK");
}

#[test]
fn test_facility_lock_errors() {
    let mut world = World::new();
    given_modem(&mut world, "A");

    // Enable CMEE
    when_at_command_sent(&mut world, "A", "AT+CMEE=1");
    then_response_is(&mut world, "A", "OK");

    // 1. Disable lock with wrong password -> expect CME ERROR 16
    when_at_command_sent(&mut world, "A", r#"AT+CLCK="SC",0,"wrong""#);
    then_response_is(&mut world, "A", CME_ERROR_INCORRECT_PASSWORD);

    // 2. Disable lock with missing password -> expect CME ERROR 16
    when_at_command_sent(&mut world, "A", r#"AT+CLCK="SC",0"#);
    then_response_is(&mut world, "A", CME_ERROR_INCORRECT_PASSWORD);
}

#[test]
fn test_facility_lock_puk_required() {
    let mut world = World::new();
    given_modem_with_locked_sim(&mut world, "A");

    when_at_command_sent(&mut world, "A", "AT+CMEE=1");
    then_response_is(&mut world, "A", "OK");

    // Locked SIM starts in PinRequired.
    // Enter wrong PIN 3 times to trigger PukRequired (correct is "1111")
    when_at_command_sent(&mut world, "A", &format!("AT+CPIN=\"{INVALID_PIN}\""));
    then_response_is(&mut world, "A", CME_ERROR_INCORRECT_PASSWORD);
    when_at_command_sent(&mut world, "A", &format!("AT+CPIN=\"{INVALID_PIN}\""));
    then_response_is(&mut world, "A", CME_ERROR_INCORRECT_PASSWORD);
    when_at_command_sent(&mut world, "A", &format!("AT+CPIN=\"{INVALID_PIN}\""));
    then_response_is(&mut world, "A", CME_ERROR_INCORRECT_PASSWORD);

    // Verify it is PukRequired
    when_at_command_sent(&mut world, "A", "AT+CPIN?");
    then_response_is(&mut world, "A", "+CPIN: SIM PUK");
    then_response_is(&mut world, "A", "OK");

    // Try to disable lock -> expect CME ERROR 12 (SIM PUK required)
    when_at_command_sent(&mut world, "A", &format!("AT+CLCK=\"SC\",0,\"{LOCKED_PIN}\""));
    then_response_is(&mut world, "A", CME_ERROR_SIM_PUK_REQUIRED);
}

#[test]
fn test_ccwa_lifecycle() {
    let mut world = World::new();
    given_modem(&mut world, "A");

    // 1. Query CCWA with default class, should be disabled (status 0, class 7)
    when_at_command_sent(&mut world, "A", "AT+CCWA=1,2");
    then_response_is(&mut world, "A", "+CCWA: 0,7");
    then_response_is(&mut world, "A", "OK");

    // 2. Query CCWA with specific class (returns status 0, class 1 for voice)
    when_at_command_sent(&mut world, "A", "AT+CCWA=1,2,1");
    then_response_is(&mut world, "A", "+CCWA: 0,1");
    then_response_is(&mut world, "A", "OK");

    // 3. Enable CCWA for voice (class 1)
    when_at_command_sent(&mut world, "A", "AT+CCWA=1,1,1");
    then_response_is(&mut world, "A", "OK");

    // 4. Query CCWA again, should be enabled (status 1, class 1)
    when_at_command_sent(&mut world, "A", "AT+CCWA=1,2,1");
    then_response_is(&mut world, "A", "+CCWA: 1,1");
    then_response_is(&mut world, "A", "OK");

    // 4a. Query CCWA with default class (7), should return only active classes
    // (status 1, class 1 for voice)
    when_at_command_sent(&mut world, "A", "AT+CCWA=1,2");
    then_response_is(&mut world, "A", "+CCWA: 1,1");
    then_response_is(&mut world, "A", "OK");

    // 4b. Query CCWA for data (class 2), should be disabled (status 0, class 2)
    when_at_command_sent(&mut world, "A", "AT+CCWA=1,2,2");
    then_response_is(&mut world, "A", "+CCWA: 0,2");
    then_response_is(&mut world, "A", "OK");

    // 4c. Enable CCWA for Voice+Data+Fax (class 7)
    when_at_command_sent(&mut world, "A", "AT+CCWA=1,1,7");
    then_response_is(&mut world, "A", "OK");

    // 4d. Query CCWA for Voice (class 1), should be enabled (status 1, class 1)
    when_at_command_sent(&mut world, "A", "AT+CCWA=1,2,1");
    then_response_is(&mut world, "A", "+CCWA: 1,1");
    then_response_is(&mut world, "A", "OK");

    // 4e. Query CCWA for Data (class 2), should be enabled (status 1, class 2)
    when_at_command_sent(&mut world, "A", "AT+CCWA=1,2,2");
    then_response_is(&mut world, "A", "+CCWA: 1,2");
    then_response_is(&mut world, "A", "OK");

    // 4f. Query CCWA with default class (7), should return all active classes
    // (status 1, class 1, 2, 4)
    when_at_command_sent(&mut world, "A", "AT+CCWA=1,2");
    then_response_is(&mut world, "A", "+CCWA: 1,1");
    then_response_is(&mut world, "A", "+CCWA: 1,2");
    then_response_is(&mut world, "A", "+CCWA: 1,4");
    then_response_is(&mut world, "A", "OK");

    // 5. Disable CCWA for all (class 7)
    when_at_command_sent(&mut world, "A", "AT+CCWA=1,0,7");
    then_response_is(&mut world, "A", "OK");

    // 6. Query CCWA again for voice, should be disabled (status 0, class 1)
    when_at_command_sent(&mut world, "A", "AT+CCWA=1,2,1");
    then_response_is(&mut world, "A", "+CCWA: 0,1");
    then_response_is(&mut world, "A", "OK");

    // 6b. Query CCWA again for data, should be disabled (status 0, class 2)
    when_at_command_sent(&mut world, "A", "AT+CCWA=1,2,2");
    then_response_is(&mut world, "A", "+CCWA: 0,2");
    then_response_is(&mut world, "A", "OK");
}

#[test]
fn test_set_clir() {
    let mut world = World::new();
    given_modem(&mut world, "A");

    // Set CLIR to 1 (presentation restricted)
    when_at_command_sent(&mut world, "A", "AT+CLIR=1");
    then_response_is(&mut world, "A", "OK");

    // Query CLIR, should be 1,1 (since it is provisioned)
    when_at_command_sent(&mut world, "A", "AT+CLIR?");
    then_response_is(&mut world, "A", "+CLIR: 1,1");
    then_response_is(&mut world, "A", "OK");
}

#[test]
fn test_facility_lock_unquoted_password_rejected() {
    let mut world = World::new();
    given_modem(&mut world, "A");

    when_at_command_sent(&mut world, "A", "AT+CMEE=1");
    then_response_is(&mut world, "A", "OK");

    // Unquoted password must be rejected as a syntax error
    when_at_command_sent(&mut world, "A", "AT+CLCK=\"SC\",1,1");
    then_response_is(&mut world, "A", "+CME ERROR: 50");

    // Explicitly empty password slot is accepted by parser (fails in execution)
    when_at_command_sent(&mut world, "A", "AT+CLCK=\"SC\",1,,1");
    then_response_is(&mut world, "A", "+CME ERROR: 16");

    // Valid quoted password succeeds
    when_at_command_sent(&mut world, "A", &format!("AT+CLCK=\"SC\",1,\"{TEST_PIN}\",1"));
    then_response_is(&mut world, "A", "OK");
}

#[test]
fn test_call_forwarding_time_range_validation() {
    let mut world = World::new();
    world.given_modem("A");

    world.send_and_expect_ok("A", "AT+CMEE=1");
    world.send_and_expect_error("A", "AT+CCFC=2,3,\"1234\",129,1,,,0", "+CME ERROR: 50");
    world.send_and_expect_error("A", "AT+CCFC=2,3,\"1234\",129,1,,,31", "+CME ERROR: 50");
    world.send_and_expect_error("A", "AT+CCFC=2,3,\"1234\",129,1,,,300", "+CME ERROR: 50");

    world.send_and_expect_ok("A", "AT+CCFC=2,3,\"1234\",129,1,,,1");
    world.send_and_expect("A", "AT+CCFC=2,2", &["+CCFC: 1,1,\"1234\",129,,,1", "OK"]);

    world.send_and_expect_ok("A", "AT+CCFC=2,3,\"1234\",129,1,,,30");
    world.send_and_expect("A", "AT+CCFC=2,2", &["+CCFC: 1,1,\"1234\",129,,,30", "OK"]);
}

#[test]
fn test_call_forwarding_registration_without_number_requires_prior_registration() {
    let mut world = World::new();
    world.given_modem("A");

    world.send_and_expect_ok("A", "AT+CMEE=1");
    world.send_and_expect_error("A", "AT+CCFC=0,3", "+CME ERROR: 50");
    world.send_and_expect_ok("A", "AT+CCFC=0,3,\"12345\"");
    world.send_and_expect_ok("A", "AT+CCFC=0,3");
    world.send_and_expect("A", "AT+CCFC=0,2", &["+CCFC: 1,1,\"12345\",129", "OK"]);
}

#[test]
fn test_call_forward_utility_fanout_registration_without_number_atomic() {
    let mut world = World::new();
    world.given_modem("A");

    world.send_and_expect_ok("A", "AT+CMEE=1");
    world.send_and_expect_ok("A", "AT+CCFCU=0,3,2,1,\"12345\"");
    world.send_and_expect_ok("A", "AT+CCFCU=0,0");
    world.send_and_expect_ok("A", "AT+CCFCU=2,3,2,1,\"67890\",1,\"\",\"\",,20");
    world.send_and_expect_ok("A", "AT+CCFCU=2,0");
    world.send_and_expect_error("A", "AT+CCFCU=4,3", "+CME ERROR: 50");
    world.send_and_expect("A", "AT+CCFCU=0,2", &["+CCFCU: 0,1,2,1,\"12345\"", "OK"]);
    world.send_and_expect("A", "AT+CCFCU=2,2", &["+CCFCU: 0,1,2,1,\"67890\",,,20", "OK"]);
    world.send_and_expect("A", "AT+CCFCU=1,2", &["+CCFCU: 0,1,2,129,\"\"", "OK"]);
}

#[test]
fn test_call_forward_utility_uri_ton_conversion_to_ccfc() {
    let mut world = World::new();
    world.given_modem("A");

    world.send_and_expect_ok("A", "AT+CMEE=1");
    world.send_and_expect_ok("A", "AT+CCFCU=0,3,1,0,\"tel:+123456789\",1");
    world.send_and_expect("A", "AT+CCFC=0,2", &["+CCFC: 1,1,\"tel:+123456789\",129", "OK"]);
    world.send_and_expect("A", "AT+CCFCU=0,2", &["+CCFCU: 1,1,1,0,\"tel:+123456789\"", "OK"]);
}

#[test]
fn test_call_forward_utility_ton_echo() {
    let mut world = World::new();
    world.given_modem("A");

    world.send_and_expect_ok("A", "AT+CMEE=1");
    world.send_and_expect_ok("A", "AT+CCFCU=0,3,2,1,\"12345\"");
    world.send_and_expect("A", "AT+CCFCU=0,2", &["+CCFCU: 1,1,2,1,\"12345\"", "OK"]);
    world.send_and_expect("A", "AT+CCFC=0,2", &["+CCFC: 1,1,\"12345\",145", "OK"]);

    world.send_and_expect_error("A", "AT+CCFCU=0,3,2,145,\"+12345\"", "+CME ERROR: 50");

    let mut goldfish_world = World::new();
    goldfish_world.given_goldfish_37_modem("A");
    goldfish_world.send_and_expect_ok("A", "AT+CMEE=1");
    goldfish_world.send_and_expect_ok("A", "AT+CCFCU=0,3,2,145,\"+12345\"");
    goldfish_world.send_and_expect("A", "AT+CCFCU=0,2", &["+CCFCU: 1,1,2,145,\"+12345\"", "OK"]);
}

#[test]
fn test_call_forward_enable_disable_updates_number_when_supplied() {
    let mut world = World::new();
    world.given_modem("A");

    world.send_and_expect_ok("A", "AT+CMEE=1");
    world.send_and_expect_ok("A", "AT+CCFCU=0,3,2,1,\"11111\"");
    world.send_and_expect_ok("A", "AT+CCFCU=0,1,2,2,\"22222\"");
    world.send_and_expect("A", "AT+CCFCU=0,2", &["+CCFCU: 1,1,2,2,\"22222\"", "OK"]);

    world.send_and_expect_ok("A", "AT+CCFCU=0,0,2,1,\"33333\"");
    world.send_and_expect("A", "AT+CCFCU=0,2", &["+CCFCU: 0,1,2,1,\"33333\"", "OK"]);

    world.send_and_expect_ok("A", "AT+CCFCU=0,1");
    world.send_and_expect("A", "AT+CCFCU=0,2", &["+CCFCU: 1,1,2,1,\"33333\"", "OK"]);
}

#[test]
fn test_call_forwarding_trailing_garbage_rejected() {
    let mut world = World::new();
    world.given_modem("A");

    world.send_and_expect_ok("A", "AT+CMEE=1");
    world.send_and_expect_error("A", "AT+CCFC=0,3,\"+15551234\"JUNK", "+CME ERROR: 50");
    world.send_and_expect("A", "AT+CCFC=0,2", &["+CCFC: 0,1,\"\",129", "OK"]);
}

#[test]
fn test_call_forward_enable_does_not_update_time() {
    let mut world = World::new();
    world.given_modem("A");

    world.send_and_expect_ok("A", "AT+CCFCU=2,3,2,1,\"12345\",1,\"\",\"\",,20");
    world.send_and_expect_ok("A", "AT+CCFCU=2,1,2,1,\"12345\",1,\"\",\"\",,30");
    world.send_and_expect("A", "AT+CCFCU=2,2", &["+CCFCU: 1,1,2,1,\"12345\",,,20", "OK"]);

    world.send_and_expect_ok("A", "AT+CCFC=2,3,\"67890\",129,1,,,20");
    world.send_and_expect_ok("A", "AT+CCFC=2,1,\"67890\",129,1,,,30");
    world.send_and_expect("A", "AT+CCFC=2,2", &["+CCFC: 1,1,\"67890\",129,,,20", "OK"]);
}

#[test]
fn test_call_forward_enable_empty_number_without_registration_fails() {
    let mut world = World::new();
    world.given_modem("A");

    world.send_and_expect_ok("A", "AT+CMEE=1");
    world.send_and_expect_error("A", "AT+CCFC=0,1", "+CME ERROR: 50");
    world.send_and_expect_ok("A", "AT+CCFC=0,3,\"12345\"");
    world.send_and_expect_ok("A", "AT+CCFC=0,0");
    world.send_and_expect_ok("A", "AT+CCFC=0,1");
    world.send_and_expect("A", "AT+CCFC=0,2", &["+CCFC: 1,1,\"12345\",129", "OK"]);
}

#[test]
fn test_call_forward_utility_enable_empty_number_without_registration_fails() {
    let mut world = World::new();
    world.given_modem("A");

    world.send_and_expect_ok("A", "AT+CMEE=1");
    world.send_and_expect_error("A", "AT+CCFCU=0,1", "+CME ERROR: 50");
    world.send_and_expect_ok("A", "AT+CCFCU=0,3,2,1,\"12345\"");
    world.send_and_expect_ok("A", "AT+CCFCU=0,0");
    world.send_and_expect_ok("A", "AT+CCFCU=0,1");
    world.send_and_expect("A", "AT+CCFCU=0,2", &["+CCFCU: 1,1,2,1,\"12345\"", "OK"]);
}

#[test]
fn test_call_forward_utility_enable_with_inline_number_succeeds() {
    let mut world = World::new();
    world.given_modem("A");

    world.send_and_expect_ok("A", "AT+CCFCU=0,1,2,1,\"12345\"");
    world.send_and_expect("A", "AT+CCFCU=0,2", &["+CCFCU: 1,1,2,1,\"12345\"", "OK"]);
}

#[test]
fn test_call_forward_utility_erasure_resets_timer_to_default_20() {
    let mut world = World::new();
    world.given_modem("A");

    world.send_and_expect_ok("A", "AT+CCFCU=2,3,2,1,\"12345\",1,\"\",\"\",,15");
    world.send_and_expect("A", "AT+CCFCU=2,2", &["+CCFCU: 1,1,2,1,\"12345\",,,15", "OK"]);
    world.send_and_expect_ok("A", "AT+CCFCU=2,4");
    world.send_and_expect("A", "AT+CCFCU=2,2", &["+CCFCU: 0,1,2,129,\"\",,,20", "OK"]);
}

#[test]
fn test_call_forward_utility_novalidinfo_with_number_rejected() {
    let mut world = World::new();
    world.given_modem("A");

    world.send_and_expect_ok("A", "AT+CMEE=1");
    world.send_and_expect_error("A", "AT+CCFCU=0,3,0,1,\"12345\"", "+CME ERROR: 50");
}

#[test]
fn test_call_forward_utility_novalidinfo_without_number_allowed() {
    let mut world = World::new();
    world.given_modem("A");

    world.send_and_expect_ok("A", "AT+CMEE=1");
    world.send_and_expect_ok("A", "AT+CCFCU=0,3,2,1,\"12345\"");
    world.send_and_expect_ok("A", "AT+CCFCU=0,3,0,1,\"\"");
    world.send_and_expect("A", "AT+CCFCU=0,2", &["+CCFCU: 1,1,2,1,\"12345\"", "OK"]);
}

#[test]
fn test_call_forward_utility_uri_with_nonzero_ton_rejected() {
    let mut world = World::new();
    world.given_modem("A");

    world.send_and_expect_ok("A", "AT+CMEE=1");
    world.send_and_expect_error(
        "A",
        "AT+CCFCU=0,3,1,145,\"sip:alice@example.com\"",
        "+CME ERROR: 50",
    );
}

#[test]
fn test_call_forward_utility_uri_with_zero_ton_accepted() {
    let mut world = World::new();
    world.given_modem("A");

    world.send_and_expect_ok("A", "AT+CCFCU=0,3,1,0,\"sip:alice@example.com\"");
    world.send_and_expect(
        "A",
        "AT+CCFCU=0,2",
        &["+CCFCU: 1,1,1,0,\"sip:alice@example.com\"", "OK"],
    );
}

#[test]
fn test_call_forward_utility_uri_with_default_octet_129_accepted() {
    let mut world = World::new();
    world.given_goldfish_37_modem("A");

    world.send_and_expect_ok("A", "AT+CCFCU=0,3,1,129,\"sip:bob@example.com\"");
    world.send_and_expect("A", "AT+CCFCU=0,2", &["+CCFCU: 1,1,1,0,\"sip:bob@example.com\"", "OK"]);
}

#[test]
fn test_call_forward_utility_type_toggle_preserves_digits() {
    let mut world = World::new();
    world.given_modem("A");

    world.send_and_expect_ok("A", "AT+CCFCU=0,3,2,1,\"12345\"");
    world.send_and_expect("A", "AT+CCFCU=0,2", &["+CCFCU: 1,1,2,1,\"12345\"", "OK"]);

    world.send_and_expect_ok("A", "AT+CCFCU=0,3,1,0,\"\"");
    world.send_and_expect("A", "AT+CCFCU=0,2", &["+CCFCU: 1,1,1,0,\"12345\"", "OK"]);

    world.send_and_expect_ok("A", "AT+CCFCU=0,3,2,1,\"\"");
    world.send_and_expect("A", "AT+CCFCU=0,2", &["+CCFCU: 1,1,2,1,\"12345\"", "OK"]);
}

#[test]
fn test_call_forward_utility_compound_enable_atomicity() {
    let mut world = World::new();
    world.given_modem("A");

    world.send_and_expect_ok("A", "AT+CMEE=1");
    world.send_and_expect_ok("A", "AT+CCFCU=0,3,2,1,\"12345\"");
    world.send_and_expect_ok("A", "AT+CCFCU=0,0");
    world.send_and_expect("A", "AT+CCFCU=0,2", &["+CCFCU: 0,1,2,1,\"12345\"", "OK"]);

    world.send_and_expect_error("A", "AT+CCFCU=4,1", "+CME ERROR: 50");
    world.send_and_expect("A", "AT+CCFCU=0,2", &["+CCFCU: 0,1,2,1,\"12345\"", "OK"]);
}

#[test]
fn test_call_forward_utility_goldfish_enable_wire_format() {
    let mut world = World::new();
    world.given_goldfish_37_modem("A");

    world.send_and_expect_ok("A", "AT+CCFCU=0,3,2,129,\"12345\",1,\"\"");
    world.send_and_expect_ok("A", "AT+CCFCU=0,1,2,129,\"12345\",1,\"\"");
    world.send_and_expect("A", "AT+CCFCU=0,2,2,129,\"\",1", &["+CCFCU: 1,1,2,129,\"12345\"", "OK"]);
}

#[test]
fn test_call_forward_utility_goldfish_registration_with_time_wire_format() {
    let mut world = World::new();
    world.given_goldfish_37_modem("A");

    world.send_and_expect_ok("A", "AT+CCFCU=2,3,2,129,\"12345\",1,\"\",\"\",,25");
    world.send_and_expect(
        "A",
        "AT+CCFCU=2,2,2,129,\"\",1",
        &["+CCFCU: 1,1,2,129,\"12345\",,,25", "OK"],
    );
}

#[test]
fn test_call_forward_utility_goldfish_international_toa_wire_format() {
    let mut world = World::new();
    world.given_goldfish_37_modem("A");

    world.send_and_expect_ok("A", "AT+CCFCU=0,3,2,145,\"+12345\",1,\"\"");
    world.send_and_expect(
        "A",
        "AT+CCFCU=0,2,2,145,\"\",1",
        &["+CCFCU: 1,1,2,145,\"+12345\"", "OK"],
    );
}

#[test]
fn test_call_forward_utility_novalidinfo_does_not_relabel_uri() {
    let mut world = World::new();
    world.given_goldfish_37_modem("A");

    world.send_and_expect_ok("A", "AT+CCFCU=0,3,1,0,\"sip:alice@example.com\"");
    world.send_and_expect_ok("A", "AT+CCFCU=0,3,0,129,\"\"");
    world.send_and_expect(
        "A",
        "AT+CCFCU=0,2",
        &["+CCFCU: 1,1,1,0,\"sip:alice@example.com\"", "OK"],
    );
}
