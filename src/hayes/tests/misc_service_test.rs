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

// Scenario: Set Fixed Local Rate
//   Given a modem "A"
//   When AT command "AT+IPR=9600" is sent to "A"
//   Then response from "A" is "OK"
#[test]
fn test_set_ipr() {
    let mut world = World::new();
    given_modem(&mut world, "A");
    when_at_command_sent(&mut world, "A", "AT+IPR=9600");
    then_response_is(&mut world, "A", "OK");
}

// Scenario: Set and Query Mobile Equipment Error Reporting
//   Given a modem "A"
//   When AT commands for setting and querying +CMEE are sent
//   Then responses match the spec requirements
#[test]
fn test_cmee_queries_and_set() {
    let mut world = World::new();
    given_modem(&mut world, "A");

    // By default, +CMEE is 0 (disable)
    when_at_command_sent(&mut world, "A", "AT+CMEE?");
    then_response_is(&mut world, "A", "+CMEE: 0");
    then_response_is(&mut world, "A", "OK");

    // Test command returns supported range (0-2)
    when_at_command_sent(&mut world, "A", "AT+CMEE=?");
    then_response_is(&mut world, "A", "+CMEE: (0-2)");
    then_response_is(&mut world, "A", "OK");

    // Set to 1 (numeric)
    when_at_command_sent(&mut world, "A", "AT+CMEE=1");
    then_response_is(&mut world, "A", "OK");
    when_at_command_sent(&mut world, "A", "AT+CMEE?");
    then_response_is(&mut world, "A", "+CMEE: 1");
    then_response_is(&mut world, "A", "OK");

    // Set to 2 (verbose)
    when_at_command_sent(&mut world, "A", "AT+CMEE=2");
    then_response_is(&mut world, "A", "OK");
    when_at_command_sent(&mut world, "A", "AT+CMEE?");
    then_response_is(&mut world, "A", "+CMEE: 2");
    then_response_is(&mut world, "A", "OK");

    // Set to invalid value (3) returns CME ERROR (since CMEE is 2) and doesn't
    // change mode
    when_at_command_sent(&mut world, "A", "AT+CMEE=3");
    then_response_is(&mut world, "A", "+CME ERROR: incorrect parameters");
    when_at_command_sent(&mut world, "A", "AT+CMEE?");
    then_response_is(&mut world, "A", "+CMEE: 2");
    then_response_is(&mut world, "A", "OK");
}

// Scenario: Set Echo
//   Given a modem "A"
//   When AT command "ATE1" is sent to "A"
//   Then response from "A" is "OK"
#[test]
fn test_set_echo() {
    let mut world = World::new();
    given_modem(&mut world, "A");
    when_at_command_sent(&mut world, "A", "ATE1");
    then_response_is(&mut world, "A", "OK");
}

// Scenario: Set Speaker Volume
//   Given a modem "A"
//   When AT command "ATL1" is sent to "A"
//   Then response from "A" is "OK"
//   When AT command "AT&V" is sent to "A"
//   Then response from "A" contains "L:1"
#[test]
fn test_set_speaker_volume() {
    let mut world = World::new();
    given_modem(&mut world, "A");

    when_at_command_sent(&mut world, "A", "ATL1");
    then_response_is(&mut world, "A", "OK");

    when_at_command_sent(&mut world, "A", "AT&V");
    then_response_contains(&mut world, "A", "L:1");
    then_response_is(&mut world, "A", "OK");
}

// Scenario: Set Speaker Mute
//   Given a modem "A"
//   When AT command "ATM1" is sent to "A"
//   Then response from "A" is "OK"
//   When AT command "AT&V" is sent to "A"
//   Then response from "A" contains "M:1"
#[test]
fn test_set_speaker_mute() {
    let mut world = World::new();
    given_modem(&mut world, "A");

    when_at_command_sent(&mut world, "A", "ATM1");
    then_response_is(&mut world, "A", "OK");

    when_at_command_sent(&mut world, "A", "AT&V");
    then_response_contains(&mut world, "A", "M:1");
    then_response_is(&mut world, "A", "OK");
}

