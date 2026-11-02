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

// Scenario: Multi Call (Conference/Swap)
//   Given a modem "A"
//   And a modem "B" with number "111"
//   And a modem "C" with number "222"
//   When AT command "ATD111;" is sent to "A"
//   Then wait for connection (OK from A, Ring B, ATA, OK)
//   When AT command "ATD222;" is sent to "A"
//   Then wait for connection (OK from A, Ring C, ATA, OK)
//   When AT command "AT+CHLD=2" is sent to "A"
//   Then response from "A" is "OK"
//   When AT command "AT+CLCC" is sent to "A"
//   Then response from "A" shows "111" is Active and "222" is Held
#[test]
fn test_multi_call_scenario() {
    let mut world = World::new();
    world.given_modem_with_number("A", "333");
    world.given_modem_with_number("B", "111");
    world.given_modem_with_number("C", "222");

    // 1. A calls B
    world.connect_call("A", "B");

    // 2. A calls C (B put on hold automatically by logic)
    world.connect_call("A", "C");

    // 3. Swap calls
    world.swap_calls("A");

    // Verify B's state: Should now be Active (0) since A swapped back to B
    world.assert_clcc("B", &["+CLCC: 1,1,0,0,0,"]);

    // Verify C's state: Should now be Held (1) since A swapped away from C
    world.assert_clcc("C", &["+CLCC: 1,1,1,0,0,"]);

    // 4. Verify states
    // Expected: 111 (Call 1) is Active (State 0)
    //           222 (Call 2) is Held (State 1)
    world.assert_clcc("A", &["+CLCC: 1,0,0,0,0,111", "+CLCC: 2,0,1,0,0,222"]);
}

#[test]
fn test_chld_zero() {
    let mut world = World::new();
    world.given_modem_with_number("A", "333");
    world.given_modem_with_number("B", "111");
    world.given_modem_with_number("C", "222");

    // A calls B, B answers
    world.connect_call("A", "B");

    // A calls C, B put on hold
    world.connect_call("A", "C");

    // Release held calls (B)
    world.release_held_calls("A");

    // Verify B (held call) got NO CARRIER
    then_wait_for_response_containing(&mut world, "B", "NO CARRIER");

    // Verify: Only C (Call 2) is active. B (Call 1) is gone.
    world.assert_clcc("A", &["+CLCC: 2,0,0,0,0,222"]);
}

#[test]
fn test_chld_one() {
    let mut world = World::new();
    world.given_modem_with_number("A", "333");
    world.given_modem_with_number("B", "111");
    world.given_modem_with_number("C", "222");

    // A calls B, B answers
    world.connect_call("A", "B");

    // A calls C, B put on hold
    world.connect_call("A", "C");

    // Release active (C), accept held (B)
    world.release_active_and_accept_held("A");

    // Verify C (active call) got NO CARRIER
    then_wait_for_response_containing(&mut world, "C", "NO CARRIER");

    // Verify B's state: Should now be Active (0) since A accepted it
    world.assert_clcc("B", &["+CLCC: 1,1,0,0,0,"]);

    // Verify: B (Call 1) is active, C (Call 2) is gone.
    world.assert_clcc("A", &["+CLCC: 1,0,0,0,0,111"]);
}

#[test]
fn test_chld_one_x() {
    let mut world = World::new();
    world.given_modem_with_number("A", "333");
    world.given_modem_with_number("B", "111");
    world.given_modem_with_number("C", "222");

    // A calls B, B answers
    world.connect_call("A", "B");

    // A calls C, B put on hold
    world.connect_call("A", "C");

    // Release specific call 1 (B)
    world.release_call("A", 1);

    // Verify: C (Call 2) remains active, B (Call 1) is gone.
    world.assert_clcc("A", &["+CLCC: 2,0,0,0,0,222"]);
}

