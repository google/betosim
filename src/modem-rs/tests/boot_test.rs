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

use netsim_model::Quirks;

use crate::{
    common::constants::{INVALID_CTEC_CMD, NETWORK_ATTACH_WAIT_MS},
    world::World,
};

// Mirrors kNetsimResetCmd in CellularProtocol::reset_netsim (netsim_chardev.cc, ag/43490499, b/580681324).
const CHAINED_RADIO_RESET_CMD: &str =
    "AT+CREG=0;+CGREG=0;+CEREG=0;+CFUN=0;+CREG=2;+CGREG=2;+CEREG=2;+CMEE=1;+CMEE=?";
const CHAINED_CFUN0_WITHOUT_URC_DISABLE_CMD: &str = "AT+CFUN=0;+CMEE=?";
const RESET_ACK_PACKET_CRLF: &[u8] = b"+CMEE: (0-2)\r\nOK\r\n";
const RESET_ACK_PACKET_CR: &[u8] = b"+CMEE: (0-2)\rOK\r";
const DETACH_AND_CMEE_PACKET_CRLF: &[u8] =
    b"+CREG: 0\r\n+CGREG: 0\r\n+CEREG: 0\r\n+CMEE: (0-2)\r\nOK\r\n";

const TEST_ZONED_TIME: &str = "2026-06-15T12:30:45-07:00[America/Los_Angeles]";
const EXPECTED_NITZ_URC: &str = "%CTZV: 26/06/15:19:30:45-28:1:America!Los_Angeles";
const UPDATED_ZONED_TIME: &str = "2026-06-15T14:05:10-07:00[America/Los_Angeles]";
const EXPECTED_UPDATED_NITZ_URC: &str = "%CTZV: 26/06/15:21:05:10-28:1:America!Los_Angeles";

// Scenario: Cold-boot AT initialization sequence leaves radio powered on
//   Given modem "A" with a default SIM profile
//   When the cold-boot AT initialization sequence runs on "A"
//   Then "AT+CFUN?" reports "+CFUN: 1" followed by "OK"
#[test]
fn test_goldfish_boot_sequence() {
    let mut world = World::new();
    // Given modem "A" with a default SIM profile
    world.given_modem_with_sim_profile("A");

    // When the cold-boot AT initialization sequence runs on "A"
    world.when_cold_boot_init_sequence_runs("A");

    // Then "AT+CFUN?" reports full radio functionality
    world.send_and_expect("A", "AT+CFUN?", &["+CFUN: 1", "OK"]);
}

// Scenario: Chained radio power-off reset on an attached modem suppresses detach URCs and re-attaches
//   Given modem "A" completed cold-boot AT initialization and attached to LTE
//   When the chained power-off reset command is sent to "A" and time advances past the attach delay
//   Then "A" returns one coalesced "+CMEE: (0-2)\r\nOK\r\n" packet with no detach URCs
//   And "A" stays powered off with location URC modes (2) and numeric CMEE (1) retained
//   When "AT+CFUN=1" is sent to "A" and time advances past the attach delay
//   Then "A" emits location registration URCs (+CREG/+CGREG/+CEREG) and default LTE signal quality
#[test]
fn test_chained_radio_reset_after_cold_boot_suppresses_detach_urcs_and_reattaches() {
    let mut world = World::new();
    // Given modem "A" completed cold-boot AT initialization and attached to LTE
    world.given_attached_cold_boot_modem("A");

    // When the chained power-off reset command is sent to "A" and time advances past the attach delay
    world.send_and_expect_raw_packets("A", CHAINED_RADIO_RESET_CMD, &[RESET_ACK_PACKET_CRLF]);
    world.when_time_advances_ms(NETWORK_ATTACH_WAIT_MS);

    // Then "A" stays powered off with URC modes (2) and numeric CMEE (1) retained
    world.send_and_expect_powered_off_reset_state("A");

    // When "AT+CFUN=1" is sent to "A" and time advances past the attach delay
    world.send_and_expect_ok("A", "AT+CFUN=1");
    world.when_time_advances_ms(NETWORK_ATTACH_WAIT_MS);

    // Then "A" re-attaches and emits location registration URCs and LTE signal quality
    world.then_location_attach_urcs_are("A");
    world.then_no_response("A");
}

// Scenario: Chained AT+CFUN=0 without disabling registration URCs prepends detach URCs before +CMEE=?
//   Given modem "A" completed cold-boot AT initialization and attached to LTE
//   When "AT+CFUN=0;+CMEE=?" is sent without prepending "+CREG=0;+CGREG=0;+CEREG=0"
//   Then "A" prepends "+CREG: 0", "+CGREG: 0", and "+CEREG: 0" ahead of "+CMEE: (0-2)" in the reply packet
#[test]
fn test_chained_cfun0_without_urc_suppression_prepends_detach_urcs() {
    let mut world = World::new();
    // Given modem "A" completed cold-boot AT initialization and attached to LTE
    world.given_attached_cold_boot_modem("A");

    // When "AT+CFUN=0;+CMEE=?" is sent without disabling registration URCs first
    // Then the coalesced packet starts with the three detach URCs instead of "+CMEE: (0-2)"
    world.send_and_expect_raw_packets(
        "A",
        CHAINED_CFUN0_WITHOUT_URC_DISABLE_CMD,
        &[DETACH_AND_CMEE_PACKET_CRLF],
    );
}

