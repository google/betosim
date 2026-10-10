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

// Rust definitions for statistics related structures
use std::collections::VecDeque;

use serde::{Deserialize, Serialize};

/// Represents an invalid packet with a reason and description.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct InvalidPacket {
    pub reason: String,
    pub description: String,
    // Packet content is intentionally omitted from the model to avoid large payloads in stats
}

/// The kind of radio.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RadioKind {
    #[default]
    Unspecified,
    BluetoothLowEnergy,
    BluetoothClassic,
    BleBeacon,
    Wifi,
    Uwb,
    Nfc,
}

impl From<i32> for RadioKind {
    fn from(v: i32) -> Self {
        match v {
            1 => Self::BluetoothLowEnergy,
            2 => Self::BluetoothClassic,
            3 => Self::BleBeacon,
            4 => Self::Wifi,
            5 => Self::Uwb,
            6 => Self::Nfc,
            _ => Self::Unspecified,
        }
    }
}

impl From<RadioKind> for i32 {
    fn from(v: RadioKind) -> Self {
        match v {
            RadioKind::Unspecified => 0,
            RadioKind::BluetoothLowEnergy => 1,
            RadioKind::BluetoothClassic => 2,
            RadioKind::BleBeacon => 3,
            RadioKind::Wifi => 4,
            RadioKind::Uwb => 5,
            RadioKind::Nfc => 6,
        }
    }
}

/// Represents the statistics for a radio.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct NetsimRadioStats {
    /// The unique ID of the chip.
    pub id: u32,
    /// The name of the chip.
    pub name: String,
    /// The kind of the radio (e.g. BLUETOOTH_LOW_ENERGY, WIFI).
    pub kind: RadioKind,
    /// Duration of the stats session in seconds.
    pub duration_secs: u64,
    /// Number of packets transmitted.
    pub tx_count: u64,
    /// Number of packets received.
    pub rx_count: u64,
    /// Number of medium-layer payload packets transmitted.
    pub p2p_tx_count: u64,
    /// Number of medium-layer payload packets received.
    pub p2p_rx_count: u64,
    /// Number of bytes transmitted.
    pub tx_bytes: u64,
    /// Number of bytes received.
    pub rx_bytes: u64,
    /// List of invalid packets encountered (capped at 50).
    /// Use `push_invalid_packet` to add items to enforce the cap.
    invalid_packets: VecDeque<InvalidPacket>,
}

impl NetsimRadioStats {
    /// The maximum number of invalid packets to store.
    pub const MAX_INVALID_PACKETS: usize = 50;

    /// Adds an invalid packet to the stats, maintaining a maximum of 50 items.
    pub fn push_invalid_packet(&mut self, packet: InvalidPacket) {
        if self.invalid_packets.len() >= Self::MAX_INVALID_PACKETS {
            self.invalid_packets.pop_front();
        }
        self.invalid_packets.push_back(packet);
    }
}

/// Represents the statistics for a device.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct NetsimDeviceStats {
    /// The unique ID of the device.
    pub device_id: Option<u32>,
    /// The name of the device.
    pub name: Option<String>,
    /// The kind of the device (e.g., "pixel6").
    pub kind: Option<String>,
    /// The version of the device.
    pub version: Option<String>,
    /// The SDK version of the device.
    pub sdk_version: Option<String>,
    /// The build ID of the device.
    pub build_id: Option<String>,
    /// The variant of the device.
    pub variant: Option<String>,
    /// The architecture of the device.
    pub arch: Option<String>,
}

/// Represents the statistics for the frontend.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct NetsimFrontendStats {
    pub get_version: u32,
    pub create_device: u32,
    pub delete_chip: u32,
    pub patch_device: u32,
    pub reset: u32,
    pub list_device: u32,
    pub subscribe_device: u32,
    pub patch_capture: u32,
    pub list_capture: u32,
    pub get_capture: u32,
    pub delete_device: u32,
}

use std::sync::atomic::{AtomicU32, Ordering};

#[derive(Debug, Default)]
pub struct FrontendStats {
    pub get_version: AtomicU32,
    pub create_device: AtomicU32,
    pub delete_chip: AtomicU32,
    pub patch_device: AtomicU32,
    pub reset: AtomicU32,
    pub list_device: AtomicU32,
    pub subscribe_device: AtomicU32,
    pub patch_capture: AtomicU32,
    pub list_capture: AtomicU32,
    pub get_capture: AtomicU32,
    pub delete_device: AtomicU32,
}