#[test]
fn test_chld_three_conference() {
    let mut world = World::new();
    world.given_modem_with_number("A", "333");
    world.given_modem_with_number("B", "111");
    world.given_modem_with_number("C", "222");

    // A calls B, B answers
    world.connect_call("A", "B");

    // A calls C, B put on hold
    world.connect_call("A", "C");

    // Conference (Add held call B to conversation)
    world.conference("A");

    // Verify: Both calls are active and multiparty (1)
    world.assert_clcc("A", &["+CLCC: 1,0,0,0,1,111", "+CLCC: 2,0,0,0,1,222"]);
}

#[test]
fn test_chld_two_x_separate() {
    let mut world = World::new();
    world.given_modem_with_number("A", "333");
    world.given_modem_with_number("B", "111");
    world.given_modem_with_number("C", "222");

    // A calls B, B answers
    world.connect_call("A", "B");

    // A calls C, B put on hold
    world.connect_call("A", "C");

    // Conference
    world.conference("A");

    // Separate call 1 (B)
    world.separate_call("A", 1);

    // Verify: B (Call 1) is Active and NOT multiparty (0)
    //         C (Call 2) is Held and NOT multiparty (0) (degraded because single
    // held)
    world.assert_clcc("A", &["+CLCC: 1,0,0,0,0,111", "+CLCC: 2,0,1,0,0,222"]);
}

#[test]
fn test_chld_four_ect() {
    let mut world = World::new();
    world.given_modem_with_number("A", "333");
    world.given_modem_with_number("B", "111");
    world.given_modem_with_number("C", "222");

    // A calls B, B answers (A-B Active)
    world.connect_call("A", "B");

    // A calls C, B put on hold (A-C Active, A-B Held)
    world.connect_call("A", "C");

    // Trigger ECT (CHLD=4) -> Pragmatic Hangup
    world.send_and_expect_ok("A", "AT+CHLD=4"); // Returns OK to guest

    // Verify: All calls on A are cleared locally
    world.assert_idle("A"); // Only OK, no call lines
}

#[test]
fn test_chld_zero_waiting() {
    let mut world = World::new();
    world.given_modem_with_number("A", "333");
    world.given_modem_with_number("B", "111");
    world.given_modem_with_number("C", "222");

    // A calls B, B answers (Active)
    world.connect_call("A", "B");

    // C calls A, A gets RING (Waiting)
    world.dial("C", "A");

    // Release waiting call (C)
    world.release_held_calls("A");

    // Verify C got BUSY
    then_wait_for_response_containing(&mut world, "C", "BUSY");

    // Verify: Only B (Call 1) is active. C is gone.
    world.assert_clcc("A", &["+CLCC: 1,0,0,0,0,111"]);
}

#[test]
fn test_chld_one_waiting() {
    let mut world = World::new();
    world.given_modem_with_number("A", "333");
    world.given_modem_with_number("B", "111");
    world.given_modem_with_number("C", "222");

    // A calls B, B answers (Active)
    world.connect_call("A", "B");

    // C calls A, A gets CCWA (Waiting)
    world.dial("C", "A");

    // Release active (B), accept waiting (C)
    world.release_active_and_accept_held("A");

    // Verify B got NO CARRIER
    then_wait_for_response_containing(&mut world, "B", "NO CARRIER");

    // Verify C was answered
    then_wait_for_response_containing(&mut world, "C", "RING");

    // Verify: C (Call 2) is active, B (Call 1) is gone.
    world.assert_clcc("A", &["+CLCC: 2,1,0,0,0,222"]);
}

#[test]
fn test_chld_one_x_waiting() {
    let mut world = World::new();
    world.given_modem_with_number("A", "333");
    world.given_modem_with_number("B", "111");
    world.given_modem_with_number("C", "222");

    world.connect_call("A", "B");
    world.dial("C", "A");

    // Reject waiting call (Call 2 - C)
    world.release_call("A", 2);

    then_wait_for_response_containing(&mut world, "C", "BUSY");

    // Verify B is still active
    world.assert_clcc("A", &["+CLCC: 1,0,0,0,0,111"]);
}

