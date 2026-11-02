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

use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use crate::{
    clock::{Clock, MockClock},
    timers::{ScheduledEvent, TimerEvent, TimerManager},
};

#[test]
fn test_timers_default() {
    let _default_manager = TimerManager::default();
}

#[test]
fn test_timer_event_equality() {
    let clock = MockClock::new();
    let now = clock.now();
    let e1 = ScheduledEvent { time: now, event: TimerEvent::Test };
    let e2 = ScheduledEvent { time: now, event: TimerEvent::Tcp(1) };
    let e3 = ScheduledEvent { time: now + Duration::from_secs(1), event: TimerEvent::Test };

    assert_eq!(e1, e2); // Based on time only!
    assert_ne!(e1, e3);
}

#[test]
fn test_timer_manager_scheduling() {
    let clock = Box::new(MockClock::new());
    let mut timers = TimerManager::new(clock);
    assert_eq!(timers.next_event_in(), None);

    // Schedule an event
    timers.schedule(Duration::from_millis(100), TimerEvent::Test);
    let next_in = timers.next_event_in().unwrap();
    assert!(next_in <= Duration::from_millis(100));

    // No events should have fired yet
    assert!(timers.tick().is_empty());
}

#[test]
fn test_timer_ordering() {
    let clock = Box::new(MockClock::new());
    let mut timers = TimerManager::new(clock);

    // Schedule events out of order
    timers.schedule(Duration::from_millis(200), TimerEvent::Test);
    timers.schedule(Duration::from_millis(100), TimerEvent::Test);

    // The next event should be the one with the shortest delay
    let next_in = timers.next_event_in().unwrap();
    assert!(next_in <= Duration::from_millis(100));
}

#[test]
fn test_timer_lifecycle_with_mock_clock() {
    let clock = Arc::new(Mutex::new(MockClock::new()));
    let mut timers = TimerManager::new(Box::new(clock.clone()));

    // Schedule events
    timers.schedule(Duration::from_millis(100), TimerEvent::Test);
    timers.schedule(Duration::from_millis(200), TimerEvent::Tcp(42));

    assert_eq!(timers.next_event_in().unwrap(), Duration::from_millis(100));

    // Tick before they are due -> nothing fires
    assert!(timers.tick().is_empty());

    // Advance clock past the first event
    clock.lock().unwrap().advance(Duration::from_millis(150));

    // next_event_in should return ZERO for the past event
    assert_eq!(timers.next_event_in().unwrap(), Duration::ZERO);

    // Tick -> first event fires
    let fired = timers.tick();
    assert_eq!(fired.len(), 1);
    assert_eq!(fired[0], TimerEvent::Test);

    // Next event is now the second one, which is 50ms away (200ms original -
    // 150ms advanced)
    assert_eq!(timers.next_event_in().unwrap(), Duration::from_millis(50));

    // Advance past second event
    clock.lock().unwrap().advance(Duration::from_millis(100));

    // Tick -> second event fires
    let fired = timers.tick();
    assert_eq!(fired.len(), 1);
    assert_eq!(fired[0], TimerEvent::Tcp(42));

    assert_eq!(timers.next_event_in(), None);
}

#[test]
fn test_timer_cancellation_and_clear() {
    let clock = MockClock::new();
    let mut timers = TimerManager::new(Box::new(clock));

    timers.schedule(Duration::from_millis(100), TimerEvent::Test);
    timers.schedule(Duration::from_millis(200), TimerEvent::Tcp(1));
    timers.schedule(Duration::from_millis(300), TimerEvent::Tcp(2));
    timers.schedule(Duration::from_millis(400), TimerEvent::Test);

    // Cancel specific event type
    timers.cancel_by_event(&TimerEvent::Test);

    // We should only have the Tcp events left (next in 200ms and 300ms)
    assert_eq!(timers.next_event_in().unwrap(), Duration::from_millis(200));

    // Clear everything
    timers.clear();
    assert_eq!(timers.next_event_in(), None);
}