// Scenario: Set Quiet Mode
//   Given a modem "A"
//   When AT command "ATQ1" is sent to "A"
//   Then response from "A" is "OK"
//   When AT command "AT&V" is sent to "A"
//   Then response from "A" contains "Q:1"
#[test]
fn test_set_quiet_mode() {
    let mut world = World::new();
    given_modem(&mut world, "A");

    when_at_command_sent(&mut world, "A", "ATQ1");
    then_response_is(&mut world, "A", "OK");

    when_at_command_sent(&mut world, "A", "AT&V");
    then_response_contains(&mut world, "A", "Q:1");
    then_response_is(&mut world, "A", "OK");
}

// Scenario: Set Verbose Mode
//   Given a modem "A"
//   When AT command "ATV1" is sent to "A"
//   Then response from "A" is "OK"
//   When AT command "AT&V" is sent to "A"
//   Then response from "A" contains "V:1"
#[test]
fn test_set_verbose_mode() {
    let mut world = World::new();
    given_modem(&mut world, "A");

    when_at_command_sent(&mut world, "A", "ATV1");
    then_response_is(&mut world, "A", "OK");

    when_at_command_sent(&mut world, "A", "AT&V");
    then_response_contains(&mut world, "A", "V:1");
    then_response_is(&mut world, "A", "OK");
}

// Scenario: Reset to Factory Defaults
//   Given a modem "A"
//   When AT command "ATL3" is sent to "A"
//   Then response from "A" is "OK"
//   When AT command "AT&F" is sent to "A"
//   Then response from "A" is "OK"
//   When AT command "AT&V" is sent to "A"
//   Then response from "A" contains "L:1 M:1 Q:0 V:1 ICF:3,3 IFC:2,2"
#[test]
fn test_reset_to_factory_defaults() {
    let mut world = World::new();
    given_modem(&mut world, "A");

    // Change settings first
    when_at_command_sent(&mut world, "A", "ATL3");
    then_response_is(&mut world, "A", "OK");

    when_at_command_sent(&mut world, "A", "AT&F");
    then_response_is(&mut world, "A", "OK");

    // Verify defaults: L:1 M:1 Q:0 V:1 ICF:3,3 IFC:2,2
    when_at_command_sent(&mut world, "A", "AT&V");
    then_response_contains(&mut world, "A", "L:1 M:1 Q:0 V:1 ICF:3,3 IFC:2,2");
    then_response_is(&mut world, "A", "OK");
}

// Scenario: View Active Configuration
//   Given a modem "A"
//   When AT command "AT&V" is sent to "A"
//   Then response from "A" contains "ACTIVE PROFILE:"
#[test]
fn test_view_active_configuration() {
    let mut world = World::new();
    given_modem(&mut world, "A");

    when_at_command_sent(&mut world, "A", "AT&V");
    then_response_contains(&mut world, "A", "ACTIVE PROFILE:");
    then_response_contains(&mut world, "A", "L:1 M:1 Q:0 V:1 ICF:3,3 IFC:2,2");
    then_response_is(&mut world, "A", "OK");
}

// Scenario: Write Active Configuration
//   Given a modem "A"
//   When AT command "AT&W" is sent to "A"
//   Then response from "A" is "OK"
#[test]
fn test_write_active_configuration() {
    let mut world = World::new();
    given_modem(&mut world, "A");
    when_at_command_sent(&mut world, "A", "AT&W");
    then_response_is(&mut world, "A", "OK");
}

// Scenario: Reset Modem
//   Given a modem "A"
//   When AT command "ATZ" is sent to "A"
//   Then response from "A" is "OK"
#[test]
fn test_reset() {
    let mut world = World::new();
    given_modem(&mut world, "A");
    when_at_command_sent(&mut world, "A", "ATZ");
    then_response_is(&mut world, "A", "OK");
}

// Scenario: Get Identification Information
//   Given a modem "A"
//   When AT command "ATI" is sent to "A"
//   Then response from "A" is "OK"
#[test]
fn test_get_identification_information() {
    let mut world = World::new();
    given_modem(&mut world, "A");
    when_at_command_sent(&mut world, "A", "ATI");
    then_response_is(&mut world, "A", "OK");
}