#[test]
fn test_chld_two_waiting() {
    let mut world = World::new();
    world.given_modem_with_number("A", "333");
    world.given_modem_with_number("B", "111");
    world.given_modem_with_number("C", "222");

    world.connect_call("A", "B");
    world.dial("C", "A");

    // Hold active (B), accept waiting (C)
    world.swap_calls("A");

    // Verify C was answered
    then_wait_for_response_containing(&mut world, "C", "RING");

    // Verify: C is active (0), B is held (1).
    world.assert_clcc("A", &["+CLCC: 1,0,1,0,0,111", "+CLCC: 2,1,0,0,0,222"]);
}

#[test]
fn test_chld_two_with_idx_waiting() {
    let mut world = World::new();
    world.given_modem_with_number("A", "333");
    world.given_modem_with_number("B", "111");
    world.given_modem_with_number("C", "222");

    world.connect_call("A", "B");
    world.dial("C", "A");

    // Recover specific waiting call (Call 2 - C), which puts 1 (B) on hold
    world.separate_call("A", 2);

    // Verify C was answered
    then_wait_for_response_containing(&mut world, "C", "RING");

    // Verify: C is active (0), B is held (1).
    world.assert_clcc("A", &["+CLCC: 1,0,1,0,0,111", "+CLCC: 2,1,0,0,0,222"]);
}

#[test]
fn test_peer_specific_receive_hold_and_resume() {
    let mut world = World::new();
    world.given_modem_with_number("A", "333");
    world.given_modem_with_number("B", "111");
    world.given_modem_with_number("C", "222");

    // 1. A calls B, B answers
    world.connect_call("A", "B");

    // 2. A calls C, C answers (puts B on hold)
    world.connect_call("A", "C");

    // 3. A swaps calls with AT+CHLD=2 (C goes to Hold, B resumes to Active)
    world.swap_calls("A");

    // 4. Verify Peer B: Received receive_resume(A) -> Call with A (333) is Active
    //    (0)
    world.assert_clcc("B", &["+CLCC: 1,1,0,0,0,333"]);

    // 5. Verify Peer C: Received receive_hold(A) -> Call with A (333) is Held (1)
    world.assert_clcc("C", &["+CLCC: 1,1,1,0,0,333"]);
}

#[test]
fn test_chld_three_conference_peer_state_sync() {
    let mut world = World::new();
    world.given_modem_with_number("A", "333");
    world.given_modem_with_number("B", "111");
    world.given_modem_with_number("C", "222");

    // A calls B, B answers
    world.connect_call("A", "B");

    // A calls C, C answers (puts B on hold)
    world.connect_call("A", "C");

    // A merges B & C into conference with AT+CHLD=3
    world.conference("A");

    // Both Peer B and Peer C should receive ResumeCall and report state 0 (Active)
    world.assert_clcc("B", &["+CLCC: 1,1,0,0,0,333"]);

    world.assert_clcc("C", &["+CLCC: 1,1,0,0,0,333"]);
}

#[test]
fn test_chld_invalid_call_index_returns_error() {
    let mut world = World::new();
    world.given_modem_with_number("A", "333");
    world.given_modem_with_number("B", "111");

    world.connect_call("A", "B");

    // AT+CHLD=19 with invalid call index 9 must return ERROR
    world.send_and_expect_error("A", "AT+CHLD=19", "ERROR");
}

#[test]
fn test_ath0_parsing_and_hangup() {
    let mut world = World::new();
    world.given_modem_with_number("A", "333");
    world.given_modem_with_number("B", "111");

    world.connect_call("A", "B");

    // ATH0 should hang up call cleanly
    world.send_and_expect_ok("A", "ATH0");
    then_wait_for_response_containing(&mut world, "B", "NO CARRIER");
}

#[test]
fn test_cmut_out_of_bounds_parameter() {
    let mut world = World::new();
    world.given_modem("A");

    // AT+CMUT=2 is out-of-bounds (0 or 1 valid) and must return ERROR
    world.send_and_expect_error("A", "AT+CMUT=2", "ERROR");
}

#[test]
fn test_vts_quoted_digit_syntax() {
    let mut world = World::new();
    world.given_modem("A");

    // AT+VTS="1" quoted syntax should parse correctly
    world.send_and_expect_ok("A", "AT+VTS=\"1\"");
}