impl FrontendStats {
    pub fn snapshot(&self) -> NetsimFrontendStats {
        NetsimFrontendStats {
            get_version: self.get_version.load(Ordering::Relaxed),
            create_device: self.create_device.load(Ordering::Relaxed),
            delete_chip: self.delete_chip.load(Ordering::Relaxed),
            patch_device: self.patch_device.load(Ordering::Relaxed),
            reset: self.reset.load(Ordering::Relaxed),
            list_device: self.list_device.load(Ordering::Relaxed),
            subscribe_device: self.subscribe_device.load(Ordering::Relaxed),
            patch_capture: self.patch_capture.load(Ordering::Relaxed),
            list_capture: self.list_capture.load(Ordering::Relaxed),
            get_capture: self.get_capture.load(Ordering::Relaxed),
            delete_device: self.delete_device.load(Ordering::Relaxed),
        }
    }
}

/// Detailed Wi-Fi statistics.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WifiStats {
    // === Error Counters ===
    /// Errors related to the hostapd.
    pub hostapd_errors: u32,
    /// Errors related to network connectivity (e.g., Slirp/Tap interface).
    pub network_errors: u32,
    /// Errors related to client-specific operations or state.
    pub client_errors: u32,
    /// Errors encountered while parsing, decoding, or handling IEEE 802.11
    /// frames.
    pub frame_errors: u32,
    /// Errors related to transmission or reception of frame.
    pub transmission_errors: u32,
    /// Other uncategorized errors.
    pub other_errors: u32,

    // === Core Traffic Flow & Type Counters ===
    /// 802.11 frames received from clients via Hwsim messages.
    pub hwsim_frames_rx: u32,
    /// 802.11 frames transmitted to clients via Hwsim messages.
    pub hwsim_frames_tx: u32,
    /// L3 packets transmitted to external network (e.g., Slirp).
    pub network_packets_tx: u32,
    /// L3 packets received from external network.
    pub network_packets_rx: u32,
    /// 802.11 frames transmitted to hostapd process.
    pub hostapd_frames_tx: u32,
    /// 802.11 frames received from hostapd process.
    pub hostapd_frames_rx: u32,
    /// Station-to-station 802.11 frames transmitted via medium.
    pub wmedium_frames_tx: u32,
    /// Unicast 802.11 frames transmitted to another station via medium.
    pub wmedium_unicast_frames_tx: u32,
    /// 802.11 Management frames received by medium.
    pub mgmt_frames_rx: u32,

    // === Specific Protocol Counters ===
    /// mDNS frames count.
    pub mdns_count: u32,

    // === Performance Statistics ===
    /// Max throughput from Internet to device(s) in Mbits per second.
    pub max_download_throughput: f32,
    /// Max throughput from device(s) to Internet in Mbits per second.
    pub max_upload_throughput: f32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_push_invalid_packet_capping() {
        let mut stats = NetsimRadioStats::default();
        let overflow = 10;
        for i in 0..(NetsimRadioStats::MAX_INVALID_PACKETS + overflow) {
            stats.push_invalid_packet(InvalidPacket {
                reason: format!("reason {}", i),
                description: format!("description {}", i),
            });
        }
        assert_eq!(stats.invalid_packets.len(), NetsimRadioStats::MAX_INVALID_PACKETS);
        assert_eq!(stats.invalid_packets[0].reason, format!("reason {}", overflow));
        assert_eq!(
            stats.invalid_packets[NetsimRadioStats::MAX_INVALID_PACKETS - 1].reason,
            format!("reason {}", NetsimRadioStats::MAX_INVALID_PACKETS + overflow - 1)
        );
    }

    #[test]
    fn test_wifi_stats_default_and_mutation() {
        let mut stats = WifiStats::default();
        assert_eq!(stats.hostapd_errors, 0);
        assert_eq!(stats.hwsim_frames_rx, 0);
        assert_eq!(stats.max_download_throughput, 0.0);

        stats.hostapd_errors = 3;
        stats.hwsim_frames_rx = 42;
        stats.max_download_throughput = 150.5;

        let cloned = stats.clone();
        assert_eq!(cloned, stats);
        assert_eq!(cloned.hostapd_errors, 3);
        assert_eq!(cloned.hwsim_frames_rx, 42);
        assert_eq!(cloned.max_download_throughput, 150.5);
    }
}