// Scenario: Chained radio reset on an uninitialized modem enables registration URCs and numeric CMEE
//   Given a fresh modem "A" that has not run cold-boot AT initialization (default CMEE=0)
//   When the chained power-off reset command is sent to "A" and time advances past the attach delay
//   Then "A" returns one coalesced "+CMEE: (0-2)\r\nOK\r\n" packet, enters CFUN=0, and switches from "ERROR" to numeric "+CME ERROR: 50"
//   When "AT+CFUN=1" is sent to "A" and time advances past the attach delay
//   Then "A" emits location registration URCs and default LTE signal quality
#[test]
fn test_chained_radio_reset_without_cold_boot_init_configures_urcs_and_cmee() {
    let mut world = World::new();
    // Given a fresh modem "A" with default CMEE=0 and no prior cold-boot init
    world.given_modem("A");
    world.send_and_expect_error("A", INVALID_CTEC_CMD, "ERROR");

    // When the chained power-off reset command is sent to "A" and time advances past the attach delay
    world.send_and_expect_raw_packets("A", CHAINED_RADIO_RESET_CMD, &[RESET_ACK_PACKET_CRLF]);
    world.when_time_advances_ms(NETWORK_ATTACH_WAIT_MS);

    // Then "A" configures URC modes (2) and numeric CMEE (1) while remaining powered off
    world.send_and_expect_powered_off_reset_state("A");

    // When "AT+CFUN=1" is sent to "A" and time advances past the attach delay
    world.send_and_expect_ok("A", "AT+CFUN=1");
    world.when_time_advances_ms(NETWORK_ATTACH_WAIT_MS);

    // Then "A" emits location registration URCs and LTE signal quality
    world.then_location_attach_urcs_are("A");
    world.then_no_response("A");
}

// Scenario: Chained radio reset re-emits updated NITZ time (%CTZV) across repeat resets
//   Given a zoned simulator clock and a modem "A" with auto_ctzv enabled
//   When the chained power-off reset command and "AT+CFUN=1" run on "A" and time advances past the attach delay
//   Then "A" emits location registration URCs, default LTE signal quality, and unsolicited NITZ time (%CTZV)
//   When the simulator clock updates and a second chained reset followed by "AT+CFUN=1" runs on "A"
//   Then "A" re-emits location registration URCs and the updated NITZ timestamp (%CTZV)
#[test]
fn test_chained_radio_reset_reemits_updated_nitz_across_repeat_resets() {
    let mut world = World::new();
    // Given a zoned simulator clock and a modem "A" with auto_ctzv enabled
    world.given_zoned_clock(TEST_ZONED_TIME);
    world.given_modem_with_quirks("A", Quirks { auto_ctzv: true, ..Default::default() });

    // When the chained power-off reset command and "AT+CFUN=1" run on "A"
    world.send_and_expect_raw_packets("A", CHAINED_RADIO_RESET_CMD, &[RESET_ACK_PACKET_CRLF]);
    world.send_and_expect_ok("A", "AT+CFUN=1");
    world.when_time_advances_ms(NETWORK_ATTACH_WAIT_MS);

    // Then "A" emits location registration URCs, LTE signal quality, and initial NITZ time (%CTZV)
    world.then_location_attach_urcs_are("A");
    world.then_response_is("A", EXPECTED_NITZ_URC);
    world.then_no_response("A");

    // When wall-clock time updates and a second chained reset + "AT+CFUN=1" cycle runs
    world.when_zoned_clock_set(UPDATED_ZONED_TIME);
    world.send_and_expect_raw_packets("A", CHAINED_RADIO_RESET_CMD, &[RESET_ACK_PACKET_CRLF]);
    world.send_and_expect_ok("A", "AT+CFUN=1");
    world.when_time_advances_ms(NETWORK_ATTACH_WAIT_MS);

    // Then "A" re-emits location registration URCs and the updated NITZ timestamp (%CTZV)
    world.then_location_attach_urcs_are("A");
    world.then_response_is("A", EXPECTED_UPDATED_NITZ_URC);
    world.then_no_response("A");
}

// Scenario: Chained radio reset on an attached CR-delimited Goldfish 37 modem suppresses detach URCs and emits one CR-framed ack packet
//   Given a Goldfish 37 modem "A" (CR line framing) that completed cold-boot AT initialization and attached to LTE
//   When the chained power-off reset command is sent to "A" and time advances past the attach delay
//   Then "A" returns one coalesced "+CMEE: (0-2)\rOK\r" packet with no detach URCs and powers off with URC modes (2) and CMEE (1)
//   When "AT+CFUN=1" is sent to "A" and time advances past the attach delay
//   Then "A" emits CR-delimited location registration URCs and default LTE signal quality
#[test]
fn test_chained_radio_reset_coalesces_cr_delimited_ack_on_goldfish_37() {
    let mut world = World::new();
    // Given an attached cold-boot Goldfish 37 modem "A" with CR line framing
    world.given_attached_cold_boot_modem_with_quirks(
        "A",
        Quirks { goldfish_ril_37_or_earlier: true, ..Default::default() },
    );

    // When the chained power-off reset command is sent to "A" and time advances past the attach delay
    world.send_and_expect_raw_packets("A", CHAINED_RADIO_RESET_CMD, &[RESET_ACK_PACKET_CR]);
    world.when_time_advances_ms(NETWORK_ATTACH_WAIT_MS);

    // Then "A" stays powered off with URC modes (2) and numeric CMEE (1) under CR framing
    world.send_and_expect_powered_off_reset_state("A");

    // When "AT+CFUN=1" is sent to "A" and time advances past the attach delay
    world.send_and_expect_ok("A", "AT+CFUN=1");
    world.when_time_advances_ms(NETWORK_ATTACH_WAIT_MS);

    // Then "A" re-attaches and emits CR-delimited location registration URCs and LTE signal quality
    world.then_location_attach_urcs_are("A");
    world.then_no_response("A");
}