#[test]
fn test_outbound_call_cancelled_before_answer_ath() {
    let mut world = World::new();
    world.given_modem_with_number("A", "333");
    world.given_modem_with_number("B", "111");

    // A dials B
    world.dial("A", "B");

    // Before B answers, A cancels via ATH
    world.hangup("A");

    // B should receive remote hangup URC
    then_wait_for_response_containing(&mut world, "B", "NO CARRIER");

    // Verify both modems are now idle
    world.assert_idle("A");
    world.assert_idle("B");
}

#[test]
fn test_outbound_call_cancelled_before_answer_chld() {
    let mut world = World::new();
    world.given_modem_with_number("A", "333");
    world.given_modem_with_number("B", "111");

    // A dials B
    world.dial("A", "B");

    // Before B answers, A terminates the dialing call 1 via AT+CHLD=11
    world.release_call("A", 1);

    // B should receive remote hangup URC
    then_wait_for_response_containing(&mut world, "B", "NO CARRIER");

    // Verify both modems are now idle
    world.assert_idle("A");
    world.assert_idle("B");
}

#[test]
fn test_chld_one_x_selective_hangup_in_conference() {
    let mut world = World::new();
    world.given_modem_with_number("A", "333");
    world.given_modem_with_number("B", "111");
    world.given_modem_with_number("C", "222");

    // A calls B, B answers
    world.connect_call("A", "B");

    // A calls C, B put on hold, C answers
    world.connect_call("A", "C");

    // Merge calls into multi-party conference (AT+CHLD=3)
    world.conference("A");

    // Selectively hang up call 1 (B) within conference using AT+CHLD=11
    world.release_call("A", 1);

    // Verify B received remote hangup URC
    then_wait_for_response_containing(&mut world, "B", "NO CARRIER");

    // Verify: C (Call 2) remains active on A with is_multi_party degraded to false
    // (0), B (Call 1) is gone
    world.assert_clcc("A", &["+CLCC: 2,0,0,0,0,222"]);
}

#[test]
fn test_simultaneous_double_hangup_race() {
    let mut world = World::new();
    world.given_modem_with_number("A", "333");
    world.given_modem_with_number("B", "111");

    // A calls B, B answers
    world.connect_call("A", "B");

    // A initiates hangup with ATH
    world.hangup("A");

    // B receives the asynchronous peer hangup notification URC
    then_wait_for_response_containing(&mut world, "B", "NO CARRIER");

    // B attempts teardown via AT+CHLD=1 (idempotent OK on idle state)
    world.release_active_and_accept_held("B");

    // Verify both modems are cleanly idle without panics or leaked state
    world.assert_idle("A");
    world.assert_idle("B");
}

#[test]
fn test_chld_two_x_separate_from_4way_conference() {
    let mut world = World::new();
    world.given_modem_with_number("A", "333");
    world.given_modem_with_number("B", "111");
    world.given_modem_with_number("C", "222");
    world.given_modem_with_number("D", "444");

    // A calls B, B answers
    world.connect_call("A", "B");

    // A calls C, C answers (puts B on hold)
    world.connect_call("A", "C");

    // A merges B & C into conference
    world.conference("A");

    // A calls D, D answers (puts B & C conference on hold)
    world.connect_call("A", "D");

    // A merges D into the conference -> 3 remote participants in Active conference
    world.conference("A");

    // A separates Call 1 (B) via AT+CHLD=21:
    // Call 1 becomes Active (mpty=0)
    // Calls 2 (C) and 3 (D) become Held, and retain mpty=1 (held conference of 2
    // peers)
    world.separate_call("A", 1);

    world.assert_clcc(
        "A",
        &["+CLCC: 1,0,0,0,0,111", "+CLCC: 2,0,1,0,1,222", "+CLCC: 3,0,1,0,1,444"],
    );
}

