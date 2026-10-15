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

use crate::{common::constants::*, world::World};

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
    world.given_modem_with_locked_sim("A");

    // First failed attempt
    world.send_and_expect_error("A", &format!("AT+CPIN=\"{INVALID_PIN}\""), "ERROR");

    // Second failed attempt
    world.send_and_expect_error("A", &format!("AT+CPIN=\"{INVALID_PIN}\""), "ERROR");

    // Query retries
    world.send_and_expect(
        "A",
        "AT+CPINR=\"SIM PIN\"",
        &[&format!("+CPINR: \"SIM PIN\",{},{DEFAULT_PIN_RETRIES}", DEFAULT_PIN_RETRIES - 2), "OK"],
    );
}

#[test]
fn test_pin_validation_errors() {
    let mut world = World::new();
    world.given_modem_with_locked_sim("A");

    // Enable verbose errors
    world.send_and_expect_ok("A", "AT+CMEE=1");

    // 1. Invalid PIN length (too short) -> expect CME ERROR 16 (Incorrect Password)
    world.send_and_expect_error(
        "A",
        &format!("AT+CPIN=\"{TOO_SHORT_PIN}\""),
        CME_ERROR_INCORRECT_PASSWORD,
    );

    // 2. Invalid PIN length (too long) -> expect CME ERROR 16
    world.send_and_expect_error(
        "A",
        &format!("AT+CPIN=\"{TOO_LONG_PIN}\""),
        CME_ERROR_INCORRECT_PASSWORD,
    );

    // 3. Non-ASCII PIN (multi-byte UTF-8) -> expect CME ERROR 16
    world.send_and_expect_error("A", "AT+CPIN=\"\u{00E9}\u{00E9}\"", CME_ERROR_INCORRECT_PASSWORD);

    // Transition to PukRequired to test PUK validation
    world.send_and_expect_error(
        "A",
        &format!("AT+CPIN=\"{INVALID_PIN}\""),
        CME_ERROR_INCORRECT_PASSWORD,
    );
    world.send_and_expect_error(
        "A",
        &format!("AT+CPIN=\"{INVALID_PIN}\""),
        CME_ERROR_INCORRECT_PASSWORD,
    );
    world.send_and_expect_error(
        "A",
        &format!("AT+CPIN=\"{INVALID_PIN}\""),
        CME_ERROR_INCORRECT_PASSWORD,
    );

    // Verify state is PUK required
    world.send_and_expect("A", "AT+CPIN?", &["+CPIN: SIM PUK", "OK"]);

    // 3. Invalid PUK length (too short) -> expect CME ERROR 16
    world.send_and_expect_error(
        "A",
        &format!("AT+CPIN=\"{TOO_SHORT_PUK}\",\"{LOCKED_PIN}\""),
        CME_ERROR_INCORRECT_PASSWORD,
    );

    // 4. Invalid new PIN length (too short) -> expect CME ERROR 16
    world.send_and_expect_error(
        "A",
        &format!("AT+CPIN=\"{TEST_PUK}\",\"{TOO_SHORT_PIN}\""),
        CME_ERROR_INCORRECT_PASSWORD,
    );

    // 5. Invalid new PIN length (too long) -> expect CME ERROR 16
    world.send_and_expect_error(
        "A",
        &format!("AT+CPIN=\"{TEST_PUK}\",\"{TOO_LONG_PIN}\""),
        CME_ERROR_INCORRECT_PASSWORD,
    );

    // 6. Missing new PIN -> expect CME ERROR 50 (Incorrect Parameters)
    world.send_and_expect_error(
        "A",
        &format!("AT+CPIN=\"{TEST_PUK}\""),
        CME_ERROR_INCORRECT_PARAMETERS,
    );
}

