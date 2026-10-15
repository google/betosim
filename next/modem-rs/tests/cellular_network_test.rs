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

// Scenario: Add Modem to Manager
//   Given a modem "A"
//   Then modem count is 1
#[test]
fn test_add_modem_to_manager() {
    let mut world = World::new();
    given_modem(&mut world, "A");

    then_modem_count_is(&world, 1);
}

// Scenario: Verify Metrics
//   Given a modem "A"
//   Then metrics are (AT=0, Calls=0)
//   When AT command "AT" is sent to "A"
//   Then metrics are (AT=1, Calls=0)
//   When AT command "ATD12345;" is sent to "A"
//   Then metrics are (AT=2, Calls=1)
#[test]
fn test_metrics_counters() {
    let mut world = World::new();
    given_modem_with_number(&mut world, "A", "12345");

    // Check initial state
    then_metrics_are(&world, 0, 0);

    // Send a command and check again
    when_at_command_sent(&mut world, "A", "AT");
    then_response_is(&mut world, "A", "OK");
    then_metrics_are(&world, 1, 0);

    // Initiate a call and check again
    when_at_command_sent(&mut world, "A", "ATD12345;");
    then_response_is(&mut world, "A", "OK");
    then_metrics_are(&world, 2, 1);
}