#[test]
fn test_concurrent_two_pair_dialing_no_crosstalk() {
    let mut world = World::new();
    world.given_modem_with_number("A", "111");
    world.given_modem_with_number("B", "222");
    world.given_modem_with_number("C", "333");
    world.given_modem_with_number("D", "444");

    // Pair 1: A dials B (222)
    world.dial("A", "B");

    // Pair 2: C dials D (444) simultaneously
    world.dial("C", "D");

    // Callee D (Pair 2) answers first with ATA
    world.answer("D", "C");

    // Callee B (Pair 1) answers second with ATA
    world.answer("B", "A");

    // Verify Pair 1: A is connected to B (222), B is connected to A (111)
    world.assert_clcc("A", &["+CLCC: 1,0,0,0,0,222"]);
    world.assert_clcc("B", &["+CLCC: 1,1,0,0,0,111"]);

    // Verify Pair 2: C is connected to D (444), D is connected to C (333)
    world.assert_clcc("C", &["+CLCC: 1,0,0,0,0,444"]);
    world.assert_clcc("D", &["+CLCC: 1,1,0,0,0,333"]);
}

#[test]
fn test_chld_two_swap_entire_conference_group() {
    let mut world = World::new();
    world.given_modem_with_number("A", "333");
    world.given_modem_with_number("B", "111");
    world.given_modem_with_number("C", "222");
    world.given_modem_with_number("D", "444");

    // A calls B, B answers
    world.connect_call("A", "B");

    // A calls C, C answers (puts B on hold)
    world.connect_call("A", "C");

    // A merges B & C into Active conference (AT+CHLD=3)
    world.conference("A");

    // D calls A while A is in conference -> arrives as Waiting
    world.dial("D", "A");

    // A sends AT+CHLD=2:
    // Entire conference {B, C} goes to Held (mpty=1)
    // Call 3 (D) is answered and becomes Active (mpty=0)
    world.swap_calls("A");
    then_wait_for_response_containing(&mut world, "D", "RING");

    world.assert_clcc(
        "A",
        &["+CLCC: 1,0,1,0,1,111", "+CLCC: 2,0,1,0,1,222", "+CLCC: 3,1,0,0,0,444"],
    );

    // A sends AT+CHLD=2 again:
    // Call 3 (D) goes to Held (mpty=0)
    // Entire conference {B, C} resumes to Active (mpty=1)
    world.swap_calls("A");

    world.assert_clcc(
        "A",
        &["+CLCC: 1,0,0,0,1,111", "+CLCC: 2,0,0,0,1,222", "+CLCC: 3,1,1,0,0,444"],
    );
}

#[test]
fn test_remote_peer_hangup_in_active_conference() {
    let mut world = World::new();
    world.given_modem_with_number("A", "333");
    world.given_modem_with_number("B", "111");
    world.given_modem_with_number("C", "222");

    // A calls B, B answers
    world.connect_call("A", "B");

    // A calls C, C answers (puts B on hold)
    world.send_and_expect_ok("A", "ATD222;");
    then_wait_for_response_containing(&mut world, "B", "RING");
    then_wait_for_response_containing(&mut world, "C", "RING");
    world.answer("C", "A");

    // A merges B & C into conference
    world.conference("A");
    then_wait_for_response_containing(&mut world, "B", "RING");

    // Remote participant B hangs up with ATH
    world.hangup("B");

    // Host A receives remote hangup URC
    then_wait_for_response_containing(&mut world, "A", "NO CARRIER");

    // Verify: Call 1 (B) is removed, and Call 2 (C) automatically degraded to
    // mpty=0
    world.assert_clcc("A", &["+CLCC: 2,0,0,0,0,222"]);

    // Verify C is still active with A
    world.assert_clcc("C", &["+CLCC: 1,1,0,0,0,333"]);
}

