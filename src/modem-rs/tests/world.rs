// Copyright 2025 The Android Open Source Project
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

use std::{collections::HashMap, sync::Arc};

use modem_rs::{
    ModemId, ModemNetworkSimulator, SimProfile, test_utils::MockModemHandler, time::MockClock,
};
use netsim_model::Quirks;

use crate::steps::{
    action::{
        when_at_command_sent, when_hex_bytes_sent, when_sim_status_set, when_time_advances_ms,
    },
    check::{
        then_no_response, then_prompt, then_prompt_is, then_response_contains, then_response_is,
        then_wait_for_response_containing,
    },
    setup::{
        given_data_modem, given_goldfish_37_data_modem, given_goldfish_37_modem, given_modem,
        given_modem_with_fdn_sim_profile, given_modem_with_locked_sim,
        given_modem_with_msisdn_in_fs, given_modem_with_network_config, given_modem_with_number,
        given_modem_with_perm_blocked_sim, given_modem_with_sim_profile,
        given_modem_with_xml_profile,
    },
};

/// The BDD World for Modem-rs tests.
pub struct World {
    /// The Modem Network Simulator under test.
    pub manager: ModemNetworkSimulator,
    /// Shared clock for the simulator.
    pub clock: Arc<MockClock>,
    /// Map of modem names to their IDs and handlers.
    pub modems: HashMap<String, (ModemId, MockModemHandler)>,
    /// Counter for generating unique ModemIds.
    pub modem_id_counter: usize,
    /// Keep the host event receiver alive to prevent channel closure.
    pub _host_event_rx: tokio::sync::mpsc::UnboundedReceiver<modem_rs::HostEvent>,
}

impl World {
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        crate::common::init_logger();
        // No network handler needed for constructor.
        // If tests need to inspect network events, they should capture them
        // from dispatch output. For now, we assume tests rely on modem
        // responses.
        let clock = Arc::new(MockClock::default());
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let manager = ModemNetworkSimulator::new_with_clock(clock.clone(), tx);

