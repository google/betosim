// Copyright 2024 The Android Open Source Project
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

/// Hostapd Interface for Network Simulation
use crate::wifi::error::WifiResult;
use bytes::Bytes;
use netsim_packets::ieee80211::{Ieee80211, MacAddress};
use netsim_proto::config::HostapdOptions as ProtoHostapdOptions;
use tokio::sync::mpsc;

// Provides a stub implementation while the hostapd-rs crate is not integrated into the aosp-main.
pub struct Hostapd {}
impl Hostapd {
    pub async fn input(&self, _bytes: Bytes) -> WifiResult<()> {
        Ok(())
    }

    /// Retrieves the `Hostapd`'s BSSID.
    pub fn get_bssid(&self) -> MacAddress {
        MacAddress::try_from(0).unwrap()
    }

    /// Attempt to encrypt the given IEEE 802.11 frame.
    pub fn try_encrypt(&self, _ieee80211: &Ieee80211) -> Option<Ieee80211> {
        None
    }

    /// Attempt to decrypt the given IEEE 802.11 frame.
    pub fn try_decrypt(&self, _ieee80211: &Ieee80211) -> Option<Ieee80211> {
        None
    }
}

pub async fn hostapd_run(
    _opt: ProtoHostapdOptions,
    _tx: mpsc::Sender<Bytes>,
    _wifi_args: Option<Vec<String>>,
) -> WifiResult<Hostapd> {
    Ok(Hostapd {})
}