#[test]
fn test_two_independent_concurrent_conferences() {
    let mut world = World::new();
    // Group 1
    world.given_modem_with_number("A", "111");
    world.given_modem_with_number("B", "222");
    world.given_modem_with_number("C", "333");
    // Group 2
    world.given_modem_with_number("D", "444");
    world.given_modem_with_number("E", "555");
    world.given_modem_with_number("F", "666");

    // Build Conference 1 (A + B + C)
    world.connect_call("A", "B");

    world.send_and_expect_ok("A", "ATD333;");
    then_wait_for_response_containing(&mut world, "B", "RING");
    then_wait_for_response_containing(&mut world, "C", "RING");
    world.answer("C", "A");

    world.conference("A");
    then_wait_for_response_containing(&mut world, "B", "RING");

    // Build Conference 2 (D + E + F) concurrently
    world.connect_call("D", "E");

    world.send_and_expect_ok("D", "ATD666;");
    then_wait_for_response_containing(&mut world, "E", "RING");
    then_wait_for_response_containing(&mut world, "F", "RING");
    world.answer("F", "D");

    world.conference("D");
    then_wait_for_response_containing(&mut world, "E", "RING");

    // Verify Group 1: A has active conference with B and C
    world.assert_clcc("A", &["+CLCC: 1,0,0,0,1,222", "+CLCC: 2,0,0,0,1,333"]);

    // Verify Group 2: D has active conference with E and F
    world.assert_clcc("D", &["+CLCC: 1,0,0,0,1,555", "+CLCC: 2,0,0,0,1,666"]);

    // B hangs up from Group 1
    world.hangup("B");
    then_wait_for_response_containing(&mut world, "A", "NO CARRIER");

    // Verify Group 1: A's remaining call with C degrades to mpty=0
    world.assert_clcc("A", &["+CLCC: 2,0,0,0,0,333"]);

    // Verify Group 2 is completely isolated and unaffected (still mpty=1 on both
    // calls)
    world.assert_clcc("D", &["+CLCC: 1,0,0,0,1,555", "+CLCC: 2,0,0,0,1,666"]);
}

#[test]
fn test_no_duplicate_ring_to_already_held_peer_on_second_hold_dial() {
    let mut world = World::new();
    world.given_modem_with_number("A", "333");
    world.given_modem_with_number("B", "111");
    world.given_modem_with_number("C", "222");
    world.given_modem_with_number("D", "444");

    // 1. A calls B, B answers
    world.connect_call("A", "B");

    // 2. A calls C, C answers (puts B on hold)
    world.send_and_expect_ok("A", "ATD222;");
    then_wait_for_response_containing(&mut world, "B", "RING"); // B receives HOLD URC
    then_wait_for_response_containing(&mut world, "C", "RING");
    world.answer("C", "A");

    // At this point: B is Held (has consumed its 1 hold RING), C is Active.
    // 3. A calls D (puts C on hold, B was ALREADY on hold)
    world.send_and_expect_ok("A", "ATD444;");
    then_wait_for_response_containing(&mut world, "C", "RING"); // C receives HOLD URC
    then_wait_for_response_containing(&mut world, "D", "RING");

    // Now query B: B should NOT have any buffered/unconsumed duplicate RING!
    world.assert_clcc("B", &["+CLCC: 1,1,1,0,0,333,129"]);
}

#[test]
fn test_active_conference_with_waiting_call_hold_and_accept() {
    let mut world = World::new();
    world.given_modem_with_number("A", "333");
    world.given_modem_with_number("B", "111");
    world.given_modem_with_number("C", "222");
    world.given_modem_with_number("D", "444");

    // 1. Establish 3-way conference between A, B, and C
    world.connect_call("A", "B");

    world.send_and_expect_ok("A", "ATD222;");
    then_wait_for_response_containing(&mut world, "B", "RING");
    then_wait_for_response_containing(&mut world, "C", "RING");
    world.answer("C", "A");

    world.conference("A");
    then_wait_for_response_containing(&mut world, "B", "RING");

    // 2. Incoming call from D while conference is active -> Waiting
    world.dial("D", "A");

    // Verify A has 2 active conference calls and 1 waiting call
    world.assert_clcc(
        "A",
        &["+CLCC: 1,0,0,0,1,111", "+CLCC: 2,0,0,0,1,222", "+CLCC: 3,1,5,0,0,444"],
    );

    // 3. AT+CHLD=2: Hold active conference, answer waiting call (D)
    world.swap_calls("A");
    then_wait_for_response_containing(&mut world, "B", "RING");
    then_wait_for_response_containing(&mut world, "C", "RING");
    then_wait_for_response_containing(&mut world, "D", "RING");

    // Verify: Calls 1 & 2 are Held (mpty=1), Call 3 is Active
    world.assert_clcc(
        "A",
        &["+CLCC: 1,0,1,0,1,111", "+CLCC: 2,0,1,0,1,222", "+CLCC: 3,1,0,0,0,444"],
    );

    // 4. AT+CHLD=3: Merge D into the conference group -> All 3 active with mpty=1
    world.conference("A");
    then_wait_for_response_containing(&mut world, "B", "RING");
    then_wait_for_response_containing(&mut world, "C", "RING");

    world.assert_clcc(
        "A",
        &["+CLCC: 1,0,0,0,1,111", "+CLCC: 2,0,0,0,1,222", "+CLCC: 3,1,0,0,1,444"],
    );
}

