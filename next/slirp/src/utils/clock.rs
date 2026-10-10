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

/// An abstraction for time to allow for mocking in tests.
pub trait Clock: Send + Sync {
    fn now(&self) -> Instant;
}

impl<C: Clock + ?Sized> Clock for Box<C> {
    fn now(&self) -> Instant {
        (**self).now()
    }
}

impl<C: Clock> Clock for Arc<Mutex<C>> {
    fn now(&self) -> Instant {
        self.lock().unwrap().now()
    }
}

/// The production clock, which uses the system's real time.
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Instant {
        Instant::now()
    }
}

/// A mock clock for use in tests, allowing for manual time control.
#[derive(Debug, Clone)]
pub struct MockClock {
    now: Instant,
}

impl MockClock {
    pub fn new() -> Self {
        Self { now: Instant::now() }
    }

    pub fn advance(&mut self, duration: Duration) {
        self.now += duration;
    }
}

impl Default for MockClock {
    fn default() -> Self {
        Self::new()
    }
}

impl Clock for MockClock {
    fn now(&self) -> Instant {
        self.now
    }
}
