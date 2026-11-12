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

#[cfg(not(target_os = "windows"))]
use jiff::Zoned;
#[cfg(not(target_os = "windows"))]
use modem_rs::test_utils::MockModemHandler;

#[cfg(not(target_os = "windows"))]
use crate::common::constants::RESP_CSQ_LTE_DEFAULT;
use crate::{steps::*, world::World};

// Scenario: Set and Query Time
//   Given a modem "A"
//   When AT command 'AT+CCLK="25/08/02,12:30:00+00"' is sent to "A"
//   Then response from "A" is "OK"
//   When AT command "AT+CCLK?" is sent to "A"
//   Then response from "A" is '+CCLK: "25/08/02,12:30:00+00"'
//   And response from "A" is "OK"
#[test]
fn test_set_and_query_time() {
    let mut world = World::new();
    given_modem(&mut world, "A");

    let time_str = "\"25/08/02,12:30:00+00\"";
    let set_cmd = format!("AT+CCLK={time_str}");

    when_at_command_sent(&mut world, "A", &set_cmd);
    then_response_is(&mut world, "A", "OK");

    when_at_command_sent(&mut world, "A", "AT+CCLK?");
    then_response_is(&mut world, "A", &format!("+CCLK: {time_str}"));
    then_response_is(&mut world, "A", "OK");
}

// Scenario: Update Network Time
//   Given a modem "A"
//   When external network time update is received
//   Then response from "A" is "%CTZV: ..."
//   When AT command "AT+CCLK?" is sent to "A"
//   Then response from "A" matches "+CCLK: ..."
//   And response from "A" is "OK"
#[test]
#[cfg(not(target_os = "windows"))]
fn test_update_network_time() {
    let mut world = World::new();
    given_modem(&mut world, "A");

    let zoned: Zoned = "2026-06-15T12:30:45-07:00[America/Los_Angeles]".parse().unwrap();
    world.clock.set_zoned(zoned);

    when_network_time_updated(&mut world, "A");

    // Expect NITZ unsolicited (%CTZV: yy/MM/dd:HH:mm:ss+/-tz:dst:zone_name)
    then_response_is(&mut world, "A", "%CTZV: 26/06/15:19:30:45-28:1:America!Los_Angeles");

    // Expect CCLK queries current time from clock
    when_at_command_sent(&mut world, "A", "AT+CCLK?");
    then_response_is(&mut world, "A", "+CCLK: \"26/06/15,19:30:45-28\"");
    then_response_is(&mut world, "A", "OK");
}

#[test]
fn test_ctzv_command() {
    let mut world = World::new();
    given_modem(&mut world, "A");

    when_at_command_sent(&mut world, "A", "AT%CTZV?");
    then_response_is(&mut world, "A", "%CTZV: 0");
    then_response_is(&mut world, "A", "OK");

    when_at_command_sent(&mut world, "A", "AT%CTZV=1");
    then_response_is(&mut world, "A", "OK");

    when_at_command_sent(&mut world, "A", "AT%CTZV?");
    then_response_is(&mut world, "A", "%CTZV: 1");
    then_response_is(&mut world, "A", "OK");

    when_at_command_sent(&mut world, "A", "AT%CTZV=0");
    then_response_is(&mut world, "A", "OK");

    when_at_command_sent(&mut world, "A", "AT%CTZV?");
    then_response_is(&mut world, "A", "%CTZV: 0");
    then_response_is(&mut world, "A", "OK");
}

#[test]
#[cfg(not(target_os = "windows"))]
fn test_auto_ctzv_on_attach() {
    let mut world = World::new();
    let zoned: Zoned = "2026-06-15T12:30:45-07:00[America/Los_Angeles]".parse().unwrap();
    world.clock.set_zoned(zoned);

    // Modems with auto_ctzv enabled (e.g. Goldfish and Cuttlefish) emit %CTZV on
    // network attach
    given_modem_with_quirks(
        &mut world,
        "A",
        netsim_model::Quirks { auto_ctzv: true, ..Default::default() },
    );

    // By default, AT%CTZV? should report enabled (1)
    when_at_command_sent(&mut world, "A", "AT%CTZV?");
    then_response_is(&mut world, "A", "%CTZV: 1");
    then_response_is(&mut world, "A", "OK");

    // Turn radio ON to schedule network attachment
    when_at_command_sent(&mut world, "A", "AT+CFUN=1");
    then_response_is(&mut world, "A", "OK");

    // Advance time to trigger the AttachNetwork event (10ms delay)
    when_time_advances_ms(&mut world, 15);

    // Verify unsolicited %CTZV NITZ update is received
    then_wait_for_response_containing(
        &mut world,
        "A",
        "%CTZV: 26/06/15:19:30:45-28:1:America!Los_Angeles",
    );
}