#[test]
fn test_active_conference_with_waiting_call_reject_udub() {
    let mut world = World::new();
    world.given_modem_with_number("A", "333");
    world.given_modem_with_number("B", "111");
    world.given_modem_with_number("C", "222");
    world.given_modem_with_number("D", "444");

    // 1. Establish 3-way conference between A, B, and C
    world.connect_call("A", "B");

    world.send_and_expect_ok("A", "ATD222;");
    then_wait_for_response_containing(&mut world, "B", "RING");
    then_wait_for_response_containing(&mut world, "C", "RING");
    world.answer("C", "A");

    world.conference("A");
    then_wait_for_response_containing(&mut world, "B", "RING");

    // 2. Incoming call from D while conference is active -> Waiting
    world.dial("D", "A");

    // 3. AT+CHLD=0: Reject waiting call (UDUB), conference remains intact
    world.release_held_calls("A");
    then_wait_for_response_containing(&mut world, "D", "BUSY");

    // Verify: Calls 1 & 2 are still Active with mpty=1, Call 3 is gone
    world.assert_clcc("A", &["+CLCC: 1,0,0,0,1,111", "+CLCC: 2,0,0,0,1,222"]);
}

#[test]
fn test_active_conference_with_waiting_call_release_and_accept() {
    let mut world = World::new();
    world.given_modem_with_number("A", "333");
    world.given_modem_with_number("B", "111");
    world.given_modem_with_number("C", "222");
    world.given_modem_with_number("D", "444");

    // 1. Establish 3-way conference between A, B, and C
    world.connect_call("A", "B");

    world.send_and_expect_ok("A", "ATD222;");
    then_wait_for_response_containing(&mut world, "B", "RING");
    then_wait_for_response_containing(&mut world, "C", "RING");
    world.answer("C", "A");

    world.conference("A");
    then_wait_for_response_containing(&mut world, "B", "RING");

    // 2. Incoming call from D while conference is active -> Waiting
    world.dial("D", "A");

    // 3. AT+CHLD=1: Release all active conference calls (B and C), answer waiting
    //    call (D)
    world.release_active_and_accept_held("A");
    then_wait_for_response_containing(&mut world, "B", "NO CARRIER");
    then_wait_for_response_containing(&mut world, "C", "NO CARRIER");
    then_wait_for_response_containing(&mut world, "D", "RING");

    // Verify: Only Call 3 (D) is Active with mpty=0, Calls 1 & 2 are gone
    world.assert_clcc("A", &["+CLCC: 3,1,0,0,0,444"]);
}

#[test]
fn test_ath_rejects_waiting_preserves_active_e2e() {
    let mut world = World::new();
    world.given_modem_with_number("A", "333");
    world.given_modem_with_number("B", "111");
    world.given_modem_with_number("C", "222");

    // A calls B, B answers (Active)
    world.connect_call("A", "B");

    // C calls A, A gets CCWA (Waiting)
    world.dial("C", "A");

    // ATH: User Determined User Busy (rejects waiting call C, preserves active call
    // B)
    world.send_and_expect_ok("A", "ATH");

    // C gets hangup URC
    then_wait_for_response_containing(&mut world, "C", "BUSY");

    // Verify B is still Active on A
    world.assert_clcc("A", &["+CLCC: 1,0,0,0,0,111"]);
}