#[test]
fn test_query_sim_pin_facility_lock() {
    let mut world = World::new();
    world.given_modem("A"); // Default profile has PIN disabled

    // Query SC lock status -> should return +CLCK: 0 (disabled)
    world.send_and_expect("A", r#"AT+CLCK="SC",2"#, &["+CLCK: 0", "OK"]);

    // Create a modem with locked SIM (PIN enabled)
    world.given_modem_with_locked_sim("B");

    // Query SC lock status -> should return +CLCK: 1 (enabled)
    world.send_and_expect("B", r#"AT+CLCK="SC",2"#, &["+CLCK: 1", "OK"]);
}

#[test]
fn test_perm_blocked_sim_profile() {
    let mut world = World::new();
    world.given_modem_with_perm_blocked_sim("A");

    // Enable verbose errors
    world.send_and_expect_ok("A", "AT+CMEE=1");

    // Query SIM status -> should return CME ERROR 13 (SimFailure)
    world.send_and_expect_error("A", "AT+CPIN?", CME_ERROR_SIM_FAILURE);

    // Attempting PIN entry should return CME ERROR 3 (OperationNotAllowed)
    world.send_and_expect_error(
        "A",
        &format!("AT+CPIN=\"{LOCKED_PIN}\""),
        CME_ERROR_OPERATION_NOT_ALLOWED,
    );

    // Attempting PUK entry should return CME ERROR 3 (OperationNotAllowed)
    world.send_and_expect_error(
        "A",
        &format!("AT+CPIN=\"{TEST_PUK}\",\"{NEW_PIN}\""),
        CME_ERROR_OPERATION_NOT_ALLOWED,
    );

    // Attempting password change should return CME ERROR 3
    world.send_and_expect_error(
        "A",
        &format!("AT+CPWD=\"SC\",\"{LOCKED_PIN}\",\"{NEW_PIN}\""),
        CME_ERROR_OPERATION_NOT_ALLOWED,
    );
}

#[test]
fn test_puk_retries_exhaustion_transitions_to_perm_blocked() {
    let mut world = World::new();
    world.given_modem_with_locked_sim("A");

    world.send_and_expect_ok("A", "AT+CMEE=1");

    // Exhaust PIN retries (3 attempts)
    for _ in 0..DEFAULT_PIN_RETRIES {
        world.send_and_expect_error(
            "A",
            &format!("AT+CPIN=\"{INVALID_PIN}\""),
            CME_ERROR_INCORRECT_PASSWORD,
        );
    }

    // Verify state transitioned to SIM PUK
    world.send_and_expect("A", "AT+CPIN?", &["+CPIN: SIM PUK", "OK"]);

    // Exhaust PUK retries (10 attempts)
    for _ in 0..DEFAULT_PUK_RETRIES {
        world.send_and_expect_error(
            "A",
            &format!("AT+CPIN=\"{INVALID_PUK}\",\"{NEW_PIN}\""),
            CME_ERROR_INCORRECT_PASSWORD,
        );
    }

    // After exhausting PUK retries, SIM is PermBlocked:
    // AT+CPIN? returns CME ERROR 13.
    world.send_and_expect_error("A", "AT+CPIN?", CME_ERROR_SIM_FAILURE);

    // Any further unlock attempts must fail with CME ERROR 3
    world.send_and_expect_error(
        "A",
        &format!("AT+CPIN=\"{TEST_PUK}\",\"{NEW_PIN}\""),
        CME_ERROR_OPERATION_NOT_ALLOWED,
    );
}

#[test]
fn test_sim_registration_emergency_camping_matrix() {
    let mut world = World::new();

    // Absent SIM
    world.given_modem("absent");
    crate::steps::when_sim_status_set(&mut world, "absent", false);
    world.then_response_contains("absent", "+CPIN: ABSENT");
    world.send_and_expect_ok("absent", "AT+CREG=1");
    world.send_and_expect_ok("absent", "AT+CGREG=1");
    world.send_and_expect_ok("absent", "AT+CFUN=1");
    world.when_time_advances_ms(50);
    world.send_and_expect("absent", "AT+CREG?", &["+CREG: 1,0", "OK"]);
    world.send_and_expect("absent", "AT+CGREG?", &["+CGREG: 1,0", "OK"]);

    // PIN-locked SIM
    world.given_modem_with_locked_sim("pin_locked");
    world.send_and_expect_ok("pin_locked", "AT+CREG=1");
    world.send_and_expect_ok("pin_locked", "AT+CGREG=1");
    world.send_and_expect_ok("pin_locked", "AT+CFUN=1");
    world.when_time_advances_ms(15);
    world.then_response_is("pin_locked", "+CREG: 8");
    world.then_response_is("pin_locked", "+CGREG: 0");
    world.then_response_contains("pin_locked", "+CSQ:");
    world.send_and_expect("pin_locked", "AT+CREG?", &["+CREG: 1,8", "OK"]);
    world.send_and_expect("pin_locked", "AT+CGREG?", &["+CGREG: 1,0", "OK"]);

    // PUK-locked SIM
    world.given_modem_with_locked_sim("puk_locked");
    world.send_and_expect_ok("puk_locked", "AT+CREG=1");
    world.send_and_expect_ok("puk_locked", "AT+CGREG=1");
    world.send_and_expect_ok("puk_locked", "AT+CFUN=1");
    world.when_time_advances_ms(15);
    world.then_response_is("puk_locked", "+CREG: 8");
    world.then_response_is("puk_locked", "+CGREG: 0");
    world.then_response_contains("puk_locked", "+CSQ:");
    // Exhaust PIN retries to transition to PukRequired
    for _ in 0..DEFAULT_PIN_RETRIES {
        world.send_and_expect_error("puk_locked", &format!("AT+CPIN=\"{INVALID_PIN}\""), "ERROR");
    }
    world.send_and_expect("puk_locked", "AT+CPIN?", &["+CPIN: SIM PUK", "OK"]);
    world.send_and_expect("puk_locked", "AT+CREG?", &["+CREG: 1,8", "OK"]);
    world.send_and_expect("puk_locked", "AT+CGREG?", &["+CGREG: 1,0", "OK"]);

    // Ready SIM
    world.given_modem("ready");
    world.send_and_expect_ok("ready", "AT+CREG=1");
    world.send_and_expect_ok("ready", "AT+CGREG=1");
    world.send_and_expect_ok("ready", "AT+CFUN=1");
    world.when_time_advances_ms(15);
    world.then_response_is("ready", "+CREG: 1");
    world.then_response_is("ready", "+CGREG: 1");
    world.then_response_contains("ready", "+CSQ:");
    world.send_and_expect("ready", "AT+CREG?", &["+CREG: 1,1", "OK"]);
    world.send_and_expect("ready", "AT+CGREG?", &["+CGREG: 1,1", "OK"]);
}

#[test]
fn test_puk_unlock_triggers_reattach() {
    let mut world = World::new();
    world.given_modem_with_locked_sim("modem1");

    world.send_and_expect_ok("modem1", "AT+CREG=1");
    world.send_and_expect_ok("modem1", "AT+CGREG=1");
    world.send_and_expect_ok("modem1", "AT+CFUN=1");

    world.when_time_advances_ms(15);
    world.then_response_is("modem1", "+CREG: 8");
    world.then_response_is("modem1", "+CGREG: 0");
    world.then_response_contains("modem1", "+CSQ:");
    world.send_and_expect("modem1", "AT+CREG?", &["+CREG: 1,8", "OK"]);
    world.send_and_expect("modem1", "AT+CGREG?", &["+CGREG: 1,0", "OK"]);

    // Exhaust PIN retries to transition to PUK required
    for _ in 0..DEFAULT_PIN_RETRIES {
        world.send_and_expect_error("modem1", &format!("AT+CPIN=\"{INVALID_PIN}\""), "ERROR");
    }
    world.send_and_expect("modem1", "AT+CPIN?", &["+CPIN: SIM PUK", "OK"]);

    world.send_and_expect_ok("modem1", &format!("AT+CPIN=\"{TEST_PUK}\",\"{NEW_PIN}\""));

    world.when_time_advances_ms(15);
    world.then_response_is("modem1", "+CREG: 1");
    world.then_response_is("modem1", "+CGREG: 1");
    world.then_response_contains("modem1", "+CSQ:");

    world.send_and_expect("modem1", "AT+CREG?", &["+CREG: 1,1", "OK"]);
    world.send_and_expect("modem1", "AT+CGREG?", &["+CGREG: 1,1", "OK"]);
}

#[test]
fn test_clck_unlock_triggers_reattach() {
    let mut world = World::new();
    world.given_modem_with_locked_sim("modem1");

    world.send_and_expect_ok("modem1", "AT+CREG=1");
    world.send_and_expect_ok("modem1", "AT+CGREG=1");
    world.send_and_expect_ok("modem1", "AT+CFUN=1");

    world.when_time_advances_ms(15);
    world.then_response_is("modem1", "+CREG: 8");
    world.then_response_is("modem1", "+CGREG: 0");
    world.then_response_contains("modem1", "+CSQ:");
    world.send_and_expect("modem1", "AT+CREG?", &["+CREG: 1,8", "OK"]);
    world.send_and_expect("modem1", "AT+CGREG?", &["+CGREG: 1,0", "OK"]);

    world.send_and_expect_ok("modem1", &format!("AT+CLCK=\"SC\",0,\"{LOCKED_PIN}\""));

    world.when_time_advances_ms(15);
    world.then_response_is("modem1", "+CREG: 1");
    world.then_response_is("modem1", "+CGREG: 1");
    world.then_response_contains("modem1", "+CSQ:");

    world.send_and_expect("modem1", "AT+CREG?", &["+CREG: 1,1", "OK"]);
    world.send_and_expect("modem1", "AT+CGREG?", &["+CGREG: 1,1", "OK"]);
}
