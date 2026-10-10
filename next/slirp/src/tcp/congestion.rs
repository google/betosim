// Copyright 2023-2025 The Android Open Source Project
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

//! TCP congestion control implementation.

pub(crate) const INITIAL_CWND_PACKETS: u32 = 10;
pub(crate) const INITIAL_SSTHRESH: u32 = 65535;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct CongestionControl {
    pub cwnd: u32,
    pub ssthresh: u32,
}

impl CongestionControl {
    pub(crate) fn new(mss: u16) -> Self {
        Self { cwnd: INITIAL_CWND_PACKETS * mss as u32, ssthresh: INITIAL_SSTHRESH }
    }

    pub(crate) fn on_ack(&mut self, mss: u16) {
        if self.cwnd < self.ssthresh {
            // Slow start
            self.cwnd += mss as u32;
        } else {
            // Congestion avoidance
            self.cwnd += (mss as u32 * mss as u32) / self.cwnd;
        }
    }

    pub(crate) fn on_retransmission(&mut self, mss: u16) {
        self.ssthresh = self.cwnd / 2;
        self.cwnd = mss as u32;
    }
}