#[test]
fn test_ath_drops_active_preserves_held_e2e() {
    let mut world = World::new();
    world.given_modem_with_number("A", "333");
    world.given_modem_with_number("B", "111");
    world.given_modem_with_number("C", "222");

    // A connects to B
    world.connect_call("A", "B");

    // A holds B and calls C
    world.send_and_expect_ok("A", "ATD222;");
    then_wait_for_response_containing(&mut world, "B", "RING");
    then_wait_for_response_containing(&mut world, "C", "RING");
    world.answer("C", "A");

    // Now A has B (Held) and C (Active)
    world.assert_clcc("A", &["+CLCC: 1,0,1,0,0,111", "+CLCC: 2,0,0,0,0,222"]);

    // ATH: Terminate foreground call (C), preserving held call (B)
    world.send_and_expect_ok("A", "ATH");

    // C gets hangup URC
    then_wait_for_response_containing(&mut world, "C", "NO CARRIER");

    // B remains Held on A
    world.assert_clcc("A", &["+CLCC: 1,0,1,0,0,111"]);
}

#[test]
fn test_ata_auto_holds_active_call_e2e() {
    let mut world = World::new();
    world.given_modem_with_number("A", "333");
    world.given_modem_with_number("B", "111");
    world.given_modem_with_number("C", "222");

    // A calls B, B answers (Active)
    world.connect_call("A", "B");

    // C calls A, A gets CCWA (Waiting)
    world.dial("C", "A");

    // ATA: Answering waiting call C automatically puts active call B on hold
    world.send_and_expect_ok("A", "ATA");

    // B gets hold URC
    then_wait_for_response_containing(&mut world, "B", "RING");
    // C gets connect URC
    then_wait_for_response_containing(&mut world, "C", "RING");

    // Verify on A: Call 1 (B) is Held (state 1), Call 2 (C) is Active (state 0)
    world.assert_clcc("A", &["+CLCC: 1,0,1,0,0,111", "+CLCC: 2,1,0,0,0,222"]);
}

#[test]
fn test_chld_0_rejects_waiting_preserves_held_e2e() {
    let mut world = World::new();
    world.given_modem_with_number("A", "333");
    world.given_modem_with_number("B", "111");
    world.given_modem_with_number("C", "222");

    // A calls B, B answers (Active)
    world.connect_call("A", "B");

    // A puts B on hold
    world.send_and_expect_ok("A", "AT+CHLD=2");
    then_wait_for_response_containing(&mut world, "B", "RING");

    // C calls A (Inbound/Waiting)
    world.dial("C", "A");

    // Under 3GPP TS 27.007 § 7.13 / TS 22.030 § 4.5.5.1:
    // AT+CHLD=0 sets UDUB on waiting call C, preserving held call B
    world.send_and_expect_ok("A", "AT+CHLD=0");

    // Only waiting caller C receives hangup URC
    then_wait_for_response_containing(&mut world, "C", "BUSY");

    // B remains Held on A
    world.assert_clcc("A", &["+CLCC: 1,0,1,0,0,111"]);
}

#[test]
fn test_ath_teardown_multiparty_conference_e2e() {
    let mut world = World::new();
    world.given_modem_with_number("A", "333");
    world.given_modem_with_number("B", "111");
    world.given_modem_with_number("C", "222");

    // A connects to B
    world.connect_call("A", "B");

    // A holds B and calls C
    world.send_and_expect_ok("A", "ATD222;");
    then_wait_for_response_containing(&mut world, "B", "RING");
    then_wait_for_response_containing(&mut world, "C", "RING");
    world.answer("C", "A");

    // A joins B and C into 3-way conference
    world.send_and_expect_ok("A", "AT+CHLD=3");
    world.assert_clcc("A", &["+CLCC: 1,0,0,0,1,111", "+CLCC: 2,0,0,0,1,222"]);

    // ATH: Drops all active foreground conference legs
    world.send_and_expect_ok("A", "ATH");

    // Both B and C receive hangup URC
    then_wait_for_response_containing(&mut world, "B", "NO CARRIER");
    then_wait_for_response_containing(&mut world, "C", "NO CARRIER");

    // Call list on A is completely empty
    world.assert_clcc("A", &[]);
}