#[test]
#[cfg(not(target_os = "windows"))]
fn test_auto_ctzv_disabled_by_default() {
    let mut world = World::new();
    let zoned: Zoned = "2026-06-15T12:30:45-07:00[America/Los_Angeles]".parse().unwrap();
    world.clock.set_zoned(zoned);

    // Default modem (auto_ctzv: false) should NOT emit %CTZV on network attach
    given_modem(&mut world, "A");

    // By default, AT%CTZV? should report disabled (0)
    when_at_command_sent(&mut world, "A", "AT%CTZV?");
    then_response_is(&mut world, "A", "%CTZV: 0");
    then_response_is(&mut world, "A", "OK");

    // Turn radio ON to schedule network attachment
    when_at_command_sent(&mut world, "A", "AT+CFUN=1");
    then_response_is(&mut world, "A", "OK");

    // Advance time to trigger AttachNetwork event
    when_time_advances_ms(&mut world, 15);

    // In a default modem, only standard CSQ is sent on attach, no %CTZV response
    then_response_is(&mut world, "A", RESP_CSQ_LTE_DEFAULT);
    then_no_response(&mut world, "A");
}

#[test]
#[cfg(not(target_os = "windows"))]
fn test_auto_ctzv_suppressed_on_locked_sim() {
    let mut world = World::new();
    let zoned: Zoned = "2026-06-15T12:30:45-07:00[America/Los_Angeles]".parse().unwrap();
    world.clock.set_zoned(zoned);

    given_modem_with_locked_sim(&mut world, "A");
    when_at_command_sent(&mut world, "A", "AT%CTZV=1");
    then_response_is(&mut world, "A", "OK");
    when_at_command_sent(&mut world, "A", "AT+CFUN=1");
    then_response_is(&mut world, "A", "OK");
    when_time_advances_ms(&mut world, 15);
    then_response_is(&mut world, "A", RESP_CSQ_LTE_DEFAULT);
    then_no_response(&mut world, "A");

    // Unlocking the SIM attaches to normal service, emitting the unsolicited %CTZV
    when_at_command_sent(&mut world, "A", "AT+CPIN=\"1111\"");
    then_response_is(&mut world, "A", "OK");
    when_time_advances_ms(&mut world, 15);
    then_wait_for_response_containing(
        &mut world,
        "A",
        "%CTZV: 26/06/15:19:30:45-28:1:America!Los_Angeles",
    );
}

#[test]
#[cfg(not(target_os = "windows"))]
fn test_auto_ctzv_emitted_on_locked_sim_when_quirk_enabled() {
    let mut world = World::new();
    let zoned: Zoned = "2026-06-15T12:30:45-07:00[America/Los_Angeles]".parse().unwrap();
    world.clock.set_zoned(zoned);

    let id = world.next_modem_id();
    let (handler, sink) = MockModemHandler::new(false);
    world
        .manager
        .new_modem_with_profile(
            id,
            sink,
            Some(create_locked_sim_profile()),
            None,
            netsim_model::Quirks { auto_ctzv: true, ..Default::default() },
            Vec::new(),
        )
        .expect("Failed to create modem");
    world.modems.insert("A".to_string(), (id, handler));

    when_at_command_sent(&mut world, "A", "AT+CFUN=1");
    then_response_is(&mut world, "A", "OK");
    when_time_advances_ms(&mut world, 15);

    then_wait_for_response_containing(
        &mut world,
        "A",
        "%CTZV: 26/06/15:19:30:45-28:1:America!Los_Angeles",
    );
}