// Scenario: Set Auto Answer
//   Given a modem "A"
//   When AT command "ATS0=1" is sent to "A"
//   Then response from "A" is "OK"
#[test]
fn test_set_auto_answer() {
    let mut world = World::new();
    given_modem(&mut world, "A");
    when_at_command_sent(&mut world, "A", "ATS0=1");
    then_response_is(&mut world, "A", "OK");
}

// Scenario: Set Command Termination Character
//   Given a modem "A"
//   When AT command "ATS3=1" is sent to "A"
//   Then response from "A" is "OK"
#[test]
fn test_set_command_termination_character() {
    let mut world = World::new();
    given_modem(&mut world, "A");
    when_at_command_sent(&mut world, "A", "ATS3=1");
    then_response_is(&mut world, "A", "OK");
}

// Scenario: Set Response Formatting Character
//   Given a modem "A"
//   When AT command "ATS4=1" is sent to "A"
//   Then response from "A" is "OK"
#[test]
fn test_set_response_formatting_character() {
    let mut world = World::new();
    given_modem(&mut world, "A");
    when_at_command_sent(&mut world, "A", "ATS4=1");
    then_response_is(&mut world, "A", "OK");
}

// Scenario: Set Command Line Editing Character
//   Given a modem "A"
//   When AT command "ATS5=1" is sent to "A"
//   Then response from "A" is "OK"
#[test]
fn test_set_command_line_editing_character() {
    let mut world = World::new();
    given_modem(&mut world, "A");
    when_at_command_sent(&mut world, "A", "ATS5=1");
    then_response_is(&mut world, "A", "OK");
}

// Scenario: Set Pause Before Blind Dialing
//   Given a modem "A"
//   When AT command "ATS6=1" is sent to "A"
//   Then response from "A" is "OK"
#[test]
fn test_set_pause_before_blind_dialing() {
    let mut world = World::new();
    given_modem(&mut world, "A");
    when_at_command_sent(&mut world, "A", "ATS6=1");
    then_response_is(&mut world, "A", "OK");
}

// Scenario: Set Connection Completion Timeout
//   Given a modem "A"
//   When AT command "ATS7=1" is sent to "A"
//   Then response from "A" is "OK"
#[test]
fn test_set_connection_completion_timeout() {
    let mut world = World::new();
    given_modem(&mut world, "A");
    when_at_command_sent(&mut world, "A", "ATS7=1");
    then_response_is(&mut world, "A", "OK");
}

// Scenario: Set Comma Dial Modifier Time
//   Given a modem "A"
//   When AT command "ATS8=1" is sent to "A"
//   Then response from "A" is "OK"
#[test]
fn test_set_comma_dial_modifier_time() {
    let mut world = World::new();
    given_modem(&mut world, "A");
    when_at_command_sent(&mut world, "A", "ATS8=1");
    then_response_is(&mut world, "A", "OK");
}

// Scenario: Get Capabilities
//   Given a modem "A"
//   When AT command "AT+GCAP" is sent to "A"
//   Then response from "A" is "+GCAP: +FCLASS,+DS\r\nOK"
#[test]
fn test_get_capabilities() {
    let mut world = World::new();
    given_modem(&mut world, "A");
    when_at_command_sent(&mut world, "A", "AT+GCAP");
    then_response_is(&mut world, "A", "+GCAP: +FCLASS,+DS");
    then_response_is(&mut world, "A", "OK");
}

// Scenario: Set Automatic Disconnect Delay
//   Given a modem "A"
//   When AT command "ATS10=1" is sent to "A"
//   Then response from "A" is "OK"
#[test]
fn test_set_automatic_disconnect_delay() {
    let mut world = World::new();
    given_modem(&mut world, "A");
    when_at_command_sent(&mut world, "A", "ATS10=1");
    then_response_is(&mut world, "A", "OK");
}

// Scenario: Get Manufacturer Identification
//   Given a modem "A"
//   When AT command "AT+GMI" is sent to "A"
//   Then response from "A" is "Android"
//   Then response from "A" is "OK"
#[test]
fn test_get_manufacturer_identification() {
    let mut world = World::new();
    given_modem(&mut world, "A");
    when_at_command_sent(&mut world, "A", "AT+GMI");
    then_response_is(&mut world, "A", "Android");
    then_response_is(&mut world, "A", "OK");
}