        World { manager, clock, modems: HashMap::new(), modem_id_counter: 0, _host_event_rx: rx }
    }

    pub fn next_modem_id(&mut self) -> ModemId {
        self.modem_id_counter += 1;
        self.modem_id_counter as ModemId
    }

    /// Helper to get modem data by name
    pub fn get_modem(&mut self, name: &str) -> (ModemId, &mut MockModemHandler) {
        if let Some((id, handler)) = self.modems.get_mut(name) {
            (*id, handler)
        } else {
            panic!("Modem '{name}' not found");
        }
    }

    // --- BDD Given (Setup / Preconditions) ---

    /// Creates a modem with the given name.
    pub fn given_modem(&mut self, name: &str) {
        given_modem(self, name);
    }

    pub fn given_data_modem(&mut self, name: &str) {
        given_data_modem(self, name);
    }

    pub fn given_goldfish_37_data_modem(&mut self, name: &str) {
        given_goldfish_37_data_modem(self, name);
    }

    pub fn given_modem_with_network_config(
        &mut self,
        name: &str,
        network_config: netsim_model::CellNetworkConfig,
    ) {
        given_modem_with_network_config(self, name, network_config);
    }

    /// Creates a Goldfish 37 (or earlier) modem with the given name.
    pub fn given_goldfish_37_modem(&mut self, name: &str) {
        given_goldfish_37_modem(self, name);
    }

    /// Creates a modem with EF_MSISDN configured in the SIM filesystem.
    pub fn given_modem_with_msisdn_in_fs(&mut self, name: &str) {
        given_modem_with_msisdn_in_fs(self, name);
    }

    /// Creates a modem with the given name and phone number.
    pub fn given_modem_with_number(&mut self, name: &str, number: &str) {
        given_modem_with_number(self, name, number);
    }

    /// Creates a modem with the default SIM profile.
    pub fn given_modem_with_sim_profile(&mut self, name: &str) {
        given_modem_with_sim_profile(self, name);
    }

    /// Creates a modem with an FDN-enabled SIM profile.
    pub fn given_modem_with_fdn_sim_profile(&mut self, name: &str) {
        given_modem_with_fdn_sim_profile(self, name);
    }

    /// Creates a modem with a locked SIM profile.
    pub fn given_modem_with_locked_sim(&mut self, name: &str) {
        given_modem_with_locked_sim(self, name);
    }

    /// Creates a modem with a permanently blocked SIM profile.
    pub fn given_modem_with_perm_blocked_sim(&mut self, name: &str) {
        given_modem_with_perm_blocked_sim(self, name);
    }

    /// Creates a modem with an XML SIM profile string.
    pub fn given_modem_with_xml_profile(&mut self, name: &str, xml: &str) {
        given_modem_with_xml_profile(self, name, xml);
    }

    /// Creates a modem with a specific SIM profile.
    pub fn given_modem_with_profile(&mut self, name: &str, profile: SimProfile) {
        if self.modems.contains_key(name) {
            panic!("Modem with name '{name}' already exists");
        }
        let id = self.next_modem_id();
        let (handler, sink) = MockModemHandler::new(false);
        self.manager
            .new_modem_with_profile(id, sink, Some(profile), None, Quirks::default(), Vec::new())
            .expect("Failed to create modem with profile");
        self.modems.insert(name.to_string(), (id, handler));
    }

    // --- BDD When (Pure Actions / Stimulus) ---

    /// Sends an AT command string to the specified modem without asserting its
    /// response.
    pub fn when_at_command(&mut self, name: &str, command: &str) {
        when_at_command_sent(self, name, command);
    }

    /// Sends raw bytes, specified as a hex string, to the modem.
    pub fn when_hex_bytes(&mut self, name: &str, hex_bytes: &str) {
        when_hex_bytes_sent(self, name, hex_bytes);
    }

    /// Sets the SIM status (inserted or removed) for the modem.
    pub fn when_sim_status(&mut self, name: &str, present: bool) {
        when_sim_status_set(self, name, present);
    }

    /// Advances simulated time and ticks the modem network simulator.
    pub fn when_time_advances_ms(&mut self, ms: u64) {
        when_time_advances_ms(self, ms);
    }

    // --- BDD Then (Pure Verifications / Assertions) ---

    /// Asserts that the next response from the modem exactly matches the
    /// expected string.
    pub fn then_response_is(&mut self, name: &str, expected: &str) {
        then_response_is(self, name, expected);
    }

    /// Asserts that the modem eventually receives a response containing the
    /// expected substring.
    pub fn then_response_contains(&mut self, name: &str, expected: &str) {
        then_response_contains(self, name, expected);
    }

    /// Asserts that the modem emitted an interactive prompt (such as `> `).
    pub fn then_prompt_is(&mut self, name: &str, expected: &str) {
        then_prompt_is(self, name, expected);
    }

    /// Convenience alias for asserting the standard SMS prompt `> `.
    pub fn then_prompt(&mut self, name: &str) {
        then_prompt(self, name);
    }

    /// Asserts that the modem has no pending responses in its queue.
    pub fn then_no_response(&mut self, name: &str) {
        then_no_response(self, name);
    }

    // --- Compound AT Transactions (Action + Assertion) ---

    /// Sends an AT command and asserts an immediate "OK" response.
    pub fn send_and_expect_ok(&mut self, name: &str, command: &str) {
        self.when_at_command(name, command);
        self.then_response_is(name, "OK");
    }

    /// Sends an AT command and asserts expected sequential lines.
    pub fn send_and_expect(&mut self, name: &str, command: &str, expected_lines: &[&str]) {
        self.when_at_command(name, command);
        for line in expected_lines {
            self.then_response_is(name, line);
        }
    }

    /// Sends an AT command and asserts an expected error response.
    pub fn send_and_expect_error(&mut self, name: &str, command: &str, expected_error: &str) {
        self.when_at_command(name, command);
        self.then_response_is(name, expected_error);
    }

    /// Triggers Emergency Callback Mode (ECBM) on the named modem.
    pub fn trigger_emergency_callback_mode(&mut self, name: &str, enabled: bool) {
        let (id, _) = self.get_modem(name);
        self.manager.trigger_emergency_callback_mode(id, enabled);
    }

    // --- Compound Telephony Workflows ---

    /// Dials from `caller` to `callee`'s assigned phone number, verifies `OK`
    /// on caller, and waits for `RING` on callee.
    pub fn dial(&mut self, caller: &str, callee: &str) {
        let (callee_id, _) = self.get_modem(callee);
        let callee_num: String = self
            .manager
            .get_modem(callee_id)
            .and_then(|m| m.phone_number().map(|p| p.to_string()))
            .unwrap_or_else(|| panic!("Modem '{callee}' has no assigned phone number"));

        self.dial_number(caller, &callee_num, callee);
    }

    /// Dials a specific number from `caller`, verifies `OK` on caller, and
    /// waits for `RING` on `callee`.
    pub fn dial_number(&mut self, caller: &str, number: &str, callee: &str) {
        self.send_and_expect_ok(caller, &format!("ATD{number};"));
        then_wait_for_response_containing(self, callee, "RING");
    }

    /// `callee` answers incoming call via `ATA`, verifies `OK` on callee,
    /// and waits for the connection `RING` URC on `caller`.
    pub fn answer(&mut self, callee: &str, caller: &str) {
        self.send_and_expect_ok(callee, "ATA");
        then_wait_for_response_containing(self, caller, "RING");
    }

    /// Connects a full 2-party voice call from `caller` to `callee`.
    pub fn connect_call(&mut self, caller: &str, callee: &str) {
        self.dial(caller, callee);
        self.answer(callee, caller);
    }

    /// Swaps active and held calls on the modem via `AT+CHLD=2`.
    pub fn swap_calls(&mut self, name: &str) {
        self.send_and_expect_ok(name, "AT+CHLD=2");
    }

    /// Merges active and held calls into a multi-party conference via
    /// `AT+CHLD=3`.
    pub fn conference(&mut self, name: &str) {
        self.send_and_expect_ok(name, "AT+CHLD=3");
    }

    /// Connects the host modem to multiple peers sequentially and merges them
    /// into a conference.
    pub fn connect_conference(&mut self, host: &str, peers: &[&str]) {
        for peer in peers {
            self.connect_call(host, peer);
        }
        self.conference(host);
    }

    /// Releases all active calls via `ATH`.
    pub fn hangup(&mut self, name: &str) {
        self.send_and_expect_ok(name, "ATH");
    }

    /// Releases all held calls or sets User Determined User Busy (UDUB) for a
    /// waiting call via `AT+CHLD=0`.
    pub fn release_held_calls(&mut self, name: &str) {
        self.send_and_expect_ok(name, "AT+CHLD=0");
    }

    /// Releases all active calls and accepts other (held or waiting) call via
    /// `AT+CHLD=1`.
    pub fn release_active_and_accept_held(&mut self, name: &str) {
        self.send_and_expect_ok(name, "AT+CHLD=1");
    }

    /// Releases a specific active call index via `AT+CHLD=1{idx}`.
    pub fn release_call(&mut self, name: &str, idx: usize) {
        self.send_and_expect_ok(name, &format!("AT+CHLD=1{idx}"));
    }

    /// Separates a call from multiparty conference or recovers a specific call
    /// via `AT+CHLD=2{idx}`.
    pub fn separate_call(&mut self, name: &str, idx: usize) {
        self.send_and_expect_ok(name, &format!("AT+CHLD=2{idx}"));
    }

    /// Queries `AT+CLCC` on the modem and asserts all returned `+CLCC:` lines
    /// followed by `OK`.
    pub fn assert_clcc(&mut self, name: &str, expected_lines: &[&str]) {
        self.when_at_command(name, "AT+CLCC");
        for line in expected_lines {
            then_wait_for_response_containing(self, name, line);
        }
        then_wait_for_response_containing(self, name, "OK");
    }

    /// Asserts that the modem has zero active, held, or waiting calls
    /// (`AT+CLCC` returns only `OK`).
    pub fn assert_idle(&mut self, name: &str) {
        self.assert_clcc(name, &[]);
    }

    // --- Compound SMS Workflows ---

    /// Configures preferred SMS storage to SIM (`SM`) for reads, writes, and
    /// receives.
    pub fn select_sim_storage(&mut self, name: &str) {
        self.send_and_expect_ok(name, "AT+CPMS=\"SM\",\"SM\",\"SM\"");
    }
}
