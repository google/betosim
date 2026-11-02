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
    time::{Duration, Instant},
};

use crate::utils::clock::{Clock, MockClock, SystemClock};

#[test]
fn test_system_clock() {
    let clock = SystemClock;
    let now1 = clock.now();
    std::thread::sleep(Duration::from_millis(2));
    let now2 = clock.now();
    assert!(now2 > now1);
}

#[test]
fn test_mock_clock_default_and_advance() {
    let mut clock = MockClock::default();
    let now1 = clock.now();
    clock.advance(Duration::from_secs(5));
    let now2 = clock.now();
    assert_eq!(now2.duration_since(now1), Duration::from_secs(5));
}

#[test]
fn test_box_clock() {
    let clock: Box<dyn Clock> = Box::new(MockClock::new());
    let now = clock.now();
    assert!(now <= Instant::now());
}

#[test]
fn test_arc_mutex_clock() {
    let mock = MockClock::new();
    let clock = Arc::new(Mutex::new(mock));
    let now = clock.now();
    assert!(now <= Instant::now());
}