// Scenario: Get Model Identification
//   Given a modem "A"
//   When AT command "AT+GMM" is sent to "A"
//   Then response from "A" is "gLinux"
//   Then response from "A" is "OK"
#[test]
fn test_get_model_id() {
    let mut world = World::new();
    given_modem(&mut world, "A");
    when_at_command_sent(&mut world, "A", "AT+GMM");
    then_response_is(&mut world, "A", "gLinux");
    then_response_is(&mut world, "A", "OK");
}

// Scenario: Get Revision
//   Given a modem "A"
//   When AT command "AT+GMR" is sent to "A"
//   Then response from "A" is "1.0"
//   Then response from "A" is "OK"
#[test]
fn test_get_revision() {
    let mut world = World::new();
    given_modem(&mut world, "A");
    when_at_command_sent(&mut world, "A", "AT+GMR");
    then_response_is(&mut world, "A", "1.0");
    then_response_is(&mut world, "A", "OK");
}

// Scenario: Get Serial Number
//   Given a modem "A"
//   When AT command "AT+GSN" is sent to "A"
//   Then response from "A" is "0123456789"
//   Then response from "A" is "OK"
#[test]
fn test_get_serial_number() {
    let mut world = World::new();
    given_modem(&mut world, "A");
    when_at_command_sent(&mut world, "A", "AT+GSN");
    then_response_is(&mut world, "A", "0123456789");
    then_response_is(&mut world, "A", "OK");
}

// Scenario: Set TE-TA Control Character Framing
//   Given a modem "A"
//   When AT command "AT+ICF=3,3" is sent to "A"
//   Then response from "A" is "OK"
//   When AT command "AT&V" is sent to "A"
//   Then response from "A" contains "ICF:3,3"
#[test]
fn test_set_te_ta_control_character_framing() {
    let mut world = World::new();
    given_modem(&mut world, "A");
    when_at_command_sent(&mut world, "A", "AT+ICF=3,3");
    then_response_is(&mut world, "A", "OK");

    when_at_command_sent(&mut world, "A", "AT&V");
    then_response_contains(&mut world, "A", "ICF:3,3");
}

// Scenario: Set TE-TA Local Data Flow Control
//   Given a modem "A"
//   When AT command "AT+IFC=2,2" is sent to "A"
//   Then response from "A" is "OK"
//   When AT command "AT&V" is sent to "A"
//   Then response from "A" contains "IFC:2,2"
#[test]
fn test_set_te_ta_local_data_flow_control() {
    let mut world = World::new();
    given_modem(&mut world, "A");
    when_at_command_sent(&mut world, "A", "AT+IFC=2,2");
    then_response_is(&mut world, "A", "OK");

    when_at_command_sent(&mut world, "A", "AT&V");
    then_response_contains(&mut world, "A", "IFC:2,2");
}

// Scenario: Get Product Serial Number (IMEI)
//   Given a modem "A"
//   When AT command "AT+CGSN" is sent to "A"
//   Then response from "A" is "867400022047199"
//   And response from "A" is "OK"
#[test]
fn test_get_product_serial_number_gsm() {
    let mut world = World::new();
    given_modem(&mut world, "A");
    when_at_command_sent(&mut world, "A", "AT+CGSN");
    then_response_is(&mut world, "A", "867400022047199");
    then_response_is(&mut world, "A", "OK");
}

// Scenario: Get Product Serial Number with Type (IMEI + SVN)
//   Given a modem "A"
//   When AT command "AT+CGSN=2" is sent to "A"
//   Then response from "A" is "86740002204719901"
//   And response from "A" is "OK"
#[test]
fn test_get_product_serial_number_gsm_with_type() {
    let mut world = World::new();
    given_modem(&mut world, "A");
    when_at_command_sent(&mut world, "A", "AT+CGSN=2");
    then_response_is(&mut world, "A", "86740002204719901");
    then_response_is(&mut world, "A", "OK");
}

#[test]
fn test_command_parse_failure() {
    let mut world = World::new();
    given_modem(&mut world, "A");

    // Sending a command without "AT" prefix should fail parsing immediately and
    // return ERROR.
    when_at_command_sent(&mut world, "A", "INVALID");
    then_response_is(&mut world, "A", "ERROR");
}
