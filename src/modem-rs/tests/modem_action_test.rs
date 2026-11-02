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

// Scenario: Trigger Incoming Call via Action
//   Given a modem "A"
//   When Action IncomingCall is dispatched to "A" from "123456"
//   Then response from "A" is "RING"
//   And response from "A" is "+CLIP: \"123456\",129,,,,0"
#[test]
fn test_action_incoming_call() {
    let mut world = World::new();
    given_modem_with_number(&mut world, "A", "123456");

    // Enable CLIP
    when_at_command_sent(&mut world, "A", "AT+CLIP=1");
    then_response_is(&mut world, "A", "OK");

    when_incoming_call_received(&mut world, "A", "123456");

    then_response_is(&mut world, "A", "RING");
    then_response_is(&mut world, "A", "+CLIP: \"123456\",129,,,,0");
}

// Scenario: Trigger Incoming SMS via Action
//   Given a modem "A"
//   When Action IncomingSms is dispatched to "A" from "5555" with text "Hello"
//   Then response from "A" is contains "+CMT:"
//   And response from "A" is "Hello"
#[test]
fn test_action_incoming_sms() {
    let mut world = World::new();
    given_modem(&mut world, "A");
    when_voice_registration_set(&mut world, "A", RegistrationStatus::RegisteredHome);

    // Set to Text Mode first to align with the expected +CMT and text output
    when_at_command_sent(&mut world, "A", "AT+CMGF=1");
    then_response_is(&mut world, "A", "OK");

    when_incoming_sms_received(&mut world, "A", "5555", "Hello");

    then_wait_for_response_containing(&mut world, "A", "+CMT: \"5555\"");
    then_response_is(&mut world, "A", "Hello");
}

// Scenario: Set Registration Status via Action
//   Given a modem "A"
//   When Action SetVoiceRegistration is dispatched to "A" with status Roaming
//   Then response from "A" is "+CREG: 5"
#[test]
fn test_action_set_registration() {
    let mut world = World::new();
    given_modem(&mut world, "A");

    // Enable unsolicited reports first to make it spec-compliant
    when_at_command_sent(&mut world, "A", "AT+CREG=1");
    then_response_is(&mut world, "A", "OK");

    when_voice_registration_set(&mut world, "A", RegistrationStatus::Roaming);

    then_response_is(&mut world, "A", "+CREG: 5");
}

#[test]
fn test_sim_hot_plugging() {
    let mut world = World::new();
    // Start with a locked SIM (PIN enabled, state PinRequired)
    given_modem_with_locked_sim(&mut world, "A");

    // Enable unsolicited registration reports and verbose errors
    when_at_command_sent(&mut world, "A", "AT+CREG=1");
    then_response_is(&mut world, "A", "OK");
    when_at_command_sent(&mut world, "A", "AT+CMEE=1");
    then_response_is(&mut world, "A", "OK");

    // Advance time to allow AttachNetwork to run (10ms delay)
    when_time_advances_ms(&mut world, 15);
    // Consume the initial attachment URC
    then_response_is(&mut world, "A", "+CREG: 8");
    then_wait_for_response_containing(&mut world, "A", "+CSQ:");

    // Initial state check: PinRequired
    when_at_command_sent(&mut world, "A", "AT+CPIN?");
    then_response_is(&mut world, "A", "+CPIN: SIM PIN");
    then_response_is(&mut world, "A", "OK");

    // Initial registration check: locked SIM registers for emergency only
    when_at_command_sent(&mut world, "A", "AT+CREG?");
    then_response_is(&mut world, "A", "+CREG: 1,8");
    then_response_is(&mut world, "A", "OK");

    // 1. Remove SIM
    when_sim_status_set(&mut world, "A", false);
    // Should receive unsolicited CREG report and CPIN report
    then_wait_for_response_containing(&mut world, "A", "+CREG: 0");
    then_wait_for_response_containing(&mut world, "A", "+CPIN: ABSENT");

    // Verify CPIN fails with SIM not inserted
    when_at_command_sent(&mut world, "A", "AT+CPIN?");
    then_response_is(&mut world, "A", "+CME ERROR: 10");

    // Verify CREG is 0
    when_at_command_sent(&mut world, "A", "AT+CREG?");
    then_response_is(&mut world, "A", "+CREG: 1,0");
    then_response_is(&mut world, "A", "OK");

    // 2. Re-insert SIM
    when_sim_status_set(&mut world, "A", true);
    // Advance time to allow AttachNetwork event to run (10ms delay)
    when_time_advances_ms(&mut world, 15);

    // Should receive unsolicited CPIN report, CREG report, and CSQ
    then_wait_for_response_containing(&mut world, "A", "+CPIN: SIM PIN");
    then_wait_for_response_containing(&mut world, "A", "+CREG: 8");
    then_wait_for_response_containing(&mut world, "A", "+CSQ:");

    // Verify CPIN is back to SIM PIN
    when_at_command_sent(&mut world, "A", "AT+CPIN?");
    then_response_is(&mut world, "A", "+CPIN: SIM PIN");
    then_response_is(&mut world, "A", "OK");

    // 3. Unlock PIN
    when_at_command_sent(&mut world, "A", &format!("AT+CPIN=\"{LOCKED_PIN}\""));
    then_response_is(&mut world, "A", "OK");

    // Verify CPIN is READY
    when_at_command_sent(&mut world, "A", "AT+CPIN?");
    then_response_is(&mut world, "A", "+CPIN: READY");
    then_response_is(&mut world, "A", "OK");

    // 4. Remove SIM again
    when_sim_status_set(&mut world, "A", false);
    then_wait_for_response_containing(&mut world, "A", "+CREG: 0");
    then_wait_for_response_containing(&mut world, "A", "+CPIN: ABSENT");

    // 5. Re-insert SIM again -> should go back to PinRequired (not READY)
    when_sim_status_set(&mut world, "A", true);
    when_time_advances_ms(&mut world, 15);
    then_wait_for_response_containing(&mut world, "A", "+CPIN: SIM PIN");
    then_wait_for_response_containing(&mut world, "A", "+CREG: 8");
    then_wait_for_response_containing(&mut world, "A", "+CSQ:");

    when_at_command_sent(&mut world, "A", "AT+CPIN?");
    then_response_is(&mut world, "A", "+CPIN: SIM PIN"); // Back to locked!
    then_response_is(&mut world, "A", "OK");
}

#[test]
fn test_sim_hot_plugging_goldfish_37() {
    let mut world = World::new();
    given_goldfish_37_modem(&mut world, "A");

    when_at_command_sent(&mut world, "A", "AT+CREG=1");
    then_response_is(&mut world, "A", "OK");

    when_time_advances_ms(&mut world, 15);
    then_response_is(&mut world, "A", "+CREG: 1,\"2142\",\"0000B804\",7");
    then_wait_for_response_containing(&mut world, "A", "+CSQ:");

    // Remove SIM on goldfish_37 modem: should NOT emit +CPIN URC
    when_sim_status_set(&mut world, "A", false);
    then_response_is(&mut world, "A", "+CREG: 0");

    // CPIN status query still works as expected (returns ERROR when CMEE is not
    // enabled)
    when_at_command_sent(&mut world, "A", "AT+CPIN?");
    then_response_is(&mut world, "A", "ERROR");

    when_at_command_sent(&mut world, "A", "AT+COPS?");
    then_response_is(&mut world, "A", "+COPS: 0,2,\"\"");
    then_response_is(&mut world, "A", "OK");
}