// Scenario: Independent per-modem timezone clocks in one simulator
//   Given modem "A" configured with timezone "America/New_York"
//   And modem "B" configured with timezone "Asia/Tokyo"
//   When external network time updates are received for "A" and "B"
//   Then "A" emits "%CTZV: 26/06/15:19:30:45-16:1:America!New_York"
//   And "B" emits "%CTZV: 26/06/15:19:30:45+36:0:Asia!Tokyo"
//   When "AT+CCLK?" is sent to "A" and "B"
//   Then "A" reports "+CCLK: \"26/06/15,19:30:45-16\""
//   And "B" reports "+CCLK: \"26/06/15,19:30:45+36\""
#[test]
#[cfg(not(target_os = "windows"))]
fn test_per_modem_timezone_independence() {
    let mut world = World::new();
    let zoned: Zoned = "2026-06-15T12:30:45-07:00[America/Los_Angeles]".parse().unwrap();
    world.clock.set_zoned(zoned);

    // Given modem "A" in America/New_York and modem "B" in Asia/Tokyo
    world.given_modem_with_timezone("A", "America/New_York");
    world.given_modem_with_timezone("B", "Asia/Tokyo");

    // When external network time updates are triggered for both modems
    when_network_time_updated(&mut world, "A");
    when_network_time_updated(&mut world, "B");

    // Then each modem emits NITZ formatted for its own configured timezone
    then_response_is(&mut world, "A", "%CTZV: 26/06/15:19:30:45-16:1:America!New_York");
    then_response_is(&mut world, "B", "%CTZV: 26/06/15:19:30:45+36:0:Asia!Tokyo");

    // When querying AT+CCLK? on both modems
    when_at_command_sent(&mut world, "A", "AT+CCLK?");
    // Then "A" reports the America/New_York quarter-hour offset (-16)
    then_response_is(&mut world, "A", "+CCLK: \"26/06/15,19:30:45-16\"");
    then_response_is(&mut world, "A", "OK");

    when_at_command_sent(&mut world, "B", "AT+CCLK?");
    // And "B" reports the Asia/Tokyo quarter-hour offset (+36)
    then_response_is(&mut world, "B", "+CCLK: \"26/06/15,19:30:45+36\"");
    then_response_is(&mut world, "B", "OK");
}

// Scenario: Host Radio Re-enable Sends NITZ on Attach
//   Given a modem "A" with auto_ctzv that attached after "AT+CFUN=1"
//   When the host disables and re-enables the radio of "A"
//   And time advances 15 ms
//   Then response from "A" is "+CSQ: ..."
//   And response from "A" is "%CTZV: ..."
#[test]
#[cfg(not(target_os = "windows"))]
fn test_host_radio_reenable_sends_nitz_on_attach() {
    let mut world = World::new();
    let zoned: Zoned = "2026-06-15T12:30:45-07:00[America/Los_Angeles]".parse().unwrap();
    world.clock.set_zoned(zoned);
    given_modem_with_quirks(
        &mut world,
        "A",
        netsim_model::Quirks { auto_ctzv: true, ..Default::default() },
    );
    world.send_and_expect_ok("A", "AT+CFUN=1");
    when_time_advances_ms(&mut world, 15);
    then_response_is(&mut world, "A", RESP_CSQ_LTE_DEFAULT);
    then_response_is(&mut world, "A", "%CTZV: 26/06/15:19:30:45-28:1:America!Los_Angeles");

    when_host_radio_set(&mut world, "A", false);
    when_host_radio_set(&mut world, "A", true);
    when_time_advances_ms(&mut world, 15);

    then_response_is(&mut world, "A", RESP_CSQ_LTE_DEFAULT);
    then_response_is(&mut world, "A", "%CTZV: 26/06/15:19:30:45-28:1:America!Los_Angeles");
    then_no_response(&mut world, "A");
}

// Scenario: Guest Radio Cycle While Host Radio Disabled Suppresses Auto CTZV
//   Given a host-disabled modem "A" with auto_ctzv
//   When AT command "AT+CFUN=0" is sent to "A"
//   Then response from "A" is "OK"
//   When AT command "AT+CFUN=1" is sent to "A"
//   Then response from "A" is "OK"
//   When time advances 15 ms
//   Then "A" receives no response
#[test]
fn test_guest_radio_cycle_while_host_radio_disabled_suppresses_auto_ctzv() {
    let mut world = World::new();
    given_modem_with_quirks(
        &mut world,
        "A",
        netsim_model::Quirks { auto_ctzv: true, ..Default::default() },
    );
    when_host_radio_set(&mut world, "A", false);

    world.send_and_expect_ok("A", "AT+CFUN=0");
    world.send_and_expect_ok("A", "AT+CFUN=1");
    when_time_advances_ms(&mut world, 15);

    then_no_response(&mut world, "A");
}

// Scenario: Network Time Update Suppressed While Host Radio Disabled
//   Given a host-disabled modem "A"
//   When external network time update is received
//   Then "A" receives no response
#[test]
fn test_update_network_time_suppressed_while_host_radio_disabled() {
    let mut world = World::new();
    given_modem(&mut world, "A");
    when_host_radio_set(&mut world, "A", false);

    when_network_time_updated(&mut world, "A");

    then_no_response(&mut world, "A");
}
