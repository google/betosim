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

use bytes::Bytes;
use netsim_packets::{HwsimFrame, Ieee80211};

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub(crate) enum InfraTarget {
    None,
    Ap,
    Slirp,
}

pub struct TxPacketState {
    pub(crate) infra_target: InfraTarget,
    /// delivery to other stations (peers) in the simulated medium.
    /// This covers: unicast to another station, multicast, and broadcast.
    pub(crate) stations: bool,
    pub(crate) frame: HwsimFrame,
    pub(crate) plaintext_ieee80211: Option<Ieee80211>,
    pub(crate) plaintext_bytes: Option<Bytes>,
}

impl TxPacketState {
    pub(crate) fn get_ieee80211(&self) -> &Ieee80211 {
        self.plaintext_ieee80211.as_ref().unwrap_or(&self.frame.ieee80211)
    }

    pub(crate) fn get_ieee80211_bytes(&self) -> Bytes {
        if let Some(bytes) = &self.plaintext_bytes {
            bytes.clone()
        } else if let Some(ieee80211) = &self.plaintext_ieee80211 {
            bytes::Bytes::copy_from_slice(ieee80211.as_bytes())
        } else {
            self.frame.data.clone().into()
        }
    }
}
